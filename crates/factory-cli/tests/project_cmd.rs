use std::path::{Path, PathBuf};

use clap::{error::ErrorKind, Parser};
use factory_cli::{project, Cli, Command};
use factory_core::Home;
use git2::{Repository, RepositoryInitOptions, Signature};

struct Env {
    tmp: tempfile::TempDir,
    home: Home,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    Env { tmp, home }
}

fn make_repo(parent: &Path, folder: &str, branch: &str) -> PathBuf {
    let path = parent.join(folder);
    std::fs::create_dir_all(&path).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head(branch);
    let repo = Repository::init_opts(&path, &opts).expect("init");
    std::fs::write(path.join("a.txt"), "a").expect("write");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("a.txt")).expect("add");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).expect("commit");
    path
}

fn run(e: &Env, args: &[&str]) -> Result<String, factory_cli::CliError> {
    let mut full = vec!["factory", "project"];
    full.extend_from_slice(args);
    let cli = Cli::try_parse_from(full).expect("parse");
    match cli.command {
        Command::Project { cmd } => project::run(&e.home, cmd),
        _ => unreachable!(),
    }
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf8")
}

fn add_demo(e: &Env) -> PathBuf {
    let repo = make_repo(e.tmp.path(), "demo-repo", "main");
    run(e, &["add", s(&repo), "--preset", "rust", "--name", "demo", "--base", "main"]).expect("add");
    repo
}

#[test]
fn add_then_list() {
    let e = env();
    let repo = make_repo(e.tmp.path(), "r1", "main");
    let out = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "demo", "--base", "main"]).expect("add");
    assert!(out.contains(s(&e.home.project_dir("demo"))), "{out}");
    assert!(e.home.presets_dir().join("rust/config.toml").exists());
    let canon = std::fs::canonicalize(&repo).expect("canon");
    assert_eq!(run(&e, &["list"]).expect("list"), format!("demo rust main {}", canon.display()));
}

#[test]
fn add_records_the_current_branch_when_no_base_is_given() {
    let e = env();
    let repo = make_repo(e.tmp.path(), "r1", "trunk");
    run(&e, &["add", s(&repo), "--preset", "go", "--name", "demo"]).expect("add");
    assert!(run(&e, &["list"]).expect("list").starts_with("demo go trunk "));
}

#[test]
fn the_name_defaults_to_the_lowercased_folder() {
    let e = env();
    let ok = make_repo(e.tmp.path(), "Good-Repo", "main");
    run(&e, &["add", s(&ok), "--preset", "rust"]).expect("add");
    assert!(run(&e, &["list"]).expect("list").starts_with("good-repo "));
    let bad = make_repo(e.tmp.path(), "My Repo", "main");
    let err = run(&e, &["add", s(&bad), "--preset", "rust"]).expect_err("refused").to_string();
    assert!(err.contains("--name"), "{err}");
}

#[test]
fn add_refusals() {
    let e = env();
    let plain = e.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    assert!(run(&e, &["add", s(&plain), "--preset", "rust", "--name", "p"]).is_err());
    let sub = make_repo(e.tmp.path(), "outer", "main").join("inner");
    std::fs::create_dir_all(&sub).expect("mkdir");
    assert!(run(&e, &["add", s(&sub), "--preset", "rust", "--name", "p"]).is_err(), "a subfolder is not a repo root");
    let repo = make_repo(e.tmp.path(), "r1", "main");
    let err = run(&e, &["add", s(&repo), "--preset", "nope", "--name", "p"]).expect_err("preset").to_string();
    assert!(err.contains("unknown preset"), "{err}");
    let err = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "p", "--base", "ghost"]).expect_err("base").to_string();
    assert!(err.contains("ghost"), "{err}");
    run(&e, &["add", s(&repo), "--preset", "rust", "--name", "p"]).expect("add");
    let err = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "p"]).expect_err("dup name").to_string();
    assert!(err.contains("already exists"), "{err}");
    let err = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "q"]).expect_err("dup repo").to_string();
    assert!(err.contains("already project p"), "{err}");
}

#[test]
fn list_shows_a_missing_repo_as_disabled_and_keeps_the_others() {
    let e = env();
    let gone = make_repo(e.tmp.path(), "gone", "main");
    run(&e, &["add", s(&gone), "--preset", "rust", "--name", "demo"]).expect("add");
    let other = make_repo(e.tmp.path(), "other", "main");
    run(&e, &["add", s(&other), "--preset", "go", "--name", "other"]).expect("add");
    std::fs::remove_dir_all(&gone).expect("rm");
    let out = run(&e, &["list"]).expect("list");
    assert!(out.contains("demo  disabled: repo path is missing"), "{out}");
    assert!(out.contains("other go main"), "{out}");
}

