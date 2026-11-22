mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{reply, scripted, Effect, Fixture, SeriesFixture};
use factory_agent::AgentSpec;
use factory_core::{ResumeAt, SeriesPos, Stage};
use factory_engine::{
    create_job, create_replacement, retry_job, run_job, run_series, BoardStore, Replacement, RunEnd,
};
use factory_git::Worktree;

fn ledger_of(fx: &SeriesFixture, id: &str) -> String {
    let job = fx.driver.job(id).expect("job");
    std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).unwrap_or_default()
}

/// A two-plan series. Returns the ids of plan 1 and plan 2.
fn two_plans(fx: &SeriesFixture) -> (String, String) {
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let first = create_job(&fx.driver, "series", "two plans", Some(&a), None).expect("create");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    (first.id, board.jobs[1].id.clone())
}

/// Asserts that job `id` parked as series-waiting before any builder ran.
fn assert_waiting(fx: &SeriesFixture, id: &str) {
    let job = fx.driver.job(id).expect("job");
    assert_eq!(job.stage, Stage::NeedsHuman);
    assert_eq!(job.park_kind.as_deref(), Some("series-waiting"));
    assert_eq!(job.park_resume, Some(ResumeAt::Build));
    assert!(job.worktree.is_none(), "no builder ran");
    assert!(
        !ledger_of(fx, id).contains("dispatched"),
        "{}",
        ledger_of(fx, id)
    );
}

#[test]
fn plan_two_parks_while_plan_one_is_not_merged() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let end = run_series(&fx.driver, &two).expect("series");
    let RunEnd::NeedsHuman(reason) = end else {
        panic!("expected a park, got {end:?}");
    };
    assert!(reason.contains("plan 1"), "{reason}");
    assert!(reason.contains(&one), "{reason}");
    assert_waiting(&fx, &two);
}

#[test]
fn plan_two_parks_when_plan_one_was_cancelled() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    fx.driver
        .store()
        .update_job(&one, |j| {
            j.cancelled = true;
            j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0)
        })
        .expect("cancel");
    let end = run_series(&fx.driver, &two).expect("series");
    let RunEnd::NeedsHuman(reason) = end else {
        panic!("expected a park, got {end:?}");
    };
    assert!(reason.contains(&one), "{reason}");
    assert!(
        reason.contains("cancel this plan, or replace plan 1 with factory new --replaces"),
        "{reason}"
    );
    assert!(reason.contains(&format!("--replaces {one}")), "{reason}");
    assert_waiting(&fx, &two);
}

/// Cancels `id` and adds a new job at its series place with plan file `plan`. Returns the new id.
/// The job is made from a lone plan, so `create_job` does not walk the chain behind `plan`.
fn stand_in(fx: &SeriesFixture, id: &str, plan: &Path) -> String {
    let old = fx.driver.job(id).expect("job");
    cancel(fx, id);
    let lone = lone_plan(fx, &format!("stand-in-{id}"));
    let made = create_job(&fx.driver, "fixed", "fixed plan", Some(&lone), None).expect("create");
    let mut pos = old.series.expect("series");
    pos.plan_file = plan.to_string_lossy().into_owned();
    fx.driver
        .store()
        .update_job(&made.id, |j| {
            j.series = Some(pos.clone());
            Ok(())
        })
        .expect("series");
    made.id
}

#[test]
fn series_guard_prefers_the_replacement_of_a_cancelled_plan() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let new_one = stand_in(&fx, &one, &fixed);
    assert_eq!(run_job(&fx.driver, &new_one).expect("run"), RunEnd::Merged);
    assert_eq!(
        run_series(&fx.driver, &two).expect("series"),
        RunEnd::Merged,
        "{}",
        ledger_of(&fx, &two)
    );
}

#[test]
fn run_series_finds_a_replacement_at_a_new_path_after_the_plan_before_merges() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-02-fixed.md", 1, "none");
    let new_two = stand_in(&fx, &two, &fixed);
    assert_eq!(
        run_series(&fx.driver, &one).expect("series"),
        RunEnd::Merged
    );
    assert_eq!(fx.driver.job(&new_two).expect("job").stage, Stage::Merged);
    assert!(fx.driver.job(&two).expect("job").cancelled);
    assert_eq!(
        fx.driver.store().load().expect("board").jobs.len(),
        3,
        "no job made for the old plan 2"
    );
}

