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

#[test]
fn a_path_through_a_file_is_absent() {
    let fx = fixture_with_factory_files();
    let repo = fx.repo();
    // libgit2 reports NotFound (class Tree) for a child of a blob, so it is plainly absent.
    assert_eq!(repo.file_at_branch("v-4.0.0", "base.txt/x").expect("absent"), None);
    assert!(repo.dir_at_branch("v-4.0.0", "base.txt/x").expect("absent").is_empty());
}

#[test]
fn a_missing_object_is_an_error_not_an_absent_file() {
    let fx = fixture_with_factory_files();
    let git = fx.git();
    let tree = git.find_branch("v-4.0.0", git2::BranchType::Local).expect("b").get().peel_to_tree().expect("tree");
    let sub = tree.get_name(".factory").expect(".factory entry").id().to_string();
    drop(tree);
    drop(git);
    // Simulate corruption: delete the loose object of the .factory folder.
    let obj = fx.path.join(".git/objects").join(&sub[..2]).join(&sub[2..]);
    assert!(obj.exists(), "loose object expected at {}", obj.display());
    std::fs::remove_file(&obj).expect("remove object");
    let repo = fx.repo();
    assert!(repo.file_at_branch("v-4.0.0", ".factory/override.toml").is_err());
    assert!(repo.dir_at_branch("v-4.0.0", ".factory/agents").is_err());
    // A path that is plainly absent from the root tree is still just absent.
    assert_eq!(repo.file_at_branch("v-4.0.0", "nothing-here").expect("absent"), None);
    assert!(repo.dir_at_branch("v-4.0.0", "no/such").expect("absent").is_empty());
}

/// Commits a symlink blob onto a branch without a working tree.
fn commit_symlink(fx: &Fixture, branch: &str, dir: &str, name: &str) {
    let git = fx.git();
    let parent = git.find_branch(branch, git2::BranchType::Local).expect("b").get().peel_to_commit().expect("c");
    let blob = git.blob(b"/etc/passwd").expect("blob");
    let mut inner = git.treebuilder(None).expect("tb");
    inner.insert(name, blob, 0o120000).expect("insert");
    let inner_id = inner.write().expect("write");
    let mut root = git.treebuilder(Some(&parent.tree().expect("tree"))).expect("root");
    root.insert(dir, inner_id, 0o040000).expect("dir");
    let tree = git.find_tree(root.write().expect("tree")).expect("find");
    let refname = format!("refs/heads/{branch}");
    git.commit(Some(&refname), &sig(), &sig(), "link", &tree, &[&parent]).expect("commit");
}

#[test]
fn a_symlink_is_refused_by_file_at_branch() {
    let fx = Fixture::on_branch("main");
    commit_symlink(&fx, "main", ".factory", "override.toml");
    let err = fx.repo().file_at_branch("main", ".factory/override.toml").expect_err("symlink");
    assert!(err.to_string().contains("not a regular file"), "{err}");
}

#[test]
fn a_symlink_in_a_listed_folder_is_an_error() {
    let fx = Fixture::on_branch("main");
    commit_symlink(&fx, "main", ".factory", "factory-x.md");
    let err = fx.repo().dir_at_branch("main", ".factory").expect_err("symlink");
    assert!(err.to_string().contains("not a regular file"), "{err}");
}

#[test]
fn an_executable_file_is_still_read() {
    let fx = Fixture::on_branch("main");
    let git = fx.git();
    let parent = git.head().expect("head").peel_to_commit().expect("c");
    let blob = git.blob(b"run").expect("blob");
    let mut tb = git.treebuilder(Some(&parent.tree().expect("tree"))).expect("tb");
    tb.insert("tool.sh", blob, 0o100755).expect("insert");
    let tree = git.find_tree(tb.write().expect("w")).expect("find");
    git.commit(Some("refs/heads/main"), &sig(), &sig(), "exe", &tree, &[&parent]).expect("commit");
    assert_eq!(fx.repo().file_at_branch("main", "tool.sh").expect("read").as_deref(), Some("run"));
}
