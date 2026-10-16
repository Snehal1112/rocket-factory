mod common;

use common::*;
use git2::Oid;

fn fixture_with_factory_files() -> Fixture {
    let fx = Fixture::on_branch("v-4.0.0");
    let git = fx.git();
    let workdir = fx.path.clone();
    std::fs::create_dir_all(workdir.join(".factory/agents")).expect("mkdir");
    commit_files(
        &git,
        &[
            (".factory/override.toml", "risk_paths = [\"a/**\"]\n"),
            (".factory/agents/factory-builder.md", "committed builder\n"),
        ],
        "factory files",
    );
    fx
}

#[test]
fn file_at_branch_reads_the_committed_text_and_none_when_missing() {
    let fx = fixture_with_factory_files();
    let repo = fx.repo();
    assert_eq!(
        repo.file_at_branch("v-4.0.0", ".factory/override.toml").expect("read").as_deref(),
        Some("risk_paths = [\"a/**\"]\n")
    );
    assert_eq!(repo.file_at_branch("v-4.0.0", ".factory/nope.toml").expect("read"), None);
}

#[test]
fn uncommitted_working_tree_changes_are_not_seen() {
    let fx = fixture_with_factory_files();
    std::fs::write(fx.path.join(".factory/override.toml"), "risk_paths = []\n").expect("write");
    std::fs::write(fx.path.join(".factory/agents/factory-builder.md"), "edited\n").expect("write");
    std::fs::write(fx.path.join(".factory/agents/new.md"), "untracked\n").expect("write");
    let repo = fx.repo();
    assert_eq!(
        repo.file_at_branch("v-4.0.0", ".factory/override.toml").expect("read").as_deref(),
        Some("risk_paths = [\"a/**\"]\n")
    );
    let dir = repo.dir_at_branch("v-4.0.0", ".factory/agents").expect("dir");
    assert_eq!(dir, vec![("factory-builder.md".to_string(), "committed builder\n".to_string())]);
}

#[test]
fn a_file_committed_only_on_a_job_branch_is_not_seen_from_the_base() {
    let fx = Fixture::on_branch("v-4.0.0");
    let git = fx.git();
    let tip = git.find_commit(Oid::from_str(&fx.branch_tip("v-4.0.0")).expect("oid")).expect("c");
    git.branch("job", &tip, false).expect("branch");
    commit_to_branch(&git, "job", &[("override.toml", "evil\n")], "job file");
    let repo = fx.repo();
    assert_eq!(repo.file_at_branch("job", "override.toml").expect("read").as_deref(), Some("evil\n"));
    assert_eq!(repo.file_at_branch("v-4.0.0", "override.toml").expect("read"), None);
}

#[test]
fn dir_at_branch_lists_files_and_is_empty_for_an_absent_folder() {
    let fx = fixture_with_factory_files();
    let repo = fx.repo();
    let dir = repo.dir_at_branch("v-4.0.0", ".factory/agents").expect("dir");
    assert_eq!(dir, vec![("factory-builder.md".to_string(), "committed builder\n".to_string())]);
    assert!(repo.dir_at_branch("v-4.0.0", "no/such").expect("dir").is_empty());
}

#[test]
fn a_missing_branch_is_an_error() {
    let fx = Fixture::on_branch("v-4.0.0");
    let repo = fx.repo();
    assert!(repo.file_at_branch("nope", "base.txt").is_err());
    assert!(repo.dir_at_branch("nope", "x").is_err());
}
