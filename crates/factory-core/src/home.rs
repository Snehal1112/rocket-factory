//! The factory home: presets and one folder per registered project.

use std::path::PathBuf;

use crate::CoreError;

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
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn at_gives_the_folders() {
        let h = Home::at(PathBuf::from("/h"));
        assert_eq!(h.presets_dir(), PathBuf::from("/h/presets"));
        assert_eq!(h.projects_dir(), PathBuf::from("/h/projects"));
        assert_eq!(h.project_dir("rocket"), PathBuf::from("/h/projects/rocket"));
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
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var("FACTORY_HOME").ok();
        std::env::set_var("FACTORY_HOME", "/tmp/some-factory-home");
        let got = Home::from_env();
        match old {
            Some(v) => std::env::set_var("FACTORY_HOME", v),
            None => std::env::remove_var("FACTORY_HOME"),
        }
        assert_eq!(got.expect("home").dir, PathBuf::from("/tmp/some-factory-home"));
    }
}
