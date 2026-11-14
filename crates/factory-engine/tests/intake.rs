mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use factory_agent::{AgentRun, AgentRunner, AgentSpec, StopReason};
use factory_core::{AgentFile, Config, Stage};
use factory_engine::{
    create_job, run_intake, run_series, IntakeOutcome, IssueInfo, STATE_DONE, STATE_NEEDS_HUMAN,
    STATE_RUNNING,
};

const GOOD_SPEC: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n## Acceptance criteria\n\n- It is done.\n\n## Decisions\n\n- Use the readme. Source: README.md:1. Decided by: call\n";

/// What the scripted drafter writes into its draft folder, and what the spec reviewer answers.
struct Script {
    draft: Box<dyn Fn(&Path) + Send + Sync>,
    review: String,
    drafter_reply: fn() -> AgentRun,
    drafter_specs: Mutex<Vec<AgentSpec>>,
    /// The job whose reviewers ask for changes, if any.
    changes_for: Option<&'static str>,
}

type DrafterReply = fn() -> AgentRun;

fn drafted() -> AgentRun {
    AgentRun::done("drafted")
}

/// Plays the drafter and the spec reviewer from the script and hands every other role to the
/// series fixture's runner.
struct IntakeRunner {
    inner: common::RoleRunner,
    script: Arc<Script>,
}

impl AgentRunner for IntakeRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        match spec.role.as_str() {
            "intake-drafter" => {
                self.script
                    .drafter_specs
                    .lock()
                    .expect("specs")
                    .push(spec.clone());
                let draft = spec.cwd.parent().expect("issue folder").join("draft");
                (self.script.draft)(&draft);
                (self.script.drafter_reply)()
            }
            "spec-reviewer" => AgentRun::done(&self.script.review),
            role if self.script.changes_for == Some(spec.job_id.as_str())
                && !["builder", "committer"].contains(&role) =>
            {
                AgentRun::done("[blocker] wrong\nVERDICT: CHANGES")
            }
            _ => self.inner.run(spec),
        }
    }
}

struct Intake {
    _tmp: tempfile::TempDir,
    main: std::path::PathBuf,
    driver: factory_engine::Driver,
    tracker: common::FakeTracker,
    script: Arc<Script>,
}

fn issue(number: u32) -> IssueInfo {
    IssueInfo {
        number,
        title: format!("Fix thing {number}"),
        body: "The thing is broken.".to_string(),
        labels: vec!["factory".to_string()],
        created_at: "2026-11-01T00:00:00Z".to_string(),
        updated_at: "2026-11-01T00:00:00Z".to_string(),
    }
}

fn config() -> Config {
    Config {
        intake: true,
        intake_trusted: vec!["alice".to_string()],
        ..Config::default()
    }
}

fn plan(tasks: u32, next: &str) -> String {
    let mut s = String::from("# Plan\n\n");
    for n in 1..=tasks {
        s.push_str(&format!("### Task {n}: t\n\n"));
    }
    s.push_str(&format!("Next plan: {next}\n"));
    s
}

/// A draft of one plan.
fn one_plan(draft: &Path) {
    std::fs::write(draft.join("spec.md"), GOOD_SPEC).expect("spec");
    std::fs::write(draft.join("plan-01-a.md"), plan(1, "none")).expect("plan");
}

/// A draft of two plans.
fn two_plans(draft: &Path) {
    std::fs::write(draft.join("spec.md"), GOOD_SPEC).expect("spec");
    std::fs::write(draft.join("plan-01-a.md"), plan(1, "plan-02-b.md")).expect("plan 1");
    std::fs::write(draft.join("plan-02-b.md"), plan(1, "none")).expect("plan 2");
}

const SPEC_WITH_FOLLOW_UPS: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n## Acceptance criteria\n\n- It is done.\n\n## Follow-ups\n\n- SECRET-TITLE other work. More detail.\n";

/// A draft of one plan with a follow-up in the spec.
fn one_plan_with_follow_ups(draft: &Path) {
    std::fs::write(draft.join("spec.md"), SPEC_WITH_FOLLOW_UPS).expect("spec");
    std::fs::write(draft.join("plan-01-a.md"), plan(1, "none")).expect("plan");
}

