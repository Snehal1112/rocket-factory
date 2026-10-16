//! The project registry: one folder per project under the factory home.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{preset, Config, CoreError, Home, Layout};

const MAX_NAME: usize = 40;
const PROJECT_FILE: &str = "project.toml";

/// Lowercase letters, digits and `-`, starting with a letter or digit, at most 40 characters.
/// This keeps a name from escaping the home or naming a hidden folder.
pub fn validate_name(name: &str) -> Result<(), CoreError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(CoreError::Config(format!(
            "project name {name:?} must be 1 to {MAX_NAME} characters of a-z, 0-9 and -, starting with a letter or digit"
        )))
    }
}

/// The contents of `projects/<name>/project.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The main checkout. Must be an absolute path.
    pub repo: PathBuf,
    pub preset: String,
    /// Config keys that replace the preset's.
    #[serde(default)]
    pub settings: toml::Table,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectEntry {
    pub name: String,
    pub file: Result<ProjectFile, String>,
}

fn read_file(path: &Path) -> Result<ProjectFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let file: ProjectFile = toml::from_str(&text).map_err(|e| format!("bad {}: {e}", path.display()))?;
    if !file.repo.is_absolute() {
        return Err(format!("bad {}: repo must be an absolute path", path.display()));
    }
    Ok(file)
}

/// Registers a project. Every check runs before anything is created.
pub fn add(
    home: &Home,
    name: &str,
    repo: &Path,
    preset_name: &str,
    base: Option<&str>,
    repo_ok: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, CoreError> {
    validate_name(name)?;
    if preset::embedded(preset_name).is_none() {
        return Err(CoreError::Config(format!(
            "unknown preset {preset_name:?}, choose one of {}",
            preset::PRESET_NAMES.join(", ")
        )));
    }
    if !repo.is_absolute() {
        return Err(CoreError::Config(format!("repo path {} must be absolute", repo.display())));
    }
    let canon = std::fs::canonicalize(repo)
        .map_err(|e| CoreError::Config(format!("repo path {} is not usable: {e}", repo.display())))?;
    if !canon.is_dir() {
        return Err(CoreError::Config(format!("repo path {} is not a folder", canon.display())));
    }
    repo_ok(&canon).map_err(|why| CoreError::Config(format!("{}: {why}", canon.display())))?;
    let mut settings = toml::Table::new();
    if let Some(b) = base {
        let mut probe = toml::Table::new();
        probe.insert("base_branch".into(), toml::Value::String(b.to_string()));
        let text = toml::to_string(&probe).map_err(|e| CoreError::Config(e.to_string()))?;
        Config::parse(&text)?;
        settings.insert("base_branch".into(), toml::Value::String(b.to_string()));
    }
    let dir = home.project_dir(name);
    if dir.exists() {
        return Err(CoreError::Config(format!("project {name} already exists")));
    }
    for entry in list(home)? {
        if let Ok(f) = entry.file {
            if std::fs::canonicalize(&f.repo).is_ok_and(|c| c == canon) {
                return Err(CoreError::Config(format!("repo {} is already project {}", canon.display(), entry.name)));
            }
        }
    }
    let file = ProjectFile { repo: canon, preset: preset_name.to_string(), settings };
    let text = toml::to_string(&file).map_err(|e| CoreError::Config(e.to_string()))?;
    preset::seed(&home.presets_dir())?;
    let made = (|| -> Result<(), CoreError> {
        for sub in ["agents", "state", "worktrees"] {
            let p = dir.join(sub);
            std::fs::create_dir_all(&p).map_err(|e| CoreError::io(&p, e))?;
        }
        let f = dir.join(PROJECT_FILE);
        std::fs::write(&f, text).map_err(|e| CoreError::io(&f, e))
    })();
    if let Err(e) = made {
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
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        out.push(ProjectEntry { file: read_file(&path.join(PROJECT_FILE)), name });
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

/// Deletes `projects/<name>/` and nothing else.
pub fn remove(home: &Home, name: &str) -> Result<(), CoreError> {
    validate_name(name)?;
    let dir = home.project_dir(name);
    if !dir.is_dir() {
        return Err(CoreError::Config(format!("no project named {name}")));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))
}

pub fn layout(home: &Home, name: &str, file: &ProjectFile) -> Layout {
    let dir = home.project_dir(name);
    let own = dir.join("commit-context.md");
    let commit_context_file =
        if own.exists() { own } else { home.presets_dir().join(&file.preset).join("commit-context.md") };
    let state_dir = dir.join("state");
    Layout {
        repo: file.repo.clone(),
        jobs_dir: state_dir.join("jobs"),
        state_dir,
        worktrees_dir: dir.join("worktrees"),
        config_file: dir.join(PROJECT_FILE),
        commit_context_file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ok(_: &Path) -> Result<(), String> {
        Ok(())
    }

    struct Env {
        _t: TempDir,
        home: Home,
        repo: PathBuf,
    }

    fn env() -> Env {
        let t = TempDir::new().expect("tempdir");
        let repo = t.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let home = Home::at(t.path().join("home"));
        Env { _t: t, home, repo }
    }

    fn other_repo(e: &Env, n: &str) -> PathBuf {
        let p = e._t.path().join(n);
        std::fs::create_dir_all(&p).expect("mkdir");
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
        assert_eq!(f.settings.get("base_branch").and_then(|v| v.as_str()), Some("v-4.0.0"));
    }

    #[test]
    fn add_without_base_stores_no_settings() {
        let e = env();
        add(&e.home, "rocket", &e.repo, "go", None, &ok).expect("add");
        assert!(get(&e.home, "rocket").expect("get").settings.is_empty());
    }

    fn assert_refused(e: &Env, r: Result<PathBuf, CoreError>, name: &str) {
        assert!(r.is_err());
        assert!(!e.home.project_dir(name).exists(), "folder left behind");
    }

    #[test]
    fn add_refuses_bad_input_and_leaves_nothing_behind() {
        let e = env();
        assert_refused(&e, add(&e.home, "p", &e.repo, "nope", None, &ok), "p");
        assert_refused(&e, add(&e.home, "p", Path::new("rel/repo"), "rust", None, &ok), "p");
        assert_refused(&e, add(&e.home, "p", &e.repo.join("missing"), "rust", None, &ok), "p");
        let no = |_: &Path| Err("not a git repo".to_string());
        assert_refused(&e, add(&e.home, "p", &e.repo, "rust", None, &no), "p");
        assert_refused(&e, add(&e.home, "p", &e.repo, "rust", Some("bad branch"), &ok), "p");
        assert_refused(&e, add(&e.home, "../p", &e.repo, "rust", None, &ok), "p");
        assert!(!e.home.dir.exists(), "refusals must not even create the home");
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
        std::fs::write(e.home.project_dir("mid").join("project.toml"), "this is not = = toml").expect("write");
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
        let l = layout(&e.home, "p", &f);
        let dir = e.home.project_dir("p");
        assert_eq!(l.repo, f.repo);
        assert_eq!(l.state_dir, dir.join("state"));
        assert_eq!(l.jobs_dir, dir.join("state/jobs"));
        assert_eq!(l.worktrees_dir, dir.join("worktrees"));
        assert_eq!(l.config_file, dir.join("project.toml"));
        assert_eq!(l.commit_context_file, e.home.presets_dir().join("rust/commit-context.md"));
        std::fs::write(dir.join("commit-context.md"), "mine\n").expect("write");
        let l = layout(&e.home, "p", &f);
        assert_eq!(l.commit_context_file, dir.join("commit-context.md"));
    }
}
