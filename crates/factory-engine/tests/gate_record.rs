mod common;

use common::{scripted, Effect, Fixture, ScriptedRunner};
use factory_core::gate_record::{read, GateKey, GateResult, GATES_FILE};
use factory_core::{ParkKind, ParkRecord};
use factory_engine::{park_with_parts, StageOutcome};

/// A builder that writes each file with the given body and leaves it uncommitted.
fn write_effect(files: Vec<(&'static str, &'static str)>) -> Effect {
    Box::new(move |spec| {
        for (name, body) in &files {
            std::fs::write(spec.cwd.join(name), body).expect("write file");
        }
        "done".to_string()
    })
}

/// A fixture that records gates and requires a test in a code diff.
fn recording_fixture(on: bool) -> Fixture {
    let mut fx = Fixture::new();
    fx.config.gate_record = on;
    fx.config.require_tests = true;
    fx
}

/// Builds the job with a builder that adds code and no test, then runs the checks, which park it as no-tests.
fn park_no_tests(fx: &Fixture) -> StageOutcome {
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks")
}

fn pairs(fx: &Fixture) -> Vec<(GateKey, GateResult)> {
    read(&fx.job_dir)
        .iter()
        .map(|l| (l.gate, l.result))
        .collect()
}

#[test]
fn a_park_writes_its_fail_line_and_approve_waiting() {
    let fx = recording_fixture(true);
    let out = park_no_tests(&fx);
    let StageOutcome::NeedsHuman(reason) = out else {
        panic!("expected a park, got {out:?}");
    };
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NoTests);
    let lines = read(&fx.job_dir);
    let fail = lines
        .iter()
        .position(|l| l.gate == GateKey::NoTests && l.result == GateResult::Fail)
        .expect("no-tests fail line");
    assert_eq!(lines[fail].detail, reason);
    let waiting = lines
        .iter()
        .position(|l| l.gate == GateKey::Approve && l.result == GateResult::Waiting)
        .expect("approve waiting line");
    assert!(fail < waiting, "{lines:?}");
}

#[test]
fn a_park_without_approve_writes_no_waiting_line() {
    let mut fx = Fixture::new();
    fx.config.gate_record = true;
    let plan = fx.plan_file.clone();
    let effect: Effect = Box::new(move |_| {
        std::fs::write(&plan, "# changed\n").expect("tamper");
        "done".to_string()
    });
    let runner = scripted(vec![effect]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::BuilderTampered);
    let got = pairs(&fx);
    assert!(
        got.contains(&(GateKey::BuilderGuard, GateResult::Fail)),
        "{got:?}"
    );
    assert!(!got.iter().any(|(k, _)| *k == GateKey::Approve), "{got:?}");
}

#[test]
fn a_second_park_writes_no_second_fail_line() {
    let config = factory_core::Config {
        gate_record: true,
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), config);
    park_with_parts(
        &fx.driver,
        &fx.id,
        "first",
        ParkKind::DiffTooLarge,
        Vec::new(),
    )
    .expect("first park");
    park_with_parts(
        &fx.driver,
        &fx.id,
        "second",
        ParkKind::DiffTooLarge,
        Vec::new(),
    )
    .expect("second park");
    let job = fx.driver.job(&fx.id).expect("job");
    let lines = read(&fx.driver.job_dir(&job));
    let fails: Vec<_> = lines
        .iter()
        .filter(|l| l.result == GateResult::Fail)
        .collect();
    assert_eq!(fails.len(), 1, "{lines:?}");
    assert_eq!(fails[0].gate, GateKey::DiffCap);
    assert_eq!(fails[0].detail, "first");
}

#[test]
fn a_first_round_line_has_round_one() {
    let fx = recording_fixture(true);
    park_no_tests(&fx);
    assert_eq!(fx.job().round, 0);
    let lines = read(&fx.job_dir);
    assert!(!lines.is_empty());
    assert!(lines.iter().all(|l| l.round == 1), "{lines:?}");
}

#[test]
fn no_gate_file_when_the_setting_is_off() {
    let fx = recording_fixture(false);
    park_no_tests(&fx);
    assert!(ParkRecord::read(&fx.job_dir).is_some());
    assert!(!fx.job_dir.join(GATES_FILE).exists());
}

