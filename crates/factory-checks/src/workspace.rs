//! The cargo workspace map: which package owns a path and which packages depend on which.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use factory_core::{CargoTarget, PathPackage};

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

/// The reason line of cargo's stderr: the first `error` line, else the first non-empty one.
fn stderr_reason(stderr: &str) -> String {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    lines()
        .find(|l| l.starts_with("error"))
        .or_else(|| lines().next())
        .unwrap_or("no message")
        .to_string()
}

/// The time limit of one cargo call, which may download crates or wait for the package cache.
const CARGO_TIMEOUT: Duration = Duration::from_secs(300);

/// Reads a child pipe to its end.
fn read_pipe<R: Read>(reader: Option<R>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut r) = reader {
        let _ = r.read_to_end(&mut buf);
    }
    buf
}

/// Runs `program <sub> <args>` in `dir` and returns its stdout. The call is killed after `timeout`.
fn run_program(
    program: &str,
    sub: &str,
    args: &[&str],
    dir: &Path,
    timeout: Duration,
) -> Result<String, CheckError> {
    let mut cmd = Command::new(program);
    cmd.arg(sub)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = factory_procgroup::spawn(&mut cmd)
        .map_err(|e| CheckError::Cargo(format!("cargo did not start: {e}")))?;
    let out_reader = child.stdout.take();
    let err_reader = child.stderr.take();
    let out_thread = thread::spawn(move || read_pipe(out_reader));
    let err_thread = thread::spawn(move || read_pipe(err_reader));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                return Err(CheckError::Cargo(format!(
                    "cargo {sub} could not wait: {e}"
                )));
            }
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    match status {
        None => Err(CheckError::Cargo(format!(
            "cargo {sub} timed out after {} s",
            timeout.as_secs()
        ))),
        Some(s) if s.success() => Ok(String::from_utf8_lossy(&stdout).into_owned()),
        Some(_) => {
            let reason = stderr_reason(&String::from_utf8_lossy(&stderr));
            Err(CheckError::Cargo(format!("cargo {sub} failed: {reason}")))
        }
    }
}

/// Runs `program <sub> <args>` in `dir` with the cargo time limit and returns its stdout.
fn run_cargo(program: &str, sub: &str, args: &[&str], dir: &Path) -> Result<String, CheckError> {
    run_program(program, sub, args, dir, CARGO_TIMEOUT)
}

/// The folder of the manifest, which is where cargo looks for its config.
fn manifest_dir(manifest: &Path) -> Result<&Path, CheckError> {
    manifest
        .parent()
        .ok_or_else(|| CheckError::Cargo(format!("no folder for {}", manifest.display())))
}

/// The workspace root folder that owns `manifest`.
pub fn workspace_root(manifest: &Path) -> Result<PathBuf, CheckError> {
    let dir = manifest_dir(manifest)?;
    let path = manifest.to_string_lossy();
    let out = run_cargo(
        "cargo",
        "locate-project",
        &[
            "--workspace",
            "--message-format",
            "plain",
            "--manifest-path",
            &path,
        ],
        dir,
    )?;
    Path::new(out.trim())
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CheckError::Cargo("cargo locate-project printed no path".into()))
}

/// Proves the lock current with `--locked` and lists the path packages, with canonical paths.
pub fn locked_path_packages(root_manifest: &Path) -> Result<Vec<PathPackage>, CheckError> {
    let dir = manifest_dir(root_manifest)?;
    let path = root_manifest.to_string_lossy();
    let json = run_cargo(
        "cargo",
        "metadata",
        &[
            "--locked",
            "--format-version",
            "1",
            "--manifest-path",
            &path,
        ],
        dir,
    )?;
    let canon = |p: &Path| {
        p.canonicalize()
            .map_err(|e| CheckError::Cargo(format!("cannot resolve {}: {e}", p.display())))
    };
    let mut packages = path_packages_from_metadata(&json)?;
    for pkg in &mut packages {
        pkg.manifest_path = canon(&pkg.manifest_path)?;
        for t in &mut pkg.targets {
            t.src_path = canon(&t.src_path)?;
        }
    }
    Ok(packages)
}