#[test]
fn run_series_continues_the_next_plan_after_a_replacement_merges() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let new_one = stand_in(&fx, &one, &fixed);
    assert_eq!(
        run_series(&fx.driver, &new_one).expect("series"),
        RunEnd::Merged
    );
    assert_eq!(fx.driver.job(&two).expect("job").stage, Stage::Merged);
}

#[test]
fn run_series_stops_when_the_next_plan_is_cancelled() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    fx.driver
        .store()
        .update_job(&two, |j| {
            j.cancelled = true;
            j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0)
        })
        .expect("cancel");
    assert_eq!(
        run_series(&fx.driver, &one).expect("series"),
        RunEnd::Merged
    );
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2, "no new job for a cancelled plan");
    assert!(fx.driver.job(&two).expect("job").cancelled);
}

#[test]
fn merging_plan_one_continues_a_waiting_plan_two() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let end = run_series(&fx.driver, &two).expect("series");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let end = run_series(&fx.driver, &one).expect("series");
    assert_eq!(end, RunEnd::Merged, "{}", ledger_of(&fx, &two));
    let first = fx.driver.job(&one).expect("job");
    let second = fx.driver.job(&two).expect("job");
    assert_eq!(first.stage, Stage::Merged);
    assert_eq!(second.stage, Stage::Merged);
    let head = first.head.expect("plan 1 head");
    let base = second.base.expect("plan 2 base");
    assert!(
        fx.driver
            .repo()
            .is_ancestor(&head, &base)
            .expect("ancestor"),
        "plan 2 builds on plan 1's merge"
    );
    assert!(
        ledger_of(&fx, &two).contains("retry at build, plan 1 merged"),
        "{}",
        ledger_of(&fx, &two)
    );
}

#[test]
fn a_manual_retry_after_the_merge_builds_plan_two() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let end = run_series(&fx.driver, &two).expect("series");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    assert_eq!(run_job(&fx.driver, &one).expect("run"), RunEnd::Merged);
    let end = retry_job(&fx.driver, &two).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger_of(&fx, &two));
}

