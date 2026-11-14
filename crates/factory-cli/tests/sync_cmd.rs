mod common;

use common::Fixture;

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
fn sync_refuses_a_job_with_a_live_lease() {
    let mut fx = Fixture::new();
    new_job(&mut fx);
    fx.hold_lease("001", "someone-else");
    let e = fx.run(&["sync", "1"]).expect_err("leased");
    assert!(e.to_string().contains("leased by someone-else"), "{e}");
}