#[test]
fn an_auto_merge_with_the_setting_off_writes_no_gate_file() {
    let config = factory_core::Config {
        checks: vec![factory_core::CheckRule {
            paths: vec!["**".into()],
            commands: vec!["true".into()],
            ..factory_core::CheckRule::default()
        }],
        ..factory_core::Config::default()
    };
    assert!(!config.gate_record);
    let fx = common::approved_job_config(common::approving_runner(3), config);
    force_stage(&fx, factory_core::Stage::Review);
    let checks = fx.driver.checks(&fx.id).expect("checks");
    assert_eq!(checks, StageOutcome::Advanced(factory_core::Stage::Review));
    let outcome = fx.driver.review(&fx.id).expect("review");
    let applied = fx.driver.apply(&fx.id, &outcome).expect("apply");
    assert_eq!(
        applied,
        StageOutcome::Advanced(factory_core::Stage::Approve)
    );
    let merged = factory_engine::merge_stage(&fx.driver, &fx.id).expect("merge");
    assert!(
        matches!(merged, factory_engine::MergeResult::Merged(_)),
        "{merged:?}"
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert!(!fx.driver.job_dir(&job).join(GATES_FILE).exists());
}

/// The park record with its time set to zero, so two runs compare equal.
fn timeless(fx: &Fixture) -> ParkRecord {
    let mut rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    rec.at = 0;
    rec
}

#[test]
fn a_gate_file_that_is_a_folder_changes_nothing() {
    let plain = recording_fixture(true);
    let plain_out = park_no_tests(&plain);

    let fx = recording_fixture(true);
    std::fs::create_dir_all(fx.job_dir.join(GATES_FILE)).expect("folder");
    let (out, lines) = factory_telemetry::capture("info", || park_no_tests(&fx));
    assert_eq!(out, plain_out);
    assert_eq!(timeless(&fx), timeless(&plain));
    let warns: Vec<_> = lines
        .iter()
        .filter(|l| l["event"] == "gates.record_error")
        .collect();
    assert!(!warns.is_empty(), "{lines:?}");
    assert_eq!(warns[0]["level"], "warn");
}

/// Commits one file in the job worktree, as a builder that commits its work would.
fn commit_effect() -> Effect {
    Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write file");
        let repo = git2::Repository::open(&spec.cwd).expect("open worktree");
        let mut index = repo.index().expect("index");
        index
            .add_path(std::path::Path::new("a.txt"))
            .expect("add file");
        index.write().expect("write index");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("find tree");
        let sig = git2::Signature::now("t", "t@example.com").expect("sig");
        let parent = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.commit(Some("HEAD"), &sig, &sig, "work", &tree, &[&parent])
            .expect("commit");
        "done".to_string()
    })
}

fn lines_of(fx: &Fixture, gate: GateKey) -> Vec<factory_core::gate_record::GateLine> {
    read(&fx.job_dir)
        .into_iter()
        .filter(|l| l.gate == gate)
        .collect()
}

#[test]
fn a_build_writes_guard_builder_and_cap_passes() {
    let fx = recording_fixture(true);
    let runner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    let got = pairs(&fx);
    let at = |want: (GateKey, GateResult)| {
        got.iter()
            .position(|p| *p == want)
            .unwrap_or_else(|| panic!("missing {want:?} in {got:?}"))
    };
    let guard = at((GateKey::BuilderGuard, GateResult::Pass));
    let builder = at((GateKey::Builder, GateResult::Pass));
    let cap = at((GateKey::DiffCap, GateResult::Pass));
    assert!(guard < builder && builder < cap, "{got:?}");
    let detail = &lines_of(&fx, GateKey::DiffCap)[0].detail;
    assert!(detail.starts_with(|c: char| c.is_ascii_digit()), "{detail}");
    assert!(detail.contains("/ 4000"), "{detail}");
    assert_eq!(lines_of(&fx, GateKey::Builder)[0].detail, "whole plan");
}

#[test]
fn a_diff_cap_park_still_writes_builder_pass() {
    let mut fx = recording_fixture(true);
    fx.config.diff_cap_lines = 1;
    let runner = scripted(vec![write_effect(vec![("a.txt", "x\ny\nz\n")])]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::DiffTooLarge);
    let got = pairs(&fx);
    let at = |want: (GateKey, GateResult)| {
        got.iter()
            .position(|p| *p == want)
            .unwrap_or_else(|| panic!("missing {want:?} in {got:?}"))
    };
    let builder = at((GateKey::Builder, GateResult::Pass));
    let cap = at((GateKey::DiffCap, GateResult::Fail));
    assert!(builder < cap, "{got:?}");
}

