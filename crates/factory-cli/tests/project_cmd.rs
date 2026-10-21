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
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .expect("commit");
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
    run(
        e,
        &[
            "add",
            s(&repo),
            "--preset",
            "rust",
            "--name",
            "demo",
            "--base",
            "main",
        ],
    )
    .expect("add");
    repo
}

#[test]
fn add_then_list() {
    let e = env();
    let repo = make_repo(e.tmp.path(), "r1", "main");
    let out = run(
        &e,
        &[
            "add",
            s(&repo),
            "--preset",
            "rust",
            "--name",
            "demo",
            "--base",
            "main",
        ],
    )
    .expect("add");
    assert!(out.contains(s(&e.home.project_dir("demo"))), "{out}");
    assert!(e.home.presets_dir().join("rust/config.toml").exists());
    let canon = std::fs::canonicalize(&repo).expect("canon");
    assert_eq!(
        run(&e, &["list"]).expect("list"),
        format!("demo rust main {}", canon.display())
    );
}

#[test]
fn add_records_the_current_branch_when_no_base_is_given() {
    let e = env();
    let repo = make_repo(e.tmp.path(), "r1", "trunk");
    run(&e, &["add", s(&repo), "--preset", "go", "--name", "demo"]).expect("add");
    assert!(run(&e, &["list"])
        .expect("list")
        .starts_with("demo go trunk "));
}

#[test]
fn the_name_defaults_to_the_lowercased_folder() {
    let e = env();
    let ok = make_repo(e.tmp.path(), "Good-Repo", "main");
    run(&e, &["add", s(&ok), "--preset", "rust"]).expect("add");
    assert!(run(&e, &["list"]).expect("list").starts_with("good-repo "));
    let bad = make_repo(e.tmp.path(), "My Repo", "main");
    let err = run(&e, &["add", s(&bad), "--preset", "rust"])
        .expect_err("refused")
        .to_string();
    assert!(err.contains("--name"), "{err}");
}

#[test]
fn add_refusals() {
    let e = env();
    let plain = e.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    assert!(run(&e, &["add", s(&plain), "--preset", "rust", "--name", "p"]).is_err());
    let repo = make_repo(e.tmp.path(), "r1", "main");
    let err = run(&e, &["add", s(&repo), "--preset", "nope", "--name", "p"])
        .expect_err("preset")
        .to_string();
    assert!(err.contains("unknown preset"), "{err}");
    let err = run(
        &e,
        &[
            "add",
            s(&repo),
            "--preset",
            "rust",
            "--name",
            "p",
            "--base",
            "ghost",
        ],
    )
    .expect_err("base")
    .to_string();
    assert!(err.contains("ghost"), "{err}");
    run(&e, &["add", s(&repo), "--preset", "rust", "--name", "p"]).expect("add");
    let other = make_repo(e.tmp.path(), "r2", "main");
    let err = run(&e, &["add", s(&other), "--preset", "rust", "--name", "p"])
        .expect_err("dup name")
        .to_string();
    assert!(err.contains("already exists"), "{err}");
    let err = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "q"])
        .expect_err("dup repo")
        .to_string();
    assert!(err.contains("project p already covers"), "{err}");
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
    assert!(
        out.contains("demo  disabled: repo path is missing"),
        "{out}"
    );
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
        "tickets on merge: false".to_string(),
        "agent files:".to_string(),
        "problems: none".to_string(),
    ] {
        assert!(out.contains(&want), "missing {want:?} in\n{out}");
    }
    assert!(out.contains("factory-builder.md"), "{out}");
    assert!(!out.contains("degraded"), "{out}");
}

fn set_plan_rules(e: &Env, line: &str) {
    // The `[settings]` table is the last table of the file, so a line appended here is a setting.
    let path = e.home.project_dir("demo").join("project.toml");
    let mut text = std::fs::read_to_string(&path).expect("read");
    text.push_str(line);
    std::fs::write(&path, text).expect("write");
}

