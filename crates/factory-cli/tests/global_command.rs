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
        EnvVarGuard {
            key,
            old,
            _lock: lock,
        }
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
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .expect("commit");
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
    project_run(
        &home,
        &["add", s(&repo), "--preset", "rust", "--name", "demo"],
    )
    .expect("add");
    assert_eq!(
        project_run(&home, &["which", s(&repo)]).expect("root"),
        "demo"
    );
    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir");
    assert_eq!(
        project_run(&home, &["which", s(&sub)]).expect("sub"),
        "demo"
    );
    // A real linked worktree made by git2, outside the repo.
    let wt = tmp.path().join("linked");
    Repository::open(&repo)
        .expect("open")
        .worktree("linked", &wt, None)
        .expect("worktree");
    assert_eq!(
        project_run(&home, &["which", s(&wt)]).expect("worktree"),
        "demo"
    );
}

#[test]
fn which_refuses_an_unregistered_folder_with_the_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&other).expect("mkdir");
    let err = project_run(&home, &["which", s(&other)])
        .expect_err("none")
        .to_string();
    assert!(err.contains("no registered project contains"), "{err}");
    assert!(err.contains(s(&other)), "{err}");
}

#[test]
fn which_fails_with_an_ambiguity_error_for_two_projects_on_one_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let repo = make_repo(tmp.path(), "demo-repo");
    project_run(
        &home,
        &["add", s(&repo), "--preset", "rust", "--name", "first"],
    )
    .expect("add");
    // `project add` refuses a second name for one repo, so copy the project file by hand.
    let second = home.project_dir("second");
    std::fs::create_dir_all(&second).expect("mkdir");
    std::fs::copy(
        home.project_dir("first").join("project.toml"),
        second.join("project.toml"),
    )
    .expect("copy");
    // main.rs only maps an Err to exit code 1, so the Err from project::run is what is asserted.
    let err = project_run(&home, &["which", s(&repo)])
        .expect_err("ambiguous")
        .to_string();
    assert!(err.contains("ambiguous"), "{err}");
    assert!(err.contains("first") && err.contains("second"), "{err}");
}

#[test]
fn install_writes_the_embedded_command_and_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("cmds");
    let out = install_args(&["--dir", s(&dir)]).expect("install");
    assert!(out.contains(s(&dir.join("factory.md"))), "{out}");
    assert_eq!(
        std::fs::read_to_string(dir.join("factory.md")).expect("read"),
        GLOBAL_COMMAND
    );
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
    let err = install_args(&["--dir", s(&dir)])
        .expect_err("refused")
        .to_string();
    assert!(err.contains(s(&file)), "{err}");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "my edits");
    install_args(&["--dir", s(&dir), "--force"]).expect("force");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        GLOBAL_COMMAND
    );
}

#[test]
fn install_into_an_unwritable_place_fails_and_leaves_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "x").expect("write");
    let dir = blocker.join("cmds");
    assert!(install_args(&["--dir", s(&dir)]).is_err());
    assert!(!dir.exists());
    let names: Vec<_> = std::fs::read_dir(tmp.path())
        .expect("ls")
        .map(|e| e.expect("e").file_name())
        .collect();
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
    assert_eq!(
        std::fs::read_to_string(expected).expect("Claude command"),
        GLOBAL_COMMAND
    );
    let skill = factory_core::codex_skills_dir(tmp.path())
        .join("factory")
        .join("SKILL.md");
    let text = std::fs::read_to_string(skill).expect("Codex skill");
    assert!(
        text.starts_with("---\nname: factory\ndescription: "),
        "{text}"
    );
    assert!(text.contains("$factory"), "{text}");
    assert!(
        text.contains("Use the user's accompanying request as the factory arguments."),
        "{text}"
    );
    for needed in [
        "Keep a visible task list for steps 1 to 6 and update it at each milestone.",
        "Interview the user about the design before writing the spec.",
        "Write the plans using the rules below.",
        "git commit --only -- <spec path>",
        "git commit --only -- <plan paths>",
        "git diff --cached --name-only",
        "stage the spec path",
        "stage the plan paths",
        "lists only that path",
        "lists only them",
        "wait for an explicit approval",
        "Do not write any plan before that approval",
        "Do not run `<factory> --project <name> new` before that approval",
        "Report every `warning:` line",
        "report every `note:` line",
        "Report that refusal as it is",
        "ask whether to go ahead over the budget",
        "as a long-running command, see the `Shell and background runs` note",
        "keep the handle for harness notification",
        "Do not poll, do not sleep, do not read logs",
    ] {
        assert!(text.contains(needed), "missing {needed:?} in Codex skill");
    }
    for unsupported in [
        "TaskCreate",
        "TaskUpdate",
        "dev-workflow-skills:1-git-commit",
        "run_in_background",
        "superpowers:brainstorming",
        "superpowers:writing-plans",
        "superpowers:subagent-driven-development",
    ] {
        assert!(
            !text.contains(unsupported),
            "unsupported {unsupported:?} in Codex skill"
        );
    }
}

