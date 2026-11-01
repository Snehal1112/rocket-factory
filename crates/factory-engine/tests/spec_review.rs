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
fn the_reviewer_gets_its_own_budget_cap() {
    let cfg = Config {
        spec_review_budget_weighted: 12_345,
        ..Config::default()
    };
    let fx = fixture(vec![reviewer(APPROVE)], cfg);
    new(&fx, None).expect("created");
    assert_eq!(
        fx.runner.specs_for("spec-reviewer")[0].budget_weighted,
        Some(12_345)
    );
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("created");
    assert_eq!(
        fx.runner.specs_for("spec-reviewer")[0].budget_weighted,
        Some(100_000)
    );
}

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
    let reviews = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .count();
    assert_eq!(reviews, 1);
}

#[test]
fn a_missing_verdict_refuses() {
    let fx = fixture(vec![reviewer("looks fine to me")], Config::default());
    let err = new(&fx, None).expect_err("refused");
    assert!(err.to_string().contains("no verdict"), "{err}");
    assert_eq!(jobs(&fx), 0);
}

#[test]
fn a_preset_without_the_spec_reviewer_refuses_without_a_run() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    // A preset from before this release resolves agent files, but none for the spec reviewer.
    let builder = factory_core::AgentFile {
        name: "factory-builder.md".to_string(),
        content: "---\nname: factory-builder\n---\nbody\n".to_string(),
    };
    let fx = Fx {
        driver: fx.driver.with_agent_files(vec![builder]),
        ..fx
    };
    let err = new(&fx, None).expect_err("refused");
    assert_eq!(
        err.to_string(),
        "the spec reviewer agent is missing; run factory upgrade or factory presets refresh, or set spec_review = false"
    );
    assert!(fx.runner.last_spec_for("spec-reviewer").is_none());
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

#[test]
fn accept_spec_after_a_changes_review_makes_no_new_call() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    new(&fx, None).expect_err("refused");
    let dir = fx.driver.layout().spec_reviews_dir();
    let saved = || {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .collect();
        files.sort_by_key(|e| e.file_name());
        std::fs::read_to_string(files[0].path()).expect("review")
    };
    let before = saved();
    let job = new(&fx, Some("read the findings")).expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
    assert_eq!(saved(), before, "the findings are kept");
    assert!(job
        .spec_warning
        .expect("warning")
        .contains("read the findings"));
}

#[test]
fn a_single_plan_one_job_with_accept_spec_reviews_once() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    let made = factory_engine::create_one_job_accepting(
        &fx.driver,
        "demo",
        "f",
        &fx.plan,
        Some(&fx.spec),
        None,
        false,
        Some("checked"),
    )
    .expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
    let warning = made.job.spec_warning.expect("warning");
    assert_eq!(warning.matches("spec accepted by the user").count(), 1);
}

#[test]
fn the_review_run_is_logged_for_the_token_reports() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("created");
    let log = std::fs::read_to_string(fx.driver.layout().spec_review_usage_file()).expect("log");
    let line: serde_json::Value =
        serde_json::from_str(log.lines().next().expect("line")).expect("json");
    assert_eq!(line["role"], "spec-reviewer");
    assert_eq!(line["job"], "spec-review");
    let costs = factory_engine::gather_costs(&fx.driver).expect("costs");
    let rec = costs
        .iter()
        .find(|c| c.id == "spec-review")
        .expect("record");
    assert_eq!(rec.runs, 1);
}

fn saved_reviews(fx: &Fx) -> usize {
    std::fs::read_dir(fx.driver.layout().spec_reviews_dir())
        .map(|dir| {
            dir.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
                .count()
        })
        .unwrap_or(0)
}

fn one_job(fx: &Fx, accept: Option<&str>) -> Result<factory_engine::OneJob, EngineError> {
    factory_engine::create_one_job_accepting(
        &fx.driver,
        "demo",
        "f",
        &fx.plan,
        Some(&fx.spec),
        None,
        false,
        accept,
    )
}

