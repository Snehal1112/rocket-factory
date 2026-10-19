//! Reads the driver's `costs.json` for the dashboard. The server never writes it.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

/// Median and range over the jobs with exact output counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpreadView {
    pub n: u32,
    pub median: u64,
    pub low: u64,
    pub high: u64,
}

/// The figures of one size group.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupView {
    pub n: u32,
    pub n_approx: u32,
    pub median: u64,
    pub low: u64,
    pub high: u64,
    pub roles: BTreeMap<String, u32>,
    pub corrected: Option<SpreadView>,
}

/// What the state carries: the group figures and the role split, never the per-job list.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CostsView {
    pub generated_at: f64,
    pub groups: BTreeMap<String, GroupView>,
    pub roles: BTreeMap<String, u32>,
}

/// `None` when the file is missing or unreadable.
pub fn load_costs(paths: &Paths) -> Option<CostsView> {
    let text = std::fs::read_to_string(&paths.costs).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, text: &str) {
        let dir = root.join(".superpowers").join("factory");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("costs.json"), text).expect("costs.json");
    }

    #[test]
    fn a_valid_file_gives_groups_and_roles_and_drops_unknown_fields() {
        let root = tempfile::tempdir().expect("tempdir");
        write(
            root.path(),
            r#"{"generated_at":5.0,"thresholds":{"small_max_bytes":1,"medium_max_bytes":2},"secret":"x","jobs":[{"id":"001"}],
                "groups":{"small":{"n":4,"n_approx":4,"median":280000,"low":220000,"high":370000,"roles":{"builder":62},"corrected":null,"extra":1}},
                "roles":{"builder":62,"reviewer":38}}"#,
        );
        let v = load_costs(&Paths::for_root(root.path())).expect("costs");
        assert_eq!(v.groups["small"].median, 280_000);
        assert_eq!(v.roles["reviewer"], 38);
        let json = serde_json::to_string(&v).expect("json");
        assert!(
            !json.contains("secret") && !json.contains("\"jobs\"") && !json.contains("extra"),
            "{json}"
        );
    }

    #[test]
    fn a_missing_or_broken_file_gives_none() {
        let root = tempfile::tempdir().expect("tempdir");
        assert!(load_costs(&Paths::for_root(root.path())).is_none());
        write(root.path(), "not json");
        assert!(load_costs(&Paths::for_root(root.path())).is_none());
        write(root.path(), r#"{"groups":"nope"}"#);
        assert!(load_costs(&Paths::for_root(root.path())).is_none());
    }

    #[test]
    fn corrected_figures_are_kept_when_present() {
        let root = tempfile::tempdir().expect("tempdir");
        write(
            root.path(),
            r#"{"groups":{"small":{"n":3,"n_approx":1,"median":3,"low":1,"high":5,"roles":{},"corrected":{"n":2,"median":4,"low":3,"high":5}}},"roles":{}}"#,
        );
        let v = load_costs(&Paths::for_root(root.path())).expect("costs");
        assert_eq!(
            v.groups["small"].corrected.as_ref().map(|c| c.median),
            Some(4)
        );
    }
}
