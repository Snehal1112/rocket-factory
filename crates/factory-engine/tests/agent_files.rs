mod common;

use std::sync::{Arc, Mutex};

use common::{commit_file, committing_fixture, scripted_auto, Effect, Fixture};
use factory_core::{AgentFile, Stage};
use factory_engine::{EngineError, StageOutcome};
use git2::{Repository, StatusOptions};

const BUILDER: &str = ".claude/agents/factory-builder.md";

fn files() -> Vec<AgentFile> {
    vec![
        AgentFile {
            name: "factory-builder.md".into(),
            content: "BUILDER".into(),
        },
        AgentFile {
            name: "factory-reviewer.md".into(),
            content: "REVIEWER".into(),
        },
    ]
}

/// A builder that records the agent file text it sees, then writes a change.
fn recording_effect(seen: Arc<Mutex<Vec<Option<String>>>>) -> Effect {
    Box::new(move |spec| {
        let text = std::fs::read_to_string(spec.cwd.join(BUILDER)).ok();
        seen.lock().expect("seen").push(text);
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    })
}

#[test]
fn files_exist_before_the_builder_runs_and_stay_out_of_the_commit() {
    let fx = committing_fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen.clone())]);
    let out = fx
        .engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("BUILDER".to_string())]
    );

    let job = fx.job();
    let wt = std::path::PathBuf::from(job.worktree.clone().expect("worktree"));
    assert_eq!(
        std::fs::read_to_string(wt.join(".claude/agents/factory-reviewer.md")).expect("reviewer"),
        "REVIEWER"
    );
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let repo = Repository::open(&wt).expect("open wt");
    let st = repo.statuses(Some(&mut opts)).expect("statuses");
    assert!(
        st.iter()
            .all(|e| !e.path().unwrap_or("").contains("factory-")),
        "agent files must be excluded"
    );
    let head = repo
        .find_commit(git2::Oid::from_str(&job.head.expect("head")).expect("oid"))
        .expect("commit");
    let tree = head.tree().expect("tree");
    assert!(tree
        .get_path(std::path::Path::new(".claude/agents"))
        .is_err());
    assert!(tree.get_path(std::path::Path::new("a.txt")).is_ok());
}

#[test]
fn a_file_tracked_on_the_base_branch_is_not_overwritten() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "REPO");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen.clone())]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("REPO".to_string())]
    );
    let wt = std::path::PathBuf::from(fx.job().worktree.expect("worktree"));
    assert_eq!(
        std::fs::read_to_string(wt.join(".claude/agents/factory-reviewer.md")).expect("reviewer"),
        "REVIEWER"
    );
}

#[test]
fn no_files_means_no_agents_folder() {
    let fx = committing_fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen)]);
    fx.engine(&runner).build_stage().expect("build");
    let wt = std::path::PathBuf::from(fx.job().worktree.expect("worktree"));
    assert!(!wt.join(".claude/agents").exists());
}

const TWO_TASKS: &str = "# P\n\n### Task 1: first\n\nx\n\n### Task 2: second\n\ny\n";

fn write_effect(file: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "x\n").expect("write");
        "done".to_string()
    })
}

fn worktree(fx: &Fixture) -> std::path::PathBuf {
    std::path::PathBuf::from(fx.job().worktree.expect("worktree"))
}

#[test]
fn a_later_build_step_restores_edited_files() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    let wt = worktree(&fx);
    std::fs::write(wt.join(BUILDER), "EDITED").expect("edit");
    e.build_stage().expect("second");
    assert_eq!(
        std::fs::read_to_string(wt.join(BUILDER)).expect("read"),
        "BUILDER"
    );
}

#[test]
fn a_file_edited_by_the_builder_is_restored_before_the_reviewer_runs() {
    let fx = committing_fixture();
    let tamper: Effect = Box::new(|spec| {
        std::fs::write(
            spec.cwd.join(".claude/agents/factory-reviewer.md"),
            "WEAKENED",
        )
        .expect("tamper");
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    });
    let runner = scripted_auto(vec![tamper]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(files())
        .checks_stage()
        .expect("checks");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let reviewer = scripted_auto(vec![Box::new(move |spec| {
        seen2.lock().expect("seen").push(
            std::fs::read_to_string(spec.cwd.join(".claude/agents/factory-reviewer.md")).ok(),
        );
        "ok\nVERDICT: APPROVE".to_string()
    })]);
    fx.engine(&reviewer)
        .with_agent_files(files())
        .review_stage()
        .expect("review");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("REVIEWER".to_string())]
    );
}

#[test]
fn a_symlink_at_an_agent_file_path_is_replaced_not_followed() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    let wt = worktree(&fx);
    let outside = fx.tmp.path().join("outside.md");
    std::fs::write(&outside, "OUTSIDE").expect("outside");
    std::fs::remove_file(wt.join(BUILDER)).expect("rm");
    std::os::unix::fs::symlink(&outside, wt.join(BUILDER)).expect("symlink");
    e.build_stage().expect("second");
    assert_eq!(std::fs::read_to_string(&outside).expect("read"), "OUTSIDE");
    assert_eq!(
        std::fs::read_to_string(wt.join(BUILDER)).expect("read"),
        "BUILDER"
    );
}

