mod common;

use std::path::PathBuf;

use common::Fixture;

const HEADER: &str = "# Plan\n\n> For agents: use superpowers:subagent-driven-development.\n\n**Goal:** g\n\n**Architecture:** a\n\n**Tech Stack:** t\n\n**Spec:** s\n\n**Global Constraints:** c\n\n**Review Focus:** r\n\n";
const TASK_BODY: &str = "**Files:**\n- Modify: `a.rs`\n\n**Interfaces:**\n- Produces: x\n\n";
const SPEC: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
                    ## Acceptance criteria\n\n- It is done.\n\n## Decisions\n\n- Use a queue. Decided by: user\n";

/// Writes `count` lint clean plans of `tasks` tasks and a valid spec into one folder. Returns the folder.
fn plans_folder(fx: &Fixture, count: usize, tasks: usize) -> PathBuf {
    let plans = fx.tmp.path().join("plans");
    std::fs::create_dir_all(&plans).expect("plans dir");
    for n in 1..=count {
        let next = if n == count { "none".to_string() } else { plans.join(format!("plan-0{}-x.md", n + 1)).to_string_lossy().into_owned() };
        let mut text = String::from(HEADER);
        for t in 1..=tasks {
            text.push_str(&format!("### Task {t}: t\n\n{TASK_BODY}"));
        }
        text.push_str(&format!("Next plan: {next}\n"));
        std::fs::write(plans.join(format!("plan-0{n}-x.md")), text).expect("plan");
    }
    std::fs::write(plans.join("spec.md"), SPEC).expect("spec");
    plans
}

fn set_budget(fx: &Fixture, budget: u64) {
    let path = fx.root.join(".claude/factory/config.toml");
    let mut text = std::fs::read_to_string(&path).expect("config");
    text.push_str(&format!("budget_weighted = {budget}\n"));
    std::fs::write(path, text).expect("config");
}

#[test]
fn one_job_output_starts_with_the_created_line() {
    let fx = Fixture::new();
    set_budget(&fx, 2_000_000);
    let plans = plans_folder(&fx, 2, 2);
    let out = fx.run(&["new", "demo", "f", "--plans", plans.to_str().expect("utf8"), "--one-job"]).expect("new");
    let first = out.lines().next().expect("a line");
    assert!(first.starts_with("created job "), "{out}");
    assert!(out.lines().skip(1).any(|l| l.starts_with("note: ")), "{out}");
}

#[test]
fn over_budget_new_prints_needs_consent_and_creates_nothing() {
    let fx = Fixture::new();
    let plans = plans_folder(&fx, 4, 3);
    let err = fx.run(&["new", "demo", "f", "--plans", plans.to_str().expect("utf8"), "--one-job"]).expect_err("needs consent");
    assert!(err.to_string().starts_with("needs consent:"), "{err}");
    let board = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    )
    .load()
    .expect("board");
    assert!(board.jobs.is_empty());
}

#[test]
fn without_the_flag_new_is_unchanged() {
    let fx = Fixture::new();
    let plans = plans_folder(&fx, 4, 3);
    let out = fx.run(&["new", "demo", "f", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(out.starts_with("created job 001 in "), "{out}");
    assert!(!out.contains("note: "), "{out}");
}

#[test]
fn accept_over_budget_creates_the_job_and_prints_a_budget_warning() {
    let fx = Fixture::new();
    let plans = plans_folder(&fx, 4, 3);
    let args = ["new", "demo", "f", "--plans", plans.to_str().expect("utf8"), "--one-job", "--accept-over-budget"];
    let out = fx.run(&args).expect("new");
    let first = out.lines().next().expect("a line");
    assert!(first.starts_with("created job "), "{out}");
    assert!(out.lines().skip(1).any(|l| l.starts_with("warning: ") && l.contains("budget")), "{out}");
}

#[test]
fn binary_prints_the_consent_line_bare_and_exits_non_zero() {
    let fx = Fixture::new();
    let plans = plans_folder(&fx, 4, 3);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_factory"))
        .arg("--root")
        .arg(&fx.root)
        .args(["new", "demo", "f", "--plans", plans.to_str().expect("utf8"), "--one-job"])
        .output()
        .expect("run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("needs consent:"), "{stderr}");
    assert!(!output.status.success());
    assert!(output.status.code().is_some_and(|c| c != 0));
}