/// Writes a one-plan series into its own folder, so two of them are not one chain.
fn lone_plan(fx: &SeriesFixture, dir: &str) -> std::path::PathBuf {
    let dir = fx.tmp.path().join(dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("plan-01-x.md");
    std::fs::write(&path, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
    path
}

fn set_series(fx: &SeriesFixture, id: &str, plan: &Path, index: u32) {
    let pos = SeriesPos {
        plan_file: plan.to_string_lossy().into_owned(),
        index,
        total: 2,
        id: None,
    };
    fx.driver
        .store()
        .update_job(id, |j| {
            j.series = Some(pos);
            Ok(())
        })
        .expect("series");
}

#[test]
fn legacy_series_do_not_match_each_other() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = lone_plan(&fx, "p1");
    let b = lone_plan(&fx, "p2");
    let one = create_job(&fx.driver, "one", "first", Some(&a), None).expect("create");
    let two = create_job(&fx.driver, "two", "second", Some(&b), None).expect("create");
    set_series(&fx, &one.id, &a, 1);
    set_series(&fx, &two.id, &b, 2);
    fx.driver.build(&two.id).expect("build");
    let job = fx.driver.job(&two.id).expect("job");
    assert_eq!(job.park_kind, None, "{:?}", job.reason);
    assert!(job.worktree.is_some(), "plan 2 built");
}

/// A per-task builder that blocks, then `second`, over a fixture whose main moves in between.
/// Returns the fixture, the runner and the new main tip.
fn rework_after_main_moved(
    plan: &str,
    second: Effect,
    prepare: impl Fn(&Fixture),
) -> (Fixture, common::ScriptedRunner, String) {
    let fx = Fixture::new();
    std::fs::write(&fx.plan_file, plan).expect("plan");
    let runner = scripted(vec![reply("Status: BLOCKED\nReason: x"), second]);
    fx.engine(&runner).build_stage().expect("first build");
    let parked = fx.job();
    assert_eq!(parked.stage, Stage::NeedsHuman, "{:?}", parked.reason);
    let wt = parked.worktree.clone().expect("worktree");
    assert_eq!(
        fx.repo.tip(&parked.branch).expect("tip"),
        parked.base.clone().expect("base")
    );
    assert!(Path::new(&wt).is_dir());
    let tip = common::commit_file(&fx.main, "main.txt", "moved\n");
    prepare(&fx);
    fx.resume_to_build();
    fx.engine(&runner).build_stage().expect("second build");
    (fx, runner, tip)
}

/// An effect that notes whether main's new file is in the worktree, then builds.
fn sees_main(seen: Arc<AtomicBool>) -> Effect {
    Box::new(move |spec: &AgentSpec| {
        seen.store(spec.cwd.join("main.txt").exists(), Ordering::SeqCst);
        std::fs::write(spec.cwd.join("built.txt"), "built\n").expect("write");
        "Status: DONE".to_string()
    })
}

#[test]
fn a_per_task_rework_after_main_moved_recuts_and_keeps_the_note() {
    let seen = Arc::new(AtomicBool::new(false));
    let (fx, runner, tip) =
        rework_after_main_moved("### Task 1: t\n", sees_main(seen.clone()), |fx| {
            std::fs::write(fx.job_dir.join("rework-note.md"), "mind the gap").expect("note")
        });
    assert_eq!(fx.job().base.as_deref(), Some(tip.as_str()));
    assert!(
        fx.ledger().contains("re-cut the branch from"),
        "{}",
        fx.ledger()
    );
    assert!(seen.load(Ordering::SeqCst), "the builder saw main's file");
    let specs = runner.specs_for("builder");
    assert!(
        specs[1].prompt.contains("mind the gap"),
        "{}",
        specs[1].prompt
    );
}

#[test]
fn a_whole_plan_rework_after_main_moved_recuts_and_keeps_the_note() {
    let seen = Arc::new(AtomicBool::new(false));
    let (fx, runner, tip) =
        rework_after_main_moved("- [ ] Task 1\n", sees_main(seen.clone()), |fx| {
            std::fs::write(fx.job_dir.join("changes-r2.md"), "- [blocker] fix it\n")
                .expect("changes");
            BoardStore::new(&fx.board_path, Duration::from_secs(5))
                .update_job("001", |j| {
                    j.round = 2;
                    j.fix_source = Some("rework".to_string());
                    Ok(())
                })
                .expect("round");
        });
    assert_eq!(fx.job().base.as_deref(), Some(tip.as_str()));
    assert!(
        fx.ledger().contains("re-cut the branch from"),
        "{}",
        fx.ledger()
    );
    assert!(seen.load(Ordering::SeqCst), "the builder saw main's file");
    let specs = runner.specs_for("builder");
    let findings = fx.job_dir.join("changes-r2.md").display().to_string();
    assert!(specs[1].prompt.contains(&findings), "{}", specs[1].prompt);
}

/// Plan 2 of a series with a worktree cut while plan 1 was not merged. Returns the ids and the worktree.
fn stale_plan_two(fx: &SeriesFixture) -> (String, String, Worktree) {
    let (one, two) = two_plans(fx);
    let job = fx.driver.job(&two).expect("job");
    let repo = fx.driver.repo();
    let wt = repo
        .create_worktree(&repo.job_ref(&two, &job.slug))
        .expect("worktree");
    let (path, branch, base) = (
        wt.path.to_string_lossy().into_owned(),
        wt.branch.clone(),
        wt.base.clone(),
    );
    fx.driver
        .store()
        .update_job(&two, |j| {
            j.worktree = Some(path);
            j.branch = branch;
            j.base = Some(base);
            j.set_stage(Stage::Build, None, true, 2.0)
        })
        .expect("record");
    (one, two, wt)
}

#[test]
fn a_branch_with_commits_is_never_recut() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two, wt) = stale_plan_two(&fx);
    let own = common::commit_file(&wt.path, "own.txt", "mine\n");
    assert_eq!(run_job(&fx.driver, &one).expect("run"), RunEnd::Merged);
    fx.driver.build(&two).expect("build");
    let job = fx.driver.job(&two).expect("job");
    assert_eq!(job.base.as_deref(), Some(wt.base.as_str()));
    let tip = fx.driver.repo().tip(&wt.branch).expect("tip");
    assert!(
        fx.driver.repo().is_ancestor(&own, &tip).expect("ancestor"),
        "the job's commit stays on its branch"
    );
    assert!(
        !ledger_of(&fx, &two).contains("re-cut"),
        "{}",
        ledger_of(&fx, &two)
    );
    assert!(
        !ledger_of(&fx, &two).contains("kept the stale worktree"),
        "{}",
        ledger_of(&fx, &two)
    );
}

