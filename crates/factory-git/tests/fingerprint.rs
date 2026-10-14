mod common;
use common::{commit_files, Fixture};

fn print(fx: &Fixture) -> String {
    fx.repo().checkout_fingerprint().expect("fingerprint")
}

#[test]
fn an_unchanged_checkout_has_a_stable_fingerprint() {
    let fx = Fixture::new();
    assert_eq!(print(&fx), print(&fx));
}

#[test]
fn a_new_untracked_file_changes_it() {
    let fx = Fixture::new();
    let before = print(&fx);
    std::fs::write(fx.path.join("new.txt"), "x").expect("write");
    assert_ne!(before, print(&fx));
}

#[test]
fn editing_a_tracked_file_changes_it() {
    let fx = Fixture::new();
    let before = print(&fx);
    std::fs::write(fx.path.join("base.txt"), "edited\n").expect("write");
    assert_ne!(before, print(&fx));
}

#[test]
fn editing_an_already_dirty_file_changes_it() {
    let fx = Fixture::new();
    std::fs::write(fx.path.join("base.txt"), "first edit\n").expect("write");
    let before = print(&fx);
    std::fs::write(fx.path.join("base.txt"), "second edit\n").expect("write");
    assert_ne!(before, print(&fx));
}

#[test]
fn an_ignored_file_does_not_change_it() {
    let fx = Fixture::new();
    commit_files(&fx.git(), &[(".gitignore", "ignored.txt\n")], "ignore");
    let before = print(&fx);
    std::fs::write(fx.path.join("ignored.txt"), "x").expect("write");
    assert_eq!(before, print(&fx));
}

#[test]
fn a_new_commit_changes_it() {
    let fx = Fixture::new();
    let before = print(&fx);
    commit_files(&fx.git(), &[("more.txt", "m")], "more");
    assert_ne!(before, print(&fx));
}

#[cfg(unix)]
#[test]
fn retargeting_a_symlink_changes_it() {
    let fx = Fixture::new();
    let link = fx.path.join("link");
    std::os::unix::fs::symlink("one", &link).expect("symlink");
    let before = print(&fx);
    std::fs::remove_file(&link).expect("remove");
    std::os::unix::fs::symlink("two", &link).expect("symlink");
    assert_ne!(before, print(&fx));
}

#[test]
fn deleting_another_branch_changes_it() {
    let fx = Fixture::new();
    fx.job_branch("job/x", &[("x.txt", "x")]);
    let before = print(&fx);
    fx.git().find_branch("job/x", git2::BranchType::Local).expect("branch").delete().expect("delete");
    assert_ne!(before, print(&fx));
}

#[test]
fn creating_a_tag_changes_it() {
    let fx = Fixture::new();
    let before = print(&fx);
    let git = fx.git();
    let head = git.head().expect("head").peel_to_commit().expect("commit");
    git.tag_lightweight("v1", head.as_object(), false).expect("tag");
    assert_ne!(before, print(&fx));
}

#[test]
fn switching_to_a_new_branch_at_the_same_commit_changes_it() {
    let fx = Fixture::new();
    let before = print(&fx);
    let git = fx.git();
    let head = git.head().expect("head").peel_to_commit().expect("commit");
    git.branch("other", &head, false).expect("branch");
    git.set_head("refs/heads/other").expect("set head");
    assert_ne!(before, print(&fx));
}
