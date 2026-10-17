mod common;

use common::Fixture;

const SPEC: &str = "docs/superpowers/specs";

fn spec_file(fx: &Fixture, slug: &str) -> std::path::PathBuf {
    let date = factory_engine::today();
    fx.root.join(SPEC).join(format!("{date}-{slug}-design.md"))
}

#[test]
fn spec_start_stage_resume_round_trip() {
    let fx = Fixture::new();
    let date = factory_engine::today();
    let rel = format!("{SPEC}/{date}-demo-design.md");
    let out = fx.run(&["spec", "start", "demo", "a demo feature"]).expect("start");
    assert_eq!(out, format!("draft demo\nstage interviewing\nfile {rel} (missing)"));
    fx.run(&["spec", "stage", "demo", "writing-spec"]).expect("stage");
    std::fs::create_dir_all(spec_file(&fx, "demo").parent().expect("parent")).expect("mkdir");
    std::fs::write(spec_file(&fx, "demo"), "# spec\n").expect("write");
    let out = fx.run(&["spec", "resume", "demo"]).expect("resume");
    assert_eq!(out, format!("draft demo\nstage writing-spec\nfile {rel} (exists)"));
    let err = fx.run(&["spec", "start", "demo", "again"]).expect_err("duplicate");
    assert!(err.to_string().contains("already exists"), "{err}");
    let err = fx.run(&["spec", "stage", "demo", "nope"]).expect_err("bad stage");
    assert!(err.to_string().contains("unknown draft stage"), "{err}");
    let err = fx.run(&["spec", "resume", "ghost"]).expect_err("no draft");
    assert!(err.to_string().contains("no draft named ghost"), "{err}");
}

#[test]
fn cancel_by_slug_removes_the_card_and_keeps_the_spec_file() {
    let fx = Fixture::new();
    fx.run(&["spec", "start", "demo", "f"]).expect("start");
    let file = spec_file(&fx, "demo");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    std::fs::write(&file, "# spec\n").expect("write");
    let out = fx.run(&["cancel", "demo"]).expect("cancel");
    assert!(out.contains("draft demo cancelled"), "{out}");
    assert!(file.exists());
    assert!(fx.run(&["spec", "resume", "demo"]).is_err());
}

#[test]
fn cancel_by_number_still_cancels_a_job() {
    let fx = Fixture::new();
    fx.run(&["new", "hello", "f"]).expect("new");
    let out = fx.run(&["cancel", "1"]).expect("cancel");
    assert_eq!(out, "job 001 cancelled");
}

#[test]
fn cancel_of_an_unknown_slug_is_a_clear_error() {
    let fx = Fixture::new();
    let err = fx.run(&["cancel", "ghost"]).expect_err("unknown");
    assert_eq!(err.to_string(), "no job or draft named ghost");
}

#[test]
fn status_lists_drafts() {
    let fx = Fixture::new();
    fx.run(&["spec", "start", "demo", "a demo feature"]).expect("start");
    let out = fx.run(&["status"]).expect("status");
    assert!(out.contains("drafts:") && out.contains("demo") && out.contains("interviewing"), "{out}");
    fx.run(&["new", "hello", "f"]).expect("new");
    let out = fx.run(&["status"]).expect("status");
    assert!(out.contains("hello") && out.contains("drafts:") && out.contains("demo"), "{out}");
}