/// A draft of two plans with a follow-up in the spec.
fn two_plans_with_follow_ups(draft: &Path) {
    std::fs::write(draft.join("spec.md"), SPEC_WITH_FOLLOW_UPS).expect("spec");
    std::fs::write(draft.join("plan-01-a.md"), plan(1, "plan-02-b.md")).expect("plan 1");
    std::fs::write(draft.join("plan-02-b.md"), plan(1, "none")).expect("plan 2");
}

fn setup_with(
    config: Config,
    issues: Vec<IssueInfo>,
    draft: impl Fn(&Path) + Send + Sync + 'static,
    review: &str,
) -> Intake {
    setup_full(
        config,
        issues,
        draft,
        review,
        "fine\nVERDICT: APPROVE",
        drafted,
    )
}

/// `verdict` is what the code reviewers answer, `drafter_reply` the run the drafter returns.
fn setup_full(
    config: Config,
    issues: Vec<IssueInfo>,
    draft: impl Fn(&Path) + Send + Sync + 'static,
    review: &str,
    verdict: &str,
    drafter_reply: fn() -> AgentRun,
) -> Intake {
    setup_inner(config, issues, draft, review, verdict, drafter_reply, None)
}

/// Like `setup_full` with the two-plan draft, and every reviewer of `job` answers with a blocker.
fn setup_changes_for(job: &'static str, config: Config) -> Intake {
    setup_inner(
        config,
        vec![issue(7)],
        two_plans,
        "fine\nVERDICT: APPROVE",
        "fine\nVERDICT: APPROVE",
        drafted,
        Some(job),
    )
}

fn setup_inner(
    config: Config,
    issues: Vec<IssueInfo>,
    draft: impl Fn(&Path) + Send + Sync + 'static,
    review: &str,
    verdict: &str,
    drafter_reply: fn() -> AgentRun,
    changes_for: Option<&'static str>,
) -> Intake {
    let script = Arc::new(Script {
        draft: Box::new(draft),
        review: review.to_string(),
        drafter_reply,
        drafter_specs: Mutex::new(Vec::new()),
        changes_for,
    });
    let wrapped = script.clone();
    let fx = common::SeriesFixture::build(verdict, config, move |inner| {
        Arc::new(IntakeRunner {
            inner,
            script: wrapped.clone(),
        })
    });
    let tracker = common::FakeTracker::new("o/r")
        .with_issues(issues)
        .with_label_actor(7, "factory", "alice");
    let common::SeriesFixture { tmp, main, driver } = fx;
    Intake {
        _tmp: tmp,
        main,
        driver: driver.with_tracker(Box::new(tracker.clone())),
        tracker,
        script,
    }
}

fn setup(draft: impl Fn(&Path) + Send + Sync + 'static) -> Intake {
    setup_with(config(), vec![issue(7)], draft, "fine\nVERDICT: APPROVE")
}

fn edits(it: &Intake) -> Vec<(u32, Vec<String>, Vec<String>)> {
    it.tracker.state.lock().expect("lock").label_edits.clone()
}

fn comments(it: &Intake) -> Vec<(u32, String)> {
    it.tracker.state.lock().expect("lock").comments.clone()
}

fn pass(it: &Intake) -> IntakeOutcome {
    run_intake(&it.driver, &it.tracker, false).expect("run_intake")
}

fn s(text: &str) -> Vec<String> {
    vec![text.to_string()]
}

#[test]
fn a_multi_plan_series_names_the_unmerged_job_in_the_failure_comment() {
    // No fix rounds, so the first changes verdict on job 002 parks it.
    let cfg = Config {
        fix_rounds: 0,
        ..config()
    };
    let it = setup_changes_for("002", cfg);
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "build-failed".to_string()
        }
    );
    let board = it.driver.store().load().expect("board");
    let first = board.find("001").expect("job 001");
    assert_eq!(first.stage, Stage::Merged);
    let said = comments(&it);
    assert_eq!(said[0].1, "factory picked this up as job 001, 002");
    let last = &said.last().expect("comment").1;
    assert!(last.contains("reason: build-failed"), "{last}");
    assert!(last.contains("job: 002"), "{last}");
    assert!(!last.contains("job: 001"), "{last}");
}

