//! The `factory project` commands. They work on the factory home and need no repository in the
//! current directory.

use std::path::{Path, PathBuf};

use factory_core::project::{self, ProjectFile};
use factory_core::PROJECT_LEASE_FILE;
use factory_core::{resolve, Board, Config, Home};
use factory_engine::{now_secs, project_lease_status, ProjectLeaseStatus, RepoTip};
use factory_git::Repo;

use crate::cli::ProjectCmd;
use crate::CliError;

pub fn run(home: &Home, cmd: ProjectCmd) -> Result<String, CliError> {
    match cmd {
        ProjectCmd::Add {
            path,
            preset,
            base,
            name,
        } => add(home, &path, &preset, base.as_deref(), name.as_deref()),
        ProjectCmd::List => list(home),
        ProjectCmd::Which { path } => which(home, path.as_deref()),
        ProjectCmd::Show { name } => show(home, &name),
        ProjectCmd::Remove { name, force } => remove(home, &name, force),
    }
}

/// The central base branch of a project. A missing or invalid value gives the default, like `resolve` does.
pub(crate) fn base_of(file: &ProjectFile) -> String {
    file.base_branch()
        .unwrap_or_else(|| Config::default().base_branch)
}

/// The base branch only when `project.toml` holds a valid one. A project never falls back to the default.
pub(crate) fn base_of_strict(file: &ProjectFile) -> Option<String> {
    file.base_branch()
}

/// Opens the repo at exactly `path` (not a parent repo) with the project's base branch.
pub(crate) fn open_repo(path: &Path, base: &str) -> Result<Repo, String> {
    let repo = Repo::open(path).map_err(|e| format!("not a git repository: {e}"))?;
    let same = std::fs::canonicalize(repo.root()).ok() == std::fs::canonicalize(path).ok();
    if !same {
        return Err(format!(
            "{} is not the top folder of a git repository",
            path.display()
        ));
    }
    Ok(repo.with_base_branch(base))
}

/// The project name for a repo folder: the lowercased folder name, when it is a valid name.
fn name_from_folder(path: &Path) -> Result<String, CliError> {
    let folder = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let guess = folder.to_lowercase();
    if project::validate_name(&guess).is_err() {
        return Err(CliError::Refused(format!(
            "cannot derive a project name from the folder {folder:?}, pass --name"
        )));
    }
    Ok(guess)
}

fn add(
    home: &Home,
    path: &Path,
    preset: &str,
    base: Option<&str>,
    name: Option<&str>,
) -> Result<String, CliError> {
    // A relative or non-canonical path (such as ".") is resolved first, so the name and the registry see the real folder.
    let path = std::fs::canonicalize(path).map_err(|e| {
        CliError::Refused(format!("repo path {} is not usable: {e}", path.display()))
    })?;
    let path = path.as_path();
    let name = match name {
        Some(n) => n.to_string(),
        None => name_from_folder(path)?,
    };
    project::validate_name(&name)?;
    let probe = open_repo(path, "").map_err(CliError::Refused)?;
    let base = match base {
        Some(b) => b.to_string(),
        None => probe
            .current_branch()
            .ok_or_else(|| CliError::Refused("HEAD is not on a branch, pass --base".to_string()))?,
    };
    let check = |p: &Path| -> Result<(), String> {
        let repo = open_repo(p, &base)?;
        let names = repo.local_branch_names().map_err(|e| e.to_string())?;
        if names.contains(&base) {
            Ok(())
        } else {
            Err(format!(
                "base branch {base} does not exist in the repository"
            ))
        }
    };
    let dir = project::add(home, &name, path, preset, Some(&base), &check)?;
    Ok(format!(
        "added project {name} ({preset}, base {base}) at {}",
        dir.display()
    ))
}

/// Why a readable project cannot be used, or `None` when its repo opens.
fn disabled_reason(file: &ProjectFile) -> Option<String> {
    if !file.repo.is_dir() {
        return Some("repo path is missing".to_string());
    }
    open_repo(&file.repo, &base_of(file)).err()
}

