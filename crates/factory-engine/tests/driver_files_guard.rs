mod common;

use common::{committing_fixture, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{Board, ParkRecord, Stage};
use factory_engine::{restore_file, StageOutcome};

fn change_effect(file: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "line 0\n").expect("write change");
        "done".to_string()
    })
}

fn expect_parked(out: StageOutcome, want: &str) {
    match out {
        StageOutcome::NeedsHuman(reason) => assert_eq!(reason, want),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn builder_changing_plan_md_is_restored_and_parked() {
    let fx = committing_fixture();
    let original = std::fs::read_to_string(&fx.plan_file).expect("plan");
    let plan = fx.plan_file.clone();
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        std::fs::write(&plan, "# rewritten\n").expect("write");
        "done".to_string()
    })]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed plan.md",
    );
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        original
    );
}

#[test]
fn builder_changing_progress_md_is_restored_and_parked() {
    let fx = committing_fixture();
    let progress = fx.job_dir.join("progress.md");
    std::fs::write(&progress, "Task 8: Done\n").expect("progress");
    let before = std::fs::read_to_string(&progress).expect("read");
    let p = progress.clone();
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        let mut text = std::fs::read_to_string(&p).expect("read");
        text.push_str("Task 9: Done\n");
        std::fs::write(&p, text).expect("write");
        "done".to_string()
    })]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed progress.md",
    );
    // The driver appends its own ruling line after the restore.
    let after = std::fs::read_to_string(&progress).expect("read");
    assert!(after.starts_with(&before), "{after}");
    assert!(!after.contains("Task 9"), "{after}");
}

#[test]
fn a_created_job_file_is_removed_and_parks() {
    let fx = committing_fixture();
    let spec_file = fx.job_dir.join("spec.md");
    assert!(!spec_file.exists());
    let s = spec_file.clone();
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        std::fs::write(&s, "new spec\n").expect("write");
        "done".to_string()
    })]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed spec.md",
    );
    assert!(!spec_file.exists());
}

#[test]
fn unchanged_job_files_do_not_park() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt")]);
    let out = fx.engine(&runner).build_stage().expect("stage");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
}

#[test]
fn absent_job_file_stays_absent() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt")]);
    let out = fx.engine(&runner).build_stage().expect("stage");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    assert!(!fx.job_dir.join("spec.md").exists());
}

#[test]
fn guard_holds_with_builder_permissions_false() {
    let mut fx = committing_fixture();
    fx.config.builder_permissions = false;
    let original = std::fs::read_to_string(&fx.plan_file).expect("plan");
    let plan = fx.plan_file.clone();
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        std::fs::write(&plan, "# rewritten\n").expect("write");
        "done".to_string()
    })]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed plan.md",
    );
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        original
    );
}

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn two_task_fixture() -> Fixture {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let mut board = Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

fn rewrite_plan_effect(plan: std::path::PathBuf) -> Effect {
    Box::new(move |_spec| {
        std::fs::write(&plan, "# rewritten\n").expect("write");
        "done".to_string()
    })
}

/// One clean first build, then round 2 whose builder rewrites `plan.md`.
fn fix_round_fixture() -> (Fixture, ScriptedRunner) {
    let fx = committing_fixture();
    let second = rewrite_plan_effect(fx.plan_file.clone());
    let runner = scripted_auto(vec![change_effect("a.txt"), second]);
    fx.engine(&runner).build_stage().expect("first");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    (fx, runner)
}

#[test]
fn guard_holds_in_a_per_task_run() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![rewrite_plan_effect(fx.plan_file.clone())]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed plan.md",
    );
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        TWO_TASKS
    );
}

#[test]
fn guard_holds_in_a_fix_round_resume() {
    let (fx, runner) = fix_round_fixture();
    let original = std::fs::read_to_string(&fx.plan_file).expect("plan");
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed plan.md",
    );
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        original
    );
}

#[test]
fn a_tampered_fix_round_parks_with_rework_only() {
    let (fx, runner) = fix_round_fixture();
    fx.engine(&runner).build_stage().expect("stage");
    let record = ParkRecord::read(&fx.job_dir).expect("park record");
    assert_eq!(record.kind.as_str(), "builder-tampered");
    assert!(
        !record.next.iter().any(|n| n.contains("approve")),
        "{record:?}"
    );
    assert!(
        record.next.iter().any(|n| n.contains("rework")),
        "{record:?}"
    );
}

#[test]
fn a_guard_park_keeps_the_builder_session() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![rewrite_plan_effect(fx.plan_file.clone())]);
    fx.engine(&runner).build_stage().expect("stage");
    assert_eq!(fx.job().builder_id.as_deref(), Some("sess-1"));
}

#[test]
fn a_runner_error_still_restores_the_job_files() {
    let fx = committing_fixture();
    let original = std::fs::read_to_string(&fx.plan_file).expect("plan");
    let (plan, board) = (fx.plan_file.clone(), fx.board_path.clone());
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        std::fs::write(&plan, "# rewritten\n").expect("write");
        std::fs::remove_file(&board).expect("remove board");
        "done".to_string()
    })]);
    fx.engine(&runner).build_stage().expect_err("board gone");
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        original
    );
}

#[test]
fn restore_reports_a_failed_write() {
    let tmp = tempfile::tempdir().expect("tmp");
    let err = restore_file(&tmp.path().join("missing-dir").join("plan.md"), Some(b"x"))
        .expect_err("write must fail");
    assert!(!err.is_empty());
    let file = tmp.path().join("f.md");
    std::fs::write(&file, "x").expect("write");
    restore_file(&file, None).expect("remove");
    assert!(!file.exists());
}

#[test]
fn two_changed_files_are_both_restored_and_the_first_is_named() {
    let fx = committing_fixture();
    let progress = fx.job_dir.join("progress.md");
    std::fs::write(&progress, "Task 8: Done\n").expect("progress");
    let plan_before = std::fs::read_to_string(&fx.plan_file).expect("plan");
    let (p, plan) = (progress.clone(), fx.plan_file.clone());
    let runner = scripted_auto(vec![Box::new(move |_spec| {
        std::fs::write(&p, "changed\n").expect("write");
        std::fs::write(&plan, "# rewritten\n").expect("write");
        "done".to_string()
    })]);
    expect_parked(
        fx.engine(&runner).build_stage().expect("stage"),
        "builder changed plan.md",
    );
    assert_eq!(
        std::fs::read_to_string(&fx.plan_file).expect("plan"),
        plan_before
    );
    let after = std::fs::read_to_string(&progress).expect("read");
    assert!(after.starts_with("Task 8: Done\n"), "{after}");
    assert!(!after.lines().any(|l| l == "changed"), "{after}");
}
