//! Pure rules that compare Cargo locks and manifests to see if outside code changed.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The result of comparing two `Cargo.lock` texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockDiff {
    /// No outside crate was added, removed or changed.
    Same,
    /// Outside crates differ; each list holds sorted `name version` strings.
    /// A crate in both differences (new checksum or source) is only in `changed`.
    Changed {
        added: Vec<String>,
        removed: Vec<String>,
        changed: Vec<String>,
    },
    /// The lock could not be read, so nothing can be said.
    Unknown(String),
}

/// A verdict on one Cargo file or package set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoVerdict {
    Same,
    Changed(String),
    Unknown(String),
}

/// One cargo target of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTarget {
    pub kinds: Vec<String>,
    pub src_path: PathBuf,
}

/// One path package from `cargo metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPackage {
    pub manifest_path: PathBuf,
    pub targets: Vec<CargoTarget>,
}

/// One outside lock entry: name, version, source and checksum.
type Entry = (String, String, String, String);

/// Reads the outside entries (those with a `source`) of one lock text.
fn lock_entries(text: &str, side: &str) -> Result<BTreeSet<Entry>, String> {
    let value: toml::Value = text
        .parse()
        .map_err(|e| format!("the {side} Cargo.lock does not parse: {e}"))?;
    if let Some(meta) = value.get("metadata").and_then(|m| m.as_table()) {
        if meta.keys().any(|k| k.starts_with("checksum ")) {
            return Err(format!("the {side} Cargo.lock uses the old v1 format"));
        }
    }
    let packages = value
        .get("package")
        .and_then(|p| p.as_array())
        .ok_or_else(|| format!("the {side} Cargo.lock has no packages"))?;
    let mut out = BTreeSet::new();
    for pkg in packages {
        let field = |key: &str| pkg.get(key).and_then(|v| v.as_str());
        let (Some(name), Some(version)) = (field("name"), field("version")) else {
            return Err(format!("a {side} Cargo.lock entry has no name or version"));
        };
        // Path packages have no source and are checked by the metadata step.
        let Some(source) = field("source") else {
            continue;
        };
        let checksum = field("checksum").unwrap_or("");
        out.insert((
            name.to_string(),
            version.to_string(),
            source.to_string(),
            checksum.to_string(),
        ));
    }
    Ok(out)
}

/// Turns entries into sorted, unique `name version` strings.
fn entry_names<'a>(entries: impl Iterator<Item = &'a Entry>) -> Vec<String> {
    let names: BTreeSet<String> = entries.map(|e| format!("{} {}", e.0, e.1)).collect();
    names.into_iter().collect()
}

/// Compares the outside crates of two lock texts.
pub fn compare_locks(base: &str, head: &str) -> LockDiff {
    let base = match lock_entries(base, "base") {
        Ok(set) => set,
        Err(reason) => return LockDiff::Unknown(reason),
    };
    let head = match lock_entries(head, "head") {
        Ok(set) => set,
        Err(reason) => return LockDiff::Unknown(reason),
    };
    let mut added: BTreeSet<String> = entry_names(head.difference(&base)).into_iter().collect();
    let mut removed: BTreeSet<String> = entry_names(base.difference(&head)).into_iter().collect();
    let changed: Vec<String> = added.intersection(&removed).cloned().collect();
    for name in &changed {
        added.remove(name);
        removed.remove(name);
    }
    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        LockDiff::Same
    } else {
        LockDiff::Changed {
            added: added.into_iter().collect(),
            removed: removed.into_iter().collect(),
            changed,
        }
    }
}

/// Manifest keys that can run or pull in code from outside the project.
const RISKY_KEYS: &[&[&str]] = &[
    &["package", "build"],
    &["package", "links"],
    &["lib", "proc-macro"],
    &["lib", "proc_macro"],
    &["cargo-features"],
    &["patch"],
    &["replace"],
];

