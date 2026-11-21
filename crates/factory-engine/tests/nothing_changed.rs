//! A builder that changes nothing on a branch that already holds commits goes on to review.

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use common::{scripted, Effect, Fixture};
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_core::{Board, CheckRule, Config, Job, Stage};
use factory_engine::{retry_job, run_job, Driver, RunEnd, RunnerFactory, StageOutcome};

const CHANGES: &str = "1. a.txt:1 [blocker] handle the empty case.\nVERDICT: CHANGES";
const APPROVE: &str = "ok\nVERDICT: APPROVE";
const REPLY: &str = "finding 1 is already handled in src/a.rs";
const WHOLE_LINE: &str = "builder changed nothing; it says the work is already on the branch";
const NEW_SCOPE: &str = "The builder changed nothing this round and says why in its reply. The diff is the whole branch. Judge each earlier finding against it and the reply.";
const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";
const THREE_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n\n### Task 3: third thing\n\nz\n";

/// A builder that writes `file` with `lines` lines and leaves it uncommitted.
fn change(file: &'static str, lines: usize) -> Effect {
    Box::new(move |spec| {
        let body: String = (0..lines).map(|n| format!("line {n}\n")).collect();
        std::fs::write(spec.cwd.join(file), body).expect("write change");
        "done".to_string()
    })
}

/// An agent that only replies.
fn say(text: &'static str) -> Effect {
    Box::new(move |_| text.to_string())
}

fn set_job(fx: &Fixture, f: impl FnOnce(&mut Job)) {
    let mut board = Board::load(&fx.board_path).expect("load");
    f(board.find_mut("001").expect("job"));
    board.save(&fx.board_path).expect("save");
}

/// Runs the checks, then one review with `reply`. Returns the reviewer spec and the outcome.
fn checks_and_review(fx: &Fixture, reply: &'static str) -> (AgentSpec, StageOutcome) {
    let checks = scripted(vec![]);
    fx.engine(&checks).checks_stage().expect("checks");
    let reviewer = scripted(vec![say(reply)]);
    let e = fx.engine(&reviewer);
    let out = e.review_stage().expect("review");
    let stage = e.apply_decision(&out).expect("apply");
    (
        reviewer.last_spec_for("reviewer").expect("reviewer spec"),
        stage,
    )
}

/// Builds round 1 with `a.txt`, then a review asks for changes, so the job is in fix round 2.
fn to_fix_round(fx: &Fixture) {
    let builder = scripted(vec![change("a.txt", 3)]);
    assert_eq!(
        fx.engine(&builder).build_stage().expect("build"),
        StageOutcome::Advanced(Stage::Review)
    );
    let (_, out) = checks_and_review(fx, CHANGES);
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().round, 2);
}

/// The diff file the reviewer prompt names.
fn reviewer_diff(spec: &AgentSpec) -> String {
    let at = spec.prompt.find("the diff at ").expect("diff marker") + "the diff at ".len();
    let rest = &spec.prompt[at..];
    let end = rest.find(", the list").expect("diff end");
    std::fs::read_to_string(&rest[..end]).expect("diff file")
}

fn task_fixture(plan: &str) -> Fixture {
    let fx = Fixture::new();
    std::fs::write(&fx.plan_file, plan).expect("plan");
    set_job(&fx, |j| {
        j.set_stage(Stage::Build, None, false, 1.0).expect("stage");
    });
    fx
}

