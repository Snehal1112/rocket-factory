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

fn risky_config(checks: Vec<factory_core::CheckRule>) -> factory_core::Config {
    factory_core::Config {
        risk_paths: vec!["docs/**".to_string()],
        checks,
        ..factory_core::Config::default()
    }
}

fn docs_rule(command: &str) -> factory_core::CheckRule {
    factory_core::CheckRule {
        paths: vec!["docs/**".to_string()],
        commands: vec![command.to_string()],
        ..factory_core::CheckRule::default()
    }
}

fn risky_sync_fixture(checks: Vec<factory_core::CheckRule>) -> common::MergeFixture {
    let fx = common::approved_job_config(common::approving_runner(3), false, risky_config(checks));
    // Main moves, so the merge syncs first.
    common::commit_file(&fx.main, "other.md", "o");
    fx
}

fn titles(rec: &ParkRecord) -> Vec<&str> {
    rec.parts.iter().map(|p| p.title.as_str()).collect()
}

#[test]
fn a_risky_park_after_a_sync_keeps_the_review_and_risk_evidence() {
    let fx = risky_sync_fixture(Vec::new());
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("after sync")),
        "{r:?}"
    );
    let rec = merge_record(&fx);
    assert_eq!(rec.kind, ParkKind::RiskyDiff);
    assert_eq!(rec.risky, Some(true));
    assert_eq!(
        titles(&rec),
        vec!["Review", "Risk", "Rounds", "Changed files", "Checks"]
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::NeedsHuman);
    assert!(
        job.reason
            .as_deref()
            .is_some_and(|r| r.starts_with("after sync: ")),
        "{:?}",
        job.reason
    );
}

#[test]
fn a_risky_park_after_a_sync_runs_the_final_checks() {
    let fx = risky_sync_fixture(vec![docs_rule("true")]);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    let checks =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("checks.md")).expect("checks.md");
    assert!(
        checks.lines().any(|l| l.starts_with("PASS true")),
        "{checks}"
    );
    let rec = merge_record(&fx);
    let part = rec
        .parts
        .iter()
        .find(|p| p.title == "Checks")
        .expect("Checks part");
    assert!(part.text.contains("true"), "{}", part.text);
}

#[test]
fn final_checks_that_cannot_run_after_a_sync_park_risky_and_say_why() {
    // The first run passes. The second run is the final check: it turns checks.md into a folder, so
    // the driver cannot write its report and the run fails with an error instead of a result.
    let scripts = tempfile::tempdir().expect("scripts dir");
    let script = scripts.path().join("block-report.sh");
    let marker = scripts.path().join("ran");
    let fx = risky_sync_fixture(vec![docs_rule(&script.to_string_lossy())]);
    let job = fx.driver.job(&fx.id).expect("job");
    let report = fx.driver.job_dir(&job).join("checks.md");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ -e {m} ]; then rm -rf {r}; mkdir {r}; exit 0; fi\ntouch {m}\n",
            m = marker.display(),
            r = report.display()
        ),
    )
    .expect("write script");
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    assert_eq!(rec.kind, ParkKind::RiskyDiff, "{rec:?}");
    let part = rec
        .parts
        .iter()
        .find(|p| p.title == "Checks")
        .expect("Checks part");
    assert!(
        part.text.starts_with("final checks could not run:"),
        "{}",
        part.text
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::NeedsHuman);
}

#[test]
fn failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round() {
    // The sync checks stage also runs the rule on the whole job diff, so the script passes the first
    // run and fails every later one. Only the final checks after the sync review see it fail.
    let scripts = tempfile::tempdir().expect("scripts dir");
    let script = scripts.path().join("pass-once.sh");
    let marker = scripts.path().join("ran");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ -e {m} ]; then exit 1; fi\ntouch {m}\n",
            m = marker.display()
        ),
    )
    .expect("write script");
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");
    let fx = risky_sync_fixture(vec![docs_rule(&script.to_string_lossy())]);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    assert_eq!(rec.kind, ParkKind::ChecksFailed, "{rec:?}");
    assert!(
        rec.reason.contains("final checks failed after sync"),
        "{}",
        rec.reason
    );
    assert!(fx.runner.specs_for("builder").is_empty());
    let titles = titles(&rec);
    for want in ["Review", "Risk", "Checks"] {
        assert!(titles.contains(&want), "{titles:?}");
    }
}

#[test]
fn a_checks_failed_sync_park_keeps_one_reason_and_one_ruling() {
    let scripts = tempfile::tempdir().expect("scripts dir");
    let script = scripts.path().join("pass-once.sh");
    let marker = scripts.path().join("ran");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ -e {m} ]; then exit 1; fi\ntouch {m}\n",
            m = marker.display()
        ),
    )
    .expect("write script");
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");
    let fx = risky_sync_fixture(vec![docs_rule(&script.to_string_lossy())]);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    let want = "final checks failed after sync";
    assert_eq!(r, MergeResult::Parked(want.to_string()));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.reason.as_deref(), Some(want));
    assert_eq!(merge_record(&fx).kind, ParkKind::ChecksFailed);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    let lines: Vec<&str> = ledger.lines().filter(|l| l.contains(want)).collect();
    assert_eq!(lines.len(), 1, "{ledger}");
}

#[test]
fn other_sync_parks_stay_kind_other() {
    let fx =
        common::approved_job_config(common::approving_runner(3), false, risky_config(Vec::new()));
    let wt = common::worktree_of(&fx);
    common::commit_file(&wt, "same.txt", "branch");
    common::record_head(&fx, &common::branch_tip(&fx));
    common::commit_file(&fx.main, "same.txt", "main");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("conflict")),
        "{r:?}"
    );
    assert_eq!(merge_record(&fx).kind, ParkKind::Other);
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

#[test]
fn a_park_writes_the_summary_on_the_board() {
    // No fix rounds, so the first changes verdict parks the job.
    let fx = common::SeriesFixture::with_config(
        "[blocker] src/a.rs wrong\nVERDICT: CHANGES",
        factory_core::Config {
            fix_rounds: 0,
            ..Default::default()
        },
    );
    let a = fx.plan_file("plan-01-a.md", 1, "none");
    let job = factory_engine::create_job(&fx.driver, "stuck", "x", Some(&a), None).expect("create");
    factory_engine::run_job(&fx.driver, &job.id).expect("run");
    let parked = fx.driver.job(&job.id).expect("job");
    let summary = parked.park_summary.as_deref().expect("summary");
    assert!(summary.starts_with("verdict changes"), "{summary}");
    assert!(!summary.contains('\n') && summary.chars().count() <= 160);
}

#[test]
fn a_merge_error_is_its_own_part() {
    let fx = common::approved_job();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.head = None;
            Ok(())
        })
        .expect("clear head");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    let part = only_part(&rec);
    assert_eq!(part.title, "Merge error");
    assert_eq!(part.source, "git");
    assert!(part.text.contains("no recorded head"), "{}", part.text);
}

#[test]
fn a_job_without_a_plan_names_the_missing_file() {
    let fx = common::SeriesFixture::new("x");
    let job = factory_engine::create_job(&fx.driver, "x", "f", None, None).expect("create");
    factory_engine::run_job(&fx.driver, &job.id).expect("run");
    let job = fx.driver.job(&job.id).expect("job");
    let rec = ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::Other);
    assert_eq!(rec.parts.len(), 1, "{:?}", rec.parts);
    let part = &rec.parts[0];
    assert_eq!(part.title, "Missing plan");
    assert!(part.text.starts_with("expected "), "{}", part.text);
    assert!(part.text.ends_with("plan.md"), "{}", part.text);
}