#[test]
fn a_dirty_empty_worktree_is_kept_with_a_ruling() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two, wt) = stale_plan_two(&fx);
    let stray = wt.path.join("stray.txt");
    std::fs::write(&stray, "work\n").expect("stray");
    assert_eq!(run_job(&fx.driver, &one).expect("run"), RunEnd::Merged);
    fx.driver.build(&two).expect("build");
    let ledger = ledger_of(&fx, &two);
    assert!(ledger.contains("kept the stale worktree"), "{ledger}");
    assert!(!ledger.contains("re-cut"), "{ledger}");
    assert!(stray.exists());
    let job = fx.driver.job(&two).expect("job");
    assert_eq!(job.base.as_deref(), Some(wt.base.as_str()));
}

#[test]
fn an_interrupted_recut_is_finished() {
    let fx = Fixture::new();
    let runner = scripted(vec![
        reply("Status: BLOCKED\nReason: x"),
        Box::new(|spec: &AgentSpec| {
            std::fs::write(spec.cwd.join("built.txt"), "built\n").expect("write");
            "Status: DONE".to_string()
        }),
    ]);
    fx.engine(&runner).build_stage().expect("first build");
    let parked = fx.job();
    let path = std::path::PathBuf::from(parked.worktree.clone().expect("worktree"));
    let wt = Worktree {
        name: path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned(),
        path: path.clone(),
        branch: parked.branch.clone(),
        base: parked.base.clone().expect("base"),
    };
    fx.repo.remove_worktree(&wt, false).expect("remove");
    assert!(!path.exists());
    assert!(!fx.repo.branch_exists(&parked.branch));
    fx.resume_to_build();
    fx.engine(&runner).build_stage().expect("second build");
    assert!(
        fx.ledger().contains("finished an interrupted re-cut"),
        "{}",
        fx.ledger()
    );
    let job = fx.job();
    let fresh = job.worktree.expect("fresh worktree");
    assert!(Path::new(&fresh).join("built.txt").exists());
    assert!(job.head.is_some(), "the job built");
}

fn replace<'a>(target: &'a str, plan: &'a Path) -> Replacement<'a> {
    Replacement {
        target,
        slug: "fixed",
        feature: "fixed plan",
        plan,
        spec: None,
        model: None,
        accept_spec: None,
    }
}

/// The spec text of `an_explicit_spec_path_replaces_the_spec_beside_the_plans` in tests/runloop.rs.
const GOOD_SPEC: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n## Acceptance criteria\n\n- It is done.\n";

/// A fixture with `src-tauri/**` as a risk path and no spec reviewer, as `risky_config` in runloop.rs.
fn risky_fixture() -> SeriesFixture {
    let config = factory_core::Config {
        risk_paths: vec!["src-tauri/**".to_string()],
        spec_review: false,
        ..factory_core::Config::default()
    };
    SeriesFixture::with_config("fine\nVERDICT: APPROVE", config)
}

fn write_target_spec(fx: &SeriesFixture, id: &str, text: &str) {
    let job = fx.driver.job(id).expect("job");
    std::fs::write(fx.driver.job_dir(&job).join("spec.md"), text).expect("spec");
}

fn cancel(fx: &SeriesFixture, id: &str) {
    fx.driver
        .store()
        .update_job(id, |j| {
            j.cancelled = true;
            j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0)
        })
        .expect("cancel");
}

