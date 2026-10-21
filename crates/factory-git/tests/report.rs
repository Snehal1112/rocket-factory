mod common;

use common::*;
use factory_git::{JobRef, Repo, Worktree};
use git2::Repository;

fn job_worktree(fx: &Fixture) -> (Repo, Worktree) {
    let repo = fx.repo();
    let wt = repo
        .create_worktree(&JobRef {
            id: "001".into(),
            slug: "demo".into(),
            project: None,
        })
        .expect("create worktree");
    (repo, wt)
}

fn inside(wt: &Worktree) -> Repository {
    Repository::open(&wt.path).expect("open worktree")
}

#[test]
fn staged_report_describes_the_staged_change() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("keep.txt", "one\n")], "feat(x): add keep");
    std::fs::write(wt.path.join("keep.txt"), "one\ntwo\n").expect("modify");
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("new");
    repo.stage_all(&wt).expect("stage");

    let report = repo.staged_report(&wt, 5).expect("report");
    assert!(
        report.status.starts_with("On branch factory/001-demo\n"),
        "status: {}",
        report.status
    );
    assert!(
        report.status.contains("Changes to be committed:"),
        "status: {}",
        report.status
    );
    assert!(
        report.status.contains("\tmodified:   keep.txt\n"),
        "status: {}",
        report.status
    );
    assert!(
        report.status.contains("\tnew file:   new.txt\n"),
        "status: {}",
        report.status
    );
    assert!(report.stat.contains("keep.txt"), "stat: {}", report.stat);
    assert!(
        report.diff.contains("+++ b/keep.txt"),
        "diff: {}",
        report.diff
    );
    assert!(report.diff.contains("+two\n"), "diff: {}", report.diff);
    assert!(
        report.log[0].ends_with("feat(x): add keep"),
        "log: {:?}",
        report.log
    );
}

#[test]
fn staged_report_log_is_newest_first_and_capped() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "feat(x): one");
    commit_files(&inside(&wt), &[("b.txt", "b\n")], "feat(x): two");

    let report = repo.staged_report(&wt, 2).expect("report");
    assert_eq!(report.log.len(), 2);
    assert!(report.log[0].ends_with("feat(x): two"));
    assert!(report.log[1].ends_with("feat(x): one"));
}

#[test]
fn staged_report_with_nothing_staged_has_an_empty_diff() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    let report = repo.staged_report(&wt, 5).expect("report");
    assert!(
        report.status.contains("nothing to commit"),
        "status: {}",
        report.status
    );
    assert!(report.diff.is_empty());
    assert!(report.stat.is_empty());
}

#[test]
fn staged_report_shows_a_deletion() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::remove_file(wt.path.join("base.txt")).expect("delete");
    repo.stage_all(&wt).expect("stage");

    let report = repo.staged_report(&wt, 5).expect("report");
    assert!(
        report.status.contains("\tdeleted:    base.txt\n"),
        "status: {}",
        report.status
    );
    assert!(report.diff.contains("-base\n"), "diff: {}", report.diff);
}