#[test]
fn whole_plan_rework_that_changes_nothing_goes_to_review() {
    let mut fx = Fixture::new();
    fx.config.fix_rounds = 0;
    let builder = scripted(vec![change("a.txt", 3)]);
    fx.engine(&builder).build_stage().expect("build");
    let (_, out) = checks_and_review(&fx, CHANGES);
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    // A rework sends a note, starts the next round and grants fresh fix rounds.
    std::fs::write(fx.job_dir.join("rework-note.md"), "look again").expect("note");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "look again").expect("findings");
    set_job(&fx, |j| {
        j.round = 2;
        j.rework_base = 1;
        j.fix_source = Some("rework".into());
        j.resume(Stage::Build, 1.0).expect("resume");
    });
    let builder = scripted(vec![say(REPLY)]);
    let out = fx.engine(&builder).build_stage().expect("build r2");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert_eq!(job.no_change_round, Some(2));
    assert!(fx.ledger().contains(WHOLE_LINE), "{}", fx.ledger());
    assert!(!fx.job_dir.join("rework-note.md").exists());
    let reply = fx.job_dir.join("builder-r2.md");
    let saved = std::fs::read_to_string(&reply).expect("builder-r2.md");
    assert!(saved.contains(REPLY), "{saved}");
    let (spec, _) = checks_and_review(&fx, APPROVE);
    assert!(reviewer_diff(&spec).contains("a.txt"), "{}", spec.prompt);
    assert!(
        spec.prompt.contains(&reply.display().to_string()),
        "{}",
        spec.prompt
    );
    assert!(spec.prompt.contains("scoped re-review"), "{}", spec.prompt);
    assert!(
        !spec.prompt.contains("Inspect the fix diff only"),
        "{}",
        spec.prompt
    );
    assert!(spec.prompt.contains(NEW_SCOPE), "{}", spec.prompt);
    assert!(
        spec.prompt.contains("Out-of-scope observations"),
        "{}",
        spec.prompt
    );
}

/// Drives no-change fix rounds until the job parks, and returns the round it parked in.
fn no_change_rounds_until_park(fx: &Fixture) -> u32 {
    to_fix_round(fx);
    for _ in 0..5 {
        let builder = scripted(vec![say(REPLY)]);
        assert_eq!(
            fx.engine(&builder).build_stage().expect("build"),
            StageOutcome::Advanced(Stage::Review)
        );
        let (_, out) = checks_and_review(fx, CHANGES);
        if let StageOutcome::NeedsHuman(_) = out {
            return fx.job().round;
        }
    }
    panic!("the job never parked");
}

#[test]
fn review_fix_round_that_changes_nothing_reaches_cap() {
    let fx = Fixture::new();
    assert_eq!(no_change_rounds_until_park(&fx), 3);
    assert_eq!(fx.job().park_kind.as_deref(), Some("changes-remain"));

    let mut fx = Fixture::new();
    fx.config.fix_rounds = 5;
    fx.config.risky_fix_rounds = Some(1);
    fx.config.risk_paths = vec!["a.txt".into()];
    assert_eq!(no_change_rounds_until_park(&fx), 2);
    assert_eq!(fx.job().park_kind.as_deref(), Some("changes-remain"));
}