#[test]
fn show_prints_the_plan_rules_block_after_the_agent_files_line() {
    let e = env();
    add_demo(&e);
    set_plan_rules(&e, "plan_rules = [\"first rule\", \"second rule\"]\n");
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(
        out.contains("\nplan rules:\n- first rule\n- second rule\n"),
        "{out}"
    );
    let agents = out.find("agent files:").expect("agent files");
    assert!(
        agents < out.find("plan rules:").expect("plan rules"),
        "{out}"
    );
    assert!(!out.contains("degraded"), "{out}");
    assert!(out.contains("problems: none"), "{out}");
}

#[test]
fn show_prints_nothing_extra_without_plan_rules() {
    let e = env();
    add_demo(&e);
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(!out.contains("plan rules"), "{out}");
}

#[test]
fn show_reports_a_missing_base_branch_as_degraded() {
    let e = env();
    add_demo(&e);
    let file = e.home.project_dir("demo").join("project.toml");
    let text = std::fs::read_to_string(&file)
        .expect("read")
        .replace("\"main\"", "\"gone\"");
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
    index
        .add_path(Path::new(".factory/override.toml"))
        .expect("add");
    let tree = git
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    let parent = git.head().expect("head").peel_to_commit().expect("commit");
    git.commit(Some("HEAD"), &sig, &sig, "override", &tree, &[&parent])
        .expect("commit");
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
    board.jobs.push(factory_core::Job::new(
        "001",
        "2026-10-13-001-x",
        "x",
        "2026-10-13",
        "feature",
        0.0,
    ));
    board.save(&state.join("board.json")).expect("save");
    let err = run(&e, &["remove", "demo"])
        .expect_err("refused")
        .to_string();
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
    assert_eq!(
        run(&e, &["list"]).expect("list"),
        format!("real rust main {}", canon.display())
    );
}

/// Restores the process working directory when dropped, also when the test panics.
struct CwdGuard(PathBuf);

impl CwdGuard {
    fn change_to(dir: &Path) -> CwdGuard {
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir).expect("chdir");
        CwdGuard(old)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[test]
fn add_accepts_a_relative_path() {
    // The current directory is process wide, so only this test changes it.
    let e = env();
    make_repo(e.tmp.path(), "rel", "main");
    let out = {
        let _cwd = CwdGuard::change_to(&e.tmp.path().join("rel"));
        run(&e, &["add", ".", "--preset", "rust"])
    };
    out.expect("add");
    assert!(run(&e, &["list"])
        .expect("list")
        .starts_with("rel rust main "));
}

#[test]
fn add_refuses_a_path_that_does_not_exist() {
    let e = env();
    let err = run(
        &e,
        &["add", "/no/such/dir", "--preset", "rust", "--name", "x"],
    )
    .expect_err("refused")
    .to_string();
    assert!(err.contains("not usable"), "{err}");
}

/// Commits `entries` (path, content, filemode) on top of HEAD, creating folders in the tree.
fn commit_entries(repo: &Path, entries: &[(&str, &[u8], i32)]) {
    let git = Repository::open(repo).expect("open");
    let parent = git.head().expect("head").peel_to_commit().expect("commit");
    let mut index = git.index().expect("index");
    for (path, content, mode) in entries {
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: *mode as u32,
            uid: 0,
            gid: 0,
            file_size: content.len() as u32,
            id: git2::Oid::zero(),
            flags: 0,
            flags_extended: 0,
            path: path.as_bytes().to_vec(),
        };
        index.add_frombuffer(&entry, content).expect("add");
    }
    let tree = git
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    git.commit(Some("HEAD"), &sig, &sig, "files", &tree, &[&parent])
        .expect("commit");
}

