//! The home-wide settings in `config.toml`.

use serde::Deserialize;

use crate::{CoreError, Home};

const DEFAULT_MAX_PARALLEL: u32 = 2;
const MAX_PARALLEL_CAP: u32 = 64;

/// The contents of `<home>/config.toml`. A missing file is the default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HomeConfig {
    /// How many projects the supervisor runs at once. 1 to 64.
    pub max_parallel: u32,
}

impl Default for HomeConfig {
    fn default() -> Self {
        HomeConfig { max_parallel: DEFAULT_MAX_PARALLEL }
    }
}

impl HomeConfig {
    /// Parses and validates the text of a config file.
    pub fn parse(text: &str) -> Result<HomeConfig, CoreError> {
        let cfg: HomeConfig = toml::from_str(text).map_err(|e| CoreError::Config(format!("bad home config: {e}")))?;
        if !(1..=MAX_PARALLEL_CAP).contains(&cfg.max_parallel) {
            return Err(CoreError::Config(format!(
                "bad home config: max_parallel must be 1 to {MAX_PARALLEL_CAP}"
            )));
        }
        Ok(cfg)
    }

    /// Reads `<home>/config.toml`. A missing file gives the default, an invalid one is an error.
    pub fn load(home: &Home) -> Result<HomeConfig, CoreError> {
        let path = home.config_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => HomeConfig::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HomeConfig::default()),
            Err(e) => Err(CoreError::io(&path, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_text(text: Option<&str>) -> Result<HomeConfig, CoreError> {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().to_path_buf());
        if let Some(text) = text {
            std::fs::write(home.config_file(), text).expect("write");
        }
        HomeConfig::load(&home)
    }

    #[test]
    fn a_missing_file_is_the_default() {
        assert_eq!(load_text(None).expect("ok").max_parallel, 2);
    }

    #[test]
    fn an_empty_file_is_the_default() {
        assert_eq!(load_text(Some("")).expect("ok").max_parallel, 2);
    }

    #[test]
    fn max_parallel_parses() {
        assert_eq!(load_text(Some("max_parallel = 4\n")).expect("ok").max_parallel, 4);
    }

    #[test]
    fn zero_unknown_keys_and_bad_toml_are_errors() {
        assert!(load_text(Some("max_parallel = 0\n")).is_err());
        assert!(load_text(Some("max_parallel = -1\n")).is_err());
        assert!(load_text(Some("nope = 1\n")).is_err());
        assert!(load_text(Some("max_parallel = = 1")).is_err());
    }

    #[test]
    fn the_cap_is_64() {
        assert_eq!(load_text(Some("max_parallel = 64\n")).expect("ok").max_parallel, 64);
        assert!(load_text(Some("max_parallel = 65\n")).is_err());
    }

    #[test]
    fn a_directory_at_the_config_path_is_an_error() {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().to_path_buf());
        std::fs::create_dir(home.config_file()).expect("mkdir");
        assert!(HomeConfig::load(&home).is_err());
    }
}
