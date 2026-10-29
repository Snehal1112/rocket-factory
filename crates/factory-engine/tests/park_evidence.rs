mod common;

use common::{
    amend_head, commit_file, committing_fixture, scripted, scripted_auto, Effect, Fixture,
};
use factory_core::{ParkKind, ParkRecord, Stage};
use factory_engine::{merge_stage, MergeResult, StageOutcome};

fn change_effect(file: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "line\n").expect("write change");
        "done".to_string()
    })
}

fn start_fix_round(fx: &Fixture) {
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
}

fn only_part(rec: &ParkRecord) -> &factory_core::ParkPart {
    assert_eq!(rec.parts.len(), 1, "{:?}", rec.parts);
    &rec.parts[0]
}

#[test]
fn no_park_has_empty_evidence() {
    // A committer that rewrites the reviewed head is a CommitFailed park.
    let fx = committing_fixture();
    let first = scripted_auto(vec![change_effect("a.txt")]);
    fx.engine(&first).build_stage().expect("first");
    start_fix_round(&fx);
    let second = scripted(vec![
        change_effect("b.txt"),
        Box::new(|spec| {
            amend_head(&spec.cwd, "feat(test): reword");
            "done".to_string()
        }),
    ]);
    fx.engine(&second).build_stage().expect("fix");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::CommitFailed);
    let part = only_part(&rec);
    assert_eq!(part.title, "Why it stopped");
    assert_eq!(part.text, rec.reason);

    // A builder that commits on its own is a BuilderCommitted park.
    let fx = committing_fixture();
    let runner = scripted_auto(vec![Box::new(|spec| {
        commit_file(&spec.cwd, "a.txt", "x\n");
        "done".to_string()
    })]);
    fx.engine(&runner).build_stage().expect("build");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::BuilderCommitted);
    let part = only_part(&rec);
    assert_eq!(part.title, "Why it stopped");
    assert_eq!(part.text, rec.reason);
}

#[test]
fn nothing_changed_without_a_reply_says_so() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![Box::new(|_| String::new())]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NothingChanged);
    let part = only_part(&rec);
    assert_eq!(part.title, "Builder reply");
    assert_eq!(part.text, "The builder wrote no reply.");
}

fn merge_record(fx: &common::MergeFixture) -> ParkRecord {
    let job = fx.driver.job(&fx.id).expect("job");
    ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json")
}

#[test]
fn main_busy_lists_the_dirty_files() {
    let fx = common::approved_job();
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    let part = only_part(&rec);
    assert_eq!(part.title, "Main checkout");
    assert!(part.text.contains("README.md"), "{}", part.text);
    assert!(part.text.contains("factory approve"), "{}", part.text);
}

#[test]
fn a_moved_branch_names_both_shas() {
    let fx = common::approved_job();
    common::commit_file(&common::worktree_of(&fx), "late.txt", "x");
    let tip = common::branch_tip(&fx);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    let part = only_part(&rec);
    assert_eq!(part.title, "Branch moved");
    assert!(part.text.contains(&fx.head), "{}", part.text);
    assert!(part.text.contains(&tip), "{}", part.text);
    assert!(part.text.contains("factory rework"), "{}", part.text);
}

#[test]
fn an_earlier_park_keeps_its_evidence() {
    let fx = common::approved_job();
    let job = fx.driver.job(&fx.id).expect("job");
    let dir = fx.driver.job_dir(&job);
    let part = factory_core::ParkPart::new("Checks", "checks.md", "FAIL cargo check");
    ParkRecord::new(
        ParkKind::ChecksFailed,
        "checks failed",
        &fx.id,
        job.round,
        7,
        vec![part],
    )
    .write(&dir)
    .expect("write record");
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.park_kind = Some("checks-failed".to_string());
            j.park_at = Some(7);
            j.set_stage(Stage::NeedsHuman, Some("checks failed"), false, 7.0)
        })
        .expect("park job");
    factory_engine::park(&fx.driver, &fx.id, "after sync: checks failed").expect("park");
    let rec = ParkRecord::read(&dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::ChecksFailed);
    assert_eq!(only_part(&rec).title, "Checks");
}