#[test]
fn custom_codex_skill_does_not_block_claude_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _g = EnvVarGuard::set("HOME", Some(s(tmp.path())));
    let skill = factory_core::codex_skills_dir(tmp.path())
        .join("factory")
        .join("SKILL.md");
    std::fs::create_dir_all(skill.parent().expect("skill folder")).expect("mkdir");
    std::fs::write(&skill, "custom Codex skill").expect("write skill");

    let out = install_args(&[]).expect("Claude install succeeds");
    assert!(out.contains("Codex skill not installed"), "{out}");
    assert_eq!(
        std::fs::read_to_string(factory_core::claude_commands_dir(tmp.path()).join("factory.md"))
            .expect("Claude command"),
        GLOBAL_COMMAND
    );
    assert_eq!(
        std::fs::read_to_string(&skill).expect("custom skill survives"),
        "custom Codex skill"
    );
}

#[test]
fn codex_io_failure_is_reported_after_claude_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _g = EnvVarGuard::set("HOME", Some(s(tmp.path())));
    let skills = factory_core::codex_skills_dir(tmp.path());
    std::fs::create_dir_all(&skills).expect("skills folder");
    std::fs::write(skills.join("factory"), "blocking file").expect("block skill folder");

    let err = install_args(&[])
        .expect_err("Codex I/O failure must not report success")
        .to_string();
    assert!(err.contains("Codex skill install failed"), "{err}");
    assert_eq!(
        std::fs::read_to_string(factory_core::claude_commands_dir(tmp.path()).join("factory.md"))
            .expect("Claude command"),
        GLOBAL_COMMAND
    );
}

#[test]
fn codex_skill_does_not_select_a_claude_model() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _g = EnvVarGuard::set("HOME", Some(s(tmp.path())));
    install_args(&[]).expect("install");
    let skill = factory_core::codex_skills_dir(tmp.path())
        .join("factory")
        .join("SKILL.md");
    let text = std::fs::read_to_string(skill).expect("Codex skill");
    assert!(!text.contains("--model claude-opus-5-5"), "{text}");
    assert!(text.contains("`--opus` is a Claude-only option"), "{text}");
}

