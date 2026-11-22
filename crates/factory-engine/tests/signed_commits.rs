mod common;

use common::{
    approving_runner, failing_signer, scripted, working_signer, Effect, Fixture, MergeFixture,
};
use factory_engine::sync_stage;

fn write_effect() -> Effect {
    Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write file");
        "done".to_string()
    })
}

fn ledger_of(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger")
}

fn sync_fixture() -> MergeFixture {
    let config = factory_core::Config {
        checks: vec![factory_core::CheckRule {
            paths: vec!["**".into()],
            commands: vec!["true".into()],
            ..factory_core::CheckRule::default()
        }],
        ..factory_core::Config::default()
    };
    common::approved_job_config(approving_runner(3), config)
}

#[test]
fn an_unsigned_snapshot_writes_a_ledger_line() {
    let fx = Fixture::new();
    let runner = scripted(vec![write_effect()]);
    failing_signer(&fx.main, fx.tmp.path());
    fx.engine(&runner).build_stage().expect("build");
    let ledger = ledger_of(&fx);
    let line = ledger
        .lines()
        .find(|l| l.contains("is unsigned:"))
        .expect("unsigned line");
    assert!(line.contains("gpg: signing failed: No pinentry"), "{line}");
}

#[test]
fn a_signed_snapshot_writes_no_unsigned_line() {
    let fx = Fixture::new();
    let runner = scripted(vec![write_effect()]);
    working_signer(&fx.main, fx.tmp.path());
    fx.engine(&runner).build_stage().expect("build");
    assert!(!ledger_of(&fx).contains("is unsigned:"));
}

#[test]
fn an_unsigned_sync_merge_writes_a_ledger_line() {
    let fx = sync_fixture();
    failing_signer(&fx.main, fx.tmp.path());
    common::commit_file(&fx.main, "other.md", "o");
    sync_stage(&fx.driver, &fx.id).expect("sync");
    let job = fx.driver.job(&fx.id).expect("job");
    let head = job.head.clone().expect("head");
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    let lines: Vec<&str> = ledger
        .lines()
        .filter(|l| l.contains("is unsigned:"))
        .collect();
    assert_eq!(lines.len(), 1, "{ledger}");
    assert!(
        lines[0].contains(&format!("commit {} is unsigned:", &head[..7])),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("exited with 2"), "{}", lines[0]);
}
