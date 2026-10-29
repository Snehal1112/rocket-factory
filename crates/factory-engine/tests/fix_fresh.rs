mod common;

use common::{committing_fixture, scripted_auto, Effect, Fixture, Tagged};
use factory_core::{Board, CheckRule};

fn append_effect() -> Effect {
    Box::new(|spec| {
        let path = spec.cwd.join("a.txt");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str("x\n");
        std::fs::write(path, text).expect("write");
        "done".to_string()
    })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

/// Builds with a runner whose builder ends at `end_ctx`, then runs the checks.
fn build_round(fx: &Fixture, end_ctx: u64) -> Tagged {
    let runner = Tagged::new(scripted_auto(vec![append_effect()]), end_ctx);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    runner
}

/// Builds round 1 with failing checks, so fix round 2 is next, and sets the last builder's end time.
fn reach_fix_round(end_ctx: u64, idle_secs: u64) -> Fixture {
    let mut fx = committing_fixture();
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
    build_round(&fx, end_ctx);
    assert_eq!(fx.job().round, 2);
    let mut board = Board::load(&fx.board_path).expect("load");
    let last = board
        .find_mut("001")
        .expect("job")
        .builder_last
        .as_mut()
        .expect("the first builder is recorded");
    assert_eq!(last.end_ctx, end_ctx);
    last.ended_at = now() - idle_secs;
    board.save(&fx.board_path).expect("save");
    fx
}

#[test]
fn an_idle_large_session_gets_a_fresh_fix_builder() {
    let fx = reach_fix_round(150_000, 1_000);
    let runner = build_round(&fx, 20_000);
    let spec = runner.inner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, None);
    assert!(
        spec.prompt
            .contains("picking up a fix round in a fresh session"),
        "{}",
        spec.prompt
    );
    // The idle time may tick past 1000s while the round runs, so it is matched loosely.
    let ledger = fx.ledger();
    let ruling = ledger
        .lines()
        .find(|l| l.contains("fresh fix builder (idle 10"))
        .unwrap_or_else(|| panic!("{ledger}"));
    assert!(
        ruling.ends_with("s over 240s, context 150000 over 60000)"),
        "{ruling}"
    );
    // The fix builder is recorded too, so the next round sees it.
    assert_eq!(fx.job().builder_last.expect("recorded").end_ctx, 20_000);
}

#[test]
fn a_warm_session_still_resumes() {
    let fx = reach_fix_round(150_000, 0);
    let session = fx.job().builder_id.expect("session");
    let runner = build_round(&fx, 20_000);
    let spec = runner.inner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, Some(session));
    assert!(
        !fx.ledger().contains("fresh fix builder"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_small_idle_session_still_resumes() {
    let fx = reach_fix_round(30_000, 1_000);
    let session = fx.job().builder_id.expect("session");
    let runner = build_round(&fx, 20_000);
    let spec = runner.inner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, Some(session));
    assert!(
        !fx.ledger().contains("fresh fix builder"),
        "{}",
        fx.ledger()
    );
}

/// Sets the round and makes the last builder idle for `idle_secs`.
fn set_round_and_idle(fx: &Fixture, round: u32, idle_secs: u64) {
    let mut board = Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = round;
    job.builder_last.as_mut().expect("recorded").ended_at = now() - idle_secs;
    board.save(&fx.board_path).expect("save");
}

#[test]
fn a_cold_round_one_resume_without_findings_still_resumes() {
    let fx = committing_fixture();
    let first = Tagged::new(scripted_auto(vec![append_effect()]), 150_000);
    fx.engine(&first).build_stage().expect("build");
    assert!(fx.job().round <= 1);
    set_round_and_idle(&fx, 1, 1_000);
    let session = fx.job().builder_id.expect("session");
    // The resume prompt needs a findings file, so the round-1 resume gets one.
    std::fs::write(fx.job_dir.join("changes-r1.md"), "carry on").expect("findings");
    let runner = Tagged::new(scripted_auto(vec![append_effect()]), 20_000);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.inner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, Some(session));
    assert!(
        !fx.ledger().contains("fresh fix builder"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_cold_rework_gets_a_fresh_fix_builder() {
    let fx = reach_fix_round(150_000, 1_000);
    // `factory rework` resets the round to 0 and writes the note.
    set_round_and_idle(&fx, 0, 1_000);
    std::fs::write(fx.job_dir.join("rework-note.md"), "do it differently").expect("note");
    let runner = build_round(&fx, 20_000);
    let spec = runner.inner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, None);
    assert!(spec.prompt.contains("rework-note.md"), "{}", spec.prompt);
    assert!(
        fx.ledger().contains("fresh fix builder (idle "),
        "{}",
        fx.ledger()
    );
}