/// Parses `cargo metadata` output: the packages with no source, which are the path packages.
pub fn path_packages_from_metadata(json: &str) -> Result<Vec<PathPackage>, CheckError> {
    let bad = |m: &str| CheckError::Cargo(format!("bad cargo metadata: {m}"));
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| bad(&e.to_string()))?;
    let list = value["packages"]
        .as_array()
        .ok_or_else(|| bad("no packages"))?;
    let mut out = Vec::new();
    for p in list {
        let source = p
            .get("source")
            .ok_or_else(|| bad("a package has no source"))?;
        if !source.is_null() {
            continue;
        }
        let manifest = p["manifest_path"]
            .as_str()
            .ok_or_else(|| bad("a package has no manifest_path"))?;
        let mut targets = Vec::new();
        let raw = p["targets"]
            .as_array()
            .ok_or_else(|| bad("a package has no targets"))?;
        for t in raw {
            let src = t["src_path"]
                .as_str()
                .ok_or_else(|| bad("a target has no src_path"))?;
            let kinds = t["kind"]
                .as_array()
                .ok_or_else(|| bad("a target has no kind"))?
                .iter()
                .map(|k| {
                    k.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad("a kind is not text"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            targets.push(CargoTarget {
                kinds,
                src_path: PathBuf::from(src),
            });
        }
        out.push(PathPackage {
            manifest_path: PathBuf::from(manifest),
            targets,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_program_times_out() {
        let started = Instant::now();
        let err = run_program(
            "sh",
            "-c",
            &["sleep 5"],
            Path::new("."),
            Duration::from_secs(1),
        )
        .expect_err("timeout");
        assert_eq!(err.to_string(), "cargo -c timed out after 1 s");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

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

    fn write_tree(root: &Path, files: &[(&str, &str)]) {
        for (name, body) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, body).expect("write");
        }
    }

    fn manifest(name: &str, extra: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}")
    }

    fn lock(root: &Path) {
        let out = Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(root)
            .output()
            .expect("cargo runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A workspace with members `a` and `b`, and its lock made.
    fn two_members() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = manifest("a", "");
        let b = manifest("b", "");
        write_tree(
            dir.path(),
            &[
                ("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n"),
                ("a/Cargo.toml", a.as_str()),
                ("a/src/lib.rs", ""),
                ("a/build.rs", "fn main() {}\n"),
                ("b/Cargo.toml", b.as_str()),
                ("b/src/lib.rs", ""),
            ],
        );
        lock(dir.path());
        dir
    }

    #[test]
    fn workspace_root_finds_the_root_of_a_member() {
        let dir = two_members();
        let root = workspace_root(&dir.path().join("a/Cargo.toml")).expect("root");
        assert_eq!(
            root.canonicalize().expect("canon"),
            dir.path().canonicalize().expect("canon")
        );
    }

    #[test]
    fn a_nested_workspace_gives_its_own_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = format!("{}\n[workspace]\n", manifest("a", ""));
        write_tree(
            dir.path(),
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nmembers = []\nexclude = [\"a\"]\n",
                ),
                ("a/Cargo.toml", a.as_str()),
                ("a/src/lib.rs", ""),
            ],
        );
        let root = workspace_root(&dir.path().join("a/Cargo.toml")).expect("root");
        assert_eq!(
            root.canonicalize().expect("canon"),
            dir.path().join("a").canonicalize().expect("canon")
        );
    }

    #[test]
    fn a_current_lock_lists_the_path_packages() {
        let dir = two_members();
        let pkgs = locked_path_packages(&dir.path().join("Cargo.toml")).expect("packages");
        let base = dir.path().canonicalize().expect("canon");
        let manifests: Vec<_> = pkgs.iter().map(|p| p.manifest_path.clone()).collect();
        assert!(manifests.contains(&base.join("a/Cargo.toml")));
        assert!(manifests.contains(&base.join("b/Cargo.toml")));
        let a = pkgs
            .iter()
            .find(|p| p.manifest_path == base.join("a/Cargo.toml"))
            .expect("a");
        assert!(a
            .targets
            .iter()
            .any(|t| t.kinds == ["custom-build"] && t.src_path.ends_with("a/build.rs")));
    }

    #[test]
    fn a_dependency_the_lock_lacks_fails_under_locked() {
        let dir = two_members();
        let a = manifest("a", "\n[dependencies]\nc = { path = \"../c\" }\n");
        let c = manifest("c", "");
        write_tree(
            dir.path(),
            &[
                ("a/Cargo.toml", a.as_str()),
                ("c/Cargo.toml", c.as_str()),
                ("c/src/lib.rs", ""),
            ],
        );
        let err = locked_path_packages(&dir.path().join("Cargo.toml")).expect_err("stale lock");
        assert!(
            err.to_string().starts_with("cargo metadata failed:"),
            "{err}"
        );
    }

    #[test]
    fn a_missing_cargo_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err =
            run_cargo("cargo-does-not-exist", "metadata", &[], dir.path()).expect_err("no program");
        assert!(err.to_string().starts_with("cargo did not start:"), "{err}");
    }

    #[test]
    fn metadata_json_keeps_only_path_packages() {
        let json = r#"{"packages":[
          {"name":"serde","source":"registry+https://github.com/rust-lang/crates.io-index",
           "manifest_path":"/h/serde/Cargo.toml",
           "targets":[{"kind":["lib"],"src_path":"/h/serde/src/lib.rs"}]},
          {"name":"a","source":null,"manifest_path":"/r/a/Cargo.toml",
           "targets":[{"kind":["lib"],"src_path":"/r/a/src/lib.rs"},
                      {"kind":["custom-build"],"src_path":"/r/a/build.rs"}]}]}"#;
        let pkgs = path_packages_from_metadata(json).expect("parse");
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].manifest_path, PathBuf::from("/r/a/Cargo.toml"));
        assert_eq!(pkgs[0].targets.len(), 2);
        assert_eq!(pkgs[0].targets[1].kinds, ["custom-build"]);
        assert_eq!(pkgs[0].targets[1].src_path, PathBuf::from("/r/a/build.rs"));
    }

    #[test]
    fn metadata_with_a_bad_shape_is_an_error() {
        let no_source = r#"{"packages":[{"manifest_path":"/r/Cargo.toml","targets":[]}]}"#;
        let no_targets = r#"{"packages":[{"source":null,"manifest_path":"/r/Cargo.toml"}]}"#;
        let no_kind = r#"{"packages":[{"source":null,"manifest_path":"/r/Cargo.toml","targets":[{"src_path":"/r/build.rs"}]}]}"#;
        for json in [no_source, no_targets, no_kind] {
            let err = path_packages_from_metadata(json).expect_err("bad shape");
            assert!(err.to_string().starts_with("bad cargo metadata:"), "{err}");
        }
    }

    #[test]
    fn stderr_reason_prefers_the_error_line() {
        let err = "warning: x\n    Blocking waiting for file lock\nerror: failed to select\n";
        assert_eq!(stderr_reason(err), "error: failed to select");
        assert_eq!(stderr_reason("\nonly line\n"), "only line");
    }
}
