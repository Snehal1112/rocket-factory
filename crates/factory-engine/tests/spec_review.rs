mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{driver_for, init_repo, scripted, write_commit_context, Effect, ScriptedRunner};
use factory_agent::AgentSpec;
use factory_core::Config;
use factory_engine::{create_job_with_spec_accepting, Driver, EngineError};

const SPEC: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
                    ## Acceptance criteria\n\n- It is done.\n\n## Decisions\n\n- Use a queue. Decided by: user\n";

const PLAN: &str = "# Plan\n\n> Executor: `superpowers:subagent-driven-development`.\n\n**Goal:** g\n\n**Architecture:** a\n\n\
                    **Tech Stack:** Rust\n\n**Spec:** s\n\n## Global Constraints\n\n- c\n\n## Review Focus\n\n- r\n\n\
                    ### Task 1: t\n\n**Files:**\n- Modify: `a.rs`\n\n**Interfaces:**\n- Produces: x\n\nNext plan: none\n";

struct Fx {
    _tmp: tempfile::TempDir,
    driver: Driver,
    runner: Arc<ScriptedRunner>,
    plan: PathBuf,
    spec: PathBuf,
}

fn reviewer(text: &'static str) -> Effect {
    Box::new(move |_: &AgentSpec| text.to_string())
}

fn fixture(effects: Vec<Effect>, config: Config) -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    init_repo(&main);
    write_commit_context(&main);
    let runner = Arc::new(scripted(effects));
    let driver = driver_for(&main, runner.clone(), config);
    let plans = tmp.path().join("plans");
    std::fs::create_dir_all(&plans).expect("plans");
    let plan = plans.join("plan-01-a.md");
    std::fs::write(&plan, PLAN).expect("plan");
    let spec = plans.join("design.md");
    std::fs::write(&spec, SPEC).expect("spec");
    Fx {
        _tmp: tmp,
        driver,
        runner,
        plan,
        spec,
    }
}

fn new(fx: &Fx, accept: Option<&str>) -> Result<factory_core::Job, EngineError> {
    create_job_with_spec_accepting(
        &fx.driver,
        "demo",
        "f",
        Some(&fx.plan),
        Some(&fx.spec),
        None,
        accept,
    )
}

fn jobs(fx: &Fx) -> usize {
    fx.driver.store().load().expect("board").jobs.len()
}

const APPROVE: &str = "No findings.\nVERDICT: APPROVE";
const CHANGES: &str =
    "1. [blocker] Decisions - contradicts the code - src/lib.rs:10 - change it\nVERDICT: CHANGES";

#[test]
fn an_approved_spec_creates_the_jobs() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("created");
    assert_eq!(jobs(&fx), 1);
    let call = fx.runner.last_spec_for("spec-reviewer").expect("called");
    assert_eq!(call.tools.as_deref(), Some("Read,Grep,Glob"));
    assert_eq!(call.agent.as_deref(), Some("factory-spec-reviewer"));
    assert!(call.prompt.contains("design.md") && call.prompt.contains("plan-01-a.md"));
}

#[test]
fn a_changes_verdict_refuses_and_names_the_file() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    let err = new(&fx, None).expect_err("refused");
    let text = err.to_string();
    assert!(text.contains("contradicts the code"), "{text}");
    assert!(
        text.contains("spec-reviews/") && text.contains(".md"),
        "{text}"
    );
    assert!(text.contains("--accept-spec"), "{text}");
    assert_eq!(jobs(&fx), 0);
    let dir = fx.driver.layout().spec_reviews_dir();
    assert_eq!(std::fs::read_dir(dir).expect("dir").count(), 1);
}

#[test]
fn a_missing_verdict_refuses() {
    let fx = fixture(vec![reviewer("looks fine to me")], Config::default());
    let err = new(&fx, None).expect_err("refused");
    assert!(err.to_string().contains("no verdict"), "{err}");
    assert_eq!(jobs(&fx), 0);
}

#[test]
fn an_approved_spec_is_not_reviewed_twice() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("first");
    new(&fx, None).expect("second");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
}

#[test]
fn a_changed_plan_is_reviewed_again() {
    let fx = fixture(
        vec![reviewer(APPROVE), reviewer(APPROVE)],
        Config::default(),
    );
    new(&fx, None).expect("first");
    std::fs::write(&fx.plan, PLAN.replace("- r\n", "- r changed\n")).expect("edit");
    new(&fx, None).expect("second");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 2);
}

#[test]
fn spec_review_off_skips_the_agent() {
    let config = Config {
        spec_review: false,
        ..Config::default()
    };
    let fx = fixture(Vec::new(), config);
    new(&fx, None).expect("created");
    assert!(fx.runner.specs_for("spec-reviewer").is_empty());
}

#[test]
fn accept_spec_overrides_and_records_the_reason() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    let job = new(&fx, Some("the user checked it")).expect("created");
    let note = "spec accepted by the user: the user checked it";
    assert_eq!(job.spec_warning.as_deref(), Some(note));
    let stored = fx.driver.job(&job.id).expect("job");
    assert_eq!(stored.spec_warning.as_deref(), Some(note));
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains(note), "{ledger}");
}

#[test]
fn accept_spec_on_a_clean_spec_records_nothing() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    let job = new(&fx, Some("not needed")).expect("created");
    assert_eq!(job.spec_warning, None);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).unwrap_or_default();
    assert!(!ledger.contains("spec accepted"), "{ledger}");
}
