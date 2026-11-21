//! The Cargo lock check through the review stage: a Cargo diff with no outside code merges.

mod common;

use std::path::Path;
use std::process::Command;

use common::{scripted, Effect, Fixture, ScriptedRunner};
use factory_core::ParkKind;
use factory_engine::Decision;
use git2::{IndexAddOption, Repository, Signature};

const GLOBS: [&str; 4] = [
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/*auth*",
    "**/.cargo/**",
];

const PASS_LINE: &str = "cargo lock check: no outside code changed, Cargo files are not risky";

fn globs() -> Vec<String> {
    GLOBS.iter().map(|g| g.to_string()).collect()
}

/// A path-only package manifest with extra dependency lines.
fn package(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}")
}

/// Writes the lock of the workspace in `dir` without the network.
fn lock_in(dir: &Path) {
    let out = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(dir)
        .output()
        .expect("run cargo");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write_files(dir: &Path, files: &[(&str, String)]) {
    for (name, text) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, text).expect("write");
    }
}

/// Commits every file in `dir` that `.gitignore` allows.
fn commit_all(dir: &Path) {
    let repo = Repository::open(dir).expect("open repo");
    let mut index = repo.index().expect("index");
    index
        .add_all(["*"], IndexAddOption::DEFAULT, None)
        .expect("add all");
    index.write().expect("write index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, "workspace", &tree, &[&parent])
        .expect("commit");
}

/// The files of a path-only workspace with members `a` and `b`, below `prefix`.
fn workspace_files(prefix: &str) -> Vec<(String, String)> {
    let p = |name: &str| format!("{prefix}{name}");
    vec![
        (
            p("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n".into(),
        ),
        (p("a/Cargo.toml"), package("a", "")),
        (p("a/src/lib.rs"), "\n".into()),
        (p("b/Cargo.toml"), package("b", "")),
        (p("b/src/lib.rs"), "\n".into()),
        (p(".gitignore"), "vendor/\n".into()),
    ]
}

/// Commits a path-only workspace below `prefix` on main, with its lock.
fn commit_workspace(fx: &Fixture, prefix: &str) {
    let files = workspace_files(prefix);
    let refs: Vec<(&str, String)> = files.iter().map(|(n, t)| (n.as_str(), t.clone())).collect();
    write_files(&fx.main, &refs);
    lock_in(&fx.main.join(prefix));
    commit_all(&fx.main);
}

fn fixture() -> Fixture {
    let mut fx = Fixture::new();
    fx.config.risk_paths = globs();
    commit_workspace(&fx, "");
    fx
}

/// A builder that writes `files` in its folder and, when `regen` is set, regenerates the lock there.
fn change(files: Vec<(&'static str, String)>, regen: bool) -> Effect {
    Box::new(move |spec| {
        write_files(&spec.cwd, &files);
        if regen {
            lock_in(&spec.cwd);
        }
        "done".to_string()
    })
}

/// The change that adds an internal path dependency from `a` on `b`.
fn internal_dep() -> Vec<(&'static str, String)> {
    vec![("a/Cargo.toml", package("a", "b = { path = \"../b\" }\n"))]
}

fn approving() -> ScriptedRunner {
    scripted(vec![Box::new(|_| "fine\nVERDICT: APPROVE".to_string())])
}

/// Builds, checks and reviews the job with an approving reviewer, and gives the decision.
fn run_job(fx: &Fixture, effect: Effect) -> Decision {
    let runner = scripted(vec![effect]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    fx.engine(&approving())
        .review_stage()
        .expect("review")
        .decision
}

fn ledger(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

/// The `cargo lock check` ledger line, without its `Ruling: ` prefix.
fn check_line(fx: &Fixture) -> String {
    ledger(fx)
        .lines()
        .find_map(|l| l.split_once("cargo lock check: ").map(|(_, rest)| rest))
        .map(|rest| format!("cargo lock check: {rest}"))
        .unwrap_or_else(|| panic!("no cargo lock check line in:\n{}", ledger(fx)))
}

fn assert_risky(decision: &Decision) {
    match decision {
        Decision::NeedsHuman { kind, .. } => assert_eq!(*kind, ParkKind::RiskyDiff),
        other => panic!("unexpected {other:?}"),
    }
}

fn assert_no_check_line(fx: &Fixture) {
    assert!(!ledger(fx).contains("cargo lock check"), "{}", ledger(fx));
}

#[test]
fn an_internal_path_dependency_merges() {
    let fx = fixture();
    let decision = run_job(&fx, change(internal_dep(), true));
    assert_eq!(decision, Decision::AutoMerge);
    assert!(ledger(&fx).contains(PASS_LINE), "{}", ledger(&fx));
}

#[test]
fn a_dependency_missing_from_the_lock_parks() {
    let fx = fixture();
    let files = vec![
        ("c/Cargo.toml", package("c", "")),
        ("c/src/lib.rs", "\n".to_string()),
        ("a/Cargo.toml", package("a", "c = { path = \"../c\" }\n")),
    ];
    assert_risky(&run_job(&fx, change(files, false)));
    let line = check_line(&fx);
    assert!(
        line.starts_with("cargo lock check: cargo metadata failed:"),
        "{line}"
    );
    assert!(line.ends_with(", diff stays risky"), "{line}");
}

#[test]
fn an_outside_crate_in_the_lock_parks_naming_it() {
    let fx = fixture();
    let effect: Effect = Box::new(|spec| {
        let path = spec.cwd.join("Cargo.lock");
        let mut text = std::fs::read_to_string(&path).expect("read lock");
        text.push_str(
            "\n[[package]]\nname = \"signal-hook\"\nversion = \"0.3.17\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
             checksum = \"8621587d4798caf8eb44879d42e56b9a93ea5dcd315a6487c357130095b62801\"\n",
        );
        std::fs::write(&path, text).expect("write lock");
        "done".to_string()
    });
    assert_risky(&run_job(&fx, effect));
    assert_eq!(
        check_line(&fx),
        "cargo lock check: outside crates added: signal-hook 0.3.17; removed: none, diff stays risky"
    );
}

#[test]
fn a_metadata_failure_after_a_same_lock_keeps_the_diff_risky() {
    let fx = fixture();
    let files = vec![
        ("vendor/v/Cargo.toml", package("v", "")),
        ("vendor/v/src/lib.rs", "\n".to_string()),
        (
            "a/Cargo.toml",
            package("a", "v = { path = \"../vendor/v\" }\n"),
        ),
    ];
    assert_risky(&run_job(&fx, change(files, true)));
    let line = check_line(&fx);
    assert!(
        line.starts_with("cargo lock check: cargo metadata failed:"),
        "{line}"
    );
}

#[test]
fn a_cargo_file_beside_another_risky_path_stays_risky_without_cargo() {
    let fx = fixture();
    let files = vec![
        ("a/Cargo.toml", package("a", "# a note\n")),
        ("src/auth.rs", "\n".to_string()),
    ];
    assert_risky(&run_job(&fx, change(files, false)));
    assert!(!fx.job_dir.join("cargo-check.txt").exists());
    assert_no_check_line(&fx);
}

#[test]
fn a_cargo_config_change_parks() {
    let fx = fixture();
    let mut files = internal_dep();
    files.push((".cargo/config.toml", "[build]\n".to_string()));
    assert_risky(&run_job(&fx, change(files, true)));
    assert_no_check_line(&fx);
}

#[test]
fn an_unparsable_lock_parks_with_the_reason() {
    let fx = fixture();
    let files = vec![("Cargo.lock", "garbage [".to_string())];
    assert_risky(&run_job(&fx, change(files, false)));
    let line = check_line(&fx);
    assert!(
        line.contains("the head Cargo.lock does not parse"),
        "{line}"
    );
    assert!(line.ends_with(", diff stays risky"), "{line}");
}

#[test]
fn the_setting_off_keeps_cargo_files_risky() {
    let mut fx = fixture();
    fx.config.cargo_lock_check = false;
    assert_risky(&run_job(&fx, change(internal_dep(), true)));
    assert_no_check_line(&fx);
}

/// The change that adds a new member `c`, with extra files.
fn new_member(extra: Vec<(&'static str, String)>) -> Vec<(&'static str, String)> {
    let mut files = vec![
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\nresolver = \"2\"\n".to_string(),
        ),
        ("c/Cargo.toml", package("c", "")),
        ("c/src/lib.rs", "\n".to_string()),
    ];
    files.extend(extra);
    files
}

#[test]
fn a_new_internal_crate_merges_and_parks_with_a_build_script() {
    let fx = fixture();
    assert_eq!(
        run_job(&fx, change(new_member(vec![]), true)),
        Decision::AutoMerge
    );
    assert!(ledger(&fx).contains(PASS_LINE), "{}", ledger(&fx));

    let fx = fixture();
    let build = vec![("c/build.rs", "fn main() {}\n".to_string())];
    assert_risky(&run_job(&fx, change(new_member(build), true)));
    let line = check_line(&fx);
    assert!(line.contains("build script"), "{line}");
}

#[test]
fn a_broader_glob_keeps_the_manifest_risky() {
    let mut fx = fixture();
    fx.config.risk_paths.push("src-tauri/**".into());
    let files = vec![("src-tauri/Cargo.toml", package("tauri-app", ""))];
    assert_risky(&run_job(&fx, change(files, false)));
    assert_no_check_line(&fx);
}

#[test]
fn submodules_keep_the_diff_risky() {
    let fx = fixture();
    common::commit_file(
        &fx.main,
        ".gitmodules",
        "[submodule \"x\"]\n\tpath = x\n\turl = https://example.com/x\n",
    );
    assert_risky(&run_job(&fx, change(internal_dep(), true)));
    let line = check_line(&fx);
    assert!(line.contains("git submodules"), "{line}");
}

/// A fixture scoped to the `web` project in `apps/web`, like `scoped_fixture` in `gates.rs`.
fn scoped(fx: Fixture) -> Fixture {
    let mut fx = fx;
    fx.config.risk_paths = globs();
    fx.repo = factory_git::Repo::open(&fx.main)
        .expect("open")
        .with_scope("web", Path::new("apps/web"));
    fx
}

#[test]
fn a_subfolder_project_uses_its_own_workspace() {
    let fx = Fixture::new();
    common::commit_file(&fx.main, "apps/web/index.txt", "web\n");
    commit_workspace(&fx, "apps/web/");
    let fx = scoped(fx);
    let decision = run_job(&fx, change(internal_dep(), true));
    assert_eq!(decision, Decision::AutoMerge);
    assert!(ledger(&fx).contains(PASS_LINE), "{}", ledger(&fx));
}

#[test]
fn a_workspace_root_above_the_subpath_stays_risky() {
    let fx = Fixture::new();
    let files = vec![
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"apps/web\"]\nresolver = \"2\"\n".to_string(),
        ),
        ("apps/web/Cargo.toml", package("web", "")),
        ("apps/web/src/lib.rs", "\n".to_string()),
    ];
    write_files(&fx.main, &files);
    lock_in(&fx.main);
    commit_all(&fx.main);
    let fx = scoped(fx);
    let mut manifest = package("web", "");
    manifest.push_str("\n[features]\nf = []\n");
    assert_risky(&run_job(&fx, change(vec![("Cargo.toml", manifest)], false)));
    let line = check_line(&fx);
    assert!(line.contains("workspace root"), "{line}");
}

#[test]
fn a_new_config_runs_the_check_again() {
    let mut fx = fixture();
    assert_eq!(
        run_job(&fx, change(internal_dep(), true)),
        Decision::AutoMerge
    );
    fx.config.risk_paths.push("x/**".into());
    fx.engine(&approving())
        .review_stage()
        .expect("review again");
    let stored = std::fs::read_to_string(fx.job_dir.join("cargo-check.txt")).expect("store");
    let first = stored.lines().next().expect("line 1");
    let expected = format!("cargo_lock_check=true; risk_paths={},x/**", GLOBS.join(","));
    assert!(first.starts_with("key "), "{first}");
    assert!(first.ends_with(&expected), "{first}");
}
