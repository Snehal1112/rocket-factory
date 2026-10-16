//! The `factory project` commands. They work on the factory home and need no repository in the
//! current directory.

use std::path::{Path, PathBuf};

use factory_core::project::{self, ProjectFile};
use factory_core::{resolve, Board, Config, Home};
use factory_engine::RepoTip;
use factory_git::Repo;

use crate::cli::ProjectCmd;
use crate::CliError;

pub fn run(home: &Home, cmd: ProjectCmd) -> Result<String, CliError> {
    match cmd {
        ProjectCmd::Add { path, preset, base, name } => add(home, &path, &preset, base.as_deref(), name.as_deref()),
        ProjectCmd::List => list(home),
        ProjectCmd::Show { name } => show(home, &name),
        ProjectCmd::Remove { name, force } => remove(home, &name, force),
    }
}

/// The central base branch of a project. Projects added by this version always store it.
fn base_of(file: &ProjectFile) -> String {
    file.settings
        .get("base_branch")
        .and_then(|v| v.as_str())
        .map_or_else(|| Config::default().base_branch, str::to_string)
}

/// Opens the repo at exactly `path` (not a parent repo) with the project's base branch.
fn open_repo(path: &Path, base: &str) -> Result<Repo, String> {
    let repo = Repo::open(path).map_err(|e| format!("not a git repository: {e}"))?;
    let same = std::fs::canonicalize(repo.root()).ok() == std::fs::canonicalize(path).ok();
    if !same {
        return Err(format!("{} is not the top folder of a git repository", path.display()));
    }
    Ok(repo.with_base_branch(base))
}

/// The project name for a repo folder: the lowercased folder name, when it is a valid name.
fn name_from_folder(path: &Path) -> Result<String, CliError> {
    let folder = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let guess = folder.to_lowercase();
    if project::validate_name(&guess).is_err() {
        return Err(CliError::Refused(format!("cannot derive a project name from the folder {folder:?}, pass --name")));
    }
    Ok(guess)
}

fn add(home: &Home, path: &Path, preset: &str, base: Option<&str>, name: Option<&str>) -> Result<String, CliError> {
    // A relative or non-canonical path (such as ".") is resolved first, so the name and the registry see the real folder.
    let path = std::fs::canonicalize(path)
        .map_err(|e| CliError::Refused(format!("repo path {} is not usable: {e}", path.display())))?;
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
        if names.iter().any(|n| *n == base) {
            Ok(())
        } else {
            Err(format!("base branch {base} does not exist in the repository"))
        }
    };
    let dir = project::add(home, &name, path, preset, Some(&base), &check)?;
    Ok(format!("added project {name} ({preset}, base {base}) at {}", dir.display()))
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
            Ok(f) if !f.repo.is_dir() => format!("{}  disabled: repo path is missing", e.name),
            Ok(f) => {
                let base = base_of(&f);
                match open_repo(&f.repo, &base) {
                    Err(why) => format!("{}  disabled: {why}", e.name),
                    Ok(_) => format!("{} {} {} {}", e.name, f.preset, base, f.repo.display()),
                }
            }
        };
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn show(home: &Home, name: &str) -> Result<String, CliError> {
    let file = project::get(home, name)?;
    let base = base_of(&file);
    let repo = open_repo(&file.repo, &base).map_err(CliError::Refused)?;
    let resolved = resolve(home, name, &RepoTip(&repo))?;
    let mut problems = resolved.problems.clone();
    if !resolved.config.degraded {
        if let Err(e) = factory_checks::validate_config(&resolved.config) {
            problems.push(format!("a check command would be rejected when a job starts: {e}"));
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
    ];
    let names: Vec<&str> = resolved.agents.iter().map(|a| a.name.as_str()).collect();
    out.push(format!("agent files: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    if resolved.config.degraded {
        out.push("status: degraded (the settings failed closed)".to_string());
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
    let file = project::get(home, name)?;
    if !force {
        let layout = project::layout(home, name, &file);
        let board = Board::load(&layout.board_file())?;
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
    project::remove(home, name)?;
    let dir: PathBuf = home.project_dir(name);
    Ok(format!("removed project {name} ({}), the repository and the presets are untouched", dir.display()))
}