#[test]
fn a_failed_review_run_refuses_and_saves_nothing() {
    let fx = fixture(vec![], Config::default());
    let err = new(&fx, None).expect_err("refused");
    let text = err.to_string();
    assert!(
        text.contains("did not finish (failed: script exhausted)"),
        "{text}"
    );
    assert!(text.contains("--accept-spec"), "{text}");
    assert_eq!(saved_reviews(&fx), 0);
    assert_eq!(jobs(&fx), 0);
}

#[test]
fn a_failed_review_is_run_again_by_a_new_process() {
    let fx = fixture(vec![], Config::default());
    new(&fx, None).expect_err("first refused");
    let second = driver_for(fx.driver.root(), fx.runner.clone(), Config::default());
    create_job_with_spec_accepting(
        &second,
        "demo",
        "f",
        Some(&fx.plan),
        Some(&fx.spec),
        None,
        None,
    )
    .expect_err("second refused");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 2);
}

#[test]
fn a_failed_review_with_accept_spec_in_one_job_runs_once() {
    let fx = fixture(vec![], Config::default());
    one_job(&fx, Some("checked")).expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
}

#[test]
fn accept_spec_passes_a_failed_review_and_records_the_reason() {
    let fx = fixture(vec![], Config::default());
    let job = new(&fx, Some("checked by hand")).expect("created");
    let warning = job.spec_warning.expect("warning");
    assert!(
        warning.contains("spec accepted by the user: checked by hand"),
        "{warning}"
    );
    assert!(!warning.contains("no agent review ran"), "{warning}");
}

#[test]
fn an_unwritable_review_folder_still_reviews_once() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    let dir = fx.driver.layout().spec_reviews_dir();
    std::fs::create_dir_all(dir.parent().expect("state dir")).expect("state dir");
    std::fs::write(&dir, "not a folder").expect("block the folder");
    let made = one_job(&fx, Some("checked")).expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
    assert!(made
        .job
        .spec_warning
        .expect("warning")
        .contains("spec accepted by the user"));
    let info = fx.driver.last_spec_review().expect("info");
    assert!(!info.saved);
}

#[test]
fn last_spec_review_reports_the_file_and_verdict() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("created");
    let info = fx.driver.last_spec_review().expect("info");
    assert_eq!(info.verdict, "approved");
    assert!(info.saved && info.file.exists());
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    new(&fx, Some("checked")).expect("created");
    let info = fx.driver.last_spec_review().expect("info");
    assert_eq!(info.verdict, "asked for changes");
    assert!(info.saved && info.file.exists());
}

#[test]
fn a_reused_review_is_still_reported() {
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("first");
    new(&fx, None).expect("second");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
    assert!(fx.driver.last_spec_review().is_some());
}

#[test]
fn no_review_means_no_note() {
    let config = Config {
        spec_review: false,
        ..Config::default()
    };
    let fx = fixture(vec![], config);
    new(&fx, None).expect("created");
    assert!(fx.driver.last_spec_review().is_none());
}

#[test]
fn accept_spec_on_a_risky_spec_says_no_agent_review_ran() {
    let config = Config {
        risk_paths: vec!["a.rs".into()],
        ..Config::default()
    };
    let fx = fixture(vec![reviewer(APPROVE)], config);
    std::fs::write(
        &fx.spec,
        SPEC.replace("Decided by: user", "Decided by: call"),
    )
    .expect("spec");
    let job = new(&fx, Some("checked")).expect("created");
    let warning = job.spec_warning.expect("warning");
    assert!(
        warning.contains("spec accepted by the user: checked (no agent review ran)"),
        "{warning}"
    );
    assert!(fx.runner.specs_for("spec-reviewer").is_empty());
    assert!(fx.driver.last_spec_review().is_none());
}

struct StopRunner;

impl factory_agent::AgentRunner for StopRunner {
    fn run(&self, _spec: &AgentSpec) -> factory_agent::AgentRun {
        factory_agent::AgentRun {
            stop_reason: factory_agent::StopReason::BudgetStop,
            ..factory_agent::AgentRun::done("looks fine\nVERDICT: APPROVE")
        }
    }
}