#[test]
fn a_single_plan_writes_series_skipped() {
    let fx = recording_fixture(true);
    let runner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    let lines = lines_of(&fx, GateKey::Series);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].result, GateResult::Skipped);
    assert_eq!(lines[0].detail, "single plan");
}

#[test]
fn plan_two_writes_series_pass_after_plan_one_merged() {
    let fx = recording_fixture(true);
    let pos = |index| factory_core::SeriesPos {
        plan_file: "plan.md".into(),
        index,
        total: 2,
        id: Some("S1".into()),
    };
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.jobs[0].series = Some(pos(2));
    let mut first = factory_core::Job::new("000", "b0", "one", "2026-10-13", "one", 0.0);
    first.series = Some(pos(1));
    first.stage = factory_core::Stage::Merged;
    board.jobs.push(first);
    board.save(&fx.board_path).expect("save board");
    let runner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    let lines = lines_of(&fx, GateKey::Series);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].result, GateResult::Pass);
    assert_eq!(lines[0].detail, "plan 2 of 2, earlier plan merged");
}

#[test]
fn machine_writes_one_pass_per_round() {
    let fx = recording_fixture(true);
    let runner = scripted(vec![
        write_effect(vec![("a.txt", "x\n")]),
        write_effect(vec![("b.txt", "y\n")]),
    ]);
    fx.engine(&runner).build_stage().expect("first build");
    std::fs::write(fx.job_dir.join("changes-r1.md"), "fix it").expect("findings");
    fx.engine(&runner).build_stage().expect("second build");
    let lines = lines_of(&fx, GateKey::Machine);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].result, GateResult::Pass);
}

#[test]
fn no_change_writes_builder_note() {
    let fx = recording_fixture(true);
    let runner = scripted(vec![commit_effect()]);
    fx.engine(&runner).build_stage().expect("first build");
    // A rework starts round 2 with a note, and the builder says the work is already there.
    std::fs::write(fx.job_dir.join("changes-r2.md"), "look again").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.rework_base = 1;
    job.fix_source = Some("rework".into());
    job.resume(factory_core::Stage::Build, 1.0).expect("resume");
    board.save(&fx.board_path).expect("save board");
    let runner = scripted(vec![Box::new(|_| "it is already handled".to_string())]);
    let out = fx.engine(&runner).build_stage().expect("second build");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    let notes: Vec<_> = lines_of(&fx, GateKey::Builder)
        .into_iter()
        .filter(|l| l.result == GateResult::Note)
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", read(&fx.job_dir));
    assert_eq!(notes[0].detail, "no change, work already on the branch");
}

/// A fixture with one check rule over every path.
fn with_rule(mut fx: Fixture, commands: &[&str], format: &[&str]) -> Fixture {
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: commands.iter().map(|c| c.to_string()).collect(),
        format: format.iter().map(|c| c.to_string()).collect(),
        ..factory_core::CheckRule::default()
    }];
    fx
}