#[test]
fn create_replacement_takes_the_index_and_cancels_the_target() {
    let fx = risky_fixture();
    let (one, _two) = two_plans(&fx);
    write_target_spec(&fx, &one, GOOD_SPEC);
    let fixed = fx.plan_file("plan-01-fixed.md", 2, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    let old = fx.driver.job(&one).expect("old");
    let new = fx.driver.job(&made.id).expect("new");
    assert!(old.cancelled);
    assert_eq!(
        old.reason.as_deref(),
        Some(format!("cancelled: replaced by job {}", made.id).as_str())
    );
    let (op, np) = (
        old.series.expect("old pos"),
        new.series.clone().expect("new pos"),
    );
    assert_eq!((np.id, np.index, np.total), (op.id, op.index, op.total));
    assert_eq!(
        std::fs::canonicalize(&np.plan_file).expect("canon"),
        std::fs::canonicalize(&fixed).expect("canon")
    );
    assert_eq!(new.stage, Stage::Build);
    assert!(ledger_of(&fx, &made.id).contains(&format!("replaces job {one}")));
    assert!(ledger_of(&fx, &one).contains(&format!("replaced by job {}", made.id)));
    let plan_md =
        std::fs::read_to_string(fx.driver.job_dir(&new).join("plan.md")).expect("plan.md");
    assert!(plan_md.contains("### Task 2"), "the fixed plan was copied");
    assert_eq!(
        fx.driver.store().load().expect("board").jobs.len(),
        3,
        "one new job only"
    );
}

#[test]
fn create_replacement_checks_the_target_spec_without_a_spec_path() {
    let fx = risky_fixture();
    let (one, _two) = two_plans(&fx);
    write_target_spec(&fx, &one, GOOD_SPEC);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    let job = fx.driver.job(&made.id).expect("new");
    let text = std::fs::read_to_string(fx.driver.job_dir(&job).join("spec.md")).expect("spec");
    assert_eq!(text, GOOD_SPEC);
    write_target_spec(&fx, &made.id, "# Spec\n\n## Goal\n\nshort\n");
    let again = fx.plan_file("plan-01-again.md", 1, "plan-02-b.md");
    create_replacement(&fx.driver, &replace(&made.id, &again))
        .expect_err("the old spec is checked");
    assert!(!fx.driver.job(&made.id).expect("job").cancelled);
}

#[test]
fn create_replacement_without_spec_applies_the_risky_path_rule() {
    let fx = risky_fixture();
    let (one, _two) = two_plans(&fx);
    write_target_spec(&fx, &one, GOOD_SPEC);
    let next = fx.tmp.path().join("plans/plan-02-b.md");
    let fixed = fx.tmp.path().join("plans/plan-01-risky.md");
    std::fs::write(
        &fixed,
        format!(
            "# Plan\n\n### Task 1: t\n\n**Files:**\n- Modify: `src-tauri/src/lib.rs`\n\nNext plan: {}\n",
            next.display()
        ),
    )
    .expect("plan");
    let err = create_replacement(&fx.driver, &replace(&one, &fixed)).expect_err("risky");
    assert!(err.to_string().contains("risky"), "{err}");
    assert!(!fx.driver.job(&one).expect("old").cancelled);
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 2);
}

#[test]
fn create_replacement_of_an_unstarted_plan_makes_one_job() {
    let fx = risky_fixture();
    let (_one, two) = two_plans(&fx);
    write_target_spec(&fx, &two, GOOD_SPEC);
    let fixed = fx.plan_file("plan-02-fixed.md", 1, "none");
    let made = create_replacement(&fx.driver, &replace(&two, &fixed)).expect("replace");
    let old = fx.driver.job(&two).expect("old");
    assert!(old.cancelled);
    assert!(!fx.driver.repo().branch_exists(&old.branch));
    let new = fx.driver.job(&made.id).expect("new");
    assert_eq!(new.series.expect("pos").index, 2);
}

#[test]
fn run_series_continues_a_waiting_plan_after_its_replaced_predecessor_merges() {
    let fx = risky_fixture();
    let (one, two) = two_plans(&fx);
    write_target_spec(&fx, &one, GOOD_SPEC);
    cancel(&fx, &one);
    assert!(matches!(
        run_series(&fx.driver, &two).expect("series"),
        RunEnd::NeedsHuman(_)
    ));
    assert_waiting(&fx, &two);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    assert_eq!(
        run_series(&fx.driver, &made.id).expect("series"),
        RunEnd::Merged
    );
    assert_eq!(fx.driver.job(&two).expect("job").stage, Stage::Merged);
}

