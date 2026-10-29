//! The project registry: one folder per project under the factory home.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{preset, settings_block, Config, CoreError, Home, Layout, Scope};

const MAX_NAME: usize = 40;
const PROJECT_FILE: &str = "project.toml";

/// Lowercase letters, digits and `-`, starting with a letter or digit, at most 40 characters.
/// This keeps a name from escaping the home or naming a hidden folder.
pub fn validate_name(name: &str) -> Result<(), CoreError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(CoreError::Config(format!(
            "project name {name:?} must be 1 to {MAX_NAME} characters of a-z, 0-9 and -, starting with a letter or digit"
        )))
    }
}

/// Checks a base branch name exactly as `Config::parse` does and returns it.
pub fn valid_base_branch(name: &str) -> Result<String, CoreError> {
    let mut probe = toml::Table::new();
    probe.insert("base_branch".into(), toml::Value::String(name.to_string()));
    let text = toml::to_string(&probe).map_err(|e| CoreError::Config(e.to_string()))?;
    Config::parse(&text).map(|c| c.base_branch)
}

/// The contents of `projects/<name>/project.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The main checkout. Must be an absolute path.
    pub repo: PathBuf,
    /// The project folder inside the repo, relative and without `..`. `None` means the whole repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<PathBuf>,
    pub preset: String,
    /// Config keys that replace the preset's.
    #[serde(default)]
    pub settings: toml::Table,
}

/// A subpath must be non-empty, relative and made only of plain names.
fn validate_subpath(sub: &Path) -> Result<(), String> {
    let plain = sub
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if sub.as_os_str().is_empty() || !plain {
        return Err(format!(
            "subpath {} must be a relative path without . or .. parts",
            sub.display()
        ));
    }
    Ok(())
}

impl ProjectFile {
    /// The project folder: the repo joined with the subpath, or the repo itself.
    pub fn folder(&self) -> PathBuf {
        match &self.subpath {
            Some(sub) => self.repo.join(sub),
            None => self.repo.clone(),
        }
    }

