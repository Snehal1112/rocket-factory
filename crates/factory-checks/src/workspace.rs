//! The cargo workspace map: which package owns a path and which packages depend on which.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::CheckError;

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    /// The package folder relative to the repo root, with `/`. Empty for a root package.
    pub dir: String,
    /// Names of the workspace members this package depends on by path.
    pub deps: Vec<String>,
}

/// The members of a cargo workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    pub packages: Vec<Package>,
}

impl Workspace {
    /// Builds the map from `cargo metadata --no-deps` output. `root` is the repo root.
    pub fn from_metadata(json: &str, root: &Path) -> Result<Workspace, CheckError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| CheckError::Workspace(e.to_string()))?;
        let list = value["packages"]
            .as_array()
            .ok_or_else(|| CheckError::Workspace("no packages in the metadata".into()))?;
        let names: BTreeSet<&str> = list.iter().filter_map(|p| p["name"].as_str()).collect();
        let real_root = root.canonicalize().ok();
        let mut packages = Vec::new();
        for p in list {
            let (Some(name), Some(manifest)) = (p["name"].as_str(), p["manifest_path"].as_str())
            else {
                continue;
            };
            let folder = Path::new(manifest).parent().unwrap_or(Path::new(""));
            let rel = folder
                .strip_prefix(root)
                .ok()
                .or_else(|| {
                    real_root
                        .as_deref()
                        .and_then(|r| folder.strip_prefix(r).ok())
                })
                .ok_or_else(|| {
                    CheckError::Workspace(format!("package {name} is outside the repo"))
                })?;
            let dir = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let mut deps: Vec<String> = p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["path"].is_string())
                .filter_map(|d| d["name"].as_str())
                .filter(|n| names.contains(n) && *n != name)
                .map(str::to_string)
                .collect();
            deps.sort();
            deps.dedup();
            packages.push(Package {
                name: name.to_string(),
                dir,
                deps,
            });
        }
        Ok(Workspace { packages })
    }

    /// Runs `cargo metadata --no-deps --format-version 1` in `root` and builds the map.
    pub fn load(root: &Path) -> Result<Workspace, CheckError> {
        let out = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(root)
            .output()
            .map_err(|e| CheckError::Workspace(format!("cargo metadata did not start: {e}")))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.lines().next().unwrap_or("no message");
            return Err(CheckError::Workspace(format!(
                "cargo metadata failed: {first}"
            )));
        }
        Workspace::from_metadata(&String::from_utf8_lossy(&out.stdout), root)
    }

    /// The package that owns `path`: the one with the longest matching folder.
    pub fn owner(&self, path: &str) -> Option<&str> {
        self.packages
            .iter()
            .filter(|p| {
                p.dir.is_empty()
                    || path
                        .strip_prefix(p.dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|p| p.dir.len())
            .map(|p| p.name.as_str())
    }

    /// The packages that depend directly on any of `names`, minus `names` themselves, sorted.
    pub fn dependents(&self, names: &BTreeSet<String>) -> Vec<String> {
        let mut found: Vec<String> = self
            .packages
            .iter()
            .filter(|p| !names.contains(&p.name) && p.deps.iter().any(|d| names.contains(d)))
            .map(|p| p.name.clone())
            .collect();
        found.sort();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"packages":[
      {"name":"core","manifest_path":"/r/crates/core/Cargo.toml","dependencies":[
        {"name":"serde","path":null}]},
      {"name":"app","manifest_path":"/r/crates/app/Cargo.toml","dependencies":[
        {"name":"core","path":"/r/crates/core"}]},
      {"name":"rocket","manifest_path":"/r/src-tauri/Cargo.toml","dependencies":[
        {"name":"app","path":"/r/crates/app"},{"name":"core","path":"/r/crates/core"}]}
    ]}"#;

    fn ws() -> Workspace {
        Workspace::from_metadata(META, Path::new("/r")).expect("parse")
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn owner_follows_the_longest_prefix() {
        let ws = ws();
        assert_eq!(ws.owner("src-tauri/src/lib.rs"), Some("rocket"));
        assert_eq!(ws.owner("crates/app/src/x.rs"), Some("app"));
        assert_eq!(ws.owner("README.md"), None);
    }

    #[test]
    fn dependents_are_direct_only() {
        let ws = ws();
        assert_eq!(ws.dependents(&set(&["core"])), ["app", "rocket"]);
        assert_eq!(ws.dependents(&set(&["app"])), ["rocket"]);
    }

    #[test]
    fn a_root_package_owns_only_what_no_member_owns() {
        let json = r#"{"packages":[
          {"name":"top","manifest_path":"/r/Cargo.toml","dependencies":[]},
          {"name":"inner","manifest_path":"/r/crates/inner/Cargo.toml","dependencies":[]}]}"#;
        let ws = Workspace::from_metadata(json, Path::new("/r")).expect("parse");
        assert_eq!(ws.owner("src/main.rs"), Some("top"));
        assert_eq!(ws.owner("crates/inner/src/lib.rs"), Some("inner"));
    }

    #[test]
    fn bad_json_is_an_error() {
        assert!(Workspace::from_metadata("nope", Path::new("/r")).is_err());
    }

    #[test]
    fn load_reads_a_real_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let files = [
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/core\", \"crates/app\"]\n",
            ),
            (
                "crates/core/Cargo.toml",
                "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("crates/core/src/lib.rs", "pub fn f() {}\n"),
            (
                "crates/app/Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ncore = { path = \"../core\" }\n",
            ),
            ("crates/app/src/lib.rs", "pub fn g() {}\n"),
        ];
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, body).expect("write");
        }
        let ws = Workspace::load(dir.path()).expect("load");
        assert_eq!(ws.owner("crates/core/src/lib.rs"), Some("core"));
        assert_eq!(ws.owner("crates/app/src/lib.rs"), Some("app"));
        assert_eq!(ws.dependents(&set(&["core"])), ["app"]);
    }

    #[test]
    fn load_fails_without_a_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = Workspace::load(dir.path()).expect_err("no manifest");
        assert!(
            err.to_string()
                .starts_with("workspace map: cargo metadata failed: "),
            "{err}"
        );
    }
}
