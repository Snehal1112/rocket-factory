mod common;
use common::{reply, scripted, Effect, Fixture};
use factory_agent::AgentSpec;
use factory_engine::KbOutcome;

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
    let out = fx
        .engine(&runner)
        .kb_refresh_step(Some(("001", "demo")))
        .expect("step");
    assert_eq!(
        out,
        KbOutcome::Refreshed {
            pages: vec!["auth.md".to_string(), "http.md".to_string()]
        }
    );
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
    assert_eq!(
        fx.engine(&runner).kb_refresh_step(None).expect("step"),
        KbOutcome::UpToDate
    );
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
    assert!(
        matches!(&out, KbOutcome::Failed(r) if r.contains("skill missing")),
        "{out:?}"
    );
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

mod merge_hook {
    use super::reply;
    use crate::common::{approved_job_config, scripted, Effect, MergeFixture};
    use factory_agent::AgentSpec;
    use factory_core::{Config, Stage};
    use factory_engine::{merge_stage, MergeResult};
    use std::sync::Arc;

    fn fixture(effects: Vec<Effect>, kb: bool) -> MergeFixture {
        let config = Config {
            kb_path: kb.then(|| "kb/rocketapi".to_string()),
            ..Config::default()
        };
        approved_job_config(Arc::new(scripted(effects)), false, config)
    }

    fn merge(fx: &MergeFixture) {
        match merge_stage(&fx.driver, &fx.id).expect("merge") {
            MergeResult::Merged(_) => {}
            other => panic!("expected a merge, got {other:?}"),
        }
        assert_eq!(fx.driver.job(&fx.id).expect("job").stage, Stage::Merged);
    }

    fn tamper() -> Effect {
        Box::new(|spec: &AgentSpec| {
            std::fs::write(spec.cwd.join("stray.txt"), "x").expect("write");
            "docs/a.md".to_string()
        })
    }

    #[test]
    fn no_kb_path_means_no_kb_run() {
        let fx = fixture(vec![], false);
        merge(&fx);
        assert!(fx.runner.specs_for("kb").is_empty());
        assert!(!fx.ledger().contains("kb "));
    }

    #[test]
    fn a_merge_refreshes_the_kb() {
        let fx = fixture(vec![reply("docs/a.md")], true);
        merge(&fx);
        assert_eq!(fx.runner.specs_for("kb").len(), 1);
        assert!(fx.ledger().contains("kb refreshed, pages: docs/a.md"));
    }

    #[test]
    fn a_failing_refresh_does_not_change_the_merge() {
        let fx = fixture(vec![], true);
        merge(&fx);
        assert!(fx.ledger().contains("kb refresh failed"));
    }

    #[test]
    fn up_to_date_is_recorded() {
        let fx = fixture(vec![reply("No relevant changes")], true);
        merge(&fx);
        assert!(fx.ledger().contains("kb up to date"));
    }

    #[test]
    fn tampering_stops_later_refreshes() {
        let fx = fixture(vec![tamper(), reply("docs/a.md")], true);
        merge(&fx);
        assert!(fx.ledger().contains("changed the main checkout"));
        assert!(fx.driver.layout().kb_stop_file().exists());
        let line = fx.driver.kb_refresh(&fx.id, false).expect("skip");
        assert!(line.contains("kb refresh skipped"), "{line}");
        assert!(fx.ledger().contains("kb refresh skipped"));
        assert_eq!(fx.runner.specs_for("kb").len(), 1);
    }

    #[test]
    fn a_manual_run_clears_the_marker() {
        let fx = fixture(vec![reply("docs/a.md")], true);
        let marker = fx.driver.layout().kb_stop_file();
        std::fs::create_dir_all(marker.parent().expect("parent")).expect("mkdir");
        std::fs::write(&marker, "old\n").expect("marker");
        let line = fx.driver.kb_refresh(&fx.id, true).expect("manual");
        assert!(line.contains("kb refreshed"), "{line}");
        assert!(!marker.exists());
    }

    #[test]
    fn an_unset_kb_path_writes_nothing() {
        let fx = fixture(vec![], false);
        assert_eq!(
            fx.driver.kb_refresh(&fx.id, true).expect("line"),
            "kb_path is not set"
        );
        assert!(!fx.ledger().contains("Ruling: kb"));
    }

    #[test]
    fn a_multi_line_failure_still_gets_a_ruling() {
        let fx = fixture(vec![reply("BLOCKED: one\ntwo")], true);
        merge(&fx);
        assert!(fx.ledger().contains("kb refresh failed"));
    }

    #[test]
    fn a_manual_failed_run_keeps_the_marker() {
        let fx = fixture(vec![], true);
        let marker = fx.driver.layout().kb_stop_file();
        std::fs::create_dir_all(marker.parent().expect("parent")).expect("mkdir");
        std::fs::write(&marker, "old\n").expect("marker");
        let line = fx.driver.kb_refresh(&fx.id, true).expect("manual");
        assert!(line.contains("kb refresh failed"), "{line}");
        assert!(marker.exists());
    }

    #[test]
    fn the_marker_holds_the_reason_and_an_up_to_date_manual_run_clears_it() {
        let fx = fixture(vec![tamper(), reply("No relevant changes")], true);
        merge(&fx);
        let text = std::fs::read_to_string(fx.driver.layout().kb_stop_file()).expect("marker");
        assert!(
            text.contains("the main checkout changed during the KB refresh"),
            "{text}"
        );
        let line = fx.driver.kb_refresh(&fx.id, true).expect("manual");
        assert_eq!(line, "kb up to date");
        assert!(!fx.driver.layout().kb_stop_file().exists());
    }
}
