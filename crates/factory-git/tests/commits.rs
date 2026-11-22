mod common;

use common::*;
use factory_git::{GitError, JobRef, MergeOutcome, Repo, Worktree};
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
fn commits_above_lists_commits_oldest_first_with_their_files() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "feat(x): add a");
    commit_files(
        &inside(&wt),
        &[("b.txt", "b\n"), ("c.txt", "c\n")],
        "feat(x): add b and c",
    );
    let tip = repo.tip(&wt.branch).expect("tip");

    let commits = repo.commits_above(&wt.base, &tip).expect("commits");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "feat(x): add a");
    assert_eq!(commits[0].files, vec!["a.txt"]);
    assert_eq!(commits[1].subject, "feat(x): add b and c");
    assert_eq!(commits[1].files, vec!["b.txt", "c.txt"]);
    assert_eq!(commits[1].sha, tip);
}

#[test]
fn commits_above_is_empty_when_floor_equals_tip() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    assert!(repo
        .commits_above(&wt.base, &wt.base)
        .expect("commits")
        .is_empty());
}

#[test]
fn commit_staged_commits_the_index_and_returns_the_new_tip() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("write");
    repo.stage_all(&wt).expect("stage");

    let sha = repo
        .commit_staged(&wt, "chore: add new\n\nbody line")
        .expect("commit")
        .sha;
    assert_eq!(sha, repo.tip(&wt.branch).expect("tip"));
    let commits = repo.commits_above(&wt.base, &sha).expect("commits");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].files, vec!["new.txt"]);
    let message = inside(&wt)
        .head()
        .expect("head")
        .peel_to_commit()
        .expect("commit")
        .message()
        .map(str::to_string);
    assert_eq!(message.as_deref(), Some("chore: add new\n\nbody line"));
    assert!(repo.worktree_is_clean(&wt).expect("clean"));
}

#[test]
fn commit_staged_with_nothing_staged_is_refused() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    let before = repo.tip(&wt.branch).expect("tip");
    assert!(matches!(
        repo.commit_staged(&wt, "chore: nothing"),
        Err(GitError::Refused(_))
    ));
    assert_eq!(repo.tip(&wt.branch).expect("tip"), before);
}

/// Stages one file in a fresh job worktree, with signing on through `settings`.
fn staged_job(fx: &Fixture, settings: &[(&str, &str)]) -> (Repo, Worktree) {
    signing_on(&fx.git(), settings);
    let (repo, wt) = job_worktree(fx);
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("write");
    repo.stage_all(&wt).expect("stage");
    (repo, wt)
}

fn gpg_path(fx: &Fixture) -> String {
    fake_gpg(fx.dir.path()).to_str().expect("utf8").to_string()
}

/// A fake ssh-keygen that records its arguments and writes `<buffer file>.sig`.
fn fake_keygen(fx: &Fixture) -> String {
    script(
        fx.dir.path(),
        "fake-keygen",
        "echo \"$@\" > \"$(dirname \"$0\")/args\"\nfor last; do :; done\nprintf 'SSHSIG\\n' > \"$last.sig\"",
    )
    .to_str()
    .expect("utf8")
    .to_string()
}

#[test]
fn a_snapshot_is_signed_when_gpgsign_is_on() {
    let fx = Fixture::new();
    let gpg = gpg_path(&fx);
    let (repo, wt) = staged_job(&fx, &[("gpg.program", &gpg), ("user.signingkey", "ABC")]);
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    let oid = git2::Oid::from_str(&done.sha).expect("oid");
    let (sig, data) = fx.git().extract_signature(&oid, None).expect("signature");
    assert_eq!(sig.as_str().expect("utf8"), FAKE_SIGNATURE);
    assert!(data.as_str().expect("utf8").contains("wip: task 1"));
}

#[test]
fn the_openpgp_program_gets_no_prompt_flags() {
    let fx = Fixture::new();
    let gpg = gpg_path(&fx);
    let (repo, wt) = staged_job(&fx, &[("gpg.program", &gpg), ("user.signingkey", "ABC")]);
    repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(
        recorded_args(fx.dir.path()),
        "--status-fd=2 --pinentry-mode error -bsau ABC"
    );
}

#[test]
fn a_failing_signer_commits_unsigned_with_a_reason() {
    let fx = Fixture::new();
    let failing = script(fx.dir.path(), "fail-gpg", "exit 2");
    let program = failing.to_str().expect("utf8");
    let (repo, wt) = staged_job(&fx, &[("gpg.program", program), ("user.signingkey", "ABC")]);
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    let reason = done.unsigned.expect("unsigned reason");
    assert!(reason.contains("exited with 2"), "{reason}");
    assert_eq!(signature_of(&fx.git(), &done.sha), None);
    assert_eq!(repo.tip(&wt.branch).expect("tip"), done.sha);
}