#[test]
fn checks_fail_then_pass_across_rounds() {
    let fx = with_rule(recording_fixture(true), &["test -f ok.txt"], &[]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    let out = fx.engine(&runner).checks_stage().expect("round 1");
    assert_eq!(out, StageOutcome::Advanced(factory_core::Stage::Build));
    // The fix round adds the file the check wants and puts the job back in review.
    let job = fx.job();
    let wt = job.worktree.clone().expect("worktree");
    std::fs::write(std::path::Path::new(&wt).join("ok.txt"), "ok\n").expect("write ok");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(factory_core::Stage::Review, None, false, 2.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx.engine(&runner).checks_stage().expect("round 2");
    let lines = lines_of(&fx, GateKey::Checks);
    let got: Vec<_> = lines.iter().map(|l| (l.result, l.round)).collect();
    assert_eq!(
        got,
        vec![(GateResult::Fail, 1), (GateResult::Pass, 2)],
        "{lines:?}"
    );
    assert_eq!(lines[0].detail, "0 of 1 commands passed");
}

#[test]
fn a_failing_format_command_writes_format_fail() {
    let fx = with_rule(recording_fixture(true), &["true"], &["false"]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Fail);
    assert!(format[0].detail.contains("false"), "{:?}", format[0]);
}

#[test]
fn a_format_detail_redacts_a_secret_flag() {
    let fx = with_rule(recording_fixture(true), &["true"], &["true --api-key=abc"]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Pass);
    assert_eq!(format[0].detail, "true --api-key=[redacted]");
    assert!(!format[0].detail.contains("abc"), "{:?}", format[0]);
}

#[test]
fn a_lint_detail_redacts_a_secret_flag() {
    let mut fx = with_rule(recording_fixture(true), &["true"], &[]);
    fx.config.checks[0].lint = vec!["true --token=xyz".to_string()];
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let lint = lines_of(&fx, GateKey::Lint);
    assert_eq!(lint.len(), 1, "{lint:?}");
    assert_eq!(lint[0].result, GateResult::Pass);
    assert_eq!(lint[0].detail, "true --token=[redacted]");
}

#[test]
fn a_gate_detail_redacts_each_command() {
    let fx = with_rule(recording_fixture(true), &[], &["true --password=p", "true"]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Pass);
    assert_eq!(format[0].detail, "true --password=[redacted], true");
}

#[test]
fn no_format_command_writes_skipped() {
    let fx = recording_fixture(true);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Skipped);
    assert_eq!(format[0].detail, "no format command");
    let lint = lines_of(&fx, GateKey::Lint);
    assert_eq!(lint.len(), 1, "{lint:?}");
    assert_eq!(lint[0].result, GateResult::Skipped);
    assert_eq!(lint[0].detail, "no lint command");
}

#[test]
fn uncovered_code_writes_no_tests_note() {
    let mut fx = Fixture::new();
    fx.config.gate_record = true;
    fx.config.require_tests = false;
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let lines = lines_of(&fx, GateKey::NoTests);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].result, GateResult::Note);
    assert!(lines[0].detail.contains("a.txt"), "{:?}", lines[0]);
}

#[test]
fn covered_paths_write_no_checks_pass() {
    let fx = recording_fixture(true);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let lines = lines_of(&fx, GateKey::NoChecks);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].result, GateResult::Pass);
    assert_eq!(lines[0].detail, "all 1 code paths covered");
}

#[test]
fn a_checks_scope_violation_fails_checks_not_review() {
    let mut fx = recording_fixture(true);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    // Scope the repo to a folder that the change is outside of.
    fx.repo = factory_git::Repo::open(&fx.main)
        .expect("open")
        .with_scope("web", std::path::Path::new("apps/web"));
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let fails: Vec<_> = read(&fx.job_dir)
        .into_iter()
        .filter(|l| l.result == GateResult::Fail)
        .collect();
    assert_eq!(fails.len(), 1, "{fails:?}");
    assert_eq!(fails[0].gate, GateKey::Checks);
}

#[test]
fn checks_failed_beyond_fix_rounds_writes_one_fail_line_for_the_park() {
    let mut fx = with_rule(recording_fixture(true), &["false"], &[]);
    fx.config.fix_rounds = 0;
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let fails: Vec<_> = lines_of(&fx, GateKey::Checks)
        .into_iter()
        .filter(|l| l.result == GateResult::Fail)
        .collect();
    assert_eq!(fails.len(), 1, "{fails:?}");
}

#[test]
fn a_checks_park_with_a_failing_lint_stops_at_checks() {
    let mut fx = with_rule(recording_fixture(true), &["true"], &["false"]);
    fx.config.fix_rounds = 0;
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let lines = read(&fx.job_dir);
    assert!(
        lines
            .iter()
            .any(|l| l.gate == GateKey::Format && l.result == GateResult::Fail),
        "{lines:?}"
    );
    let last_fail = lines
        .iter()
        .rev()
        .find(|l| l.result == GateResult::Fail)
        .expect("a fail line");
    assert_eq!(last_fail.gate, GateKey::Checks, "{lines:?}");
}

/// A merge fixture that records gates and runs one passing check over every path.
fn recording_merge_fixture(runner: std::sync::Arc<ScriptedRunner>) -> common::MergeFixture {
    let config = factory_core::Config {
        gate_record: true,
        checks: vec![factory_core::CheckRule {
            paths: vec!["**".into()],
            commands: vec!["true".into()],
            ..factory_core::CheckRule::default()
        }],
        ..factory_core::Config::default()
    };
    common::approved_job_config(runner, config)
}

