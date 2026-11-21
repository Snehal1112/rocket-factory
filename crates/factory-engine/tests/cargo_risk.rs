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
fn the_check_line_appears_once_per_head() {
    let fx = fixture();
    assert_eq!(
        run_job(&fx, change(internal_dep(), true)),
        Decision::AutoMerge
    );
    let second = fx
        .engine(&approving())
        .review_stage()
        .expect("review again");
    assert_eq!(second.decision, Decision::AutoMerge);
    let lines = ledger(&fx).matches("cargo lock check: ").count();
    assert_eq!(lines, 1, "{}", ledger(&fx));
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
        "cargo lock check: outside crates added: signal-hook 0.3.17; removed: none; source or checksum changed: none, diff stays risky"
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
    assert!(!fx.job_dir.join("cargo-check.txt").exists());
}

/// A builder that deletes `gone`, writes `files` and regenerates the lock.
fn delete_and_change(gone: Vec<&'static str>, files: Vec<(&'static str, String)>) -> Effect {
    Box::new(move |spec| {
        for g in &gone {
            let path = spec.cwd.join(g);
            if path.is_dir() {
                std::fs::remove_dir_all(&path).expect("remove dir");
            } else {
                std::fs::remove_file(&path).expect("remove file");
            }
        }
        write_files(&spec.cwd, &files);
        lock_in(&spec.cwd);
        "done".to_string()
    })
}

#[test]
fn a_deleted_member_crate_merges() {
    let fx = fixture();
    let files = vec![(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\"]\nresolver = \"2\"\n".to_string(),
    )];
    let decision = run_job(&fx, delete_and_change(vec!["b"], files));
    assert_eq!(decision, Decision::AutoMerge);
    assert!(ledger(&fx).contains(PASS_LINE), "{}", ledger(&fx));
}

#[test]
fn a_deleted_nested_workspace_manifest_parks() {
    let fx = fixture();
    commit_workspace(&fx, "nested/");
    let files = vec![("a/Cargo.toml", package("a", "# a note\n"))];
    let effect = delete_and_change(vec!["nested/Cargo.toml"], files);
    assert_risky(&run_job(&fx, effect));
    let line = check_line(&fx);
    assert!(line.contains("nested/Cargo.toml"), "{line}");
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
    let prefixed: String = GLOBS
        .iter()
        .chain(["x/**"].iter())
        .map(|g| format!("{}:{g}", g.len()))
        .collect();
    let expected = format!("cargo_lock_check=true; risk_paths={prefixed}");
    assert!(first.starts_with("key "), "{first}");
    assert!(first.ends_with(&expected), "{first}");
}

fn changes() -> ScriptedRunner {
    scripted(vec![Box::new(|_| {
        "1. [blocker] a/src/lib.rs:1 bad\nVERDICT: CHANGES".to_string()
    })])
}

/// The change that adds an outside crate to the lock.
fn outside_crate() -> Effect {
    Box::new(|spec| {
        let path = spec.cwd.join("Cargo.lock");
        let mut text = std::fs::read_to_string(&path).expect("read lock");
        text.push_str(
            "\n[[package]]\nname = \"signal-hook\"\nversion = \"0.3.17\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
             checksum = \"8621587d4798caf8eb44879d42e56b9a93ea5dcd315a6487c357130095b62801\"\n",
        );
        std::fs::write(&path, text).expect("write lock");
        "done".to_string()
    })
}

#[test]
fn a_cleared_round_then_an_outside_crate_hits_the_risky_cap() {
    let mut fx = fixture();
    fx.config.fix_rounds = 2;
    fx.config.risky_fix_rounds = Some(1);
    let runner = scripted(vec![change(internal_dep(), true)]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let review = fx.engine(&changes()).review_stage().expect("review");
    assert_eq!(review.decision, Decision::FixRound);
    fx.engine(&changes())
        .apply_decision(&review)
        .expect("apply");
    assert!(!fx.job_dir.join("risky-paths.txt").exists());

    let runner = scripted(vec![outside_crate()]);
    fx.engine(&runner).build_stage().expect("fix build");
    fx.engine(&runner).checks_stage().expect("checks");
    let review = fx.engine(&approving()).review_stage().expect("review");
    assert_risky(&review.decision);
    fx.engine(&approving())
        .apply_decision(&review)
        .expect("apply");

    let list = std::fs::read_to_string(fx.job_dir.join("risky-paths.txt")).expect("risky paths");
    assert!(list.contains("Cargo.lock"), "{list}");
    let record = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    let risk = record
        .parts
        .iter()
        .find(|p| p.title == "Risk")
        .expect("risk part");
    assert!(risk.text.contains("Cargo.lock"), "{}", risk.text);
    assert!(!risk.text.contains("added by fix rounds"), "{}", risk.text);
    assert!(risk.text.contains("signal-hook 0.3.17"), "{}", risk.text);
}

#[test]
fn a_failing_check_on_a_cleared_diff_uses_normal_fix_rounds() {
    let mut fx = fixture();
    fx.config.fix_rounds = 2;
    fx.config.risky_fix_rounds = Some(0);
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
        ..factory_core::CheckRule::default()
    }];
    let runner = scripted(vec![change(internal_dep(), true)]);
    fx.engine(&runner).build_stage().expect("build");
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(
        out,
        factory_engine::StageOutcome::Advanced(factory_core::Stage::Build)
    );
    assert_eq!(fx.job().round, 2);
}

#[test]
fn the_risk_part_never_lists_a_cleared_cargo_file() {
    let mut fx = fixture();
    fx.config.fix_rounds = 0;
    let runner = scripted(vec![change(internal_dep(), true)]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let review = fx.engine(&changes()).review_stage().expect("review");
    fx.engine(&changes())
        .apply_decision(&review)
        .expect("apply");
    let record = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(record.kind, ParkKind::ChangesRemain);
    for part in &record.parts {
        assert!(!part.text.contains("risky paths"), "{}", part.text);
    }
    assert!(record.parts.iter().all(|p| p.title != "Risk"));
}

#[test]
fn a_delete_only_manifest_diff_stays_risky() {
    let fx = fixture();
    let effect: Effect = Box::new(|spec| {
        std::fs::remove_file(spec.cwd.join("b/Cargo.toml")).expect("remove manifest");
        "done".to_string()
    });
    assert_risky(&run_job(&fx, effect));
    assert!(ledger(&fx).contains("diff stays risky"), "{}", ledger(&fx));
}
