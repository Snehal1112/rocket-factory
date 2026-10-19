use factory_core::BaseTipFiles;
use factory_engine::RepoTip;
use factory_git::Repo;
use git2::{Repository, RepositoryInitOptions, Signature};
use std::path::Path;

#[test]
fn repo_tip_returns_the_committed_text_not_the_working_tree() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("repo");
    std::fs::create_dir_all(path.join(".factory/agents")).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("v-4.0.0");
    let git = Repository::init_opts(&path, &opts).expect("init");
    std::fs::write(
        path.join(".factory/override.toml"),
        "risk_paths = [\"a/**\"]\n",
    )
    .expect("write");
    std::fs::write(
        path.join(".factory/agents/factory-builder.md"),
        "committed\n",
    )
    .expect("write");
    let mut index = git.index().expect("index");
    index
        .add_path(Path::new(".factory/override.toml"))
        .expect("add");
    index
        .add_path(Path::new(".factory/agents/factory-builder.md"))
        .expect("add");
    index.write().expect("write index");
    let tree = git
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = Signature::now("t", "t@t").expect("sig");
    git.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .expect("commit");

    std::fs::write(path.join(".factory/override.toml"), "risk_paths = []\n").expect("write");
    std::fs::write(path.join(".factory/agents/factory-builder.md"), "edited\n").expect("write");

    let repo = Repo::open(&path).expect("open").with_base_branch("v-4.0.0");
    let tip = RepoTip(&repo);
    assert_eq!(
        tip.read_file(".factory/override.toml")
            .expect("read")
            .as_deref(),
        Some("risk_paths = [\"a/**\"]\n")
    );
    assert_eq!(
        tip.list_dir(".factory/agents").expect("list"),
        vec![("factory-builder.md".to_string(), "committed\n".to_string())]
    );
}
