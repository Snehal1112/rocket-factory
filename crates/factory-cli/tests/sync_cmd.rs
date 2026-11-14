mod common;

use common::Fixture;
use factory_cli::CliError;
use factory_core::Stage;

fn new_job(fx: &mut Fixture) {
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
}

#[test]
fn sync_refusals_are_cli_refused() {
    // A risky path parks the job after its build, so it has a worktree and a head.
    let mut fx = Fixture::with_risk_path("docs/**");
    new_job(&mut fx);
    fx.run(&["run", "1"]).expect("run");
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    );
    let refused = |fx: &Fixture| match fx.run(&["sync", "1"]).expect_err("refused") {
        CliError::Refused(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    };
    // A dirty worktree.
    std::fs::write(fx.worktree_path(1).join("dirty.txt"), "x").expect("write");
    assert_eq!(
        refused(&fx),
        "the worktree of job 001 has uncommitted changes"
    );
    // A cancelled job.
    store
        .update_job("001", |j| {
            j.cancelled = true;
            Ok(())
        })
        .expect("cancel");
    assert_eq!(refused(&fx), "job 001 is cancelled");
    // A merged job.
    store
        .update_job("001", |j| {
            j.cancelled = false;
            j.set_stage(Stage::Merged, None, true, 2.0)
        })
        .expect("merged");
    assert_eq!(refused(&fx), "job 001 is merged");
}

#[test]
fn sync_refuses_a_job_with_a_live_lease() {
    let mut fx = Fixture::new();
    new_job(&mut fx);
    fx.hold_lease("001", "someone-else");
    let e = fx.run(&["sync", "1"]).expect_err("leased");
    assert!(e.to_string().contains("leased by someone-else"), "{e}");
}
