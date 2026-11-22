mod common;
use common::{amend_head, commit_file, scripted, Fixture};
use factory_engine::{MessageKind, MessageRequest, NOTES_DIR};

fn request<'a>(base: &'a str, tip: &'a str, issues: &'a [u32], closes: bool) -> MessageRequest<'a> {
    MessageRequest {
        kind: MessageKind::Merge,
        job_id: "001",
        slug: "demo",
        title: "demo feature",
        base,
        tip,
        issues,
        closes_ticket: closes,
    }
}

fn write_plan(fx: &Fixture, text: &str) {
    std::fs::write(&fx.plan_file, text).expect("plan");
}

fn write_note(fx: &Fixture, file: &str, subject: &str, bullet: &str) {
    let dir = fx.job_dir.join(NOTES_DIR);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let text = format!("{subject}\n\nWhy it matters.\n\n- {bullet}\n");
    std::fs::write(dir.join(file), text).expect("note");
}

fn note_reply(subject: &str) -> String {
    format!("---BEGIN COMMIT---\n{subject}\n\nWhy it matters.\n\n- Do the thing\n---END COMMIT---")
}

const PLAN: &str = "# Retry\n\n**Goal:** Let a user retry a parked job.\n\n**Commit:** feat(retry): retry a parked job\n";

#[test]
fn save_commit_note_writes_a_valid_note() {
    let fx = Fixture::new();
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    engine
        .save_commit_note(
            "task-1.md",
            "task 1",
            &note_reply("Add the endpoint"),
            "Task 1",
            &[],
        )
        .expect("save");
    let text = std::fs::read_to_string(fx.job_dir.join(NOTES_DIR).join("task-1.md")).expect("read");
    assert!(text.starts_with("Add the endpoint\n"));
    assert!(text.contains("- Do the thing"));
    assert!(!fx.ledger().contains("commit note for"));
}

#[test]
fn save_commit_note_falls_back_with_a_ruling() {
    let fx = Fixture::new();
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let files = vec!["src/a.rs".to_string()];
    engine
        .save_commit_note(
            "task-1.md",
            "task 1",
            "no note here",
            "Add the thing",
            &files,
        )
        .expect("save");
    let text = std::fs::read_to_string(fx.job_dir.join(NOTES_DIR).join("task-1.md")).expect("read");
    assert!(text.starts_with("Add the thing\n"));
    assert!(text.contains("src/a.rs"));
    assert!(fx
        .ledger()
        .contains("commit note for task 1: missing, fallback used"));
}

#[test]
fn a_bad_note_gets_the_fallback_with_the_reason() {
    let fx = Fixture::new();
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let long = "a".repeat(60);
    engine
        .save_commit_note("task-1.md", "task 1", &note_reply(&long), "Task one", &[])
        .expect("save");
    let text = std::fs::read_to_string(fx.job_dir.join(NOTES_DIR).join("task-1.md")).expect("read");
    assert!(text.starts_with("Task one\n"));
    let ledger = fx.ledger();
    assert!(ledger.contains("the subject is 60 characters"));
    assert!(ledger.contains("fallback used"));
}

#[test]
fn the_merge_message_is_built_from_the_notes() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    write_note(&fx, "task-1.md", "Add the endpoint", "Add POST retry");
    write_note(&fx, "task-2.md", "Show the button", "Add a Retry button");
    write_note(
        &fx,
        "round-2.md",
        "Address review",
        "Refuse approve while queued",
    );
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[142], true))
        .expect("message");
    let want = "feat(retry): retry a parked job\n\n\
Let a user retry a parked job.\n\n\
Add the endpoint:\n- Add POST retry\n\n\
Show the button:\n- Add a Retry button\n\n\
Review fixes:\n- Refuse approve while queued\n\n\
Fixes: #142\nFactory-Job: 001";
    assert_eq!(message, want);
}

#[test]
fn task_sections_follow_the_task_number_order() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    write_note(&fx, "task-10.md", "Tenth", "Ten");
    write_note(&fx, "task-2.md", "Second", "Two");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    let second = message.find("Second:").expect("second");
    let tenth = message.find("Tenth:").expect("tenth");
    assert!(second < tenth);
}

