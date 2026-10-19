mod common;

use common::*;

#[test]
fn local_branch_names_lists_every_local_branch_sorted() {
    let fx = Fixture::new();
    fx.job_branch("factory/002-b", &[("b.txt", "b\n")]);
    fx.job_branch("factory/001-a", &[("a.txt", "a\n")]);
    let names = fx.repo().local_branch_names().expect("names");
    assert_eq!(
        names,
        vec![
            "factory/001-a".to_string(),
            "factory/002-b".to_string(),
            "main".to_string()
        ]
    );
}

#[test]
fn branch_exists_is_true_only_for_local_branches_that_exist() {
    let fx = Fixture::new();
    fx.job_branch("factory/001-a", &[("a.txt", "a\n")]);
    let repo = fx.repo();
    assert!(repo.branch_exists("factory/001-a"));
    assert!(repo.branch_exists("main"));
    assert!(!repo.branch_exists("factory/002-gone"));
}