fn merge_lines(fx: &common::MergeFixture) -> Vec<factory_core::gate_record::GateLine> {
    let job = fx.driver.job(&fx.id).expect("job");
    read(&fx.driver.job_dir(&job))
}

#[test]
fn an_unsigned_final_merge_writes_the_reason_in_the_merge_gate() {
    let fx = recording_merge_fixture(common::approving_runner(3));
    common::failing_signer(&fx.main, fx.tmp.path());
    force_stage(&fx, factory_core::Stage::Review);
    fx.driver.checks(&fx.id).expect("checks");
    let outcome = fx.driver.review(&fx.id).expect("review");
    fx.driver.apply(&fx.id, &outcome).expect("apply");
    let merged = factory_engine::merge_stage(&fx.driver, &fx.id).expect("merge");
    let factory_engine::MergeResult::Merged(sha) = merged else {
        panic!("expected a merge, got {merged:?}");
    };
    let lines = merge_lines(&fx);
    let merge = lines
        .iter()
        .find(|l| l.gate == GateKey::Merge)
        .expect("merge line");
    assert_eq!(merge.result, GateResult::Pass);
    assert!(merge.detail.starts_with(&sha[..7]), "{}", merge.detail);
    assert!(merge.detail.contains(", unsigned: "), "{}", merge.detail);
    assert!(merge.detail.contains("exited with 2"), "{}", merge.detail);
}

fn force_stage(fx: &common::MergeFixture, stage: factory_core::Stage) {
    fx.driver
        .store()
        .update_job(&fx.id, |j| j.set_stage(stage, Some("test"), true, 2.0))
        .expect("stage");
}

#[test]
fn an_auto_merge_writes_only_passes_notes_and_skips() {
    let fx = recording_merge_fixture(common::approving_runner(3));
    force_stage(&fx, factory_core::Stage::Review);
    let checks = fx.driver.checks(&fx.id).expect("checks");
    assert_eq!(checks, StageOutcome::Advanced(factory_core::Stage::Review));
    let outcome = fx.driver.review(&fx.id).expect("review");
    let applied = fx.driver.apply(&fx.id, &outcome).expect("apply");
    assert_eq!(
        applied,
        StageOutcome::Advanced(factory_core::Stage::Approve)
    );
    let merged = factory_engine::merge_stage(&fx.driver, &fx.id).expect("merge");
    let factory_engine::MergeResult::Merged(sha) = merged else {
        panic!("expected a merge, got {merged:?}");
    };
    let lines = merge_lines(&fx);
    assert!(
        lines.iter().all(|l| matches!(
            l.result,
            GateResult::Pass | GateResult::Note | GateResult::Skipped | GateResult::Running
        )),
        "{lines:?}"
    );
    for key in [
        GateKey::Review,
        GateKey::Risk,
        GateKey::FinalGate,
        GateKey::Merge,
        GateKey::Approve,
        GateKey::Budget,
    ] {
        assert!(
            lines
                .iter()
                .any(|l| l.gate == key && l.result == GateResult::Pass),
            "{key:?} {lines:?}"
        );
    }
    let merge = lines
        .iter()
        .find(|l| l.gate == GateKey::Merge)
        .expect("merge line");
    assert!(sha.starts_with(&merge.detail), "{} {sha}", merge.detail);
    let approve = lines
        .iter()
        .find(|l| l.gate == GateKey::Approve)
        .expect("approve line");
    assert_eq!(approve.detail, "auto-merge");
    let risk = lines
        .iter()
        .find(|l| l.gate == GateKey::Risk)
        .expect("risk line");
    assert_eq!(risk.detail, "normal");
    let budget = lines
        .iter()
        .find(|l| l.gate == GateKey::Budget)
        .expect("budget line");
    assert!(budget.detail.ends_with("% used"), "{}", budget.detail);
    let review = lines
        .iter()
        .find(|l| l.gate == GateKey::Review && l.result == GateResult::Pass)
        .expect("review pass");
    assert!(review.detail.contains("code APPROVE"), "{}", review.detail);
    assert!(review.detail.contains("review-code-r"), "{}", review.detail);
}

const TWO_BLOCKERS: &str = "- [blocker] fix a\n- [blocker] fix b\nVERDICT: CHANGES";

fn reply(text: &'static str) -> Effect {
    Box::new(move |_| text.to_string())
}