    /// The stored `settings.base_branch` when it is a valid branch name, else `None`.
    pub fn base_branch(&self) -> Option<String> {
        valid_base_branch(self.settings.get("base_branch")?.as_str()?).ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectEntry {
    pub name: String,
    pub file: Result<ProjectFile, String>,
}

fn read_file(path: &Path) -> Result<ProjectFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let file: ProjectFile =
        toml::from_str(&text).map_err(|e| format!("bad {}: {e}", path.display()))?;
    if !file.repo.is_absolute() {
        return Err(format!(
            "bad {}: repo must be an absolute path",
            path.display()
        ));
    }
    if let Some(sub) = &file.subpath {
        validate_subpath(sub).map_err(|why| format!("bad {}: {why}", path.display()))?;
    }
    Ok(file)
}

/// Refuses a new folder in `canon` that contains, or sits inside, the folder of another project on the same repo.
fn check_overlap(home: &Home, canon: &Path, subpath: Option<&Path>) -> Result<(), CoreError> {
    let new = subpath.unwrap_or(Path::new(""));
    for entry in list(home)? {
        let Ok(f) = entry.file else { continue };
        if !std::fs::canonicalize(&f.repo).is_ok_and(|c| c == canon) {
            continue;
        }
        let other = f.subpath.as_deref().unwrap_or(Path::new(""));
        if new.starts_with(other) || other.starts_with(new) {
            return Err(CoreError::Config(format!(
                "project {} already covers {}",
                entry.name,
                canon.join(new).display()
            )));
        }
    }
    Ok(())
}

/// The prompt cache lifetime a new project starts with. One-shot agent runs rarely resume, so the cheaper 5 minute write wins.
pub const DEFAULT_CACHE_TTL: &str = "5m";

/// The model a Codex project starts with for every role.
pub const CODEX_DEFAULT_MODEL: &str = "gpt-5.6-luna";

/// Registers a project. Every check runs before anything is created.
#[allow(clippy::too_many_arguments)]
pub fn add(
    home: &Home,
    name: &str,
    repo: &Path,
    subpath: Option<&Path>,
    preset_name: &str,
    base: Option<&str>,
    repo_ok: &dyn Fn(&Path) -> Result<(), String>,
    agent_cli: Option<&str>,
) -> Result<PathBuf, CoreError> {
    validate_name(name)?;
    if let Some(sub) = subpath {
        validate_subpath(sub).map_err(CoreError::Config)?;
    }
    if preset::embedded(preset_name).is_none() {
        return Err(CoreError::Config(format!(
            "unknown preset {preset_name:?}, choose one of {}",
            preset::PRESET_NAMES.join(", ")
        )));
    }
    if !repo.is_absolute() {
        return Err(CoreError::Config(format!(
            "repo path {} must be absolute",
            repo.display()
        )));
    }
    let canon = std::fs::canonicalize(repo).map_err(|e| {
        CoreError::Config(format!("repo path {} is not usable: {e}", repo.display()))
    })?;
    if !canon.is_dir() {
        return Err(CoreError::Config(format!(
            "repo path {} is not a folder",
            canon.display()
        )));
    }
    repo_ok(&canon).map_err(|why| CoreError::Config(format!("{}: {why}", canon.display())))?;
    check_overlap(home, &canon, subpath)?;
    let mut settings = toml::Table::new();
    if agent_cli == Some("codex") {
        settings.insert("agent_cli".into(), toml::Value::String("codex".into()));
        for key in [
            "builder_model",
            "reviewer_model",
            "reviewer_model_large",
            "commit_model",
            "kb_model",
        ] {
            settings.insert(key.into(), toml::Value::String(CODEX_DEFAULT_MODEL.into()));
        }
    } else {
        settings.insert(
            "cache_ttl".into(),
            toml::Value::String(DEFAULT_CACHE_TTL.into()),
        );
    }
    if let Some(b) = base {
        settings.insert(
            "base_branch".into(),
            toml::Value::String(valid_base_branch(b)?),
        );
    }
    let file = ProjectFile {
        repo: canon.clone(),
        subpath: subpath.map(Path::to_path_buf),
        preset: preset_name.to_string(),
        settings,
    };
    let text = toml::to_string(&file).map_err(|e| CoreError::Config(e.to_string()))?;
    // Every other setting follows as a comment, so the user sees what can be set.
    let preset_config = preset::embedded(preset_name).map_or("", |p| p.config);
    let text = settings_block::with_block(&text, preset_name, preset_config)?.unwrap_or(text);
    let dir = home.project_dir(name);
    let projects = home.projects_dir();
    std::fs::create_dir_all(&projects).map_err(|e| CoreError::io(&projects, e))?;
    // A non-recursive create is the claim: exactly one caller can make the folder.
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CoreError::Config(format!("project {name} already exists")));
        }
        Err(e) => return Err(CoreError::io(&dir, e)),
    }
    let made = (|| -> Result<(), CoreError> {
        // Checked again after the claim, which narrows the window for racing adds.
        // Two adds with different names can still both pass; the merge lock serializes them.
        check_overlap(home, &canon, subpath)?;
        preset::seed(&home.presets_dir())?;
        for sub in ["agents", "state", "worktrees"] {
            let p = dir.join(sub);
            std::fs::create_dir(&p).map_err(|e| CoreError::io(&p, e))?;
        }
        let f = dir.join(PROJECT_FILE);
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&f)
            .map_err(|e| CoreError::io(&f, e))?;
        std::io::Write::write_all(&mut out, text.as_bytes()).map_err(|e| CoreError::io(&f, e))
    })();
    if let Err(e) = made {
        // Only the folder this call claimed is removed.
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

/// Every project, sorted by name. A broken `project.toml` is an `Err` entry.
pub fn list(home: &Home) -> Result<Vec<ProjectEntry>, CoreError> {
    let dir = home.projects_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| CoreError::io(&dir, e))? {
        let entry = entry.map_err(|e| CoreError::io(&dir, e))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        out.push(ProjectEntry {
            file: read_file(&path.join(PROJECT_FILE)),
            name,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn get(home: &Home, name: &str) -> Result<ProjectFile, CoreError> {
    validate_name(name)?;
    let dir = home.project_dir(name);
    if !dir.is_dir() {
        return Err(CoreError::Config(format!("no project named {name}")));
    }
    read_file(&dir.join(PROJECT_FILE)).map_err(CoreError::Config)
}

/// The text of the project's preset `config.toml`. The installed file is what the driver reads, and
/// the embedded one covers a missing file.
pub fn preset_config(home: &Home, file: &ProjectFile) -> Result<String, CoreError> {
    validate_name(&file.preset)?;
    let path = home.presets_dir().join(&file.preset).join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(t) => Ok(t),
        Err(_) => preset::embedded(&file.preset)
            .map(|p| p.config.to_string())
            .ok_or_else(|| CoreError::Config(format!("unknown preset {:?}", file.preset))),
    }
}

/// Adds the commented settings block to an existing `project.toml` and keeps the rest of its text.
/// Returns false when the file already lists every setting.
pub fn add_settings_block(home: &Home, name: &str) -> Result<bool, CoreError> {
    let file = get(home, name)?;
    let path = home.project_dir(name).join(PROJECT_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let preset_config = preset_config(home, &file)?;
    match settings_block::with_block(&text, &file.preset, &preset_config)? {
        Some(new) => {
            crate::fsutil::write_atomic(&path, new.as_bytes())?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The top of the git checkout that holds `path`: the nearest ancestor with a `.git` folder or file.
fn git_top(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The main checkout a linked git worktree at `top` belongs to, read from its `.git` file and the
/// `commondir` and `gitdir` files of its git dir. `None` when `top` is not a linked worktree or the git
/// dir does not point back at this checkout.
fn worktree_main(top: &Path) -> Option<PathBuf> {
    let dot_git = top.join(".git");
    if !dot_git.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let git_dir = top.join(target);
    let back = std::fs::read_to_string(git_dir.join("gitdir")).ok()?;
    if std::fs::canonicalize(back.trim()).ok()? != std::fs::canonicalize(&dot_git).ok()? {
        return None;
    }
    let common = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(git_dir.join(common.trim())).ok()?;
    common.parent().map(Path::to_path_buf)
}

/// The registered project whose repo is the git checkout that holds `path`. A linked git worktree that
/// is not registered itself resolves to the project of its main checkout. A git repo nested inside a
/// registered one (a submodule or a vendored clone) that is not registered gives `None`, as does a path
/// outside every repo. Among projects on one repo the one whose folder holds the path wins, deepest
/// first; a path outside every project folder gives `None`. An unreadable project or one whose repo
/// folder is gone is skipped, and two projects with the same folder are an error. `path` must exist.
pub fn find_by_path(home: &Home, path: &Path) -> Result<Option<String>, CoreError> {
    let canon = std::fs::canonicalize(path).map_err(|e| CoreError::io(path, e))?;
    // Name, repo and subpath of every readable project.
    let mut repos: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    for entry in list(home)? {
        if let Ok(f) = entry.file {
            if let Ok(repo) = std::fs::canonicalize(&f.repo) {
                repos.push((entry.name, repo, f.subpath.unwrap_or_default()));
            }
        }
    }
    let Some(top) = git_top(&canon) else {
        return Ok(None);
    };
    let on = |root: &Path| -> Vec<&(String, PathBuf, PathBuf)> {
        repos.iter().filter(|(_, r, _)| r == root).collect()
    };
    let mut hits = on(&top);
    if hits.is_empty() {
        if let Some(main) = worktree_main(&top) {
            hits = on(&main);
        }
    }
    // The path relative to the checkout it is in, which is the same in a linked worktree.
    let rel = canon.strip_prefix(&top).unwrap_or(Path::new(""));
    hits.retain(|(_, _, sub)| rel.starts_with(sub));
    let deepest = hits
        .iter()
        .map(|(_, _, sub)| sub.components().count())
        .max();
    let best: Vec<&str> = hits
        .iter()
        .filter(|(_, _, sub)| Some(sub.components().count()) == deepest)
        .map(|(n, _, _)| n.as_str())
        .collect();
    match best.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).to_string())),
        many => Err(CoreError::Config(format!(
            "ambiguous match: projects {} all register the folder containing {}",
            many.join(" and "),
            canon.display()
        ))),
    }
}

/// Deletes `projects/<name>/` and nothing else.
pub fn remove(home: &Home, name: &str) -> Result<(), CoreError> {
    validate_name(name)?;
    let dir = home.project_dir(name);
    if !dir.is_dir() {
        return Err(CoreError::Config(format!("no project named {name}")));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))
}

/// The layout of a project. The preset name is validated, so no path leaves the home.
pub fn layout(home: &Home, name: &str, file: &ProjectFile) -> Result<Layout, CoreError> {
    validate_name(&file.preset)
        .map_err(|_| CoreError::Config(format!("preset name {:?} is not valid", file.preset)))?;
    Ok(build_layout(home, name, file, true))
}

/// Builds the layout. With `preset_ok` false the preset folder is never named.
pub(crate) fn build_layout(home: &Home, name: &str, file: &ProjectFile, preset_ok: bool) -> Layout {
    let dir = home.project_dir(name);
    let own = dir.join("commit-context.md");
    let commit_context_file = if own.exists() || !preset_ok {
        own
    } else {
        home.presets_dir()
            .join(&file.preset)
            .join("commit-context.md")
    };
    let state_dir = dir.join("state");
    Layout {
        repo: file.repo.clone(),
        jobs_dir: state_dir.join("jobs"),
        state_dir,
        worktrees_dir: dir.join("worktrees"),
        config_file: dir.join(PROJECT_FILE),
        commit_context_file,
        scope: file.subpath.as_ref().map(|sub| Scope {
            project: name.to_string(),
            subpath: sub.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ok(_: &Path) -> Result<(), String> {
        Ok(())
    }

    /// The Claude-project form of `add`, which most tests use.
    fn add(
        home: &Home,
        name: &str,
        repo: &Path,
        preset_name: &str,
        base: Option<&str>,
        repo_ok: &dyn Fn(&Path) -> Result<(), String>,
    ) -> Result<PathBuf, CoreError> {
        super::add(home, name, repo, None, preset_name, base, repo_ok, None)
    }

    /// The subfolder form of `add`.
    fn add_sub(home: &Home, name: &str, repo: &Path, sub: &str) -> Result<PathBuf, CoreError> {
        super::add(
            home,
            name,
            repo,
            Some(Path::new(sub)),
            "rust",
            None,
            &ok,
            None,
        )
    }

    struct Env {
        _t: TempDir,
        home: Home,
        repo: PathBuf,
    }

    fn env() -> Env {
        let t = TempDir::new().expect("tempdir");
        let repo = t.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir");
        let home = Home::at(t.path().join("home"));
        Env { _t: t, home, repo }
    }

    fn other_repo(e: &Env, n: &str) -> PathBuf {
        let p = e._t.path().join(n);
        std::fs::create_dir_all(p.join(".git")).expect("mkdir");
        p
    }

    #[test]
    fn names_are_validated() {
        for good in ["rocket", "rocket-vault", "a1", &"a".repeat(40)] {
            assert!(validate_name(good).is_ok(), "{good}");
        }
        for bad in ["", "A", "a b", "../x", "a/b", ".", "-x", &"a".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn add_builds_the_tree_and_get_reads_it_back() {
        let e = env();
        let dir = add(&e.home, "rocket", &e.repo, "rust", Some("v-4.0.0"), &ok).expect("add");
        assert_eq!(dir, e.home.project_dir("rocket"));
        for sub in ["agents", "state", "worktrees"] {
            assert!(dir.join(sub).is_dir(), "{sub}");
        }
        assert!(e.home.presets_dir().join("rust/config.toml").exists());
        let f = get(&e.home, "rocket").expect("get");
        assert!(f.repo.is_absolute());
        assert_eq!(f.repo, std::fs::canonicalize(&e.repo).expect("canon"));
        assert_eq!(f.preset, "rust");
        assert_eq!(
            f.settings.get("base_branch").and_then(|v| v.as_str()),
            Some("v-4.0.0")
        );
    }

    #[test]
    fn add_without_base_stores_only_the_default_cache_ttl() {
        let e = env();
        add(&e.home, "rocket", &e.repo, "go", None, &ok).expect("add");
        let f = get(&e.home, "rocket").expect("get");
        assert_eq!(f.settings.len(), 1);
        assert_eq!(
            f.settings.get("cache_ttl").and_then(|v| v.as_str()),
            Some("5m")
        );
    }

    #[test]
    fn add_writes_every_setting_as_a_comment() {
        let e = env();
        add(&e.home, "rocket", &e.repo, "go", None, &ok).expect("add");
        let text =
            std::fs::read_to_string(e.home.project_dir("rocket").join(PROJECT_FILE)).expect("read");
        assert!(text.contains(crate::settings_block::BLOCK_HEADER), "{text}");
        assert!(text.contains("# builder_carry_session = true"), "{text}");
        assert!(text.contains("the go preset"), "{text}");
        assert!(!text.contains("# cache_ttl"), "{text}");
    }

    #[test]
    fn the_settings_block_fills_an_existing_file_once() {
        let e = env();
        add(&e.home, "rocket", &e.repo, "go", None, &ok).expect("add");
        let path = e.home.project_dir("rocket").join(PROJECT_FILE);
        let bare = "repo = \"/r\"\npreset = \"go\"\n\n[settings]\nfix_rounds = 3\n";
        let bare = bare.replace("/r", &e.repo.to_string_lossy());
        std::fs::write(&path, &bare).expect("write");
        assert!(add_settings_block(&e.home, "rocket").expect("first"));
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.starts_with(&bare), "{text}");
        assert!(text.contains("# budget_weighted = 1000000"), "{text}");
        assert!(!text.contains("# fix_rounds"), "{text}");
        assert!(!add_settings_block(&e.home, "rocket").expect("second"));
        assert_eq!(get(&e.home, "rocket").expect("get").settings.len(), 1);
    }

    #[test]
    fn project_add_with_agent_cli_codex_seeds_models_and_no_cache_ttl() {
        let e = env();
        super::add(
            &e.home,
            "rocket",
            &e.repo,
            None,
            "go",
            None,
            &ok,
            Some("codex"),
        )
        .expect("add");
        let f = get(&e.home, "rocket").expect("get");
        let text = |k: &str| f.settings.get(k).and_then(|v| v.as_str()).map(String::from);
        assert_eq!(text("agent_cli").as_deref(), Some("codex"));
        for key in [
            "builder_model",
            "reviewer_model",
            "reviewer_model_large",
            "commit_model",
            "kb_model",
        ] {
            assert_eq!(text(key).as_deref(), Some("gpt-5.6-luna"), "{key}");
        }
        assert!(!f.settings.contains_key("cache_ttl"));
    }

    #[test]
    fn project_add_without_the_flag_is_unchanged() {
        let e = env();
        add(&e.home, "rocket", &e.repo, "go", Some("dev"), &ok).expect("add");
        let f = get(&e.home, "rocket").expect("get");
        let mut keys: Vec<&str> = f.settings.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["base_branch", "cache_ttl"]);
    }

    fn assert_refused(e: &Env, r: Result<PathBuf, CoreError>, name: &str) {
        assert!(r.is_err());
        assert!(!e.home.project_dir(name).exists(), "folder left behind");
    }

    #[test]
    fn add_refuses_bad_input_and_leaves_nothing_behind() {
        let e = env();
        assert_refused(&e, add(&e.home, "p", &e.repo, "nope", None, &ok), "p");
        assert_refused(
            &e,
            add(&e.home, "p", Path::new("rel/repo"), "rust", None, &ok),
            "p",
        );
        assert_refused(
            &e,
            add(&e.home, "p", &e.repo.join("missing"), "rust", None, &ok),
            "p",
        );
        let no = |_: &Path| Err("not a git repo".to_string());
        assert_refused(&e, add(&e.home, "p", &e.repo, "rust", None, &no), "p");
        assert_refused(
            &e,
            add(&e.home, "p", &e.repo, "rust", Some("bad branch"), &ok),
            "p",
        );
        assert_refused(&e, add(&e.home, "../p", &e.repo, "rust", None, &ok), "p");
        assert!(
            !e.home.dir.exists(),
            "refusals must not even create the home"
        );
    }

    #[test]
    fn add_refuses_an_existing_name_and_a_second_name_for_one_repo() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        let other = other_repo(&e, "other");
        assert_refused(&e, add(&e.home, "one", &other, "rust", None, &ok), "two");
        assert_refused(&e, add(&e.home, "two", &e.repo, "rust", None, &ok), "two");
        // The same repo through a different spelling of the path is still the same repo.
        let spelled = e.repo.join("..").join("repo");
        assert_refused(&e, add(&e.home, "two", &spelled, "rust", None, &ok), "two");
        assert_eq!(list(&e.home).expect("list").len(), 1);
    }

    #[test]
    fn list_is_sorted_and_survives_a_broken_project() {
        let e = env();
        assert!(list(&e.home).expect("empty").is_empty());
        add(&e.home, "zeta", &e.repo, "rust", None, &ok).expect("add");
        add(&e.home, "alpha", &other_repo(&e, "r2"), "rust", None, &ok).expect("add");
        add(&e.home, "mid", &other_repo(&e, "r3"), "rust", None, &ok).expect("add");
        std::fs::write(
            e.home.project_dir("mid").join("project.toml"),
            "this is not = = toml",
        )
        .expect("write");
        let got = list(&e.home).expect("list");
        let names: Vec<&str> = got.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alpha", "mid", "zeta"]);
        assert!(got[0].file.is_ok());
        assert!(got[1].file.is_err());
        assert!(got[2].file.is_ok());
    }

    #[test]
    fn a_relative_repo_or_unknown_key_in_project_toml_is_broken() {
        let e = env();
        add(&e.home, "p", &e.repo, "rust", None, &ok).expect("add");
        let f = e.home.project_dir("p").join("project.toml");
        std::fs::write(&f, "repo = \"rel\"\npreset = \"rust\"\n").expect("write");
        assert!(get(&e.home, "p").is_err());
        std::fs::write(&f, "repo = \"/abs\"\npreset = \"rust\"\nextra = 1\n").expect("write");
        assert!(get(&e.home, "p").is_err());
    }

    #[test]
    fn find_by_path_matches_root_subfolder_and_symlink() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        let sub = e.repo.join("src/deep");
        std::fs::create_dir_all(&sub).expect("mkdir");
        assert_eq!(
            find_by_path(&e.home, &e.repo).expect("root").as_deref(),
            Some("one")
        );
        assert_eq!(
            find_by_path(&e.home, &sub).expect("sub").as_deref(),
            Some("one")
        );
        let link = e._t.path().join("link");
        std::os::unix::fs::symlink(&e.repo, &link).expect("symlink");
        assert_eq!(
            find_by_path(&e.home, &link.join("src"))
                .expect("link")
                .as_deref(),
            Some("one")
        );
        let out = other_repo(&e, "elsewhere");
        assert_eq!(find_by_path(&e.home, &out).expect("outside"), None);
    }

    #[test]
    fn find_by_path_prefers_the_longest_nested_repo() {
        let e = env();
        let inner = e.repo.join("vendor/inner");
        std::fs::create_dir_all(inner.join(".git")).expect("mkdir");
        add(&e.home, "outer", &e.repo, "rust", None, &ok).expect("add");
        add(&e.home, "inner", &inner, "rust", None, &ok).expect("add");
        assert_eq!(
            find_by_path(&e.home, &inner).expect("inner").as_deref(),
            Some("inner")
        );
        assert_eq!(
            find_by_path(&e.home, &e.repo.join("vendor"))
                .expect("outer")
                .as_deref(),
            Some("outer")
        );
    }

    #[test]
    fn find_by_path_resolves_a_linked_worktree() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        let wt = e._t.path().join("elsewhere-wt");
        fake_worktree(&e.repo, "wt", &wt);
        assert_eq!(
            find_by_path(&e.home, &wt.join("src"))
                .expect("wt")
                .as_deref(),
            Some("one")
        );
    }

    #[test]
    fn find_by_path_skips_gone_and_broken_projects_and_errors_on_a_missing_path() {
        let e = env();
        let gone = other_repo(&e, "gone");
        add(&e.home, "gone", &gone, "rust", None, &ok).expect("add");
        add(&e.home, "live", &e.repo, "rust", None, &ok).expect("add");
        add(&e.home, "broken", &other_repo(&e, "b"), "rust", None, &ok).expect("add");
        std::fs::write(e.home.project_dir("broken").join("project.toml"), "= =").expect("write");
        std::fs::remove_dir_all(&gone).expect("rm");
        assert_eq!(
            find_by_path(&e.home, &e.repo).expect("live").as_deref(),
            Some("live")
        );
        assert!(find_by_path(&e.home, &e.repo.join("missing")).is_err());
    }

    fn fake_worktree(repo: &Path, name: &str, at: &Path) {
        let admin = repo.join(".git/worktrees").join(name);
        std::fs::create_dir_all(&admin).expect("mkdir");
        std::fs::write(admin.join("commondir"), "../..\n").expect("commondir");
        std::fs::create_dir_all(at.join("src")).expect("mkdir");
        std::fs::write(at.join(".git"), format!("gitdir: {}\n", admin.display())).expect("gitfile");
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", at.join(".git").display()),
        )
        .expect("backlink");
    }

    #[test]
    fn find_by_path_prefers_a_registered_worktree_over_its_main_repo() {
        let e = env();
        add(&e.home, "main-repo", &e.repo, "rust", None, &ok).expect("add");
        let wt = e._t.path().join("wt-registered");
        fake_worktree(&e.repo, "wt1", &wt);
        add(&e.home, "wt-project", &wt, "rust", None, &ok).expect("add");
        assert_eq!(
            find_by_path(&e.home, &wt.join("src"))
                .expect("wt")
                .as_deref(),
            Some("wt-project")
        );
        assert_eq!(
            find_by_path(&e.home, &e.repo).expect("main").as_deref(),
            Some("main-repo")
        );
    }

    #[test]
    fn find_by_path_maps_an_unregistered_worktree_to_the_main_repo() {
        let e = env();
        add(&e.home, "main-repo", &e.repo, "rust", None, &ok).expect("add");
        let wt = e._t.path().join("wt-unregistered");
        fake_worktree(&e.repo, "wt2", &wt);
        assert_eq!(
            find_by_path(&e.home, &wt.join("src"))
                .expect("wt")
                .as_deref(),
            Some("main-repo")
        );
    }

    #[test]
    fn find_by_path_does_not_match_a_sibling_with_the_same_name_prefix() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        let folder = e
            .repo
            .file_name()
            .and_then(|f| f.to_str())
            .expect("folder name");
        let sibling = e._t.path().join(format!("{folder}-two"));
        std::fs::create_dir_all(&sibling).expect("mkdir");
        assert_eq!(find_by_path(&e.home, &sibling).expect("sibling"), None);
    }

    #[test]
    fn find_by_path_reports_two_projects_on_one_repo_as_ambiguous() {
        let e = env();
        add(&e.home, "aaa", &e.repo, "rust", None, &ok).expect("add");
        // add() refuses a second name for one repo, so write the second entry by hand.
        let dir = e.home.project_dir("bbb");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::copy(
            e.home.project_dir("aaa").join("project.toml"),
            dir.join("project.toml"),
        )
        .expect("copy");
        let err = find_by_path(&e.home, &e.repo)
            .expect_err("ambiguous")
            .to_string();
        assert!(
            err.contains("aaa") && err.contains("bbb") && err.contains("ambiguous"),
            "{err}"
        );
    }

    #[test]
    fn find_by_path_ignores_an_unregistered_repo_nested_in_a_registered_one() {
        let e = env();
        add(&e.home, "outer", &e.repo, "rust", None, &ok).expect("add");
        let inner = e.repo.join("ignored/clone");
        std::fs::create_dir_all(inner.join(".git")).expect("mkdir");
        std::fs::create_dir_all(inner.join("src")).expect("mkdir");
        assert_eq!(
            find_by_path(&e.home, &inner.join("src")).expect("inner"),
            None
        );
        let plain = e.repo.join("docs/plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        assert_eq!(
            find_by_path(&e.home, &plain).expect("plain").as_deref(),
            Some("outer")
        );
        assert_eq!(
            find_by_path(&e.home, &e.repo).expect("repo").as_deref(),
            Some("outer")
        );
    }

    #[test]
    fn find_by_path_rejects_a_worktree_whose_git_dir_does_not_point_back() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        let wt = e._t.path().join("stale-wt");
        fake_worktree(&e.repo, "stale", &wt);
        std::fs::write(
            e.repo.join(".git/worktrees/stale/gitdir"),
            "/somewhere/else/.git\n",
        )
        .expect("rewrite");
        assert_eq!(find_by_path(&e.home, &wt).expect("stale"), None);
    }

    #[test]
    fn remove_deletes_only_the_project_folder() {
        let e = env();
        add(&e.home, "one", &e.repo, "rust", None, &ok).expect("add");
        add(&e.home, "two", &other_repo(&e, "r2"), "rust", None, &ok).expect("add");
        remove(&e.home, "one").expect("remove");
        assert!(!e.home.project_dir("one").exists());
        assert!(e.home.project_dir("two").exists());
        assert!(e.home.presets_dir().join("rust/config.toml").exists());
        assert!(e.repo.exists());
        assert!(remove(&e.home, "one").is_err());
        assert!(remove(&e.home, "../presets").is_err());
        assert!(e.home.presets_dir().exists());
    }

    #[test]
    fn layout_points_inside_the_project_folder() {
        let e = env();
        add(&e.home, "p", &e.repo, "rust", None, &ok).expect("add");
        let f = get(&e.home, "p").expect("get");
        let l = layout(&e.home, "p", &f).expect("layout");
        let dir = e.home.project_dir("p");
        assert_eq!(l.repo, f.repo);
        assert_eq!(l.state_dir, dir.join("state"));
        assert_eq!(l.jobs_dir, dir.join("state/jobs"));
        assert_eq!(l.worktrees_dir, dir.join("worktrees"));
        assert_eq!(l.config_file, dir.join("project.toml"));
        assert_eq!(
            l.commit_context_file,
            e.home.presets_dir().join("rust/commit-context.md")
        );
        std::fs::write(dir.join("commit-context.md"), "mine\n").expect("write");
        let l = layout(&e.home, "p", &f).expect("layout");
        assert_eq!(l.commit_context_file, dir.join("commit-context.md"));
    }

    #[test]
    fn add_stores_subpath_and_old_files_load() {
        let e = env();
        add_sub(&e.home, "web", &e.repo, "apps/web").expect("add");
        let f = get(&e.home, "web").expect("get");
        assert_eq!(f.subpath.as_deref(), Some(Path::new("apps/web")));
        assert_eq!(f.folder(), f.repo.join("apps/web"));
        let old = e.home.project_dir("web").join("project.toml");
        std::fs::write(&old, "repo = \"/abs\"\npreset = \"rust\"\n").expect("write");
        let f = get(&e.home, "web").expect("old file");
        assert_eq!(f.subpath, None);
        assert_eq!(f.folder(), PathBuf::from("/abs"));
    }

    #[test]
    fn subpath_must_be_relative_and_clean() {
        let e = env();
        for bad in ["/abs", "../x", "a/../b", ""] {
            let err = add_sub(&e.home, "web", &e.repo, bad).expect_err(bad);
            assert!(matches!(err, CoreError::Config(_)), "{bad:?}: {err}");
            assert!(!e.home.project_dir("web").exists(), "{bad:?}");
        }
        add_sub(&e.home, "web", &e.repo, "apps/web").expect("add");
        let f = e.home.project_dir("web").join("project.toml");
        for bad in ["/abs", "../x", "a/../b", ""] {
            let text = format!("repo = \"/abs\"\npreset = \"rust\"\nsubpath = \"{bad}\"\n");
            std::fs::write(&f, text).expect("write");
            let err = get(&e.home, "web").expect_err(bad);
            assert!(matches!(err, CoreError::Config(_)), "{bad:?}: {err}");
            assert!(read_file(&f).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn overlap_is_refused() {
        let e = env();
        add_sub(&e.home, "web", &e.repo, "apps/web").expect("add");
        for (name, sub) in [("inner", "apps/web/inner"), ("again", "apps/web")] {
            let err = add_sub(&e.home, name, &e.repo, sub).expect_err(sub);
            assert!(err.to_string().contains("web"), "{err}");
            assert!(!e.home.project_dir(name).exists(), "{name}");
        }
        let err = add(&e.home, "top", &e.repo, "rust", None, &ok).expect_err("top");
        assert!(err.to_string().contains("web"), "{err}");
        assert!(!e.home.project_dir("top").exists());
        add_sub(&e.home, "web2", &e.repo, "apps/web2").expect("sibling prefix");
        add_sub(&e.home, "api", &e.repo, "apps/api").expect("sibling");

        let e = env();
        add(&e.home, "top", &e.repo, "rust", None, &ok).expect("add");
        let err = add_sub(&e.home, "web", &e.repo, "apps/web").expect_err("under top");
        assert!(err.to_string().contains("top"), "{err}");
    }

    #[test]
    fn find_by_path_picks_deepest_project() {
        let e = env();
        add_sub(&e.home, "web", &e.repo, "apps/web").expect("add");
        add_sub(&e.home, "web2", &e.repo, "apps/web2").expect("add");
        for d in ["apps/web/src", "apps/web2/x"] {
            std::fs::create_dir_all(e.repo.join(d)).expect("mkdir");
        }
        let which = |p: &Path| find_by_path(&e.home, p).expect("find");
        assert_eq!(which(&e.repo.join("apps/web/src")).as_deref(), Some("web"));
        assert_eq!(which(&e.repo.join("apps/web2/x")).as_deref(), Some("web2"));
        assert_eq!(which(&e.repo), None);
        assert_eq!(which(&e.repo.join("apps")), None);
    }

    #[test]
    fn find_by_path_prefers_a_subfolder_project_over_the_top_one() {
        let e = env();
        add(&e.home, "top", &e.repo, "rust", None, &ok).expect("add");
        let dir = e.home.project_dir("web");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let text = format!(
            "repo = \"{}\"\npreset = \"rust\"\nsubpath = \"apps/web\"\n",
            e.repo.display()
        );
        std::fs::write(dir.join("project.toml"), text).expect("write");
        std::fs::create_dir_all(e.repo.join("apps/web/src")).expect("mkdir");
        assert_eq!(
            find_by_path(&e.home, &e.repo.join("apps/web/src"))
                .expect("deep")
                .as_deref(),
            Some("web")
        );
        assert_eq!(
            find_by_path(&e.home, &e.repo).expect("root").as_deref(),
            Some("top")
        );
    }

    #[test]
    fn layout_carries_scope() {
        let e = env();
        add_sub(&e.home, "web", &e.repo, "apps/web").expect("add");
        let f = get(&e.home, "web").expect("get");
        let l = layout(&e.home, "web", &f).expect("layout");
        assert_eq!(
            l.scope,
            Some(Scope {
                project: "web".into(),
                subpath: PathBuf::from("apps/web"),
            })
        );
        let e = env();
        add(&e.home, "top", &e.repo, "rust", None, &ok).expect("add");
        let f = get(&e.home, "top").expect("get");
        assert_eq!(layout(&e.home, "top", &f).expect("layout").scope, None);
    }

    #[test]
    fn two_adds_of_one_name_leave_exactly_one_winner() {
        for _ in 0..20 {
            let e = env();
            let (r1, r2) = (e.repo.clone(), other_repo(&e, "r2"));
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let run = |repo: PathBuf| {
                let (home, gate) = (e.home.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    add(&home, "same", &repo, "rust", None, &ok).map_err(|e| e.to_string())
                })
            };
            let (a, b) = (run(r1.clone()), run(r2.clone()));
            let results = [(a.join().expect("a"), r1), (b.join().expect("b"), r2)];
            let wins: Vec<_> = results.iter().filter(|(r, _)| r.is_ok()).collect();
            assert_eq!(wins.len(), 1, "{results:?}");
            let lost = results.iter().find(|(r, _)| r.is_err()).expect("loser");
            assert!(
                lost.0.as_ref().expect_err("err").contains("already exists"),
                "{lost:?}"
            );
            let f = get(&e.home, "same").expect("winner project.toml intact");
            assert_eq!(f.repo, std::fs::canonicalize(&wins[0].1).expect("canon"));
            for sub in ["agents", "state", "worktrees"] {
                assert!(e.home.project_dir("same").join(sub).is_dir());
            }
        }
    }

    #[test]
    fn a_failure_after_the_claim_removes_only_our_folder() {
        let e = env();
        std::fs::create_dir_all(e.home.project_dir("other")).expect("other project");
        // A file where the presets folder belongs makes seeding fail.
        std::fs::write(e.home.presets_dir(), "not a folder").expect("blocker");
        let err = add(&e.home, "p", &e.repo, "rust", None, &ok).expect_err("seed must fail");
        assert!(!err.to_string().contains("already exists"), "{err}");
        assert!(!e.home.project_dir("p").exists(), "our folder is removed");
        assert!(
            e.home.project_dir("other").is_dir(),
            "another project is untouched"
        );
    }

    #[test]
    fn base_branch_is_validated_like_config_parse() {
        assert_eq!(valid_base_branch("v-4.0.0").expect("ok"), "v-4.0.0");
        for bad in ["bad branch", "", "-x", "a..b"] {
            assert!(valid_base_branch(bad).is_err(), "{bad:?}");
        }
        let e = env();
        add(&e.home, "p", &e.repo, "rust", Some("dev"), &ok).expect("add");
        let mut f = get(&e.home, "p").expect("get");
        assert_eq!(f.base_branch().as_deref(), Some("dev"));
        f.settings.insert(
            "base_branch".into(),
            toml::Value::String("bad branch".into()),
        );
        assert_eq!(f.base_branch(), None);
        f.settings
            .insert("base_branch".into(), toml::Value::Integer(3));
        assert_eq!(f.base_branch(), None);
        f.settings.clear();
        assert_eq!(f.base_branch(), None);
    }

    #[test]
    fn layout_refuses_a_preset_name_that_could_leave_the_home() {
        let e = env();
        for bad in ["../x", "a/b", "..", ""] {
            let f = ProjectFile {
                repo: e.repo.clone(),
                subpath: None,
                preset: bad.to_string(),
                settings: toml::Table::new(),
            };
            assert!(layout(&e.home, "p", &f).is_err(), "{bad:?}");
            let safe = build_layout(&e.home, "p", &f, false);
            assert!(safe.commit_context_file.starts_with(&e.home.dir), "{bad:?}");
            assert!(
                !safe.commit_context_file.starts_with(e.home.presets_dir()),
                "{bad:?}"
            );
        }
    }
}
