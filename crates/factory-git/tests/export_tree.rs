mod common;

use common::*;
use factory_git::GitError;
use std::path::Path;

#[test]
fn export_tree_writes_tracked_files_only() {
    let fx = Fixture::on_branch("main");
    let git = fx.git();
    std::fs::create_dir_all(fx.path.join("src")).expect("mkdir");
    std::fs::create_dir_all(fx.path.join("docs")).expect("mkdir");
    commit_files(
        &git,
        &[("src/a.txt", "a\n"), ("docs/b.txt", "b\n")],
        "files",
    );
    std::fs::write(fx.path.join(".env"), "SECRET=1\n").expect("write");
    let dest = fx.dir.path().join("export");
    fx.repo().export_tree("main", &dest).expect("export");
    assert_eq!(
        std::fs::read_to_string(dest.join("src/a.txt")).expect("a"),
        "a\n"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("docs/b.txt")).expect("b"),
        "b\n"
    );
    assert!(dest.join("base.txt").is_file());
    assert!(!dest.join(".env").exists());
    assert!(!dest.join(".git").exists());
}

#[test]
fn export_tree_sets_the_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::on_branch("main");
    let git = fx.git();
    let parent = git.head().expect("head").peel_to_commit().expect("c");
    let blob = git.blob(b"run").expect("blob");
    let mut tb = git
        .treebuilder(Some(&parent.tree().expect("tree")))
        .expect("tb");
    tb.insert("tool.sh", blob, 0o100755).expect("insert");
    let tree = git.find_tree(tb.write().expect("w")).expect("find");
    git.commit(
        Some("refs/heads/main"),
        &sig(),
        &sig(),
        "exe",
        &tree,
        &[&parent],
    )
    .expect("commit");
    let dest = fx.dir.path().join("export");
    fx.repo().export_tree("main", &dest).expect("export");
    let mode = std::fs::metadata(dest.join("tool.sh"))
        .expect("meta")
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
    let plain = std::fs::metadata(dest.join("base.txt"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(plain & 0o111, 0);
}

#[test]
fn export_tree_skips_symlinks() {
    let fx = Fixture::on_branch("main");
    let git = fx.git();
    std::os::unix::fs::symlink("/etc/passwd", fx.path.join("link")).expect("symlink");
    let mut index = git.index().expect("index");
    index.add_path(Path::new("link")).expect("add");
    index.write().expect("write index");
    let tree = git
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let parent = git.head().expect("head").peel_to_commit().expect("c");
    git.commit(Some("HEAD"), &sig(), &sig(), "link", &tree, &[&parent])
        .expect("commit");
    let dest = fx.dir.path().join("export");
    fx.repo().export_tree("main", &dest).expect("export");
    assert!(dest.join("base.txt").is_file());
    assert!(dest.join("link").symlink_metadata().is_err());
}

#[test]
fn export_tree_refuses_a_non_empty_destination() {
    let fx = Fixture::on_branch("main");
    let dest = fx.dir.path().join("export");
    std::fs::create_dir_all(&dest).expect("mkdir");
    std::fs::write(dest.join("keep.txt"), "keep\n").expect("write");
    let err = fx.repo().export_tree("main", &dest).expect_err("non-empty");
    assert!(matches!(err, GitError::Refused(_)), "{err}");
    assert_eq!(
        std::fs::read_to_string(dest.join("keep.txt")).expect("keep"),
        "keep\n"
    );
    assert!(!dest.join("base.txt").exists());
}

#[test]
fn export_tree_accepts_an_existing_empty_destination() {
    let fx = Fixture::on_branch("main");
    let dest = fx.dir.path().join("export");
    std::fs::create_dir_all(&dest).expect("mkdir");
    fx.repo().export_tree("main", &dest).expect("export");
    assert!(dest.join("base.txt").is_file());
}

#[test]
fn export_tree_fails_for_an_unknown_branch() {
    let fx = Fixture::on_branch("main");
    let dest = fx.dir.path().join("export");
    assert!(fx.repo().export_tree("nope", &dest).is_err());
}
