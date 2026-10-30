#![allow(dead_code)]

mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture};
use factory_core::{AgentFile, Stage};

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

/// An agent file with a frontmatter block, the marker line when given, then a body.
fn agent_file(name: &str, marker: Option<&str>) -> AgentFile {
    let marker = marker.map(|m| format!("{m}\n")).unwrap_or_default();
    AgentFile {
        name: name.to_string(),
        content: format!("---\nname: x\n---\n{marker}Body text.\n"),
    }
}

/// Replaces the fixture's temp dir and every commit sha with fixed names, so the prompt is stable.
fn norm(fx: &Fixture, prompt: &str) -> String {
    let canonical = fx.tmp.path().canonicalize().expect("canonical temp dir");
    let text = prompt
        .replace(&canonical.to_string_lossy().into_owned(), "<TMP>")
        .replace(&fx.tmp.path().to_string_lossy().into_owned(), "<TMP>");
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        // A sha can be all letters by chance, so the length alone decides.
        let is_sha = run.len() >= 7;
        out.push_str(if is_sha { "<SHA>" } else { run.as_str() });
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// A builder that writes one file and leaves it uncommitted.
fn change(file: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "line\n").expect("write change");
        "done".to_string()
    })
}

fn first_prompt(files: Vec<AgentFile>) -> (Fixture, String) {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(files)
        .build_stage()
        .expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    (fx, prompt)
}