#[test]
fn an_off_config_does_nothing() {
    let it = setup_with(Config::default(), vec![issue(7)], one_plan, "x");
    assert_eq!(pass(&it), IntakeOutcome::Off);
    assert!(it.tracker.calls().is_empty());
}

#[test]
fn a_pass_with_no_eligible_issue_changes_nothing() {
    let it = setup_with(config(), Vec::new(), one_plan, "x");
    assert_eq!(pass(&it), IntakeOutcome::NoIssue);
    assert_eq!(it.tracker.count("edit_labels"), 0);
    assert!(it.script.drafter_specs.lock().expect("specs").is_empty());
}

#[test]
fn a_dry_run_changes_no_label_and_runs_no_agent() {
    let it = setup(one_plan);
    let out = run_intake(&it.driver, &it.tracker, true).expect("dry run");
    assert!(
        matches!(out, IntakeOutcome::DryRun { issue: 7, .. }),
        "{out:?}"
    );
    assert_eq!(it.tracker.count("edit_labels"), 0);
    assert_eq!(it.tracker.count("comment"), 0);
    assert!(it.script.drafter_specs.lock().expect("specs").is_empty());
}

#[test]
fn a_tracker_failure_before_the_claim_leaves_the_labels_alone() {
    for method in ["list_labelled", "current_user"] {
        // An empty trusted list makes the pass ask for the gh user.
        let cfg = Config {
            intake: true,
            ..Config::default()
        };
        let it = setup_with(cfg, vec![issue(7)], one_plan, "x");
        it.tracker.fail_on(method);
        let err = run_intake(&it.driver, &it.tracker, false).expect_err(method);
        assert!(err.to_string().contains("intake"), "{method}: {err}");
        assert_eq!(it.tracker.count("edit_labels"), 0, "{method}");
        assert!(it.script.drafter_specs.lock().expect("specs").is_empty());
    }
}

#[test]
fn a_refused_draft_labels_the_issue_needs_human() {
    let it = setup(|draft| {
        std::fs::write(draft.join("refused.md"), "SECRET-MARKER too big").expect("refused");
    });
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-refused".to_string()
        }
    );
    assert_eq!(
        edits(&it),
        vec![
            (7, s(STATE_RUNNING), s("factory")),
            (7, s(STATE_NEEDS_HUMAN), s(STATE_RUNNING)),
        ]
    );
    let said = comments(&it);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].1.contains("draft-refused"), "{}", said[0].1);
    let draft = it.driver.layout().intake_dir().join("7").join("draft");
    assert!(
        said[0].1.contains(&draft.display().to_string()),
        "{}",
        said[0].1
    );
}

#[test]
fn a_failure_comment_never_contains_agent_text() {
    let it = setup(|draft| {
        std::fs::write(draft.join("refused.md"), "SECRET-MARKER too big").expect("refused");
    });
    pass(&it);
    for (_, body) in comments(&it) {
        assert!(!body.contains("SECRET-MARKER"), "{body}");
    }
}

#[test]
fn a_job_creation_io_error_fails_at_once_without_a_redraft() {
    let it = setup(one_plan);
    // A file where the jobs folder belongs makes add_job fail with an I/O error after validation.
    let jobs = it.driver.layout().jobs_dir.clone();
    if jobs.is_dir() {
        std::fs::remove_dir_all(&jobs).expect("remove the jobs folder");
    }
    std::fs::create_dir_all(jobs.parent().expect("jobs parent")).expect("jobs parent");
    std::fs::write(&jobs, "not a folder").expect("jobs file");
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
    assert_eq!(it.script.drafter_specs.lock().expect("specs").len(), 1);
}

#[test]
fn a_draft_with_a_placeholder_is_redrafted_once() {
    let it = setup(|draft| {
        let spec = GOOD_SPEC.replace("in one step.", "in one step. TBD");
        std::fs::write(draft.join("spec.md"), spec).expect("spec");
        std::fs::write(draft.join("plan-01-a.md"), plan(1, "none")).expect("plan");
    });
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
    let calls = it.script.drafter_specs.lock().expect("specs");
    assert_eq!(calls.len(), 2, "one redraft, no more");
    assert!(calls[1].prompt.contains("still has"), "{}", calls[1].prompt);
}

