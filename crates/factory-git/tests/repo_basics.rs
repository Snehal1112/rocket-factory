mod common;

use common::*;
use factory_git::{GitError, Repo};

#[test]
fn open_reports_the_checkout_root_from_a_subdirectory() {
    let fx = Fixture::new();
    let sub = fx.path.join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir");
    let repo = Repo::open(&sub).expect("open");
    let want = std::fs::canonicalize(&fx.path).expect("canon");
    let got = std::fs::canonicalize(repo.root()).expect("canon");
    assert_eq!(got, want);
}

#[test]
fn tip_returns_the_branch_sha_and_not_found_for_a_missing_branch() {
    let fx = Fixture::new();
    let repo = fx.repo();
    assert_eq!(repo.tip("main").expect("tip"), fx.main_sha());
    assert!(matches!(repo.tip("nope"), Err(GitError::NotFound(_))));
}

#[test]
fn changed_paths_lists_added_and_modified_files_sorted() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("b.txt", "new\n"), ("base.txt", "base\nmore\n")]);
    let paths = fx.repo().changed_paths(&base, &head).expect("paths");
    assert_eq!(paths, vec!["b.txt".to_string(), "base.txt".to_string()]);
}

#[test]
fn diff_to_files_writes_diff_stat_and_names_files() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("b.txt", "new\n"), ("base.txt", "base\nmore\n")]);
    let out = fx.dir.path().join("out");
    let files = fx.repo().diff_to_files(&base, &head, &out).expect("diff");
    let diff = std::fs::read_to_string(&files.diff).expect("diff file");
    assert!(diff.contains("+++ b/b.txt"), "diff was: {diff}");
    assert!(diff.contains("+new"), "diff was: {diff}");
    assert!(diff.contains("+more"), "diff was: {diff}");
    let stat = std::fs::read_to_string(&files.stat).expect("stat file");
    assert!(stat.contains("2 files changed"), "stat was: {stat}");
    let names = std::fs::read_to_string(&files.names).expect("names file");
    assert_eq!(names, "b.txt\nbase.txt\n");
    assert_eq!((files.files, files.insertions, files.deletions), (2, 2, 0));
    assert_eq!(files.changed_lines(), 2);
    let file_name = files.diff.file_name().expect("name").to_string_lossy().into_owned();
    assert!(file_name.starts_with("review-") && file_name.ends_with(".diff"), "name was {file_name}");
}

#[test]
fn diff_counts_reports_files_and_changed_lines_without_writing_files() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("b.txt", "new\n"), ("base.txt", "base\nmore\n")]);
    let counts = fx.repo().diff_counts(&base, &head).expect("counts");
    assert_eq!((counts.files, counts.insertions, counts.deletions), (2, 2, 0));
    let out = fx.dir.path().join("out");
    assert!(!out.exists(), "diff_counts must not write anything");
}

#[test]
fn diff_counts_of_identical_commits_is_zero_and_of_an_unknown_commit_is_an_error() {
    let fx = Fixture::new();
    let tip = fx.main_sha();
    let counts = fx.repo().diff_counts(&tip, &tip).expect("counts");
    assert_eq!((counts.files, counts.insertions, counts.deletions), (0, 0, 0));
    assert!(fx.repo().diff_counts("0000000000000000000000000000000000000000", &tip).is_err());
}

#[test]
fn diff_to_files_of_identical_commits_writes_empty_files() {
    let fx = Fixture::new();
    let sha = fx.main_sha();
    let out = fx.dir.path().join("out");
    let files = fx.repo().diff_to_files(&sha, &sha, &out).expect("diff");
    assert_eq!(files.files, 0);
    assert_eq!(std::fs::read_to_string(&files.names).expect("names"), "");
    assert_eq!(std::fs::read_to_string(&files.diff).expect("diff"), "");
}

#[test]
fn log_oneline_lists_new_commits_oldest_first() {
    let fx = Fixture::new();
    let (base, first) = fx.job_branch("factory/001-x", &[("b.txt", "one\n")]);
    let second = commit_to_branch(&fx.git(), "factory/001-x", &[("c.txt", "two\n")], "second work");
    let lines = fx.repo().log_oneline(&base, &second.to_string()).expect("log");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], format!("{} job work", &first[..7]));
    assert!(lines[1].ends_with(" second work"), "lines were {lines:?}");
    assert!(fx.repo().log_oneline(&second.to_string(), &second.to_string()).expect("empty").is_empty());
}

#[test]
fn is_ancestor_handles_ancestor_equal_and_sibling() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("a.txt", "a\n")]);
    let (_, other) = fx.job_branch("factory/002-y", &[("c.txt", "c\n")]);
    let repo = fx.repo();
    assert!(repo.is_ancestor(&base, &head).expect("a"));
    assert!(repo.is_ancestor(&head, &head).expect("eq"));
    assert!(!repo.is_ancestor(&head, &base).expect("rev"));
    assert!(!repo.is_ancestor(&head, &other).expect("sibling"));
}

#[test]
fn is_merged_refuses_a_job_with_no_commits_or_no_head() {
    let fx = Fixture::new();
    let sha = fx.main_sha();
    let repo = fx.repo();
    assert!(matches!(repo.is_merged(&sha, &sha), Err(GitError::Refused(_))));
    assert!(matches!(repo.is_merged(&sha, " "), Err(GitError::Refused(_))));
}

#[test]
fn is_merged_follows_main_and_survives_a_deleted_branch() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("a.txt", "a\n")]);
    let repo = fx.repo();
    assert_eq!(repo.is_merged(&base, &head).expect("before"), None);
    // A manual merge: main now points at the job head.
    point_branch_at(&fx.git(), "main", &head);
    assert_eq!(repo.is_merged(&base, &head).expect("after"), Some(head.clone()));
    // Deleting the branch changes nothing, because the check uses shas.
    fx.git()
        .find_branch("factory/001-x", git2::BranchType::Local)
        .expect("branch")
        .delete()
        .expect("delete");
    assert_eq!(repo.is_merged(&base, &head).expect("deleted"), Some(head));
}