#[test]
fn stray_agent_files_are_noted_and_do_not_degrade() {
    let e = env();
    let repo = add_demo(&e);
    std::fs::write(e.home.project_dir("demo").join("agents/README.md"), "stray").expect("write");
    commit_entries(&repo, &[(".factory/agents/README.md", b"stray", 0o100644)]);
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(
        out.contains("note: ignored agent file README.md: names must be factory-<name>.md"),
        "{out}"
    );
    assert!(!out.contains("degraded"), "{out}");
    assert!(out.contains("problems: none"), "{out}");
    assert!(
        !out.contains("agent files: README.md") && !out.contains(", README.md"),
        "{out}"
    );
}

#[test]
fn a_committed_symlink_override_degrades_the_project() {
    let e = env();
    let repo = add_demo(&e);
    commit_entries(
        &repo,
        &[(".factory/override.toml", b"/etc/passwd", 0o120000)],
    );
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(out.contains("degraded"), "{out}");
    assert!(out.contains("not a regular file"), "{out}");
}

#[test]
fn a_committed_symlink_agent_file_degrades_the_project() {
    let e = env();
    let repo = add_demo(&e);
    commit_entries(
        &repo,
        &[(
            ".factory/agents/factory-builder.md",
            b"/etc/passwd",
            0o120000,
        )],
    );
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(out.contains("degraded"), "{out}");
    assert!(out.contains("not a regular file"), "{out}");
}

#[test]
fn show_prints_the_version_after_the_project_line() {
    let e = env();
    add_demo(&e);
    let out = run(&e, &["show", "demo"]).expect("show");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "project: demo", "{out}");
    assert_eq!(
        lines[1],
        format!("version: {}", factory_core::version::FACTORY_VERSION),
        "{out}"
    );
}

#[test]
fn disabled_show_prints_the_version_after_the_project_line() {
    let e = env();
    let repo = add_demo(&e);
    std::fs::remove_dir_all(&repo).expect("rm");
    let out = run(&e, &["show", "demo"]).expect("show");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "project: demo", "{out}");
    assert_eq!(
        lines[1],
        format!("version: {}", factory_core::version::FACTORY_VERSION),
        "{out}"
    );
    assert!(out.contains("disabled: repo path is missing"), "{out}");
}

#[test]
fn show_of_a_project_with_a_missing_repo_is_disabled_like_list() {
    let e = env();
    let repo = add_demo(&e);
    std::fs::remove_dir_all(&repo).expect("rm");
    let out = run(&e, &["show", "demo"]).expect("show succeeds");
    assert!(out.contains("disabled: repo path is missing"), "{out}");
    assert!(out.contains("project: demo"), "{out}");
    assert!(run(&e, &["list"])
        .expect("list")
        .contains("demo  disabled: repo path is missing"));
}

#[test]
fn remove_of_a_broken_project_needs_force() {
    let e = env();
    add_demo(&e);
    std::fs::write(
        e.home.project_dir("demo").join("project.toml"),
        "this is = = broken",
    )
    .expect("write");
    let err = run(&e, &["remove", "demo"])
        .expect_err("refused")
        .to_string();
    assert!(err.contains("--force"), "{err}");
    assert!(e.home.project_dir("demo").exists());
    run(&e, &["remove", "demo", "--force"]).expect("forced");
    assert!(!e.home.project_dir("demo").exists());
    assert!(
        run(&e, &["remove", "demo", "--force"]).is_err(),
        "an unknown project is still an error"
    );
}