#[test]
fn an_intake_job_has_no_user_decision_warning() {
    let it = setup(one_plan);
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }), "merged");
    let board = it.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 1);
    if let Some(warning) = &board.jobs[0].spec_warning {
        assert!(!warning.contains("answered by the user"), "{warning}");
    }
}

#[test]
fn an_unsourced_draft_is_redrafted_once_then_fails_spec_review() {
    let it = setup(|draft| {
        let spec = GOOD_SPEC.replace(" Source: README.md:1.", "");
        std::fs::write(draft.join("spec.md"), spec).expect("spec");
        std::fs::write(draft.join("plan-01-a.md"), plan(1, "none")).expect("plan");
    });
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "spec-review".to_string()
        }
    );
    let calls = it.script.drafter_specs.lock().expect("specs");
    assert_eq!(calls.len(), 2, "one redraft, no more");
    assert!(
        calls[1].prompt.contains("Use the readme."),
        "{}",
        calls[1].prompt
    );
    assert!(it.driver.store().load().expect("board").jobs.is_empty());
}

#[test]
fn a_risky_draft_with_sourced_decisions_creates_the_job() {
    let config = Config {
        risk_paths: vec!["src-tauri/**".to_string()],
        ..config()
    };
    let it = setup_with(
        config,
        vec![issue(7)],
        |draft| {
            std::fs::write(draft.join("spec.md"), GOOD_SPEC).expect("spec");
            let risky = "# Plan\n\n### Task 1: t\n\n**Files:**\n- Modify: `src-tauri/src/lib.rs`\n\nNext plan: none\n";
            std::fs::write(draft.join("plan-01-a.md"), risky).expect("plan");
        },
        "fine\nVERDICT: APPROVE",
    );
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }), "merged");
    assert_eq!(it.driver.store().load().expect("board").jobs.len(), 1);
}

#[test]
fn intake_never_creates_a_job_without_plans() {
    let it = setup(|draft| {
        std::fs::write(draft.join("spec.md"), GOOD_SPEC).expect("spec");
    });
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
    assert!(it.driver.store().load().expect("board").jobs.is_empty());
    assert_eq!(it.script.drafter_specs.lock().expect("specs").len(), 1);
}

#[test]
fn intake_refuses_a_draft_folder_with_bad_characters() {
    let tmp = tempfile::tempdir().expect("tmp");
    let main = tmp.path().join("my repo");
    common::init_repo(&main);
    let runner = Arc::new(common::scripted(Vec::new()));
    let driver = common::driver_for(&main, runner.clone(), config());
    let tracker = common::FakeTracker::new("o/r")
        .with_issues(vec![issue(7)])
        .with_label_actor(7, "factory", "alice");
    let out = run_intake(&driver, &tracker, false).expect("run_intake");
    assert_eq!(
        out,
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
    assert!(runner.last_spec().is_none(), "no agent ran");
}

#[test]
fn a_merged_issue_is_commented_closed_and_labelled_done() {
    let it = setup(one_plan);
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Merged {
            issue: 7,
            jobs: vec!["001".to_string()]
        }
    );
    let board = it.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 1);
    assert_eq!(board.jobs[0].stage, Stage::Merged);
    assert_eq!(board.jobs[0].issues, vec![7]);
    let said = comments(&it);
    assert_eq!(said[0].1, "factory picked this up as job 001");
    assert_eq!(said[1].1, "merged as job 001\nno follow-ups");
    assert_eq!(it.tracker.state.lock().expect("lock").closes, vec![7]);
    assert_eq!(
        edits(&it).last(),
        Some(&(7, s(STATE_DONE), s(STATE_RUNNING)))
    );
    // The drafter saw the export and the draft folder only.
    let calls = it.script.drafter_specs.lock().expect("specs");
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].cwd.ends_with("intake/7/tree"),
        "{:?}",
        calls[0].cwd
    );
    assert!(it.main.join("README.md").exists());
}

