mod common;

use common::Fixture;
use factory_cli::CliError;
use factory_engine::{acquire_project_lease, now_secs, EngineError};
use std::time::Duration;

/// A risky job parked after its build, so it has no resume value.
fn risky_parked() -> Fixture {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx
}

/// A job approved while the main checkout is dirty, so it parks with a merge resume value.
fn main_busy_parked() -> Fixture {
    let fx = risky_parked();
    std::fs::write(fx.main_file("README.md"), "dirty").expect("dirty");
    let end = fx.run(&["approve", "1"]).expect("approve");
    assert!(end.starts_with("parked: main busy"), "{end}");
    assert_eq!(job(&fx).park_resume, Some(factory_core::ResumeAt::Merge));
    fx
}

fn store(fx: &Fixture) -> factory_engine::BoardStore {
    factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        Duration::from_secs(15),
    )
}

fn job(fx: &Fixture) -> factory_core::Job {
    store(fx)
        .load()
        .expect("load")
        .jobs
        .into_iter()
        .find(|j| j.id == "001")
        .expect("job 001")
}

fn refusal(result: Result<String, CliError>) -> String {
    match result.expect_err("refused") {
        CliError::Refused(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn retry_merges_a_main_busy_job_once_main_is_clean() {
    let fx = main_busy_parked();
    std::fs::write(fx.main_file("README.md"), "hello\n").expect("clean");
    let out = fx.run(&["retry", "1"]).expect("retry");
    assert!(out.starts_with("merged"), "{out}");
    assert_eq!(job(&fx).stage, factory_core::Stage::Merged);
}

#[test]
fn retry_refuses_a_builder_park_with_its_next_commands() {
    let fx = risky_parked();
    let text = refusal(fx.run(&["retry", "1"]));
    assert!(text.contains("no stage to retry at"), "{text}");
    assert!(text.contains("factory rework 001 <note>"), "{text}");
}

#[test]
fn retry_refuses_a_park_without_park_json_and_names_its_next_commands() {
    let fx = risky_parked();
    let dir = factory_core::Layout::in_repo(&fx.root).job_dir(&job(&fx).dir);
    std::fs::remove_file(dir.join(factory_core::PARK_FILE)).expect("remove park.json");
    let text = refusal(fx.run(&["retry", "1"]));
    assert!(text.contains("no stage to retry at"), "{text}");
    assert!(text.contains("factory rework 001 <note>"), "{text}");
    assert_eq!(job(&fx).stage, factory_core::Stage::NeedsHuman);
}

#[test]
fn rework_refuses_a_park_with_resume_and_names_retry() {
    let fx = main_busy_parked();
    let reason = job(&fx).reason.expect("reason");
    let text = refusal(fx.run(&["rework", "1", "a note"]));
    assert_eq!(
        text,
        format!(
            "job 001 parked outside the builder ({reason}); run factory retry 001, or factory rework 001 --force <note> to send it to the builder anyway"
        )
    );
    assert_eq!(job(&fx).stage, factory_core::Stage::NeedsHuman);
}

/// A main-busy park with its kind and resume point changed.
fn parked_as(kind: &str, resume: factory_core::ResumeAt) -> Fixture {
    let fx = main_busy_parked();
    store(&fx)
        .update_job("001", |j| {
            j.park_kind = Some(kind.into());
            j.park_resume = Some(resume);
            Ok(())
        })
        .expect("kind");
    fx
}

#[test]
fn plain_rework_is_allowed_on_machine_and_stopped_parks() {
    for kind in ["machine-waiting", "builder-stopped"] {
        let fx = parked_as(kind, factory_core::ResumeAt::Build);
        fx.run(&["rework", "1", "fix x"])
            .unwrap_or_else(|e| panic!("{kind}: {e:?}"));
        assert_eq!(job(&fx).stage, factory_core::Stage::Build, "{kind}");
    }
    let fx = parked_as("review-failed", factory_core::ResumeAt::Merge);
    let text = refusal(fx.run(&["rework", "1", "fix x"]));
    assert!(text.contains("parked outside the builder"), "{text}");
}

#[test]
fn rework_force_sends_it_to_the_builder() {
    let fx = main_busy_parked();
    let out = fx
        .run(&["rework", "--force", "--", "1", "use a shorter name"])
        .expect("rework");
    assert_eq!(out, "job 001 back to build, round 2");
    assert_eq!(job(&fx).stage, factory_core::Stage::Build);
}

#[test]
fn rework_still_works_on_a_no_tests_park() {
    let fx = risky_parked();
    store(&fx)
        .update_job("001", |j| {
            j.park_kind = Some("no-tests".into());
            Ok(())
        })
        .expect("kind");
    assert!(job(&fx).park_resume.is_none());
    fx.run(&["rework", "1", "add a test"]).expect("rework");
    assert_eq!(job(&fx).stage, factory_core::Stage::Build);
}

#[test]
fn rework_revives_a_cancelled_job_with_resume() {
    let fx = main_busy_parked();
    fx.run(&["cancel", "1"]).expect("cancel");
    assert!(job(&fx).park_resume.is_some());
    fx.run(&["rework", "1", "revive it"]).expect("rework");
    let j = job(&fx);
    assert_eq!(j.stage, factory_core::Stage::Build);
    assert!(!j.cancelled);
}

#[test]
fn retry_is_busy_when_the_project_is_held() {
    let fx = main_busy_parked();
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    acquire_project_lease(
        &state,
        "003",
        "someone-else",
        600.0,
        now_secs(),
        Duration::from_secs(5),
    )
    .expect("project lease");
    let e = fx.run(&["retry", "1"]).expect_err("busy");
    assert!(
        matches!(e, CliError::Engine(EngineError::ProjectBusy(_))),
        "{e:?}"
    );
}