fn task_prompt(files: Vec<AgentFile>) -> (Fixture, String) {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    let runner = scripted_auto(vec![change("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(files)
        .build_stage()
        .expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    (fx, prompt)
}

fn fix_prompt(first: Vec<AgentFile>, fix: Vec<AgentFile>) -> (Fixture, String) {
    fix_prompt_on(committing_fixture(), first, fix)
}

fn fix_prompt_on(fx: Fixture, first: Vec<AgentFile>, fix: Vec<AgentFile>) -> (Fixture, String) {
    let runner = scripted_auto(vec![change("a.txt"), change("b.txt")]);
    fx.engine(&runner)
        .with_agent_files(first)
        .build_stage()
        .expect("first");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    fx.engine(&runner)
        .with_agent_files(fix)
        .build_stage()
        .expect("resume");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    (fx, prompt)
}

fn review_prompt(files: Vec<AgentFile>) -> (Fixture, String) {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(files.clone())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(files.clone())
        .checks_stage()
        .expect("checks");
    let rr = scripted(vec![Box::new(|_| "ok\nVERDICT: APPROVE".to_string())]);
    fx.engine(&rr)
        .with_agent_files(files)
        .review_stage()
        .expect("review");
    let prompt = rr.last_spec_for("reviewer").expect("spec").prompt;
    (fx, prompt)
}

const GOLDEN_FIRST: &str = "You are the builder in a software factory. Your working directory is the job worktree, already on branch `factory/001-demo`.\nRead the plan at <TMP>/job/plan.md. Never edit the plan.\nFollow the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md), task by task. You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review. Write the failing test first, then the code, then run the checks.\nAfter your checks pass, invoke the `simplify` skill on your uncommitted changes, even when the change is small; do not skip it. Run it inline without helper agents, apply only fixes inside the files of this task, then run the checks again.\nDo not run `git add` or `git commit`. Leave your changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. Never push, never merge, never touch the board or the ledger.\nReply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line, then at most five lines of concerns or the blocker, then one Ruling: <what> — <why> — <cost if wrong> line per judgment call. Do not commit and do not write a report file. If you are blocked, stop and reply Status: BLOCKED with the reason.";

const GOLDEN_TASK: &str = "You are the builder in a software factory. Your working directory is the job worktree, already on branch `factory/001-demo`.\nThe plan is at <TMP>/job/plan.md. Never edit the plan.\nTask excerpt: the plan header, your task, and the spec goal and acceptance criteria follow. They replace reading the plan and the spec in full. Open those files only for something this excerpt refers to outside itself. If a skill such as superpowers:subagent-driven-development tells you to read or load the plan file, use this excerpt instead and do not read the plan file in full.\n--- plan header ---\n# P\n--- your task ---\n### Task 1: first thing\n\nx\n--- end of excerpt ---\nBase commit: <SHA>. Current tip: <SHA>.\nNo earlier task has a commit yet.\nDo only Task 1: first thing. Earlier tasks are already committed, do not redo them. Later tasks belong to other agents.\nFollow the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md) for this task. You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review. Write the failing test first, then the code, then run the checks the task names.\nAfter your checks pass, invoke the `simplify` skill on your uncommitted changes, even when the change is small; do not skip it. Run it inline without helper agents, apply only fixes inside the files of this task, then run the checks again.\nDo not run `git add` or `git commit`. Leave this task's changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. Never push, never merge, never touch the board or the ledger.\nNever sleep or poll, never run a command in the background and wait for it; if a command needs more than ten minutes say so and stop.\nReply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line, then at most five lines of concerns or the blocker, then one Ruling: <what> — <why> — <cost if wrong> line per judgment call. Do not commit and do not write a report file. If you are blocked, stop and reply Status: BLOCKED with the reason.";

const GOLDEN_FIX: &str = "Fix only the problems listed in <TMP>/job/changes-r2.md. You are on branch `factory/001-demo`. Do not run `git add` or `git commit`: leave the fixes in the worktree and the driver commits them. Do not amend or rewrite earlier commits. Follow `superpowers:receiving-code-review` for the findings, and the implementer rules of `superpowers:subagent-driven-development`. You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review. Read all the findings first, then verify each against the code. Work one item at a time. If a finding is wrong, push back with a reason in your reply and do not change the code for it. If an item is unclear, reply with `Status: NEEDS_CONTEXT` and the question instead of guessing. Run the relevant checks before you finish. Never push, never merge.";

const GOLDEN_REVIEW: &str = "Review this round's diff for the job.\nInputs: the diff at <TMP>/job/review-<SHA>..<SHA>.diff, the list of changed files at <TMP>/job/review-<SHA>..<SHA>.names, the plan at <TMP>/job/plan.md (read it only to look up one step), and the worktree at <TMP>/main/.claude/worktrees/factory-001-demo. Round 1.\nPlan outline (header and task headings):\n- [ ] Task 1\nNo test was added in this job. Judge whether the new behavior is covered, and treat missing tests as a blocker.\nRead the diff in pages: use the Read tool with offset and limit 1500 lines per call. Do not read the whole file at once. Check: the diff does what the plan asks and nothing else; correctness and edge cases; tests that would fail without the change; no debug leftovers. The spec and plan are a vision document: where they are silent, what a reasonable person would expect counts. You are read-only and you do not dispatch subagents.\nReply in at most 60 lines. List findings as a numbered list with file:line, severity [blocker] or [minor], and a concrete fix. Grade each finding: Critical and Important findings carry [blocker], Minor findings carry [minor]. Before the verdict list anything you could not judge under `Declined to judge`, with the reason. Any blocker means CHANGES. Your last line, alone, must be exactly `VERDICT: APPROVE` or `VERDICT: CHANGES`.";

#[test]
fn the_full_first_builder_prompt_matches_golden_text() {
    let (fx, prompt) = first_prompt(Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_FIRST);
}

#[test]
fn the_full_task_builder_prompt_matches_golden_text() {
    let (fx, prompt) = task_prompt(Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_TASK);
}

#[test]
fn the_full_fix_prompt_matches_golden_text() {
    let (fx, prompt) = fix_prompt(Vec::new(), Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_FIX);
}

#[test]
fn the_full_reviewer_prompt_matches_golden_text() {
    let (fx, prompt) = review_prompt(Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_REVIEW);
}

fn builder_file(marker: Option<&str>) -> Vec<AgentFile> {
    vec![agent_file("factory-builder.md", marker)]
}

#[test]
fn a_current_agent_file_gives_the_slim_task_prompt() {
    let (fx, prompt) = task_prompt(builder_file(Some("<!-- factory-rules: 1 -->")));
    assert!(prompt.contains("Do only Task 1: first thing"), "{prompt}");
    assert!(prompt.contains(&fx.job().branch), "{prompt}");
    for gone in [
        "git commit",
        "Status: DONE",
        "implementer rules",
        "Write the failing test first",
        "Never push",
    ] {
        assert!(!prompt.contains(gone), "{gone} in {prompt}");
    }
    assert!(prompt.contains("Never sleep or poll"), "{prompt}");
}

#[test]
fn a_current_agent_file_gives_the_slim_first_run_prompt() {
    let (_fx, prompt) = first_prompt(builder_file(Some("<!-- factory-rules: 1 -->")));
    assert!(!prompt.contains("Status: DONE"), "{prompt}");
    assert!(!prompt.contains("git commit"), "{prompt}");
    assert!(prompt.contains("Never edit the plan."), "{prompt}");
    assert!(!prompt.contains("Never sleep"), "{prompt}");
}

#[test]
fn a_missing_marker_keeps_the_full_builder_prompt() {
    let (fx, prompt) = task_prompt(builder_file(None));
    assert_eq!(norm(&fx, &prompt), GOLDEN_TASK);
}

#[test]
fn an_older_marker_keeps_the_full_builder_prompt() {
    let (fx, prompt) = task_prompt(builder_file(Some("<!-- factory-rules: 0 -->")));
    assert_eq!(norm(&fx, &prompt), GOLDEN_TASK);
}

#[test]
fn a_newer_marker_counts_as_current() {
    let (_fx, prompt) = task_prompt(builder_file(Some("<!-- factory-rules: 99 -->")));
    assert!(!prompt.contains("Status: DONE"), "{prompt}");
}

#[test]
fn a_marker_inside_other_text_does_not_count() {
    let (fx, prompt) = task_prompt(builder_file(Some("see <!-- factory-rules: 1 --> here")));
    assert_eq!(norm(&fx, &prompt), GOLDEN_TASK);
}

#[test]
fn the_slim_task_prompt_is_shorter_than_the_full_one() {
    let (_a, slim) = task_prompt(builder_file(Some("<!-- factory-rules: 1 -->")));
    let (_b, full) = task_prompt(Vec::new());
    assert!(slim.len() < full.len());
}

#[test]
fn the_slim_fix_round_prompt_keeps_findings_hints_and_the_kb_line() {
    let mut fx = committing_fixture();
    fx.config.check_hints = vec!["cargo test".to_string()];
    fx.config.kb_path = Some(fx.tmp.path().join("kb").to_string_lossy().into_owned());
    let files = builder_file(Some("<!-- factory-rules: 1 -->"));
    let (_fx, prompt) = fix_prompt_on(fx, files.clone(), files);
    assert!(prompt.contains("changes-r2.md"), "{prompt}");
    assert!(prompt.contains("receiving-code-review"), "{prompt}");
    assert!(prompt.contains("`cargo test`"), "{prompt}");
    assert!(prompt.contains("Knowledge base:"), "{prompt}");
    assert!(!prompt.contains("git commit"), "{prompt}");
}

#[test]
fn the_fix_round_checks_the_agent_file_when_it_starts() {
    let files = builder_file(Some("<!-- factory-rules: 1 -->"));
    let (_fx, prompt) = fix_prompt(Vec::new(), files);
    assert!(!prompt.contains("git commit"), "{prompt}");
    let (fx, prompt) = fix_prompt(Vec::new(), Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_FIX);
}

#[test]
fn the_fallback_is_noted_once_per_job_and_agent() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![change("a.txt"), change("b.txt")]);
    fx.engine(&runner)
        .with_agent_files(builder_file(None))
        .build_stage()
        .expect("build");
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default();
    let note = "agent file factory-builder.md has no current rules marker";
    assert_eq!(ledger.matches(note).count(), 1, "{ledger}");
}

fn reviewer_files(marker: Option<&str>) -> Vec<AgentFile> {
    vec![
        agent_file("factory-code-reviewer.md", marker),
        agent_file("factory-builder.md", marker),
    ]
}

#[test]
fn the_slim_reviewer_prompt_drops_the_verdict_and_check_text() {
    let (_fx, prompt) = review_prompt(reviewer_files(Some("<!-- factory-rules: 1 -->")));
    for gone in [
        "VERDICT:",
        "60 lines",
        "Declined to judge",
        "vision document",
        "1500",
        "Check: the diff does",
    ] {
        assert!(!prompt.contains(gone), "{gone} in {prompt}");
    }
    assert!(
        prompt.contains("Review this round's diff for the job."),
        "{prompt}"
    );
    assert!(prompt.contains("Round 1."), "{prompt}");
}

#[test]
fn the_slim_reviewer_prompt_keeps_inputs_and_the_rereview_block() {
    let (fx, _first) = review_prompt(reviewer_files(Some("<!-- factory-rules: 1 -->")));
    let wt = std::path::PathBuf::from(fx.job().worktree.expect("worktree"));
    std::fs::write(fx.job_dir.join("review-code-r1.md"), "1. [blocker] x\n").expect("earlier");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").last_review = Some(factory_core::LastReview {
        round: 1,
        verdict: "changes".into(),
        blockers: 1,
        file: "review-code-r1.md".into(),
        source: "build".into(),
    });
    board.save(&fx.board_path).expect("save");
    let rr = scripted(Vec::new());
    let spec = fx.engine(&rr).reviewer_spec(
        &wt,
        &fx.job_dir.join("d.diff"),
        &fx.job_dir.join("n.txt"),
        2,
        &[],
        &factory_engine::ReviewerExtra::default(),
    );
    assert!(
        spec.prompt.contains("findings of round 1"),
        "{}",
        spec.prompt
    );
    assert!(spec.prompt.contains("scoped re-review"), "{}", spec.prompt);
    assert!(!spec.prompt.contains("VERDICT:"), "{}", spec.prompt);
}

