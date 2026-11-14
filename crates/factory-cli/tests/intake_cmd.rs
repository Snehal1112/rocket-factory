mod common;
use std::sync::{Arc, Mutex};

use common::Fixture;
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_engine::{IssueInfo, LabelEvent, Tracker};

/// A tracker with one trusted, labelled issue, #7. It records the label edits.
#[derive(Default)]
struct OneIssue {
    edits: Mutex<Vec<(Vec<String>, Vec<String>)>>,
}

impl Tracker for OneIssue {
    fn repo_name(&self) -> Result<String, String> {
        Ok("o/r".into())
    }
    fn labels_of(&self, _: u32) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    fn is_closed(&self, _: u32) -> Result<bool, String> {
        Ok(false)
    }
    fn create(&self, _: &str, _: &str, _: &[String]) -> Result<u32, String> {
        Err("not used".into())
    }
    fn find_created(&self, _: &str, _: &str) -> Result<Option<u32>, String> {
        Ok(None)
    }
    fn comment(&self, _: u32, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn close(&self, _: u32) -> Result<(), String> {
        Ok(())
    }
    fn list_labelled(&self, _: &str) -> Result<Vec<IssueInfo>, String> {
        Ok(vec![IssueInfo {
            number: 7,
            title: "Add a thing".into(),
            body: "Please.".into(),
            labels: vec!["factory".into()],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }])
    }
    fn labelled_by(&self, _: u32, _: &str) -> Result<Option<LabelEvent>, String> {
        Ok(Some(LabelEvent {
            actor: "boss".into(),
            at: "2026-01-02T00:00:00Z".into(),
        }))
    }
    fn edit_labels(&self, _: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        self.edits
            .lock()
            .expect("lock")
            .push((add.to_vec(), remove.to_vec()));
        Ok(())
    }
    fn current_user(&self) -> Result<String, String> {
        Ok("boss".into())
    }
}

/// A drafter that refuses every issue by writing `refused.md` into its draft folder.
struct Refuser {
    drafter_calls: Mutex<u32>,
}

impl AgentRunner for Refuser {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if spec.role == "intake-drafter" {
            *self.drafter_calls.lock().expect("lock") += 1;
            let draft = spec
                .prompt
                .lines()
                .find_map(|l| l.strip_prefix("Draft folder (write only here): "))
                .expect("draft folder line");
            std::fs::create_dir_all(draft).expect("mkdir draft");
            std::fs::write(std::path::Path::new(draft).join("refused.md"), "too big\n")
                .expect("write refused");
        }
        AgentRun::done("done")
    }
}

fn fixture(intake: bool) -> (Fixture, Arc<OneIssue>, Arc<Refuser>) {
    let mut fx = Fixture::new();
    let config = format!("intake = {intake}\n");
    std::fs::write(fx.root.join(".claude/factory/config.toml"), config).expect("config");
    let tracker = Arc::new(OneIssue::default());
    let runner = Arc::new(Refuser {
        drafter_calls: Mutex::new(0),
    });
    let mut ctx = factory_cli::CliCtx::new(fx.root.clone(), runner.clone(), "tester");
    ctx = ctx.with_tracker(tracker.clone());
    fx.ctx = ctx;
    (fx, tracker, runner)
}

#[test]
fn intake_off_prints_one_line() {
    let (fx, tracker, _) = fixture(false);
    let out = fx.run(&["intake", "--once"]).expect("intake");
    assert_eq!(out, "intake is off");
    assert!(tracker.edits.lock().expect("lock").is_empty());
}

#[test]
fn intake_dry_run_names_the_issue() {
    let (fx, tracker, runner) = fixture(true);
    let out = fx.run(&["intake", "--dry-run"]).expect("intake");
    assert!(out.starts_with("would pick #7: "), "{out}");
    assert!(tracker.edits.lock().expect("lock").is_empty());
    assert_eq!(*runner.drafter_calls.lock().expect("lock"), 0);
}

#[test]
fn intake_without_a_flag_runs_one_pass() {
    let (fx, tracker, runner) = fixture(true);
    let out = fx
        .run(&["intake"])
        .expect("a refused draft is not an error");
    assert!(out.starts_with("failed #7: "), "{out}");
    assert_eq!(*runner.drafter_calls.lock().expect("lock"), 1);
    let edits = tracker.edits.lock().expect("lock");
    assert!(
        edits
            .iter()
            .any(|(add, _)| add.iter().any(|l| l == "factory: needs-human")),
        "{edits:?}"
    );
}