#[test]
fn a_resumed_step_writes_files_missed_by_the_first_step() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let second: Effect = {
        let seen = seen.clone();
        Box::new(move |spec| {
            seen.lock()
                .expect("seen")
                .push(std::fs::read_to_string(spec.cwd.join(BUILDER)).ok());
            std::fs::write(spec.cwd.join("b.txt"), "b\n").expect("write");
            "done".to_string()
        })
    };
    let runner = scripted_auto(vec![write_effect("a.txt"), second]);
    // The first step runs without files, as after a crash that lost them.
    fx.engine(&runner).build_stage().expect("first");
    assert!(!worktree(&fx).join(".claude/agents").exists());
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("second");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("BUILDER".to_string())]
    );
}

#[test]
fn a_write_failure_on_a_resumed_step_is_an_error() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner).build_stage().expect("first");
    let wt = worktree(&fx);
    std::fs::create_dir_all(wt.join(".claude")).expect("mkdir");
    std::os::unix::fs::symlink(fx.tmp.path(), wt.join(".claude/agents")).expect("symlink");
    let err = fx
        .engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect_err("symlink");
    assert!(err.to_string().contains("symlink"), "{err}");
}

#[test]
fn a_tracked_file_missing_from_the_worktree_is_not_recreated_and_is_noted_once() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "REPO");
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    let wt = worktree(&fx);
    // Only the tracked rule keeps the file away now, create_new alone would write it.
    std::fs::remove_file(wt.join(BUILDER)).expect("delete");
    e.build_stage().expect("second");
    assert!(!wt.join(BUILDER).exists());
    assert!(wt.join(".claude/agents/factory-reviewer.md").exists());
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    let note = "repo agent files shadow the resolved ones: factory-builder.md";
    assert_eq!(ledger.matches(note).count(), 1, "{ledger}");
}

#[test]
fn names_outside_the_allow_list_are_refused() {
    for bad in [
        "../evil.md",
        "builder.md",
        "factory-x.txt",
        "factory-A.md",
        "factory-a/b.md",
        "factory-.md",
    ] {
        let fx: Fixture = committing_fixture();
        let runner = scripted_auto(vec![]);
        let list = vec![AgentFile {
            name: bad.into(),
            content: "x".into(),
        }];
        let err = fx
            .engine(&runner)
            .with_agent_files(list)
            .build_stage()
            .expect_err(bad);
        assert!(
            matches!(&err, EngineError::Msg(m) if m.contains(bad)),
            "{bad}: {err}"
        );
        assert!(fx.job().worktree.is_none(), "no worktree for {bad}");
    }
}

#[test]
fn a_resumed_step_refuses_bad_names_and_writes_nothing() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    fx.engine(&runner).build_stage().expect("first");
    let wt = worktree(&fx);
    for bad in ["../evil.md", "Factory-X.md", "factory-x.md.sh"] {
        let list = vec![
            AgentFile {
                name: "factory-good.md".into(),
                content: "G".into(),
            },
            AgentFile {
                name: bad.into(),
                content: "E".into(),
            },
        ];
        let err = fx
            .engine(&runner)
            .with_agent_files(list)
            .build_stage()
            .expect_err(bad);
        assert!(
            matches!(&err, EngineError::Msg(m) if m.contains(bad)),
            "{bad}: {err}"
        );
        assert!(!wt.join(".claude").exists(), "nothing created for {bad}");
        assert!(!wt.join("evil.md").exists() && !wt.join(".claude/evil.md").exists());
        assert!(!wt.join("../evil.md").exists());
    }
}

#[test]
fn a_hardlinked_agent_file_is_replaced_not_written_through() {
    use std::os::unix::fs::MetadataExt;
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    let wt = worktree(&fx);
    let outside = fx.tmp.path().join("outside-link.md");
    std::fs::write(&outside, "OUTSIDE").expect("outside");
    std::fs::remove_file(wt.join(BUILDER)).expect("rm");
    std::fs::hard_link(&outside, wt.join(BUILDER)).expect("hard link");
    e.build_stage().expect("second");
    assert_eq!(std::fs::read_to_string(&outside).expect("read"), "OUTSIDE");
    assert_eq!(
        std::fs::read_to_string(wt.join(BUILDER)).expect("read"),
        "BUILDER"
    );
    let own = std::fs::metadata(wt.join(BUILDER)).expect("meta").ino();
    assert_ne!(own, std::fs::metadata(&outside).expect("meta").ino());
}

#[test]
fn a_symlinked_claude_folder_is_refused() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner).build_stage().expect("first");
    let wt = worktree(&fx);
    let elsewhere = fx.tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("mkdir");
    std::os::unix::fs::symlink(&elsewhere, wt.join(".claude")).expect("symlink");
    let err = fx
        .engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect_err("symlink");
    assert!(err.to_string().contains("symlink"), "{err}");
    assert!(
        std::fs::read_dir(&elsewhere).expect("dir").next().is_none(),
        "nothing written through the link"
    );
}

#[test]
fn agent_files_are_not_written_into_a_folder_that_is_not_a_git_checkout() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    let wt = worktree(&fx);
    std::fs::remove_dir_all(&wt).expect("rm worktree");
    std::fs::create_dir_all(&wt).expect("empty folder");
    let err = e.build_stage().expect_err("no checkout");
    assert!(err.to_string().contains("not a git checkout"), "{err}");
    assert!(
        !wt.join(".claude").exists(),
        "nothing is created in the folder"
    );
}