#[test]
fn a_merged_intake_job_files_its_follow_ups_before_the_close() {
    let it = setup(one_plan_with_follow_ups);
    it.tracker.state.lock().expect("lock").labels.insert(
        7,
        vec!["factory".into(), "priority: P0".into(), "tech-debt".into()],
    );
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }));
    let calls = it.tracker.calls();
    let at = |p: &str| calls.iter().position(|c| c.starts_with(p)).expect("call");
    assert!(at("create") < at("close"), "{calls:?}");
    let s = it.tracker.state.lock().expect("lock");
    assert_eq!(s.created.len(), 1);
    assert_eq!(
        s.created[0].2,
        vec!["priority: P0".to_string(), "tech-debt".to_string()]
    );
    assert_eq!(s.closes, vec![7]);
}

#[test]
fn the_intake_comment_names_only_issue_numbers() {
    let it = setup(one_plan_with_follow_ups);
    pass(&it);
    let said = comments(&it);
    assert_eq!(said[1].1, "merged as job 001\nfollow-ups filed as #201");
    assert!(!said[1].1.contains("SECRET-TITLE"));
}

#[test]
fn a_merged_intake_job_without_follow_ups_says_so() {
    let it = setup(one_plan);
    pass(&it);
    assert_eq!(comments(&it)[1].1, "merged as job 001\nno follow-ups");
}

#[test]
fn a_failed_follow_up_still_closes_the_intake_issue_and_says_so() {
    let it = setup(one_plan_with_follow_ups);
    it.tracker.fail_on("create");
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }));
    assert_eq!(
        comments(&it)[1].1,
        "merged as job 001\nfollow-ups not filed, see the spec of job 001"
    );
    assert_eq!(it.tracker.state.lock().expect("lock").closes, vec![7]);
    assert_eq!(
        edits(&it).last(),
        Some(&(7, s(STATE_DONE), s(STATE_RUNNING)))
    );
}

#[test]
fn a_failed_follow_up_in_a_two_plan_series_names_the_last_job() {
    let it = setup(two_plans_with_follow_ups);
    it.tracker.fail_on("create");
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }));
    assert_eq!(
        comments(&it)[1].1,
        "merged as job 001, 002\nfollow-ups not filed, see the spec of job 002"
    );
    assert_eq!(it.tracker.state.lock().expect("lock").closes, vec![7]);
    assert_eq!(
        edits(&it).last(),
        Some(&(7, s(STATE_DONE), s(STATE_RUNNING)))
    );
}

#[test]
fn a_two_plan_series_files_its_follow_ups_once() {
    let it = setup(two_plans_with_follow_ups);
    pass(&it);
    assert_eq!(it.tracker.state.lock().expect("lock").created.len(), 1);
    assert_eq!(
        comments(&it)[1].1,
        "merged as job 001, 002\nfollow-ups filed as #201"
    );
}

#[test]
fn intake_marks_every_job_of_the_series() {
    let it = setup(two_plans);
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Merged {
            issue: 7,
            jobs: vec!["001".to_string(), "002".to_string()]
        }
    );
    let board = it.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert!(board.jobs.iter().all(|j| j.intake_issue == Some(7)));
}

#[test]
fn an_issue_number_in_the_title_does_not_name_another_issue() {
    let mut tricky = issue(7);
    tricky.title = "Fix #999 and #1000".to_string();
    let it = setup_with(config(), vec![tricky], one_plan, "x\nVERDICT: APPROVE");
    pass(&it);
    let board = it.driver.store().load().expect("board");
    assert_eq!(board.jobs[0].issues, vec![7]);
}

#[test]
fn a_late_created_plan_job_keeps_the_intake_marker() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "late", "two plans", Some(&a), None).expect("create");
    // Make the board look like one written before the plans got their jobs up front.
    fx.driver
        .store()
        .update(|board| {
            board.jobs.truncate(1);
            board.jobs[0].intake_issue = Some(7);
            Ok(())
        })
        .expect("rewrite board");
    run_series(&fx.driver, &job.id).expect("series");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert_eq!(board.jobs[1].intake_issue, Some(7));
}