#[test]
fn changes_then_approve_writes_review_fail_then_pass() {
    let mut fx = recording_fixture(true);
    fx.config.require_tests = false;
    let builder: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&builder).build_stage().expect("build");
    fx.engine(&builder).checks_stage().expect("checks");
    let reviewer = scripted(vec![reply(TWO_BLOCKERS)]);
    let e = fx.engine(&reviewer);
    let outcome = e.review_stage().expect("review 1");
    let out = e.apply_decision(&outcome).expect("apply 1");
    assert_eq!(out, StageOutcome::Advanced(factory_core::Stage::Build));
    let fixer: ScriptedRunner = scripted(vec![write_effect(vec![("b.txt", "y\n")])]);
    fx.engine(&fixer).build_stage().expect("build 2");
    fx.engine(&fixer).checks_stage().expect("checks 2");
    let approver = scripted(vec![reply("ok\nVERDICT: APPROVE")]);
    fx.engine(&approver).review_stage().expect("review 2");
    let reviews: Vec<_> = lines_of(&fx, GateKey::Review)
        .into_iter()
        .filter(|l| l.result != GateResult::Running)
        .collect();
    assert_eq!(reviews.len(), 2, "{reviews:?}");
    assert_eq!(reviews[0].result, GateResult::Fail);
    assert!(reviews[0].detail.contains("2 blockers"), "{reviews:?}");
    assert!(
        reviews[0].detail.contains("code CHANGES"),
        "{:?}",
        reviews[0]
    );
    assert_eq!(reviews[1].result, GateResult::Pass);
    assert_eq!(reviews[1].round, reviews[0].round + 1, "{reviews:?}");
}

