mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{reply, scripted, Effect, Fixture, SeriesFixture};
use factory_agent::AgentSpec;
use factory_core::{ResumeAt, SeriesPos, Stage};
use factory_engine::{create_job, retry_job, run_job, run_series, BoardStore, RunEnd};
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
    assert!(reason.contains("cancel this plan too"), "{reason}");
    assert_waiting(&fx, &two);
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
    assert!(fx.ledger().contains("re-cut"), "{}", fx.ledger());
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
    assert!(fx.ledger().contains("re-cut"), "{}", fx.ledger());
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
