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

#[test]
fn new_prints_the_created_line_first_then_the_warning() {
    let fx = Fixture::new();
    fx.run(&["spec", "start", "demo", "f"]).expect("start");
    let plan = fx.write_plan("plan-01-a.md", 1, "none");
    let folder = plan.parent().expect("plans folder");
    let spec = "# Spec\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
                ## Acceptance criteria\n\n- It is done.\n";
    std::fs::write(folder.join("spec.md"), spec).expect("spec");
    let out = fx.run(&["new", "demo", "f", "--plans", folder.to_str().expect("utf8")]).expect("new");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("created job "), "{out}");
    assert!(
        lines.contains(&"warning: no decision in this spec was answered by the user"),
        "{out}"
    );
    assert!(fx.run(&["spec", "resume", "demo"]).is_err(), "the draft is adopted");
}

#[test]
fn spec_start_refuses_a_digit_only_slug() {
    let fx = Fixture::new();
    let err = fx.run(&["spec", "start", "12", "f"]).expect_err("digits");
    assert!(err.to_string().contains("a draft slug cannot be only digits"), "{err}");
    let status = fx.run(&["status"]).expect("status");
    assert!(!status.contains("drafts:"), "{status}");
}
