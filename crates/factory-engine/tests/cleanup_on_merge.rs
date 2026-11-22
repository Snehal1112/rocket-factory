mod common;
use factory_engine::{merge_stage, MergeResult};
use std::path::PathBuf;

fn ledger(fx: &common::MergeFixture) -> String {
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger")
}

fn branch_exists(fx: &common::MergeFixture, branch: &str) -> bool {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let found = repo.find_branch(branch, git2::BranchType::Local).is_ok();
    found
}

#[test]
fn a_merge_removes_the_worktree_and_branch() {
    let fx = common::approved_job_with_cleanup();
    let job = fx.driver.job(&fx.id).expect("job");
    let worktree = PathBuf::from(job.worktree.clone().expect("worktree"));
    let merged = merge_stage(&fx.driver, &fx.id).expect("merge");
    assert!(matches!(merged, MergeResult::Merged(_)), "{merged:?}");
    assert!(
        !worktree.exists(),
        "worktree still at {}",
        worktree.display()
    );
    assert!(!branch_exists(&fx, &job.branch));
    assert!(fx.driver.job(&fx.id).expect("job").cleaned);
}

#[test]
fn a_dirty_worktree_is_kept_and_reported() {
    let fx = common::approved_job_with_cleanup();
    let job = fx.driver.job(&fx.id).expect("job");
    let worktree = PathBuf::from(job.worktree.clone().expect("worktree"));
    std::fs::write(worktree.join("scratch.txt"), "x").expect("scratch");
    let merged = merge_stage(&fx.driver, &fx.id).expect("merge");
    assert!(matches!(merged, MergeResult::Merged(_)), "{merged:?}");
    assert!(worktree.join("scratch.txt").exists());
    assert!(branch_exists(&fx, &job.branch));
    assert!(!fx.driver.job(&fx.id).expect("job").cleaned);
    let text = ledger(&fx);
    let kept: Vec<&str> = text
        .lines()
        // The ledger writes step lines as rulings.
        .filter(|l| {
            l.trim_start_matches("Ruling: ")
                .starts_with("cleanup kept the worktree:")
        })
        .collect();
    assert_eq!(kept.len(), 1, "{text}");
    assert!(kept[0].contains("--discard-commits"), "{}", kept[0]);
}

#[test]
fn the_setting_off_keeps_the_worktree() {
    let fx = common::approved_job();
    let job = fx.driver.job(&fx.id).expect("job");
    let worktree = PathBuf::from(job.worktree.clone().expect("worktree"));
    let merged = merge_stage(&fx.driver, &fx.id).expect("merge");
    assert!(matches!(merged, MergeResult::Merged(_)), "{merged:?}");
    assert!(worktree.exists());
    assert!(branch_exists(&fx, &job.branch));
    assert!(!ledger(&fx).contains("cleanup kept"));
}