#[test]
fn a_job_that_ends_needs_human_labels_the_issue_with_its_job_id() {
    // No fix rounds, so the first changes verdict parks the job.
    let cfg = Config {
        fix_rounds: 0,
        ..config()
    };
    let it = setup_full(
        cfg,
        vec![issue(7)],
        one_plan,
        "fine\nVERDICT: APPROVE",
        "[blocker] REVIEWER-TEXT wrong\nVERDICT: CHANGES",
        drafted,
    );
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Failed {
            issue: 7,
            reason: "build-failed".to_string()
        }
    );
    assert_eq!(
        edits(&it).last(),
        Some(&(7, s(STATE_NEEDS_HUMAN), s(STATE_RUNNING)))
    );
    let said = comments(&it);
    assert_eq!(said[0].1, "factory picked this up as job 001");
    let last = &said.last().expect("comment").1;
    assert!(last.contains("reason: build-failed"), "{last}");
    assert!(last.contains("job: 001"), "{last}");
    let draft = it.driver.layout().intake_dir().join("7").join("draft");
    assert!(last.contains(&draft.display().to_string()), "{last}");
    assert!(!last.contains("REVIEWER-TEXT"), "{last}");
    assert_eq!(it.tracker.state.lock().expect("lock").closes.len(), 0);
}

fn over_budget() -> AgentRun {
    AgentRun {
        stop_reason: StopReason::BudgetStop,
        ..AgentRun::done("SECRET-MARKER partial")
    }
}

fn stalled() -> AgentRun {
    AgentRun {
        stop_reason: StopReason::Stalled,
        ..AgentRun::done("SECRET-MARKER partial")
    }
}

fn failed() -> AgentRun {
    AgentRun {
        stop_reason: StopReason::Failed,
        ..AgentRun::done("")
    }
}

fn turn_limit() -> AgentRun {
    AgentRun {
        stop_reason: StopReason::TurnLimit,
        ..AgentRun::done("partial")
    }
}

fn secret() -> AgentRun {
    AgentRun::done("SECRET-MARKER")
}

#[test]
fn a_drafter_that_cannot_start_fails_with_the_drafter_failed_code() {
    for reply in [failed as DrafterReply, stalled] {
        let it = setup_full(
            config(),
            vec![issue(7)],
            |_| {},
            "x",
            "fine\nVERDICT: APPROVE",
            reply,
        );
        assert_eq!(
            pass(&it),
            IntakeOutcome::Failed {
                issue: 7,
                reason: "drafter-failed".to_string()
            }
        );
        assert!(it.driver.store().load().expect("board").jobs.is_empty());
        let said = comments(&it);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].1.contains("reason: drafter-failed"),
            "{}",
            said[0].1
        );
    }
}

#[test]
fn a_drafter_that_hits_the_turn_limit_is_draft_invalid() {
    let it = setup_full(
        config(),
        vec![issue(7)],
        |_| {},
        "x",
        "fine\nVERDICT: APPROVE",
        turn_limit,
    );
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
}

#[test]
fn an_empty_draft_failure_comment_never_holds_the_last_message() {
    let it = setup_full(
        config(),
        vec![issue(7)],
        |_| {},
        "x",
        "fine\nVERDICT: APPROVE",
        secret,
    );
    assert_eq!(
        pass(&it),
        IntakeOutcome::Failed {
            issue: 7,
            reason: "draft-invalid".to_string()
        }
    );
    let said = comments(&it);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(!said[0].1.contains("SECRET-MARKER"), "{}", said[0].1);
}

#[test]
fn a_drafter_that_stops_early_fails_the_issue_with_no_job() {
    let cases: [(DrafterReply, &str); 2] =
        [(over_budget, "over-budget"), (stalled, "drafter-failed")];
    for (reply, reason) in cases {
        let it = setup_full(
            config(),
            vec![issue(7)],
            one_plan,
            "x",
            "fine\nVERDICT: APPROVE",
            reply,
        );
        let out = pass(&it);
        assert_eq!(
            out,
            IntakeOutcome::Failed {
                issue: 7,
                reason: reason.to_string()
            }
        );
        assert!(it.driver.store().load().expect("board").jobs.is_empty());
        assert_eq!(
            edits(&it).last(),
            Some(&(7, s(STATE_NEEDS_HUMAN), s(STATE_RUNNING)))
        );
        let said = comments(&it);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].1.contains(&format!("reason: {reason}")),
            "{}",
            said[0].1
        );
        assert!(!said[0].1.contains("job:"), "{}", said[0].1);
        assert!(!said[0].1.contains("SECRET-MARKER"), "{}", said[0].1);
    }
}