#[test]
fn checks_round_that_changes_nothing_uses_whole_diff() {
    let fx = Fixture::new();
    let builder = scripted(vec![change("a.txt", 3)]);
    fx.engine(&builder).build_stage().expect("build");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix the checks").expect("findings");
    set_job(&fx, |j| {
        j.round = 2;
        j.fix_source = Some("checks".into());
        j.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    });
    let builder = scripted(vec![say(REPLY)]);
    assert_eq!(
        fx.engine(&builder).build_stage().expect("build r2"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(fx.job().no_change_round, Some(2));
    let (spec, _) = checks_and_review(&fx, APPROVE);
    assert!(reviewer_diff(&spec).contains("a.txt"), "{}", spec.prompt);
}

#[test]
fn last_task_that_changes_nothing_goes_to_review() {
    let fx = task_fixture(TWO_TASKS);
    let builder = scripted(vec![change("a.txt", 3), say(REPLY)]);
    let e = fx.engine(&builder);
    assert_eq!(
        e.build_stage().expect("task 1"),
        StageOutcome::Advanced(Stage::Build)
    );
    // The previous round ended at this tip, as in a rework round, so task 2 changes nothing for the round.
    let tip = fx.repo.tip(&fx.job().branch).expect("tip");
    set_job(&fx, |j| j.head = Some(tip));
    assert_eq!(
        e.build_stage().expect("task 2"),
        StageOutcome::Advanced(Stage::Review)
    );
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    // The engine counts a round-0 board job as round 1.
    let round = job.round.max(1);
    assert_eq!(job.no_change_round, Some(round));
    let ledger = fx.ledger();
    assert!(
        ledger.contains("task 2 changed nothing; it says the work is already on the branch"),
        "{ledger}"
    );
    let reply = fx.job_dir.join(format!("builder-r{round}.md"));
    let saved = std::fs::read_to_string(&reply).expect("builder reply");
    assert!(saved.contains("## Task 2"), "{saved}");
    assert!(saved.contains(REPLY), "{saved}");
    let (spec, _) = checks_and_review(&fx, APPROVE);
    assert!(reviewer_diff(&spec).contains("a.txt"), "{}", spec.prompt);
    assert!(
        spec.prompt.contains(&reply.display().to_string()),
        "{}",
        spec.prompt
    );
}

#[test]
fn last_task_that_changes_nothing_after_new_commits_is_not_a_no_change_round() {
    let fx = task_fixture(TWO_TASKS);
    let builder = scripted(vec![change("a.txt", 3), say(REPLY)]);
    let e = fx.engine(&builder);
    e.build_stage().expect("task 1");
    assert_eq!(
        e.build_stage().expect("task 2"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(fx.job().no_change_round, None);
    let (spec, _) = checks_and_review(&fx, APPROVE);
    assert!(
        !spec
            .prompt
            .contains("The builder changed nothing this round"),
        "{}",
        spec.prompt
    );
}

#[test]
fn middle_task_that_changes_nothing_advances() {
    let fx = task_fixture(THREE_TASKS);
    let builder = scripted(vec![change("a.txt", 3), say(REPLY), change("c.txt", 2)]);
    let e = fx.engine(&builder);
    e.build_stage().expect("task 1");
    let tip = fx.repo.tip(&fx.job().branch).expect("tip");
    assert_eq!(
        e.build_stage().expect("task 2"),
        StageOutcome::Advanced(Stage::Build)
    );
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    assert_eq!(job.builder_last.as_ref().map(|l| l.task), Some(2));
    assert_eq!(fx.repo.tip(&job.branch).expect("tip"), tip);
    let ledger = fx.ledger();
    assert!(ledger.contains("Task 2: implementer done"), "{ledger}");
    assert!(
        ledger.contains("task 2 changed nothing; it says the work is already on the branch"),
        "{ledger}"
    );
    assert_eq!(
        e.build_stage().expect("task 3"),
        StageOutcome::Advanced(Stage::Review)
    );
    let prompt = builder.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains("Do only Task 3"), "{prompt}");
    assert_eq!(fx.job().no_change_round, None);
}

#[test]
fn first_task_with_no_commits_still_parks() {
    let fx = task_fixture(TWO_TASKS);
    let builder = scripted(vec![say(REPLY)]);
    match fx.engine(&builder).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("task 1 changed nothing"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let job = fx.job();
    assert_eq!(job.park_kind.as_deref(), Some("nothing-changed"));
    assert_eq!(job.task_index, 0);
    assert_eq!(job.no_change_round, None);
}

#[test]
fn no_change_round_still_hits_diff_cap() {
    let mut fx = Fixture::new();
    to_fix_round(&fx);
    fx.config.diff_cap_lines = 2;
    let builder = scripted(vec![say(REPLY)]);
    match fx.engine(&builder).build_stage().expect("build r2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("the cap is 2"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().park_kind.as_deref(), Some("diff-too-large"));
}

#[test]
fn no_change_round_still_hits_risky_park() {
    let mut fx = Fixture::new();
    fx.config.risky_fix_rounds = Some(1);
    fx.config.risk_paths = vec!["a.txt".into()];
    to_fix_round(&fx);
    let builder = scripted(vec![say(REPLY)]);
    assert_eq!(
        fx.engine(&builder).build_stage().expect("build r2"),
        StageOutcome::Advanced(Stage::Review)
    );
    let (_, out) = checks_and_review(&fx, APPROVE);
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(fx.job().park_kind.as_deref(), Some("risky-diff"));
}

/// Answers by role: builders and reviewers from their own queues, every other role approves.
struct ByRole {
    builder: Mutex<VecDeque<Effect>>,
    reviewer: Mutex<VecDeque<AgentRun>>,
    reviews: Mutex<Vec<AgentSpec>>,
}

impl AgentRunner for ByRole {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let run = match spec.role.as_str() {
            "builder" => {
                let next = self.builder.lock().expect("builder").pop_front();
                next.map_or_else(
                    || AgentRun::failed("script exhausted"),
                    |effect| AgentRun::done(&effect(spec)),
                )
            }
            "reviewer" => {
                self.reviews.lock().expect("reviews").push(spec.clone());
                let next = self.reviewer.lock().expect("reviewer").pop_front();
                next.unwrap_or_else(|| AgentRun::failed("script exhausted"))
            }
            _ => AgentRun::done(APPROVE),
        };
        AgentRun {
            session_id: Some("sess-1".to_string()),
            ..run
        }
    }
}

#[test]
fn no_change_round_survives_review_retry() {
    let runner = Arc::new(ByRole {
        builder: Mutex::new(VecDeque::from([change("a.txt", 3), say(REPLY)])),
        reviewer: Mutex::new(VecDeque::from([
            AgentRun::done(CHANGES),
            AgentRun::failed("reviewer crashed"),
            AgentRun::done(CHANGES),
        ])),
        reviews: Mutex::new(Vec::new()),
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    common::init_repo(&main);
    let shared = runner.clone();
    let factory: RunnerFactory = Box::new(move |_job_id| shared.clone());
    let config = Config {
        fix_rounds: 1,
        checks: vec![CheckRule {
            paths: vec!["**".into()],
            commands: vec!["true".into()],
            ..CheckRule::default()
        }],
        ..Config::default()
    };
    let driver = Driver::new(
        factory_core::Layout::in_repo(&main),
        common::test_config(config),
        factory,
    )
    .expect("driver");
    let mut job = Job::new(
        "001",
        "2026-10-13-001-hello",
        "hello",
        "2026-10-13",
        "say hello",
        0.0,
    );
    job.set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("job dir");
    std::fs::write(dir.join("plan.md"), "- [ ] Task 1: hello\n").expect("plan");
    driver.ledger(&job).create("# SDD ledger").expect("ledger");
    driver
        .store()
        .update(|b| {
            b.jobs.push(job);
            Ok(())
        })
        .expect("board");

    let end = run_job(&driver, "001").expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = driver.job("001").expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("review-failed"));
    assert_eq!(job.no_change_round, Some(2));
    let end = retry_job(&driver, "001").expect("retry");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = driver.job("001").expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("changes-remain"));
    let reviews = runner.reviews.lock().expect("reviews").clone();
    assert_eq!(reviews.len(), 3);
    let retried = &reviews[2];
    assert!(
        reviewer_diff(retried).contains("a.txt"),
        "{}",
        retried.prompt
    );
    assert!(
        retried.prompt.contains("builder-r2.md"),
        "{}",
        retried.prompt
    );
    assert!(
        retried
            .prompt
            .contains("The builder changed nothing this round"),
        "{}",
        retried.prompt
    );
}

#[test]
fn no_change_round_is_cleared_at_next_build() {
    let fx = Fixture::new();
    to_fix_round(&fx);
    let builder = scripted(vec![say(REPLY)]);
    fx.engine(&builder).build_stage().expect("build r2");
    assert_eq!(fx.job().no_change_round, Some(2));
    let (_, out) = checks_and_review(&fx, CHANGES);
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().round, 3);
    // A round that parks before it records a head still clears the older mark.
    let blocked = scripted(vec![say("Status: BLOCKED: the schema is gone")]);
    let out = fx.engine(&blocked).build_stage().expect("build r3");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(fx.job().no_change_round, None);
    fx.resume_to_build();
    let builder = scripted(vec![change("b.txt", 2)]);
    assert_eq!(
        fx.engine(&builder).build_stage().expect("build r3 again"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(fx.job().no_change_round, None);
    let (spec, _) = checks_and_review(&fx, APPROVE);
    let diff = reviewer_diff(&spec);
    assert!(diff.contains("b.txt"), "{diff}");
    assert!(!diff.contains("a.txt"), "{diff}");
    assert!(
        spec.prompt.contains("Inspect the fix diff only"),
        "{}",
        spec.prompt
    );
}
