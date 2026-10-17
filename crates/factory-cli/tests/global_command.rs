use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clap::Parser;
use factory_cli::install::{self, GLOBAL_COMMAND};
use factory_cli::{project, Cli, Command};
use factory_core::Home;
use git2::{Repository, RepositoryInitOptions, Signature};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> EnvVarGuard {
        let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let old = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        EnvVarGuard { key, old, _lock: lock }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn make_repo(parent: &Path, folder: &str) -> PathBuf {
    let path = parent.join(folder);
    std::fs::create_dir_all(&path).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(&path, &opts).expect("init");
    std::fs::write(path.join("a.txt"), "a").expect("write");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("a.txt")).expect("add");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).expect("commit");
    path
}

fn project_run(home: &Home, args: &[&str]) -> Result<String, factory_cli::CliError> {
    let mut full = vec!["factory", "project"];
    full.extend_from_slice(args);
    match Cli::try_parse_from(full).expect("parse").command {
        Command::Project { cmd } => project::run(home, cmd),
        _ => unreachable!(),
    }
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf8")
}

fn install_args(args: &[&str]) -> Result<String, factory_cli::CliError> {
    let mut full = vec!["factory", "install"];
    full.extend_from_slice(args);
    match Cli::try_parse_from(full).expect("parse").command {
        Command::Install { dir, force } => install::run(dir.as_deref(), force),
        _ => unreachable!(),
    }
}

#[test]
fn which_prints_the_name_for_a_repo_a_subfolder_and_a_worktree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let repo = make_repo(tmp.path(), "demo-repo");
    project_run(&home, &["add", s(&repo), "--preset", "rust", "--name", "demo"]).expect("add");
    assert_eq!(project_run(&home, &["which", s(&repo)]).expect("root"), "demo");
    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir");
    assert_eq!(project_run(&home, &["which", s(&sub)]).expect("sub"), "demo");
    // A real linked worktree made by git2, outside the repo.
    let wt = tmp.path().join("linked");
    Repository::open(&repo).expect("open").worktree("linked", &wt, None).expect("worktree");
    assert_eq!(project_run(&home, &["which", s(&wt)]).expect("worktree"), "demo");
}

#[test]
fn which_refuses_an_unregistered_folder_with_the_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&other).expect("mkdir");
    let err = project_run(&home, &["which", s(&other)]).expect_err("none").to_string();
    assert!(err.contains("no registered project contains"), "{err}");
    assert!(err.contains(s(&other)), "{err}");
}

#[test]
fn which_fails_with_an_ambiguity_error_for_two_projects_on_one_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let repo = make_repo(tmp.path(), "demo-repo");
    project_run(&home, &["add", s(&repo), "--preset", "rust", "--name", "first"]).expect("add");
    // `project add` refuses a second name for one repo, so copy the project file by hand.
    let second = home.project_dir("second");
    std::fs::create_dir_all(&second).expect("mkdir");
    std::fs::copy(home.project_dir("first").join("project.toml"), second.join("project.toml")).expect("copy");
    // main.rs only maps an Err to exit code 1, so the Err from project::run is what is asserted.
    let err = project_run(&home, &["which", s(&repo)]).expect_err("ambiguous").to_string();
    assert!(err.contains("ambiguous"), "{err}");
    assert!(err.contains("first") && err.contains("second"), "{err}");
}

#[test]
fn install_writes_the_embedded_command_and_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("cmds");
    let out = install_args(&["--dir", s(&dir)]).expect("install");
    assert!(out.contains(s(&dir.join("factory.md"))), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("factory.md")).expect("read"), GLOBAL_COMMAND);
    let again = install_args(&["--dir", s(&dir)]).expect("again");
    assert!(again.contains("already installed"), "{again}");
}

#[test]
fn install_refuses_an_edited_file_unless_forced() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("cmds");
    install_args(&["--dir", s(&dir)]).expect("install");
    let file = dir.join("factory.md");
    std::fs::write(&file, "my edits").expect("edit");
    let err = install_args(&["--dir", s(&dir)]).expect_err("refused").to_string();
    assert!(err.contains(s(&file)), "{err}");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "my edits");
    install_args(&["--dir", s(&dir), "--force"]).expect("force");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), GLOBAL_COMMAND);
}

#[test]
fn install_into_an_unwritable_place_fails_and_leaves_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "x").expect("write");
    let dir = blocker.join("cmds");
    assert!(install_args(&["--dir", s(&dir)]).is_err());
    assert!(!dir.exists());
    let names: Vec<_> = std::fs::read_dir(tmp.path()).expect("ls").map(|e| e.expect("e").file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn install_without_home_and_dir_is_an_error() {
    let _g = EnvVarGuard::set("HOME", None);
    assert!(install_args(&[]).is_err());
    drop(_g);
    let _g = EnvVarGuard::set("HOME", Some(""));
    assert!(install_args(&[]).is_err());
}

#[test]
fn install_defaults_to_the_claude_commands_folder_under_home() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _g = EnvVarGuard::set("HOME", Some(s(tmp.path())));
    install_args(&[]).expect("install");
    let expected = factory_core::claude_commands_dir(tmp.path()).join("factory.md");
    assert!(expected.is_file());
}

#[test]
fn the_command_text_has_the_required_phrases_and_no_rocket_bits() {
    let text = GLOBAL_COMMAND;
    for needed in [
        "factory project which",
        "FACTORY_BIN",
        "--project",
        "superpowers:brainstorming",
        "superpowers:writing-plans",
        "at most 3 tasks",
        "explicit approval",
        "Do not write any plan before that approval",
        "Do not run",
        "git commit --only --",
        "dev-workflow-skills:1-git-commit",
        "factoryd is-running",
        "factoryd enqueue",
        "factoryd status",
        "daily budget",
        "--series",
        "run_in_background",
        "Do not poll",
        "exact shape",
        "`tickets <digits>` (nothing after the digits)",
        "<factory> --project <name> tickets <id>",
        "## Follow-ups",
        "`#N` or as the issue URL",
        "rework needs a note",
        "argument-hint: \"<feature> [--opus] | status | approve <id> | rework <id> <note> | cancel <id> | cleanup <id> | tickets <id>\"",
    ] {
        assert!(text.contains(needed), "missing phrase: {needed}");
    }
    let lower = text.to_lowercase();
    for banned in ["tools/factory", "cargo build", "opencollection", "factory-ctl", "python3", "set-stage", "sendmessage", "rocket"] {
        assert!(!lower.contains(banned), "forbidden phrase: {banned}");
    }
    assert!(text.starts_with("---\n"));
    assert!(text.contains("\ndescription: "));
}
