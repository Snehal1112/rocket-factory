mod common;

use common::*;
use factory_git::JobRef;
use git2::{Repository, StatusOptions};

const PATTERN: &str = "/.claude/agents/factory-*.md";

fn untracked(repo: &Repository) -> Vec<String> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let st = repo.statuses(Some(&mut opts)).expect("statuses");
    st.iter()
        .filter_map(|e| e.path().map(str::to_string))
        .collect()
}

fn exclude_text(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.path.join(".git/info/exclude")).expect("exclude file")
}

#[test]
fn creates_the_file_and_is_idempotent() {
    let fx = Fixture::new();
    let _ = std::fs::remove_file(fx.path.join(".git/info/exclude"));
    let repo = fx.repo();
    repo.exclude_paths(&[PATTERN]).expect("first");
    repo.exclude_paths(&[PATTERN]).expect("second");
    assert_eq!(
        exclude_text(&fx).lines().filter(|l| *l == PATTERN).count(),
        1
    );
}

#[test]
fn keeps_existing_content() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.path.join(".git/info")).expect("mkdir");
    std::fs::write(fx.path.join(".git/info/exclude"), "# mine\n*.log").expect("write");
    fx.repo().exclude_paths(&[PATTERN]).expect("add");
    let text = exclude_text(&fx);
    assert!(text.starts_with("# mine\n*.log\n"), "{text:?}");
    assert!(text.contains(PATTERN));
}

#[test]
fn matching_files_are_not_untracked_in_main_or_a_worktree() {
    let fx = Fixture::new();
    let repo = fx.repo();
    repo.exclude_paths(&[PATTERN]).expect("add");
    let wt = repo
        .create_worktree(&JobRef {
            id: "001".into(),
            slug: "demo".into(),
        })
        .expect("worktree");
    for root in [fx.path.clone(), wt.path.clone()] {
        std::fs::create_dir_all(root.join(".claude/agents")).expect("mkdir");
        std::fs::write(root.join(".claude/agents/factory-builder.md"), "x").expect("write");
        std::fs::write(root.join(".claude/agents/other.md"), "x").expect("write");
        let found = untracked(&Repository::open(&root).expect("open"));
        assert!(
            !found.iter().any(|p| p.contains("factory-builder.md")),
            "{found:?}"
        );
        assert!(found.iter().any(|p| p.contains("other.md")), "{found:?}");
    }
}

#[test]
fn is_tracked_at_branch_reads_the_committed_tree() {
    let fx = Fixture::new();
    let git = fx.git();
    commit_files(&git, &[("kept.txt", "k")], "add");
    std::fs::write(fx.path.join("loose.txt"), "l").expect("write");
    let repo = fx.repo();
    assert!(repo
        .is_tracked_at_branch("main", "kept.txt")
        .expect("tracked"));
    assert!(!repo
        .is_tracked_at_branch("main", "loose.txt")
        .expect("loose"));
    assert!(!repo
        .is_tracked_at_branch("main", "absent.txt")
        .expect("absent"));
    assert!(repo.is_tracked_at_branch("nope", "kept.txt").is_err());
}