#[test]
fn codex_skill_reports_direct_run_results_from_the_command_result() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _g = EnvVarGuard::set("HOME", Some(s(tmp.path())));
    install_args(&[]).expect("install");
    let skill = factory_core::codex_skills_dir(tmp.path())
        .join("factory")
        .join("SKILL.md");
    let text = std::fs::read_to_string(skill).expect("Codex skill");
    let (_, run_steps) = text.split_once("5. Start the run.").expect("run steps");
    let (start, after_start) = run_steps
        .split_once("6. When the run ends")
        .expect("step 6");
    let status = after_start
        .split_once("The driver never asks questions")
        .expect("end of step 6")
        .0;

    assert!(start.contains("factoryd enqueue --project <name> --job <id> --series"));
    assert!(start.contains("then run `factoryd status` once"));
    assert!(start.contains("If it returns a still-running session or process handle"));
    assert!(start.contains("keep the handle for harness notification"));
    assert!(start.contains("Do not poll, do not sleep, do not read logs"));
    assert!(start.contains("If it returns a final exit/completion result"));
    assert!(start.contains("continue to step 6 immediately"));
    assert!(start.contains("Do not report a completed or failed process as running"));
    assert!(status.contains("run `<factory> --project <name> status` once"));
    assert!(status.contains("report the line for this job"));
    assert!(status.contains("If it is `needs-human`"));
    assert!(status.contains("$factory approve <id> --without-review"));
    assert!(status.contains("$factory rework <id> <note>"));
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
        "## Current state",
        "## Error handling and edge cases",
        "quality bar",
        "<factory> --project <name> project show <name>",
        "`plan rules:` block",
        "to every plan, as if listed with the rules below",
        "does not skip the interview",
        "Use the ticket and the code survey to ask sharper questions",
        // The 7.0.0 interview, the draft card and the spec the driver needs.
        "<factory> --project <name> spec start <slug> '<feature>'",
        "<factory> --project <name> spec stage <slug> writing-spec",
        "<factory> --project <name> spec stage <slug> writing-plans",
        "<factory> --project <name> spec resume <slug>",
        "`file <repo-relative path>",
        "never only digits",
        "the same slug",
        "recommended answer",
        "## Decisions",
        "`Decided by: user`",
        "`Decided by: call`",
        "builder check",
        "opt-out",
        "## Goal",
        "## Acceptance criteria",
        // The 7.0.0 plans, run by the driver.
        "superpowers:subagent-driven-development",
        "one fresh factory-builder implementer per task",
        "File Structure",
        "`**Interfaces:**`",
        "plan self-review",
        "at most 6 plans",
        "Next plan:",
        "`#N` or as the issue URL",
        "rework needs a note",
        "`approve <digits> --without-review`",
        "<factory> --project <name> approve <id> --without-review",
        "refuses a `needs-human` job whose code review has not run",
        "argument-hint: \"<feature> [--opus] [--one-job] | status | approve <id> | rework <id> <note> | cancel <id|slug> | cleanup <id> | tickets <id>\"",
        // The overrides of the brainstorming skill.
        "Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words.",
        "two or three alternatives",
        "because open questions were too hard to answer",
        "\"a single sentence with no list of example answers\"",
        "\"Offer a short menu only when they're stuck\"",
        "Red Flags row \"Options steer\"",
        "Always size it as a project with a written design",
        "If they still decline, run `<factory> --project <name> cancel <slug>` to remove the draft card and stop without building anything.",
        "Survey the code and the ticket first",
        // Drafts are found, resumed and cancelled through the command.
        "`cancel <slug>`",
        "<factory> --project <name> cancel <slug>",
        "When the `file` line says `exists`, read the file and continue from it, never overwrite it.",
        // A split feature closes its ticket once.
        "Only the last series carries `#N` or the issue URL in its feature text",
        "`<slug>-2`",
        "docs/superpowers/plans/<date>-<slug>-2/",
        // What `new` refuses and warns about.
        "Report every `warning:` line",
        "refuses a plan with `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences",
        // The plan header's executor line, exactly.
        "`> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.`",
    ] {
        assert!(text.contains(needed), "missing phrase: {needed}");
    }
    assert!(
        !text.contains("Everything else in the skill stands"),
        "the skill's quick paths must stay overridden"
    );
    // The status check comes before `spec start`, and the post-approval commit before writing-plans.
    let pos = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("missing phrase: {needle}"))
    };
    assert!(
        pos("Run `<factory> --project <name> status`")
            < pos("<factory> --project <name> spec start <slug>")
    );
    assert!(
        pos("After it, stage the spec path")
            < pos("<factory> --project <name> spec stage <slug> writing-plans")
    );
    assert!(
        pos("the commit after the spec approval comes first")
            < pos("Invoke `superpowers:writing-plans`")
    );
    // The 6.4.1 rules that 7.0.0 removed, and the inline executor.
    for removed in [
        "executing-plans",
        "architectural path",
        "never classify",
        "spec self-review",
        "Coverage gate",
        "Propose 2 to 3",
        "approval after each section",
        "2 to 5 minute",
        "one at a time",
        "purpose and who benefits",
        "n/a: <reason>",
        "## Rejected alternatives",
        "Do not use subagents for the interview",
    ] {
        assert!(
            !text.contains(removed),
            "removed phrase still present: {removed}"
        );
    }
    let lower = text.to_lowercase();
    for banned in [
        "tools/factory",
        "cargo build",
        "opencollection",
        "factory-ctl",
        "python3",
        "set-stage",
        "sendmessage",
        "rocket",
    ] {
        assert!(!lower.contains(banned), "forbidden phrase: {banned}");
    }
    assert!(text.starts_with("---\n"));
    assert!(text.contains("\ndescription: "));
}