/// Builds the files and passes the checks, and gives the job the code and security roles.
fn two_role_fixture(files: &[&'static str], risk_paths: &[&str]) -> Fixture {
    let mut fx = Fixture::new();
    fx.config.gate_record = true;
    fx.config.risk_paths = risk_paths.iter().map(|s| s.to_string()).collect();
    let files: Vec<&'static str> = files.to_vec();
    let builder: Effect = Box::new(move |spec| {
        for f in &files {
            if let Some(parent) = std::path::Path::new(f).parent() {
                std::fs::create_dir_all(spec.cwd.join(parent)).expect("mkdir");
            }
            std::fs::write(spec.cwd.join(f), "x\n").expect("write");
        }
        "done".to_string()
    });
    let runner = scripted(vec![builder]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let roles = vec!["code".to_string(), "security".to_string()];
    board.find_mut("001").expect("job").set_reviewers(&roles);
    board.save(&fx.board_path).expect("save");
    fx
}

#[test]
fn a_docs_only_diff_skips_security_in_the_review_detail() {
    let fx = two_role_fixture(&["docs/a.md"], &[]);
    let runner = scripted(vec![reply("ok\nVERDICT: APPROVE")]);
    fx.engine(&runner).review_stage().expect("review");
    let pass = lines_of(&fx, GateKey::Review)
        .into_iter()
        .find(|l| l.result == GateResult::Pass)
        .unwrap_or_else(|| panic!("no review pass in {:?}", read(&fx.job_dir)));
    assert!(
        pass.detail.contains("security skipped (docs only)"),
        "{}",
        pass.detail
    );
}

#[test]
fn a_missing_security_agent_writes_one_review_fail() {
    let fx = two_role_fixture(&["a.txt"], &[]);
    let runner = scripted(vec![reply("ok\nVERDICT: APPROVE")]);
    let e = fx.engine(&runner);
    let outcome = e.review_stage().expect("review");
    let out = e.apply_decision(&outcome).expect("apply");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let fails: Vec<_> = read(&fx.job_dir)
        .into_iter()
        .filter(|l| l.result == GateResult::Fail)
        .collect();
    assert_eq!(fails.len(), 1, "{fails:?}");
    assert_eq!(fails[0].gate, GateKey::Review);
    assert_eq!(fails[0].detail, "security agent missing");
}

#[test]
fn a_risky_diff_writes_note_final_gate_fail_and_waiting() {
    let mut fx = recording_fixture(true);
    fx.config.require_tests = false;
    fx.config.risk_paths = vec!["a.txt".into()];
    let builder: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&builder).build_stage().expect("build");
    fx.engine(&builder).checks_stage().expect("checks");
    let reviewer = scripted(vec![reply("ok\nVERDICT: APPROVE")]);
    let e = fx.engine(&reviewer);
    let outcome = e.review_stage().expect("review");
    let out = e.apply_decision(&outcome).expect("apply");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let got = pairs(&fx);
    let at = |want: (GateKey, GateResult)| {
        got.iter()
            .position(|p| *p == want)
            .unwrap_or_else(|| panic!("missing {want:?} in {got:?}"))
    };
    let note = at((GateKey::Risk, GateResult::Note));
    let final_gate = at((GateKey::FinalGate, GateResult::Pass));
    let fail = at((GateKey::Risk, GateResult::Fail));
    let waiting = at((GateKey::Approve, GateResult::Waiting));
    assert!(
        note < final_gate && final_gate < fail && fail < waiting,
        "{got:?}"
    );
    let risk = lines_of(&fx, GateKey::Risk);
    assert_eq!(risk[0].detail, "risky: a.txt");
}

#[test]
fn a_merge_park_after_sync_writes_merge_fail() {
    let runner = std::sync::Arc::new(scripted(vec![reply(TWO_BLOCKERS)]));
    let fx = recording_merge_fixture(runner);
    // Main moves, so the merge syncs first and the sync review asks for changes.
    common::commit_file(&fx.main, "docs/other.txt", "main\n");
    let merged = factory_engine::merge_stage(&fx.driver, &fx.id).expect("merge");
    let factory_engine::MergeResult::Parked(reason) = merged else {
        panic!("expected a park, got {merged:?}");
    };
    let lines = merge_lines(&fx);
    let park = lines
        .iter()
        .filter(|l| l.result == GateResult::Fail)
        .find(|l| l.detail == reason)
        .unwrap_or_else(|| panic!("no fail line for {reason} in {lines:?}"));
    assert_eq!(park.gate, GateKey::Merge, "{lines:?}");
}

#[test]
fn sync_review_passed_writes_only_approve_waiting() {
    let runner = common::scripted_roles(vec![
        ("reviewer", reply("ok\nVERDICT: APPROVE")),
        ("security-review", reply("No findings.\nVERDICT: APPROVE")),
    ]);
    let mut fx = recording_merge_fixture(std::sync::Arc::new(runner));
    fx.driver = fx.driver.with_agent_files(vec![factory_core::AgentFile {
        name: "factory-security-reviewer.md".to_string(),
        content: "agent".to_string(),
    }]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.round = 1;
            Ok(())
        })
        .expect("round");
    force_stage(&fx, factory_core::Stage::NeedsHuman);
    // A rewritten tip makes the sync review the whole branch.
    let repo = git2::Repository::open(common::worktree_of(&fx)).expect("open worktree");
    let tip = repo.head().expect("head").peel_to_commit().expect("commit");
    tip.amend(Some("HEAD"), None, None, None, Some("rewritten\n"), None)
        .expect("amend");
    let out = factory_engine::sync_requested(&fx.driver, &fx.id).expect("sync");
    assert_eq!(out, factory_engine::SyncOutcome::Synced);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(
        job.reason.as_deref(),
        Some("sync review passed, run factory approve 001")
    );
    let lines = merge_lines(&fx);
    assert!(
        lines
            .iter()
            .any(|l| l.gate == GateKey::Approve && l.result == GateResult::Waiting),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.result == GateResult::Fail),
        "{lines:?}"
    );
}

#[test]
fn checks_failing_after_a_requested_sync_write_checks_fail() {
    let config = factory_core::Config {
        gate_record: true,
        checks: vec![factory_core::CheckRule {
            paths: vec!["**".into()],
            commands: vec!["false".into()],
            ..factory_core::CheckRule::default()
        }],
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), config);
    // Main moves, so the sync merges it and the check fails on the sync diff.
    common::commit_file(&fx.main, "docs/other.txt", "main\n");
    let out = factory_engine::sync_requested(&fx.driver, &fx.id).expect("sync");
    let want = "checks failed after sync";
    assert_eq!(out, factory_engine::SyncOutcome::Parked(want.into(), None));
    let lines = merge_lines(&fx);
    let park: Vec<_> = lines
        .iter()
        .filter(|l| l.result == GateResult::Fail && l.detail == want)
        .collect();
    assert_eq!(park.len(), 1, "{lines:?}");
    assert_eq!(park[0].gate, GateKey::Checks, "{lines:?}");
    assert!(
        !lines
            .iter()
            .any(|l| l.gate == GateKey::Review && l.result == GateResult::Fail),
        "{lines:?}"
    );
}
