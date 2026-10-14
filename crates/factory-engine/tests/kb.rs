mod common;
use common::{scripted, Effect, Fixture};
use factory_agent::AgentSpec;
use factory_engine::KbOutcome;

fn reply(text: &'static str) -> Effect {
    Box::new(move |_: &AgentSpec| text.to_string())
}

/// A fixture whose config points at an existing KB folder.
fn with_kb() -> Fixture {
    let mut fx = Fixture::new();
    let kb = fx.tmp.path().join("kb").join("rocketapi");
    std::fs::create_dir_all(&kb).expect("mkdir kb");
    fx.config.kb_path = Some(kb.to_string_lossy().into_owned());
    fx
}

#[test]
fn a_reply_with_pages_is_refreshed() {
    let fx = with_kb();
    let runner = scripted(vec![reply("auth.md\nhttp.md\n")]);
    let out = fx.engine(&runner).kb_refresh_step(Some(("001", "demo"))).expect("step");
    assert_eq!(out, KbOutcome::Refreshed { pages: vec!["auth.md".to_string(), "http.md".to_string()] });
    let specs = runner.specs_for("kb");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].add_dirs, vec![fx.config.kb_dir().expect("kb dir")]);
    assert_eq!(specs[0].cwd, fx.repo.root());
    assert!(specs[0].prompt.contains("job 001 (demo)"));
}

#[test]
fn no_relevant_changes_is_up_to_date() {
    let fx = with_kb();
    let runner = scripted(vec![reply("No relevant changes")]);
    assert_eq!(fx.engine(&runner).kb_refresh_step(None).expect("step"), KbOutcome::UpToDate);
}

#[test]
fn writing_into_the_checkout_is_tampering() {
    let fx = with_kb();
    let effect: Effect = Box::new(|spec: &AgentSpec| {
        std::fs::write(spec.cwd.join("stray.txt"), "x").expect("write");
        "auth.md".to_string()
    });
    let runner = scripted(vec![effect]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    assert!(matches!(out, KbOutcome::Tampered(_)), "{out:?}");
}

#[test]
fn editing_an_already_dirty_file_is_tampering() {
    let fx = with_kb();
    std::fs::write(fx.main.join("README.md"), "dirty once\n").expect("write");
    let effect: Effect = Box::new(|spec: &AgentSpec| {
        std::fs::write(spec.cwd.join("README.md"), "dirty twice\n").expect("write");
        "auth.md".to_string()
    });
    let runner = scripted(vec![effect]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    assert!(matches!(out, KbOutcome::Tampered(_)), "{out:?}");
}

#[test]
fn a_blocked_reply_fails() {
    let fx = with_kb();
    let runner = scripted(vec![reply("BLOCKED: skill missing")]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    assert!(matches!(&out, KbOutcome::Failed(r) if r.contains("skill missing")), "{out:?}");
}

#[test]
fn an_exhausted_runner_fails() {
    let fx = with_kb();
    let runner = scripted(vec![]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    assert!(matches!(out, KbOutcome::Failed(_)), "{out:?}");
}

#[test]
fn a_missing_kb_folder_fails_without_a_run() {
    let mut fx = Fixture::new();
    fx.config.kb_path = Some(fx.tmp.path().join("nope").to_string_lossy().into_owned());
    let runner = scripted(vec![reply("auth.md")]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    assert!(matches!(out, KbOutcome::Failed(_)), "{out:?}");
    assert!(runner.specs_for("kb").is_empty());
}