#[test]
fn a_plan_without_a_commit_line_uses_the_plan_title() {
    let fx = Fixture::new();
    write_plan(&fx, "# Retry a parked job\n\n**Goal:** Retry it.\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    let subject = message.lines().next().expect("subject");
    assert_eq!(subject, "feat: retry a parked job");
}

#[test]
fn a_title_ending_in_a_period_drops_the_period() {
    let fx = Fixture::new();
    write_plan(&fx, "# Retry a parked job.\n\n**Goal:** Retry it.\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    let subject = message.lines().next().expect("subject");
    assert_eq!(subject, "feat: retry a parked job");
    assert!(
        !fx.ledger().contains("fixed message used"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_second_merge_message_writes_the_refused_line_ruling_once() {
    let fx = Fixture::new();
    let _ = std::fs::remove_file(fx.job_dir.join("progress.md"));
    write_plan(&fx, "# Retry\n\n**Commit:** Added a thing\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    for _ in 0..2 {
        engine
            .job_message(&request("", "", &[], true))
            .expect("message");
    }
    assert_eq!(
        fx.ledger().matches("plan **Commit:** line refused").count(),
        1,
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_plan_without_a_heading_uses_the_feature() {
    let fx = Fixture::new();
    write_plan(&fx, "- [ ] Task 1\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    let subject = message.lines().next().expect("subject");
    assert_eq!(subject, "feat: demo feature");
}

#[test]
fn a_broken_commit_line_at_merge_falls_back_with_a_ruling() {
    let fx = Fixture::new();
    write_plan(&fx, "# Retry\n\n**Commit:** Added a thing\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    assert!(!message.starts_with("Added a thing"));
    assert!(fx
        .ledger()
        .contains("merge message: plan **Commit:** line refused"));
    assert!(fx.ledger().contains("plan title used"));
}

#[test]
fn a_job_without_notes_uses_the_old_commit_subjects() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let base = repo
        .head()
        .expect("head")
        .peel_to_commit()
        .expect("commit")
        .id()
        .to_string();
    let mut tip = String::new();
    for (i, msg) in [
        "feat(x): add y",
        "wip: task 1 a",
        "chore(factory): sync main into job 001 demo",
    ]
    .iter()
    .enumerate()
    {
        commit_file(&fx.main, &format!("f{i}.txt"), msg);
        tip = amend_head(&fx.main, msg);
    }
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request(&base, &tip, &[], true))
        .expect("message");
    assert!(message.contains("Changes:\n- feat(x): add y"));
    assert!(!message.contains("wip:"));
    assert!(!message.contains("chore(factory): sync"));
}

/// Commits one file per subject on the fixture repo and returns the base and the new tip.
fn commit_subjects(fx: &Fixture, subjects: &[&str]) -> (String, String) {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let base = repo
        .head()
        .expect("head")
        .peel_to_commit()
        .expect("commit")
        .id()
        .to_string();
    let mut tip = base.clone();
    for (i, msg) in subjects.iter().enumerate() {
        commit_file(&fx.main, &format!("c{i}.txt"), msg);
        tip = amend_head(&fx.main, msg);
    }
    (base, tip)
}

#[test]
fn a_fix_round_after_the_upgrade_keeps_the_old_commits() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let (base, tip) = commit_subjects(&fx, &["feat(x): add y", "wip: fix round 2"]);
    write_note(&fx, "round-2.md", "Address review", "Refuse approve");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request(&base, &tip, &[], true))
        .expect("message");
    let changes = message.find("Changes:\n- feat(x): add y").expect("changes");
    let fixes = message
        .find("Review fixes:\n- Refuse approve")
        .expect("fixes");
    assert!(changes < fixes);
    assert!(!message.contains("wip:"));
}

#[test]
fn a_job_between_tasks_at_the_upgrade_keeps_every_task() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let (base, tip) = commit_subjects(
        &fx,
        &[
            "feat(x): add the endpoint",
            "wip: task 2 Show the button",
            "wip: task 3 Add the docs",
        ],
    );
    write_note(&fx, "task-3.md", "Add the docs", "Write the guide");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request(&base, &tip, &[], true))
        .expect("message");
    let second = message
        .find("Show the button:\n\n")
        .expect("title-only task 2");
    let third = message
        .find("Add the docs:\n- Write the guide")
        .expect("task 3");
    let changes = message
        .find("Changes:\n- feat(x): add the endpoint")
        .expect("changes");
    assert!(second < third && third < changes);
    assert!(!message.contains("wip:"));
}

#[test]
fn an_old_recovered_batch_gets_task_sections() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let (base, tip) = commit_subjects(
        &fx,
        &[
            "wip: task 1 Add the endpoint",
            "wip: task 2 Show the button",
        ],
    );
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request(&base, &tip, &[], true))
        .expect("message");
    let want = "feat(retry): retry a parked job\n\n\
Let a user retry a parked job.\n\n\
Add the endpoint:\n\n\
Show the button:\n\n\
Factory-Job: 001";
    assert_eq!(message, want);
}

#[test]
fn a_new_style_job_message_is_unchanged_by_its_wip_commits() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let (base, tip) = commit_subjects(
        &fx,
        &[
            "wip: task 1 Add the endpoint",
            "chore(factory): sync main into job 001 demo",
            "wip: fix round 2",
        ],
    );
    write_note(&fx, "task-1.md", "Add the endpoint", "Add POST retry");
    write_note(&fx, "round-2.md", "Address review", "Refuse approve");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request(&base, &tip, &[], true))
        .expect("message");
    let want = "feat(retry): retry a parked job\n\n\
Let a user retry a parked job.\n\n\
Add the endpoint:\n- Add POST retry\n\n\
Review fixes:\n- Refuse approve\n\n\
Factory-Job: 001";
    assert_eq!(message, want);
}

#[test]
fn a_job_without_issues_gets_only_the_job_footer() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[], true))
        .expect("message");
    assert!(message.ends_with("\n\nFactory-Job: 001"));
    assert!(!message.contains("Fixes"));
    assert!(!message.contains("Relates to"));
}

#[test]
fn the_first_plan_of_a_series_relates() {
    let fx = Fixture::new();
    write_plan(&fx, PLAN);
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    let message = engine
        .job_message(&request("", "", &[142], false))
        .expect("message");
    assert!(message.contains("Relates to: #142"));
    assert!(!message.contains("Fixes:"));
}
