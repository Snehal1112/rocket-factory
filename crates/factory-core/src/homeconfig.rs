//! The home-wide settings in `config.toml`.

use std::net::SocketAddr;

use serde::Deserialize;

use crate::{CoreError, Home};

const DEFAULT_MAX_PARALLEL: u32 = 2;
const MAX_PARALLEL_CAP: u32 = 64;
const DEFAULT_DRAFT_STALE_HOURS: u64 = 24;
const DEFAULT_LISTEN: &str = "127.0.0.1:8766";

/// The contents of `<home>/config.toml`. A missing file is the default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HomeConfig {
    /// How many projects the supervisor runs at once. 1 to 64.
    pub max_parallel: u32,
    /// Where the local API listens. Must be a loopback address, port 0 picks a free port.
    pub listen: String,
    /// The most weighted tokens all projects may use in one UTC day before new runs wait. Absent means no limit.
    pub daily_budget_weighted: Option<u64>,
    /// Hours without a change before an interview draft shows as stale. Greater than 0.
    pub draft_stale_hours: u64,
}

impl Default for HomeConfig {
    fn default() -> Self {
        HomeConfig { max_parallel: DEFAULT_MAX_PARALLEL, listen: DEFAULT_LISTEN.to_string(), daily_budget_weighted: None, draft_stale_hours: DEFAULT_DRAFT_STALE_HOURS }
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
        if cfg.daily_budget_weighted == Some(0) {
            return Err(CoreError::Config("bad home config: daily_budget_weighted must be greater than 0".to_string()));
        }
        if cfg.draft_stale_hours == 0 {
            return Err(CoreError::Config("bad home config: draft_stale_hours must be greater than 0".to_string()));
        }
        match cfg.listen.parse::<SocketAddr>() {
            Ok(addr) if addr.ip().is_loopback() => {}
            _ => {
                return Err(CoreError::Config(format!(
                    "bad home config: listen must be a loopback address like {DEFAULT_LISTEN}"
                )))
            }
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
    fn listen_defaults_and_accepts_loopback_only() {
        assert_eq!(load_text(None).expect("ok").listen, "127.0.0.1:8766");
        assert_eq!(load_text(Some("listen = \"127.0.0.1:0\"\n")).expect("ok").listen, "127.0.0.1:0");
        assert!(load_text(Some("listen = \"0.0.0.0:8766\"\n")).is_err());
        assert!(load_text(Some("listen = \"192.168.1.5:80\"\n")).is_err());
        assert!(load_text(Some("listen = \"nonsense\"\n")).is_err());
    }

    #[test]
    fn daily_budget_parses_and_defaults_to_none() {
        assert_eq!(load_text(None).expect("ok").daily_budget_weighted, None);
        assert_eq!(load_text(Some("")).expect("ok").daily_budget_weighted, None);
        assert_eq!(load_text(Some("daily_budget_weighted = 5000000\n")).expect("ok").daily_budget_weighted, Some(5_000_000));
    }

    #[test]
    fn a_bad_daily_budget_is_an_error() {
        for bad in ["0", "-1", "\"x\"", "1.5", "true"] {
            assert!(load_text(Some(&format!("daily_budget_weighted = {bad}\n"))).is_err(), "{bad}");
        }
    }

    #[test]
    fn draft_stale_hours_defaults_to_24_and_rejects_zero() {
        assert_eq!(load_text(None).expect("ok").draft_stale_hours, 24);
        assert_eq!(load_text(Some("")).expect("ok").draft_stale_hours, 24);
        assert_eq!(load_text(Some("draft_stale_hours = 6\n")).expect("ok").draft_stale_hours, 6);
        for bad in ["0", "-1", "\"x\"", "1.5"] {
            assert!(load_text(Some(&format!("draft_stale_hours = {bad}\n"))).is_err(), "{bad}");
        }
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