#[test]
fn a_signed_snapshot_moves_the_branch() {
    let fx = Fixture::new();
    let gpg = gpg_path(&fx);
    let (repo, wt) = staged_job(&fx, &[("gpg.program", &gpg), ("user.signingkey", "ABC")]);
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(repo.tip(&wt.branch).expect("tip"), done.sha);
    assert!(repo.worktree_is_clean(&wt).expect("clean"));
}

#[test]
fn no_gpgsign_commits_as_before() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("write");
    repo.stage_all(&wt).expect("stage");
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    assert_eq!(signature_of(&fx.git(), &done.sha), None);
}

#[test]
fn ssh_signing_passes_the_key_file() {
    let fx = Fixture::new();
    let program = fake_keygen(&fx);
    let (repo, wt) = staged_job(
        &fx,
        &[
            ("gpg.format", "ssh"),
            ("gpg.ssh.program", &program),
            ("user.signingkey", "/k/id"),
        ],
    );
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    assert_eq!(
        signature_of(&fx.git(), &done.sha).as_deref(),
        Some("SSHSIG")
    );
    let args = recorded_args(fx.dir.path());
    assert!(args.starts_with("-Y sign -n git -f /k/id "), "{args}");
    assert!(!args.contains("-U"), "{args}");
}

#[test]
fn ssh_literal_key_uses_the_agent() {
    let fx = Fixture::new();
    let program = fake_keygen(&fx);
    let (repo, wt) = staged_job(
        &fx,
        &[
            ("gpg.format", "ssh"),
            ("gpg.ssh.program", &program),
            ("user.signingkey", "key::ssh-ed25519 AAAA x"),
        ],
    );
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    let args: Vec<String> = recorded_args(fx.dir.path())
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let agent = args.iter().position(|a| a == "-U").expect("-U flag");
    assert_eq!(
        agent + 2,
        args.len(),
        "-U comes just before the buffer file"
    );
}

#[test]
fn x509_uses_gpgsm_settings() {
    let fx = Fixture::new();
    let gpg = gpg_path(&fx);
    let (repo, wt) = staged_job(
        &fx,
        &[
            ("gpg.format", "x509"),
            ("gpg.x509.program", &gpg),
            ("user.signingkey", "ABC"),
        ],
    );
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    assert!(fx.dir.path().join("args").exists());
    assert_eq!(
        signature_of(&fx.git(), &done.sha).as_deref(),
        Some(FAKE_SIGNATURE)
    );
}

#[test]
fn commits_above_skips_a_sync_merge_and_the_commits_it_brought_in() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "feat(x): add a");
    commit_to_branch(&fx.git(), "main", &[("m.txt", "m\n")], "feat(main): add m");
    let outcome = repo.merge_main_into(&wt, "chore: sync main").expect("sync");
    assert!(matches!(outcome, MergeOutcome::Merged { .. }));
    let tip = repo.tip(&wt.branch).expect("tip");

    let commits = repo.commits_above(&wt.base, &tip).expect("commits");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "feat(x): add a");
    assert_eq!(commits[0].files, vec!["a.txt"]);
}

#[test]
fn commits_above_lists_the_rewritten_commit_after_an_amend() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "feat(x): add a");
    commit_files(&inside(&wt), &[("b.txt", "b\n")], "feat(x): add b");
    let old_tip = repo.tip(&wt.branch).expect("tip");

    let inner = inside(&wt);
    let head = inner
        .head()
        .expect("head")
        .peel_to_commit()
        .expect("commit");
    head.amend(
        Some("HEAD"),
        None,
        None,
        None,
        Some("feat(x): add b, reworded"),
        None,
    )
    .expect("amend");
    let new_tip = repo.tip(&wt.branch).expect("tip");
    assert_ne!(new_tip, old_tip);

    let commits = repo.commits_above(&wt.base, &new_tip).expect("commits");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[1].subject, "feat(x): add b, reworded");
    assert_eq!(commits[1].sha, new_tip);
    assert!(commits.iter().all(|c| c.sha != old_tip));
}

#[test]
fn commits_above_carries_the_full_message() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(
        &inside(&wt),
        &[("a.txt", "a\n")],
        "feat(x): add a\n\nWhy a exists.",
    );
    let tip = repo.tip(&wt.branch).expect("tip");
    let commits = repo.commits_above(&wt.base, &tip).expect("commits");
    assert_eq!(commits[0].subject, "feat(x): add a");
    assert_eq!(commits[0].message, "feat(x): add a\n\nWhy a exists.");
}