fn list(home: &Home) -> Result<String, CliError> {
    let entries = project::list(home)?;
    if entries.is_empty() {
        return Ok("no projects".to_string());
    }
    let mut lines = Vec::new();
    for e in entries {
        let line = match e.file {
            Err(why) => format!("{}  disabled: {why}", e.name),
            Ok(f) => match disabled_reason(&f) {
                Some(why) => format!("{}  disabled: {why}", e.name),
                None => format!(
                    "{} {} {} {}",
                    e.name,
                    f.preset,
                    base_of(&f),
                    f.repo.display()
                ),
            },
        };
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn which(home: &Home, path: Option<&Path>) -> Result<String, CliError> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    match project::find_by_path(home, &path)? {
        Some(name) => Ok(name),
        None => Err(CliError::Refused(format!(
            "no registered project contains {}",
            path.display()
        ))),
    }
}

fn show(home: &Home, name: &str) -> Result<String, CliError> {
    let file = project::get(home, name)?;
    let base = base_of(&file);
    if let Some(why) = disabled_reason(&file) {
        return Ok([
            format!("project: {name}"),
            format!("repo: {}", file.repo.display()),
            format!("preset: {}", file.preset),
            format!("base branch: {base}"),
            format!("disabled: {why}"),
        ]
        .join("\n"));
    }
    let repo = open_repo(&file.repo, &base).map_err(CliError::Refused)?;
    let resolved = resolve(home, name, &RepoTip(&repo))?;
    let mut problems = resolved.problems.clone();
    if !resolved.config.degraded {
        if let Err(e) = factory_checks::validate_config(&resolved.config) {
            problems.push(format!(
                "a check command would be rejected when a job starts: {e}"
            ));
        }
    }
    let l = &resolved.layout;
    let mut out = vec![
        format!("project: {name}"),
        format!("repo: {}", file.repo.display()),
        format!("preset: {}", file.preset),
        format!("base branch: {base}"),
        format!("state folder: {}", l.state_dir.display()),
        format!("worktrees folder: {}", l.worktrees_dir.display()),
        format!("risk paths: {}", resolved.config.risk_paths.len()),
        format!("tickets on merge: {}", resolved.config.tickets_on_merge),
    ];
    let names: Vec<&str> = resolved.agents.iter().map(|a| a.name.as_str()).collect();
    out.push(format!(
        "agent files: {}",
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    ));
    if !resolved.config.plan_rules.is_empty() {
        out.push("plan rules:".to_string());
        out.extend(resolved.config.plan_rules.iter().map(|r| format!("- {r}")));
    }
    if resolved.config.degraded {
        out.push("status: degraded (the settings failed closed)".to_string());
    }
    for n in &resolved.notes {
        out.push(format!("note: {n}"));
    }
    if problems.is_empty() {
        out.push("problems: none".to_string());
    } else {
        out.push("problems:".to_string());
        out.extend(problems.iter().map(|p| format!("  - {p}")));
    }
    Ok(out.join("\n"))
}

fn remove(home: &Home, name: &str, force: bool) -> Result<String, CliError> {
    project::validate_name(name)?;
    let dir: PathBuf = home.project_dir(name);
    let file = match project::get(home, name) {
        Ok(f) => Some(f),
        Err(e) if dir.is_dir() => {
            if !force {
                return Err(CliError::Refused(format!(
                    "project {name} is broken ({e}), use --force to remove it"
                )));
            }
            None
        }
        Err(e) => return Err(e.into()),
    };
    let worktrees = dir.join("worktrees");
    // A running driver owns the project, and --force does not override that.
    if let ProjectLeaseStatus::Held {
        owner,
        job,
        secs_left,
    } = project_lease_status(&dir.join("state"), now_secs())
    {
        return Err(CliError::Refused(format!(
            "project {name} is busy: job {job} is running (leased by {owner} for another {secs_left} seconds). \
             If the process that held it is gone, run `factory --project {name} cancel {job} --force`, or delete {}",
            dir.join("state").join(PROJECT_LEASE_FILE).display()
        )));
    }
    if file.is_some() && !force {
        let board = Board::load(&dir.join("state").join("board.json"))?;
        let open: Vec<&str> = board
            .jobs
            .iter()
            .filter(|j| j.stage != factory_core::Stage::Merged && !j.cancelled)
            .map(|j| j.id.as_str())
            .collect();
        if !open.is_empty() {
            return Err(CliError::Refused(format!(
                "project {name} has jobs that are not merged or cancelled ({}), use --force",
                open.join(", ")
            )));
        }
    }
    let mut notes = Vec::new();
    if let Some(file) = &file {
        match open_repo(&file.repo, &base_of(file)) {
            Ok(repo) => {
                match repo.prune_worktrees_under(&worktrees) {
                    Ok(r) => {
                        if !r.pruned.is_empty() {
                            notes.push(format!(
                                "pruned {} worktree registration(s)",
                                r.pruned.len()
                            ));
                        }
                        if !r.skipped.is_empty() {
                            notes.push(format!(
                                "left worktree registration(s) whose path leaves the worktrees folder: {}",
                                r.skipped.join(", ")
                            ));
                        }
                    }
                    Err(e) => {
                        notes.push(format!("could not prune the worktree registrations: {e}"))
                    }
                }
                let left: Vec<String> = repo
                    .local_branch_names()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|b| b.starts_with("factory/"))
                    .collect();
                if !left.is_empty() {
                    notes.push(format!("job branches left in place: {}", left.join(", ")));
                }
            }
            Err(why) => notes.push(format!(
                "worktree registrations not pruned, the repo cannot be opened: {why}"
            )),
        }
    } else {
        notes.push("project.toml is unreadable, so worktree registrations and job branches were not checked".to_string());
    }
    project::remove(home, name)?;
    let mut out = format!(
        "removed project {name} ({}), the repository and the presets are untouched",
        dir.display()
    );
    for n in notes {
        out.push('\n');
        out.push_str(&n);
    }
    Ok(out)
}