#[test]
fn global_command_describes_one_job_and_consent() {
    let text = GLOBAL_COMMAND;
    for needed in [
        "[--one-job]",
        "`--one-job` when `--one-job` was given",
        "needs consent:",
        "--accept-over-budget",
        "at most 3 tasks",
        "Never edit `project.toml` yourself",
        "re-run the same `new` call with `--accept-over-budget`",
    ] {
        assert!(text.contains(needed), "missing phrase: {needed}");
    }
}

fn cmds_dir(tmp: &Path) -> PathBuf {
    let dir = tmp.join("commands");
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

#[test]
fn upgrade_into_replaces_a_different_command_and_keeps_a_bak() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    std::fs::write(&target, "# old\n").expect("write");
    let bak = dir.join("factory.md.bak.1");
    let first = dir.join("factory.md.bak");
    std::fs::write(&first, "# older bak\n").expect("write bak");
    let line = install::upgrade_into(&dir, false).expect("upgrade");
    assert_eq!(
        line,
        format!(
            "updated {} (old copy saved as {})",
            target.display(),
            bak.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&bak).expect("read bak"), "# old\n");
    assert_eq!(
        std::fs::read_to_string(&first).expect("first bak"),
        "# older bak\n"
    );
    assert_eq!(
        std::fs::read_to_string(&target).expect("read"),
        GLOBAL_COMMAND
    );
}

#[test]
fn upgrade_into_installs_an_absent_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    let line = install::upgrade_into(&dir, false).expect("upgrade");
    assert_eq!(line, format!("installed {}", target.display()));
    assert_eq!(
        std::fs::read_to_string(&target).expect("read"),
        GLOBAL_COMMAND
    );
    assert!(!dir.join("factory.md.bak").exists());
}

#[test]
fn upgrade_into_leaves_an_equal_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    std::fs::write(&target, GLOBAL_COMMAND).expect("write");
    let line = install::upgrade_into(&dir, false).expect("upgrade");
    assert_eq!(line, format!("already installed: {}", target.display()));
    assert!(!dir.join("factory.md.bak").exists());
}

#[test]
fn upgrade_into_dry_run_writes_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    std::fs::write(&target, "# old\n").expect("write");
    let line = install::upgrade_into(&dir, true).expect("dry");
    assert!(line.starts_with("would updated "), "{line}");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "# old\n");
    assert!(!dir.join("factory.md.bak").exists());
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).expect("mkdir");
    let line = install::upgrade_into(&empty, true).expect("dry");
    assert_eq!(
        line,
        format!("would installed {}", empty.join("factory.md").display())
    );
    assert!(!empty.join("factory.md").exists());
}

#[test]
fn upgrade_into_stops_when_the_bak_cannot_be_written() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    std::fs::write(&target, "# old\n").expect("write");
    let bak = dir.join("factory.md.bak");
    std::fs::create_dir_all(&bak).expect("mkdir bak");
    std::fs::write(bak.join("keep"), "x").expect("write keep");
    let err = install::upgrade_into(&dir, false)
        .expect_err("bak blocked")
        .to_string();
    assert!(err.contains("cannot save"), "{err}");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), "# old\n");
}

#[test]
fn install_force_still_writes_no_bak() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = cmds_dir(tmp.path());
    let target = dir.join("factory.md");
    std::fs::write(&target, "# old\n").expect("write");
    let out = install_args(&["--dir", s(&dir), "--force"]).expect("force");
    assert_eq!(out, format!("installed {}", target.display()));
    assert!(!dir.join("factory.md.bak").exists());
}