#[test]
fn show_prints_the_resolved_project() {
    let e = env();
    let repo = add_demo(&e);
    let out = run(&e, &["show", "demo"]).expect("show");
    let canon = std::fs::canonicalize(&repo).expect("canon");
    for want in [
        format!("repo: {}", canon.display()),
        "preset: rust".to_string(),
        "base branch: main".to_string(),
        "state folder:".to_string(),
        "worktrees folder:".to_string(),
        "risk paths:".to_string(),
        "agent files:".to_string(),
        "problems: none".to_string(),
    ] {
        assert!(out.contains(&want), "missing {want:?} in\n{out}");
    }
    assert!(out.contains("factory-builder.md"), "{out}");
    assert!(!out.contains("degraded"), "{out}");
}

#[test]
fn show_reports_a_missing_base_branch_as_degraded() {
    let e = env();
    add_demo(&e);
    let file = e.home.project_dir("demo").join("project.toml");
    let text = std::fs::read_to_string(&file).expect("read").replace("\"main\"", "\"gone\"");
    std::fs::write(&file, text).expect("write");
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(out.contains("degraded"), "{out}");
    assert!(out.contains("gone"), "{out}");
}

#[test]
fn show_reports_an_override_the_engine_would_reject() {
    let e = env();
    let repo = add_demo(&e);
    let git = Repository::open(&repo).expect("open");
    std::fs::create_dir_all(repo.join(".factory")).expect("mkdir");
    std::fs::write(
        repo.join(".factory/override.toml"),
        "[[checks]]\npaths = [\"**\"]\ncommands = [\"cargo test --workspace\"]\n",
    )
    .expect("write");
    let mut index = git.index().expect("index");
    index.add_path(Path::new(".factory/override.toml")).expect("add");
    let tree = git.find_tree(index.write_tree().expect("tree")).expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    let parent = git.head().expect("head").peel_to_commit().expect("commit");
    git.commit(Some("HEAD"), &sig, &sig, "override", &tree, &[&parent]).expect("commit");
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(out.contains("would be rejected"), "{out}");
}

#[test]
fn remove_deletes_only_the_project_folder() {
    let e = env();
    let repo = add_demo(&e);
    run(&e, &["remove", "demo"]).expect("remove");
    assert!(!e.home.project_dir("demo").exists());
    assert!(repo.join(".git").exists());
    assert!(e.home.presets_dir().join("rust/config.toml").exists());
    assert!(run(&e, &["remove", "demo"]).is_err());
}

#[test]
fn remove_refuses_open_jobs_unless_forced() {
    let e = env();
    add_demo(&e);
    let state = e.home.project_dir("demo").join("state");
    let mut board = factory_core::Board::default();
    board.jobs.push(factory_core::Job::new("001", "2026-10-13-001-x", "x", "2026-10-13", "feature", 0.0));
    board.save(&state.join("board.json")).expect("save");
    let err = run(&e, &["remove", "demo"]).expect_err("refused").to_string();
    assert!(err.contains("--force") && err.contains("001"), "{err}");
    assert!(e.home.project_dir("demo").exists());
    run(&e, &["remove", "demo", "--force"]).expect("forced");
    assert!(!e.home.project_dir("demo").exists());
}

#[test]
fn add_without_arguments_is_a_usage_error() {
    let err = Cli::try_parse_from(["factory", "project", "add"]).expect_err("needs args");
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}

#[test]
fn add_accepts_a_non_canonical_path_and_names_it_from_the_real_folder() {
    let e = env();
    let repo = make_repo(e.tmp.path(), "Real", "main");
    std::fs::create_dir_all(e.tmp.path().join("x")).expect("mkdir");
    let odd = e.tmp.path().join("x").join("..").join("Real").join(".");
    run(&e, &["add", s(&odd), "--preset", "rust"]).expect("add");
    let canon = std::fs::canonicalize(&repo).expect("canon");
    assert_eq!(run(&e, &["list"]).expect("list"), format!("real rust main {}", canon.display()));
}

#[test]
fn add_accepts_a_relative_path() {
    // The current directory is process wide, so only this test changes it, and it restores it.
    let e = env();
    make_repo(e.tmp.path(), "rel", "main");
    let old = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(e.tmp.path().join("rel")).expect("chdir");
    let out = run(&e, &["add", ".", "--preset", "rust"]);
    std::env::set_current_dir(old).expect("restore");
    out.expect("add");
    assert!(run(&e, &["list"]).expect("list").starts_with("rel rust main "));
}

#[test]
fn add_refuses_a_path_that_does_not_exist() {
    let e = env();
    let err = run(&e, &["add", "/no/such/dir", "--preset", "rust", "--name", "x"]).expect_err("refused").to_string();
    assert!(err.contains("not usable"), "{err}");
}
