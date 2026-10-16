//! The factory home: presets and one folder per registered project.

use std::path::PathBuf;

use crate::CoreError;

const CONFIG_FILE: &str = "config.toml";
const QUEUE_DIR: &str = "queue";
const RESULTS_DIR: &str = "results";
const RUN_DIR: &str = "run";
const LOGS_DIR: &str = "logs";

/// The folder that holds `presets/` and `projects/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    pub fn at(dir: PathBuf) -> Home {
        Home { dir }
    }

    /// `FACTORY_HOME` when set and not empty, else `$HOME/.factory`.
    pub fn from_env() -> Result<Home, CoreError> {
        from_values(std::env::var("FACTORY_HOME").ok().as_deref(), std::env::var("HOME").ok().as_deref())
    }

    pub fn presets_dir(&self) -> PathBuf {
        self.dir.join("presets")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.dir.join("projects")
    }

    /// The home-wide settings file.
    pub fn config_file(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    /// Where requests wait for the supervisor.
    pub fn queue_dir(&self) -> PathBuf {
        self.dir.join(QUEUE_DIR)
    }

    /// One result record per finished request.
    pub fn results_dir(&self) -> PathBuf {
        self.dir.join(RESULTS_DIR)
    }

    /// Runtime records of the supervisor.
    pub fn run_dir(&self) -> PathBuf {
        self.dir.join(RUN_DIR)
    }

    /// The log folder of one project. The name must already be validated.
    pub fn logs_dir(&self, project: &str) -> PathBuf {
        self.dir.join(LOGS_DIR).join(project)
    }

    /// The folder of one project. The name must already be validated.
    pub fn project_dir(&self, name: &str) -> PathBuf {
        self.projects_dir().join(name)
    }
}

fn from_values(factory_home: Option<&str>, home: Option<&str>) -> Result<Home, CoreError> {
    if let Some(dir) = factory_home.filter(|d| !d.is_empty()) {
        return Ok(Home::at(PathBuf::from(dir)));
    }
    match home.filter(|h| !h.is_empty()) {
        Some(h) => Ok(Home::at(PathBuf::from(h).join(".factory"))),
        None => Err(CoreError::Config("neither FACTORY_HOME nor HOME is set".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvVarGuard;

    #[test]
    fn at_gives_the_folders() {
        let h = Home::at(PathBuf::from("/h"));
        assert_eq!(h.presets_dir(), PathBuf::from("/h/presets"));
        assert_eq!(h.projects_dir(), PathBuf::from("/h/projects"));
        assert_eq!(h.project_dir("rocket"), PathBuf::from("/h/projects/rocket"));
    }

    #[test]
    fn supervisor_names_are_under_the_home() {
        let h = Home::at(PathBuf::from("/h"));
        assert_eq!(h.config_file(), PathBuf::from("/h/config.toml"));
        assert_eq!(h.queue_dir(), PathBuf::from("/h/queue"));
        assert_eq!(h.results_dir(), PathBuf::from("/h/results"));
        assert_eq!(h.run_dir(), PathBuf::from("/h/run"));
        assert_eq!(h.logs_dir("p"), PathBuf::from("/h/logs/p"));
    }

    #[test]
    fn values_pick_factory_home_then_home_then_fail() {
        assert_eq!(from_values(Some("/f"), Some("/u")).expect("ok").dir, PathBuf::from("/f"));
        assert_eq!(from_values(None, Some("/u")).expect("ok").dir, PathBuf::from("/u/.factory"));
        assert_eq!(from_values(Some(""), Some("/u")).expect("ok").dir, PathBuf::from("/u/.factory"));
        assert!(from_values(None, None).is_err());
        assert!(from_values(Some(""), Some("")).is_err());
    }

    #[test]
    fn from_env_honors_factory_home() {
        let got = {
            let _guard = EnvVarGuard::set("FACTORY_HOME", "/tmp/some-factory-home");
            Home::from_env()
        };
        assert_eq!(got.expect("home").dir, PathBuf::from("/tmp/some-factory-home"));
    }
}