fn key_value<'a>(root: &'a toml::Value, key: &[&str]) -> Option<&'a toml::Value> {
    key.iter().try_fold(root, |v, k| v.get(*k))
}

/// Compares the risky keys of a manifest; a `base` of `None` is a new manifest.
pub fn manifest_keys_diff(base: Option<&str>, head: &str) -> CargoVerdict {
    let base_value = match base {
        Some(text) => match text.parse::<toml::Value>() {
            Ok(v) => v,
            Err(e) => {
                return CargoVerdict::Unknown(format!("the base manifest does not parse: {e}"))
            }
        },
        None => toml::Value::Table(toml::Table::new()),
    };
    let head_value = match head.parse::<toml::Value>() {
        Ok(v) => v,
        Err(e) => return CargoVerdict::Unknown(format!("the head manifest does not parse: {e}")),
    };
    for key in RISKY_KEYS {
        if key_value(&base_value, key) != key_value(&head_value, key) {
            return CargoVerdict::Changed(format!("{} changed", key.join(".")));
        }
    }
    CargoVerdict::Same
}

/// Checks the path packages of a workspace; all paths are absolute and canonical.
pub fn check_path_packages(
    project: &Path,
    packages: &[PathPackage],
    changed: &BTreeSet<PathBuf>,
    new_manifests: &BTreeSet<PathBuf>,
) -> CargoVerdict {
    for pkg in packages {
        let manifest = pkg.manifest_path.display();
        if !pkg.manifest_path.starts_with(project) {
            return CargoVerdict::Changed(format!("package {manifest} is outside the project"));
        }
        for target in &pkg.targets {
            let src = target.src_path.display();
            if !target.src_path.starts_with(project) {
                return CargoVerdict::Changed(format!("source {src} is outside the project"));
            }
            let has_kind = |kind: &str| target.kinds.iter().any(|k| k == kind);
            if has_kind("custom-build") && changed.contains(&target.src_path) {
                return CargoVerdict::Changed(format!("build script {src} changed"));
            }
            if has_kind("proc-macro") && new_manifests.contains(&pkg.manifest_path) {
                return CargoVerdict::Changed(format!("new proc-macro package {manifest}"));
            }
        }
    }
    CargoVerdict::Same
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(entries: &[&str]) -> String {
        let mut text = String::from("version = 4\n");
        for e in entries {
            text.push_str(&format!("\n[[package]]\n{e}\n"));
        }
        text
    }

    const REG: &str = "registry+https://github.com/rust-lang/crates.io-index";
    const ROOT: &str = "name = \"root\"\nversion = \"0.1.0\"";

    fn libc_a() -> String {
        format!("name = \"libc\"\nversion = \"0.2.190\"\nsource = \"{REG}\"\nchecksum = \"aaaa\"")
    }

    fn libc_b() -> String {
        format!("name = \"libc\"\nversion = \"0.2.191\"\nsource = \"{REG}\"\nchecksum = \"bbbb\"")
    }

    fn hook() -> String {
        format!(
            "name = \"signal-hook\"\nversion = \"0.3.17\"\nsource = \"{REG}\"\nchecksum = \"cccc\""
        )
    }

    #[test]
    fn reordered_packages_are_same() {
        let a = lock(&[ROOT, &libc_a(), &hook()]);
        let b = lock(&[&hook(), ROOT, &libc_a()]);
        assert_eq!(compare_locks(&a, &b), LockDiff::Same);
    }

    #[test]
    fn a_new_registry_crate_is_added() {
        let base = lock(&[ROOT, &libc_a()]);
        let head = lock(&[ROOT, &libc_a(), &hook()]);
        assert_eq!(
            compare_locks(&base, &head),
            LockDiff::Changed {
                added: vec!["signal-hook 0.3.17".into()],
                removed: vec![],
                changed: vec![],
            }
        );
    }

    #[test]
    fn a_version_bump_shows_both_versions() {
        let base = lock(&[ROOT, &libc_a()]);
        let head = lock(&[ROOT, &libc_b()]);
        assert_eq!(
            compare_locks(&base, &head),
            LockDiff::Changed {
                added: vec!["libc 0.2.191".into()],
                removed: vec!["libc 0.2.190".into()],
                changed: vec![],
            }
        );
    }

    #[test]
    fn a_changed_checksum_is_changed() {
        let other = libc_a().replace("aaaa", "dddd");
        assert_eq!(
            compare_locks(&lock(&[&libc_a()]), &lock(&[&other])),
            LockDiff::Changed {
                added: vec![],
                removed: vec![],
                changed: vec!["libc 0.2.190".into()],
            }
        );
    }

    #[test]
    fn a_changed_source_lists_it_as_changed() {
        let other = libc_a().replace(REG, "git+https://example.com/libc#abc");
        assert_eq!(
            compare_locks(&lock(&[&libc_a()]), &lock(&[&other])),
            LockDiff::Changed {
                added: vec![],
                removed: vec![],
                changed: vec!["libc 0.2.190".into()],
            }
        );
    }

    #[test]
    fn a_checksum_change_beside_an_added_crate_splits_the_lists() {
        let other = libc_a().replace("aaaa", "dddd");
        assert_eq!(
            compare_locks(&lock(&[ROOT, &libc_a()]), &lock(&[ROOT, &other, &hook()])),
            LockDiff::Changed {
                added: vec!["signal-hook 0.3.17".into()],
                removed: vec![],
                changed: vec!["libc 0.2.190".into()],
            }
        );
    }

    #[test]
    fn a_new_git_source_is_changed() {
        let git = "name = \"x\"\nversion = \"0.1.0\"\nsource = \"git+https://example.com/x#abc\"";
        let diff = compare_locks(&lock(&[ROOT]), &lock(&[ROOT, git]));
        assert!(matches!(diff, LockDiff::Changed { .. }));
    }

    #[test]
    fn a_new_path_package_is_same() {
        let path = "name = \"local\"\nversion = \"0.1.0\"";
        assert_eq!(
            compare_locks(&lock(&[ROOT]), &lock(&[ROOT, path])),
            LockDiff::Same
        );
    }

    #[test]
    fn garbage_no_packages_and_v1_are_unknown() {
        let good = lock(&[ROOT]);
        let v1 = format!(
            "{good}\n[metadata]\n\"checksum serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)\" = \"abc\"\n"
        );
        let no_version = lock(&["name = \"x\""]);
        for bad in [
            "not toml [",
            "version = 4\n",
            v1.as_str(),
            no_version.as_str(),
        ] {
            assert!(matches!(compare_locks(&good, bad), LockDiff::Unknown(_)));
            assert!(matches!(compare_locks(bad, &good), LockDiff::Unknown(_)));
        }
    }

    #[test]
    fn a_format_version_bump_alone_is_same() {
        let v4 = lock(&[ROOT, &libc_a()]);
        let v3 = v4.replacen("version = 4", "version = 3", 1);
        assert_eq!(compare_locks(&v3, &v4), LockDiff::Same);
    }

    const BASE: &str =
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nbar = \"1\"\n";
    const HEAD: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";

    #[test]
    fn manifest_key_changes_are_changed() {
        let cases = [
            ("package.build", "[package]\nname = \"x\"\nversion = \"0.1.0\"\nbuild = \"src/gen.rs\"\n"),
            ("package.links", "[package]\nname = \"x\"\nversion = \"0.1.0\"\nlinks = \"z\"\n"),
            ("lib.proc-macro", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[lib]\nproc-macro = true\n"),
            ("lib.proc_macro", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[lib]\nproc_macro = true\n"),
            ("cargo-features", "cargo-features = [\"x\"]\n[package]\nname = \"x\"\nversion = \"0.1.0\"\n"),
            ("patch", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[patch.crates-io]\nbar = { path = \"../bar\" }\n"),
            ("replace", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[replace]\n\"bar:1.0.0\" = { path = \"../bar\" }\n"),
        ];
        for (key, head) in cases {
            match manifest_keys_diff(Some(BASE), head) {
                CargoVerdict::Changed(reason) => {
                    assert!(reason.contains(key), "{reason} should name {key}")
                }
                other => panic!("{key}: {other:?}"),
            }
        }
    }

    #[test]
    fn free_manifest_edits_are_same() {
        let heads = [
            format!("{BASE}\n[[bin]]\nname = \"b\"\npath = \"src/b.rs\"\n"),
            format!("{BASE}\n[features]\nf = []\n"),
            format!("{BASE}\n[profile.release]\nopt-level = 3\n"),
            format!("{BASE}foo = \"1\"\n"),
            format!("{HEAD}# note\n\n[dependencies]\nbar=\"1\"\n"),
        ];
        for head in heads {
            assert_eq!(manifest_keys_diff(Some(BASE), &head), CargoVerdict::Same);
        }
    }

    #[test]
    fn a_new_manifest_is_compared_with_an_empty_one() {
        let with = "[package]\nname=\"x\"\n[lib]\nproc-macro = true\n";
        let without = "[package]\nname=\"x\"\n";
        assert!(matches!(
            manifest_keys_diff(None, with),
            CargoVerdict::Changed(_)
        ));
        assert_eq!(manifest_keys_diff(None, without), CargoVerdict::Same);
    }

    #[test]
    fn an_unparsable_manifest_is_unknown() {
        assert!(matches!(
            manifest_keys_diff(Some("x ["), BASE),
            CargoVerdict::Unknown(_)
        ));
        assert!(matches!(
            manifest_keys_diff(Some(BASE), "x ["),
            CargoVerdict::Unknown(_)
        ));
    }

    fn pkg(manifest: &str, kinds: &[&str], src: &str) -> PathPackage {
        PathPackage {
            manifest_path: PathBuf::from(manifest),
            targets: vec![CargoTarget {
                kinds: kinds.iter().map(|k| k.to_string()).collect(),
                src_path: PathBuf::from(src),
            }],
        }
    }

    #[test]
    fn path_packages_outside_the_project_are_changed() {
        let project = Path::new("/p");
        let none = BTreeSet::new();
        let out_manifest = pkg("/other/Cargo.toml", &["lib"], "/p/src/lib.rs");
        let out_src = pkg("/p/Cargo.toml", &["lib"], "/other/src/lib.rs");
        for p in [out_manifest, out_src] {
            assert!(matches!(
                check_path_packages(project, &[p], &none, &none),
                CargoVerdict::Changed(_)
            ));
        }
    }

    #[test]
    fn a_changed_build_script_is_changed() {
        let project = Path::new("/p");
        let src = "/p/crates/a/b/c/build.rs";
        let p = pkg("/p/crates/a/b/c/Cargo.toml", &["custom-build"], src);
        let changed = BTreeSet::from([PathBuf::from(src)]);
        let none = BTreeSet::new();
        assert!(matches!(
            check_path_packages(project, std::slice::from_ref(&p), &changed, &none),
            CargoVerdict::Changed(_)
        ));
        assert_eq!(
            check_path_packages(project, &[p], &none, &none),
            CargoVerdict::Same
        );
    }

    #[test]
    fn a_new_proc_macro_package_is_changed() {
        let project = Path::new("/p");
        let p = pkg("/p/m/Cargo.toml", &["proc-macro"], "/p/m/src/lib.rs");
        let none = BTreeSet::new();
        let new = BTreeSet::from([PathBuf::from("/p/m/Cargo.toml")]);
        assert!(matches!(
            check_path_packages(project, std::slice::from_ref(&p), &none, &new),
            CargoVerdict::Changed(_)
        ));
        assert_eq!(
            check_path_packages(project, &[p], &none, &none),
            CargoVerdict::Same
        );
    }
}