#[test]
fn a_budget_stop_with_an_approve_line_still_refuses() {
    let fx = fixture(vec![], Config::default());
    let factory: factory_engine::RunnerFactory = Box::new(|_| Arc::new(StopRunner));
    let driver = Driver::new(
        factory_core::Layout::in_repo(fx.driver.root()),
        Config::default(),
        factory,
    )
    .expect("driver");
    let err = create_job_with_spec_accepting(
        &driver,
        "demo",
        "f",
        Some(&fx.plan),
        Some(&fx.spec),
        None,
        None,
    )
    .expect_err("refused");
    assert!(
        err.to_string().contains(
            "did not finish (stopped at the cap of 100000 weighted tokens; raise spec_review_budget_weighted)"
        ),
        "{err}"
    );
}

fn with_agent_file(fx: Fx) -> Fx {
    let file = factory_core::AgentFile {
        name: "factory-spec-reviewer.md".to_string(),
        content: "---\nname: factory-spec-reviewer\n---\n<!-- factory-rules: 1 -->\nbody\n"
            .to_string(),
    };
    Fx {
        driver: fx.driver.with_agent_files(vec![file]),
        ..fx
    }
}

const REVIEWER_REL: &str = ".claude/agents/factory-spec-reviewer.md";

/// The prompt of `factory-spec-reviewer` in the agents file of the last reviewer run.
fn reviewer_prompt(fx: &Fx) -> String {
    let spec = fx
        .runner
        .last_spec_for("spec-reviewer")
        .expect("reviewer ran");
    let path = spec.agents_file.expect("agents file");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json");
    json["factory-spec-reviewer"]["prompt"]
        .as_str()
        .expect("prompt")
        .to_string()
}

#[test]
fn the_spec_reviewer_runs_through_the_agents_file() {
    let fx = with_agent_file(fixture(vec![reviewer(APPROVE)], Config::default()));
    new(&fx, None).expect("created");
    assert!(reviewer_prompt(&fx).contains("body"));
    assert!(!fx.driver.root().join(".claude/agents").exists());
}

#[test]
fn an_untracked_marker_less_reviewer_file_is_the_users_own() {
    let fx = with_agent_file(fixture(vec![reviewer(APPROVE)], Config::default()));
    let path = fx.driver.root().join(REVIEWER_REL);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(&path, "USERS-OWN\n").expect("mine");
    new(&fx, None).expect("created");
    assert!(reviewer_prompt(&fx).contains("USERS-OWN"));

    let fx = with_agent_file(fixture(vec![reviewer(APPROVE)], Config::default()));
    let path = fx.driver.root().join(REVIEWER_REL);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(&path, "<!-- factory-rules: 1 -->\nUSERS-OWN\n").expect("old copy");
    new(&fx, None).expect("created");
    let prompt = reviewer_prompt(&fx);
    assert!(!prompt.contains("USERS-OWN"), "{prompt}");
    assert!(prompt.contains("body"));
}

#[test]
fn an_unreadable_tracked_reviewer_file_refuses_the_review() {
    let fx = with_agent_file(fixture(vec![reviewer(APPROVE)], Config::default()));
    let root = fx.driver.root();
    std::fs::create_dir_all(root.join(".claude/agents")).expect("dir");
    std::os::unix::fs::symlink("/etc/hostname", root.join(REVIEWER_REL)).expect("link");
    let repo = git2::Repository::open(root).expect("open");
    let mut index = repo.index().expect("index");
    index
        .add_path(std::path::Path::new(REVIEWER_REL))
        .expect("add");
    index.write().expect("write index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = git2::Signature::now("t", "t@example.com").expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, "link", &tree, &[&parent])
        .expect("commit");
    let err = new(&fx, None).expect_err("refused");
    assert!(
        err.to_string().contains("factory-spec-reviewer.md"),
        "{err}"
    );
    assert!(fx.runner.specs_for("spec-reviewer").is_empty());
}

#[test]
fn a_tracked_spec_reviewer_text_wins() {
    let fx = with_agent_file(fixture(vec![reviewer(APPROVE)], Config::default()));
    common::commit_file(fx.driver.root(), REVIEWER_REL, "TRACKED-REVIEWER\n");
    new(&fx, None).expect("created");
    assert!(reviewer_prompt(&fx).contains("TRACKED-REVIEWER"));
}