#[test]
fn a_missing_reviewer_file_keeps_the_full_prompt_and_notes_it() {
    let (fx, prompt) = review_prompt(Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_REVIEW);
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default();
    assert!(
        ledger.contains("agent file factory-code-reviewer.md has no current rules marker"),
        "{ledger}"
    );
}

#[test]
fn a_marker_zero_reviewer_file_keeps_the_full_prompt() {
    let (fx, prompt) = review_prompt(reviewer_files(Some("<!-- factory-rules: 0 -->")));
    assert_eq!(norm(&fx, &prompt), GOLDEN_REVIEW);
}

#[test]
fn the_slim_reviewer_prompt_is_shorter_than_the_full_one() {
    let (_a, slim) = review_prompt(reviewer_files(Some("<!-- factory-rules: 1 -->")));
    let (_b, full) = review_prompt(Vec::new());
    assert!(slim.len() < full.len(), "{} vs {}", slim.len(), full.len());
}

const GOLDEN_FIX_FRESH: &str = "You are the builder in a software factory, picking up a fix round in a fresh session. You are on branch `factory/001-demo`; the job's work so far is the diff from <SHA> to <SHA> (run `git diff <SHA>..<SHA>` to see it). Read the plan at <TMP>/job/plan.md. Never edit the plan.\nFix only the problems listed in <TMP>/job/changes-r3.md. Follow `superpowers:receiving-code-review` for the findings. Run the relevant checks. Do not run `git add` or `git commit`; the driver commits for you. Reply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line.";

#[test]
fn the_fresh_fix_prompt_matches_golden_text() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change("a.txt"), change("b.txt")]);
    fx.engine(&runner).build_stage().expect("first");
    std::fs::write(fx.job_dir.join("changes-r3.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 3;
    job.escalated_model = Some("claude-opus-5-5".to_string());
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    fx.engine(&runner).build_stage().expect("fresh fix");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert_eq!(norm(&fx, &prompt), GOLDEN_FIX_FRESH);
}
