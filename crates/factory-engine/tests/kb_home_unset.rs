mod common;
use common::{scripted, Fixture};
use factory_engine::KbOutcome;

// This file holds one test, so no other test reads HOME while it is cleared.
#[test]
fn an_unset_home_with_tilde_kb_path_does_not_say_not_set() {
    std::env::remove_var("HOME");
    let mut fx = Fixture::new();
    fx.config.kb_path = Some("~/kb/rocketapi".into());
    let runner = scripted(vec![]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    match out {
        KbOutcome::Failed(text) => {
            assert!(text.contains("HOME"), "{text}");
            assert!(!text.contains("not set"), "{text}");
        }
        other => panic!("unexpected outcome {other:?}"),
    }
    assert!(runner.specs_for("kb").is_empty());
}