#[test]
fn remove_prunes_worktree_registrations_and_leaves_job_branches() {
    use factory_git::{JobRef, Repo};
    let e = env();
    let repo_path = add_demo(&e);
    let worktrees = e.home.project_dir("demo").join("worktrees");
    let repo = Repo::open(&repo_path)
        .expect("open")
        .with_worktrees_dir(worktrees.clone());
    let valid = repo
        .create_worktree(&JobRef {
            id: "001".into(),
            slug: "valid".into(),
            project: None,
        })
        .expect("wt");
    let stale = repo
        .create_worktree(&JobRef {
            id: "002".into(),
            slug: "stale".into(),
            project: None,
        })
        .expect("wt");
    std::fs::remove_dir_all(&stale.path).expect("stale folder gone");
    let out = run(&e, &["remove", "demo"]).expect("remove");
    assert!(out.contains("pruned 2 worktree registration(s)"), "{out}");
    assert!(
        out.contains("factory/001-valid") && out.contains("factory/002-stale"),
        "{out}"
    );
    assert!(!e.home.project_dir("demo").exists());
    assert!(!valid.path.exists());
    let raw = Repository::open(&repo_path).expect("open");
    assert!(raw.worktrees().expect("worktrees").is_empty());
    assert!(
        raw.find_branch("factory/001-valid", git2::BranchType::Local)
            .is_ok(),
        "branches are never deleted"
    );
    assert!(raw
        .find_branch("factory/002-stale", git2::BranchType::Local)
        .is_ok());
}

#[test]
fn show_reads_the_override_from_the_base_tip_not_from_head() {
    let e = env();
    let repo = add_demo(&e);
    commit_entries(
        &repo,
        &[(
            ".factory/override.toml",
            b"[[checks]]\npaths = [\"**\"]\ncommands = [\"cargo test --workspace\"]\n",
            0o100644,
        )],
    );
    let git = Repository::open(&repo).expect("open");
    let head = git.head().expect("head").peel_to_commit().expect("commit");
    git.branch("side", &head, false).expect("branch");
    git.set_head("refs/heads/side").expect("set head");
    git.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .expect("checkout");
    commit_entries(
        &repo,
        &[(
            ".factory/override.toml",
            b"risk_paths = [\"docs/**\"]\n",
            0o100644,
        )],
    );
    assert_eq!(git.head().expect("head").shorthand(), Some("side"));
    let out = run(&e, &["show", "demo"]).expect("show");
    assert!(
        out.contains("would be rejected"),
        "the base branch override must win:\n{out}"
    );
}

#[test]
fn remove_is_refused_while_the_project_lease_is_held_even_with_force() {
    use factory_engine::{acquire_project_lease, now_secs};
    let e = env();
    add_demo(&e);
    let state = e.home.project_dir("demo").join("state");
    let wait = std::time::Duration::from_secs(5);
    acquire_project_lease(&state, "001", "someone-else", 600.0, now_secs(), wait).expect("lease");
    for args in [&["remove", "demo"][..], &["remove", "demo", "--force"][..]] {
        let err = run(&e, args).expect_err("busy").to_string();
        assert!(
            err.contains("busy")
                && err.contains("cancel 001 --force")
                && err.contains("project.lease"),
            "{err}"
        );
        assert!(e.home.project_dir("demo").exists());
    }
    // A dead driver's lease does not block the removal.
    std::fs::remove_file(state.join("project.lease")).expect("rm");
    acquire_project_lease(
        &state,
        "001",
        "driver-4294967000-1",
        600.0,
        now_secs(),
        wait,
    )
    .expect("lease");
    run(&e, &["remove", "demo", "--force"]).expect("removed");
    assert!(!e.home.project_dir("demo").exists());
}

#[test]
fn add_writes_the_version_marker() {
    let e = env();
    add_demo(&e);
    assert_eq!(
        factory_core::version::read_marker(&e.home).expect("read"),
        Some(factory_core::version::FACTORY_VERSION.to_string())
    );
}

#[test]
fn a_refused_add_writes_no_marker() {
    let e = env();
    let plain = e.tmp.path().join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    run(&e, &["add", s(&plain), "--preset", "rust", "--name", "p"]).expect_err("refused");
    assert!(!e.home.version_file().exists());
}

#[test]
fn add_keeps_a_newer_marker() {
    let e = env();
    std::fs::create_dir_all(&e.home.dir).expect("mkdir");
    std::fs::write(e.home.version_file(), "99.0.0\n").expect("marker");
    add_demo(&e);
    assert_eq!(
        std::fs::read_to_string(e.home.version_file()).expect("read"),
        "99.0.0\n"
    );
}