#[test]
fn create_replacement_with_a_refused_spec_changes_nothing() {
    let fx = risky_fixture();
    let (one, _two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let bad = fx.tmp.path().join("bad-spec.md");
    std::fs::write(&bad, "# Spec\n\n## Goal\n\nshort\n").expect("spec");
    let mut r = replace(&one, &fixed);
    r.spec = Some(bad.as_path());
    create_replacement(&fx.driver, &r).expect_err("refused spec");
    assert!(!fx.driver.job(&one).expect("old").cancelled);
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 2);
}

#[test]
fn new_replaces_accepts_a_relative_next_line_that_resolves_to_the_same_file() {
    let fx = risky_fixture();
    let (one, _two) = two_plans(&fx);
    write_target_spec(&fx, &one, GOOD_SPEC);
    let fixed = fx.tmp.path().join("plans/plan-01-rel.md");
    std::fs::write(
        &fixed,
        "# Plan\n\n### Task 1: t\n\nNext plan: plan-02-b.md\n",
    )
    .expect("plan");
    create_replacement(&fx.driver, &replace(&one, &fixed)).expect("relative next resolves");
}

#[test]
fn create_replacement_refusals() {
    let rows = [
        ("folder", "needs one plan file"),
        ("merged", "is already merged"),
        ("no_series", "is not in a series"),
        ("live_lease", "live lease"),
        ("wrong_next", "Next plan: "),
        ("edited_in_place", "Next plan: "),
        ("wrong_next_on_last", "Next plan: none"),
        ("already_replaced", "was replaced by job"),
        ("no_spec", "has no spec.md, pass --spec"),
    ];
    for (row, text) in rows {
        let fx = risky_fixture();
        let (one, two) = two_plans(&fx);
        write_target_spec(&fx, &one, GOOD_SPEC);
        write_target_spec(&fx, &two, GOOD_SPEC);
        let plans = fx.tmp.path().join("plans");
        let second = plans.join("plan-02-b.md");
        let ok_fix = || fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
        let (target, fixed) = match row {
            "folder" => (one.clone(), plans.clone()),
            "merged" => {
                fx.driver
                    .store()
                    .update_job(&one, |j| j.set_stage(Stage::Merged, None, true, 2.0))
                    .expect("merged");
                (one.clone(), ok_fix())
            }
            "no_series" => {
                let lone = lone_plan(&fx, "p1");
                let job = create_job(&fx.driver, "lone", "lone", Some(&lone), None).expect("job");
                write_target_spec(&fx, &job.id, GOOD_SPEC);
                (job.id, lone)
            }
            "live_lease" => {
                factory_engine::acquire_lease(
                    fx.driver.store(),
                    &one,
                    "other",
                    600.0,
                    factory_engine::now_secs(),
                )
                .expect("lease");
                (one.clone(), ok_fix())
            }
            "wrong_next" => (one.clone(), fx.plan_file("plan-01-fixed.md", 1, "none")),
            "edited_in_place" => (one.clone(), fx.plan_file("plan-01-a.md", 1, "none")),
            "wrong_next_on_last" => (
                two.clone(),
                fx.plan_file("plan-02-fixed.md", 1, "plan-01-a.md"),
            ),
            "already_replaced" => {
                let fixed = ok_fix();
                create_replacement(&fx.driver, &replace(&one, &fixed)).expect("first");
                (one.clone(), fixed)
            }
            "no_spec" => {
                let job = fx.driver.job(&one).expect("job");
                std::fs::remove_file(fx.driver.job_dir(&job).join("spec.md")).expect("rm spec");
                (one.clone(), ok_fix())
            }
            other => panic!("unknown row {other}"),
        };
        let before = fx.driver.store().load().expect("board").jobs.len();
        let err = create_replacement(&fx.driver, &replace(&target, &fixed))
            .expect_err(row)
            .to_string();
        assert!(err.contains(text), "{row}: {err}");
        if row == "wrong_next" || row == "edited_in_place" {
            assert!(err.contains(&second.display().to_string()), "{row}: {err}");
        }
        assert_eq!(
            fx.driver.store().load().expect("board").jobs.len(),
            before,
            "{row}: the board changed"
        );
    }
}
