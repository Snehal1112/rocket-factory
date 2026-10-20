mod common;

use common::{committing_fixture, scripted_auto, Effect};
use factory_engine::StageOutcome;

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