#[test]
fn add_succeeds_with_a_note_when_the_marker_cannot_be_written() {
    let e = env();
    std::fs::create_dir_all(e.home.version_file()).expect("mkdir");
    let repo = make_repo(e.tmp.path(), "r1", "main");
    let out = run(
        &e,
        &[
            "add",
            s(&repo),
            "--preset",
            "rust",
            "--name",
            "demo",
            "--base",
            "main",
        ],
    )
    .expect("add");
    assert!(out.contains("added project demo"), "{out}");
    assert!(out.contains("note: version marker not written:"), "{out}");
    assert!(run(&e, &["list"])
        .expect("list")
        .starts_with("demo rust main"));
}

/// A repo with committed `apps/web/w.txt` and `apps/api/x.txt` files.
fn make_monorepo(e: &Env) -> PathBuf {
    let repo = make_repo(e.tmp.path(), "mono", "main");
    let r = Repository::open(&repo).expect("open");
    for (dir, file) in [("apps/web", "w.txt"), ("apps/api", "x.txt")] {
        std::fs::create_dir_all(repo.join(dir)).expect("mkdir");
        std::fs::write(repo.join(dir).join(file), "x").expect("write");
    }
    let mut index = r.index().expect("index");
    index
        .add_all(["apps"], git2::IndexAddOption::DEFAULT, None)
        .expect("add");
    let tree = r
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    let parent = r.head().expect("head").peel_to_commit().expect("commit");
    r.commit(Some("HEAD"), &sig, &sig, "apps", &tree, &[&parent])
        .expect("commit");
    repo
}

#[test]
fn add_accepts_a_subfolder() {
    let e = env();
    let repo = make_monorepo(&e);
    let web = repo.join("apps/web");
    let out = run(&e, &["add", s(&web), "--preset", "rust"]).expect("add");
    assert!(out.contains("added project web "), "{out}");
    assert!(out.contains(" in apps/web"), "{out}");
    let text = std::fs::read_to_string(e.home.project_dir("web").join("project.toml"))
        .expect("project.toml");
    assert!(text.contains("subpath = \"apps/web\""), "{text}");
    let canon = std::fs::canonicalize(&repo).expect("canon");
    assert!(
        text.contains(&format!("repo = {:?}", canon.display().to_string())),
        "{text}"
    );
    let listed = run(&e, &["list"]).expect("list");
    assert!(listed.contains("apps/web"), "{listed}");
    let shown = run(&e, &["show", "web"]).expect("show");
    assert!(shown.contains("subpath: apps/web"), "{shown}");
}

#[test]
fn add_refuses_overlap_and_root_after_subfolder() {
    let e = env();
    let repo = make_monorepo(&e);
    run(&e, &["add", s(&repo.join("apps/web")), "--preset", "rust"]).expect("add web");
    let err = run(&e, &["add", s(&repo), "--preset", "rust", "--name", "root"])
        .expect_err("root after subfolder")
        .to_string();
    assert!(err.contains("web"), "{err}");
}

#[test]
fn list_shows_subfolder_project_disabled_when_folder_is_gone() {
    let e = env();
    let repo = make_monorepo(&e);
    run(&e, &["add", s(&repo.join("apps/web")), "--preset", "rust"]).expect("add");
    std::fs::remove_dir_all(repo.join("apps/web")).expect("rm");
    let out = run(&e, &["list"]).expect("list");
    assert!(
        out.contains("web  disabled: project folder is missing"),
        "{out}"
    );
}

#[test]
fn which_resolves_the_deepest_project() {
    let e = env();
    let repo = make_monorepo(&e);
    run(&e, &["add", s(&repo.join("apps/web")), "--preset", "rust"]).expect("add");
    std::fs::create_dir_all(repo.join("apps/web/src")).expect("mkdir");
    let out = run(&e, &["which", s(&repo.join("apps/web/src"))]).expect("which");
    assert_eq!(out, "web");
}