#[test]
fn a_drafter_run_adds_a_usage_line() {
    let it = setup(one_plan);
    pass(&it);
    let log = std::fs::read_to_string(it.driver.layout().intake_usage_file()).expect("usage log");
    let line: serde_json::Value =
        serde_json::from_str(log.lines().next().expect("line")).expect("json");
    assert_eq!(line["role"], "intake-drafter");
    assert_eq!(line["job"], "intake");
}

/// Gives the driver of `it` agent files with the given names.
fn with_agent_files(it: Intake, names: &[&str]) -> Intake {
    Intake {
        driver: it.driver.with_agent_files(
            names
                .iter()
                .map(|n| AgentFile {
                    name: n.to_string(),
                    content: "x".to_string(),
                })
                .collect(),
        ),
        ..it
    }
}

#[test]
fn intake_refuses_when_the_drafter_agent_file_is_missing() {
    let it = with_agent_files(
        setup_with(config(), vec![issue(7)], one_plan, "x"),
        &["factory-builder.md"],
    );
    for dry_run in [false, true] {
        let err = run_intake(&it.driver, &it.tracker, dry_run).expect_err("missing agent file");
        let text = err.to_string();
        assert!(text.contains("factory presets refresh"), "{text}");
        assert!(text.contains("factory-intake-drafter.md"), "{text}");
        assert_eq!(it.tracker.count("edit_labels"), 0);
        assert_eq!(it.tracker.count("comment"), 0);
        assert!(it.script.drafter_specs.lock().expect("specs").is_empty());
    }
}

#[test]
fn intake_accepts_the_drafter_agent_file_with_or_without_md() {
    for name in ["factory-intake-drafter.md", "factory-intake-drafter"] {
        let it = with_agent_files(setup_with(config(), vec![issue(7)], one_plan, "x"), &[name]);
        let out = run_intake(&it.driver, &it.tracker, true).expect("dry run");
        assert!(matches!(out, IntakeOutcome::DryRun { .. }), "{name}");
    }
}

#[test]
fn intake_does_not_check_the_agent_file_when_the_driver_has_none() {
    let it = setup(one_plan);
    let out = run_intake(&it.driver, &it.tracker, true).expect("dry run");
    assert!(matches!(out, IntakeOutcome::DryRun { .. }));
}

#[test]
fn intake_refuses_to_run_under_codex() {
    let cfg = Config {
        agent_cli: Some("codex".to_string()),
        ..config()
    };
    let it = setup_with(cfg, vec![issue(7)], one_plan, "x");
    let err = run_intake(&it.driver, &it.tracker, false).expect_err("codex");
    assert!(err.to_string().contains("codex"), "{err}");
    assert_eq!(it.tracker.count("edit_labels"), 0);
    assert!(it.script.drafter_specs.lock().expect("specs").is_empty());
}

#[test]
fn an_issue_reference_in_the_title_does_not_reach_the_job_feature() {
    let mut tricky = issue(7);
    tricky.title = "Fixes #999".to_string();
    let it = setup_with(config(), vec![tricky], one_plan, "x\nVERDICT: APPROVE");
    pass(&it);
    let board = it.driver.store().load().expect("board");
    assert!(
        !board.jobs[0].feature.contains("#999"),
        "{}",
        board.jobs[0].feature
    );
}

#[test]
fn a_spec_review_refusal_redrafts_once_then_fails() {
    let it = setup_with(
        config(),
        vec![issue(7)],
        one_plan,
        "FINDING-ONE needs a fix\nVERDICT: CHANGES",
    );
    let out = pass(&it);
    assert_eq!(
        out,
        IntakeOutcome::Failed {
            issue: 7,
            reason: "spec-review".to_string()
        }
    );
    let calls = it.script.drafter_specs.lock().expect("specs");
    assert_eq!(calls.len(), 2, "one redraft, no more");
    assert!(!calls[0].prompt.contains("FINDING-ONE"));
    assert!(
        calls[1].prompt.contains("FINDING-ONE"),
        "{}",
        calls[1].prompt
    );
    assert!(it.driver.store().load().expect("board").jobs.is_empty());
    for (_, body) in comments(&it) {
        assert!(!body.contains("FINDING-ONE"), "{body}");
    }
}
