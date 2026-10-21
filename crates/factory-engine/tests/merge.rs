mod common;
use factory_engine::{merge_stage, MergeResult};

#[test]
fn merges_the_reviewed_head_with_the_fixed_message() {
    let fx = common::approved_job();
    let before = common::main_tip(&fx.main);
    let MergeResult::Merged(sha) = merge_stage(&fx.driver, &fx.id).expect("merge") else {
        panic!("expected a merge");
    };
    assert_ne!(sha, before);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let commit = repo
        .find_commit(git2::Oid::from_str(&sha).expect("oid"))
        .expect("commit");
    assert_eq!(
        commit.message().expect("msg").lines().next(),
        Some("chore(factory): merge job 001 hello")
    );
    assert_eq!(
        commit.parent_id(1).expect("second parent").to_string(),
        fx.head
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Merged);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains(&format!("Task 1: complete (merge {sha})")));
}

#[test]
fn a_blocking_untracked_file_parks_the_job_and_leaves_main_alone() {
    let fx = common::approved_job();
    // The reviewed change adds docs/hello.txt, so an untracked file at that path blocks the merge.
    std::fs::create_dir_all(fx.main.join("docs")).expect("mkdir");
    std::fs::write(fx.main.join("docs/hello.txt"), "mine").expect("write");
    let before = common::main_tip(&fx.main);
    let MergeResult::Parked(reason) = merge_stage(&fx.driver, &fx.id).expect("stage") else {
        panic!("expected a park");
    };
    assert!(!reason.is_empty());
    assert_eq!(common::main_tip(&fx.main), before);
    assert_eq!(
        std::fs::read_to_string(fx.main.join("docs/hello.txt")).expect("read"),
        "mine"
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(job.reason.as_deref(), Some(reason.as_str()));
}

#[test]
fn merge_refuses_an_out_of_scope_head() {
    let fx = common::approved_scoped_job(&["apps/web/ok.txt", "README.md"]);
    let before = common::main_tip(&fx.main);
    let MergeResult::Parked(reason) = merge_stage(&fx.driver, &fx.id).expect("stage") else {
        panic!("expected a park");
    };
    assert_eq!(reason, "the job changed files outside apps/web: README.md");
    assert_eq!(common::main_tip(&fx.main), before);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
}

#[test]
fn an_in_scope_head_merges() {
    let fx = common::approved_scoped_job(&["apps/web/ok.txt"]);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("stage"),
        MergeResult::Merged(_)
    ));
}

#[test]
fn a_moved_branch_tip_parks_without_merging() {
    let fx = common::approved_job();
    common::commit_file(&common::worktree_of(&fx), "late.txt", "x");
    let before = common::main_tip(&fx.main);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("stage"),
        MergeResult::Parked(_)
    ));
    assert_eq!(common::main_tip(&fx.main), before);
}

#[test]
fn a_dirty_main_parks_as_main_busy() {
    let fx = common::approved_job();
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.starts_with("main busy")),
        "got {r:?}"
    );
}

#[test]
fn main_moved_triggers_a_sync_then_merges() {
    let fx = common::approved_job();
    common::commit_file(&fx.main, "other.md", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
}

#[test]
fn another_projects_merge_is_not_out_of_scope_after_a_sync() {
    let fx = common::approved_scoped_job(&["apps/web/ok.md"]);
    // Project api merges into main after the web job forked.
    common::commit_file(&fx.main, "apps/api/merged.txt", "api\n");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
    // The synced head is still in scope.
    let job = fx.driver.job(&fx.id).expect("job");
    let head = job.head.expect("head");
    assert_eq!(
        factory_engine::scope_violation(fx.driver.repo(), &head).expect("scope"),
        None
    );
}

#[test]
fn a_sync_conflict_parks() {
    let fx = common::approved_job();
    let wt = common::worktree_of(&fx);
    common::commit_file(&wt, "same.txt", "branch");
    // Re-review is out of scope here: record the new head as the reviewed head.
    common::record_head(&fx, &common::branch_tip(&fx));
    common::commit_file(&fx.main, "same.txt", "main");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("conflict")),
        "got {r:?}"
    );
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").stage,
        factory_core::Stage::NeedsHuman
    );
}

#[test]
fn a_third_sync_is_refused() {
    let fx = common::approved_job();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = 2;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.txt", "o");
    let before = common::main_tip(&fx.main);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("syncs")),
        "got {r:?}"
    );
    assert_eq!(common::main_tip(&fx.main), before);
}

#[test]
fn park_on_an_unparked_job_writes_an_other_record() {
    let fx = common::approved_job();
    factory_engine::park(&fx.driver, &fx.id, "main busy").expect("park");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(job.park_kind.as_deref(), Some("other"));
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::Other);
    assert_eq!(rec.reason, "main busy");
}

#[test]
fn park_keeps_the_record_of_a_job_that_is_already_parked() {
    let fx = common::approved_job();
    let job = fx.driver.job(&fx.id).expect("job");
    let dir = fx.driver.job_dir(&job);
    // This is what a failed check inside the sync leaves behind.
    let part = factory_core::ParkPart::new("Checks", "checks.md", "FAIL cargo check");
    let rec = factory_core::ParkRecord::new(
        factory_core::ParkKind::ChecksFailed,
        "checks failed",
        &fx.id,
        job.round,
        7,
        vec![part],
    );
    rec.write(&dir).expect("write record");
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.park_kind = Some("checks-failed".to_string());
            j.park_at = Some(7);
            j.set_stage(
                factory_core::Stage::NeedsHuman,
                Some("checks failed"),
                false,
                7.0,
            )
        })
        .expect("park job");
    factory_engine::park(&fx.driver, &fx.id, "after sync: checks failed").expect("park");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("checks-failed"));
    assert_eq!(job.park_at, Some(7));
    assert_eq!(job.reason.as_deref(), Some("after sync: checks failed"));
    let rec = factory_core::ParkRecord::read(&dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::ChecksFailed);
    assert_eq!(rec.parts.len(), 1);
}

#[test]
fn a_base_off_the_base_branch_parks_with_the_branch_name() {
    let config = factory_core::Config {
        base_branch: "trunk".to_string(),
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    // The branch trunk has its own root commit, so the job base is not on it.
    let repo = git2::Repository::open(&fx.main).expect("open");
    let sig = git2::Signature::now("t", "t@example.com").expect("sig");
    let tree = repo
        .find_tree(repo.treebuilder(None).expect("tb").write().expect("tree"))
        .expect("find");
    let root = repo
        .commit(None, &sig, &sig, "root", &tree, &[])
        .expect("commit");
    repo.branch("trunk", &repo.find_commit(root).expect("commit"), true)
        .expect("branch");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("not on trunk")),
        "got {r:?}"
    );
}

/// A fixture whose base is `trunk` while a local branch `main` also exists. The checkout sits on trunk.
fn trunk_fixture() -> common::MergeFixture {
    let config = factory_core::Config {
        base_branch: "trunk".to_string(),
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    let repo = git2::Repository::open(&fx.main).expect("open");
    repo.set_head("refs/heads/trunk").expect("head on trunk");
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .expect("checkout");
    fx
}

fn tip_of(fx: &common::MergeFixture, branch: &str) -> String {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let id = repo
        .revparse_single(branch)
        .expect("branch")
        .peel_to_commit()
        .expect("commit")
        .id();
    id.to_string()
}

#[test]
fn a_sync_merges_the_base_branch_and_leaves_main_alone() {
    let fx = trunk_fixture();
    let main_before = tip_of(&fx, "main");
    common::commit_file(&fx.main, "other.md", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
    assert_eq!(
        tip_of(&fx, "main"),
        main_before,
        "the branch main is untouched"
    );
    assert!(
        fx.main.join("docs/hello.txt").exists(),
        "the job reached trunk"
    );
}

#[test]
fn the_sync_cap_text_names_the_base_branch() {
    let fx = trunk_fixture();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = 2;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.txt", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.starts_with("trunk moved again after 2 syncs")),
        "got {r:?}"
    );
}

/// Holds the merge lock of the repo at `main` for `hold`, and signals once it is taken.
fn hold_merge_lock(
    main: &std::path::Path,
    hold: std::time::Duration,
) -> (std::thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let main = main.to_path_buf();
    let handle = std::thread::spawn(move || {
        let repo = factory_git::Repo::open(&main).expect("open");
        let _lock =
            factory_engine::lock_repo_merge(&repo, std::time::Duration::ZERO).expect("lock");
        tx.send(()).expect("signal");
        std::thread::sleep(hold);
    });
    (handle, rx)
}

#[test]
fn merge_waits_for_the_repo_lock() {
    let mut fx = common::approved_job();
    fx.driver
        .set_merge_lock_wait(std::time::Duration::from_secs(5));
    let (handle, taken) = hold_merge_lock(&fx.main, std::time::Duration::from_millis(300));
    taken.recv().expect("lock taken");
    let started = std::time::Instant::now();
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(200),
        "the merge did not wait for the lock"
    );
    handle.join().expect("holder");
}

#[test]
fn merge_parks_when_the_lock_wait_runs_out() {
    let mut fx = common::approved_job();
    fx.driver
        .set_merge_lock_wait(std::time::Duration::from_millis(100));
    let (handle, taken) = hold_merge_lock(&fx.main, std::time::Duration::from_millis(1500));
    taken.recv().expect("lock taken");
    let before = common::main_tip(&fx.main);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(
        matches!(&r, MergeResult::Parked(m) if m.contains("another project is merging")),
        "got {r:?}"
    );
    assert_eq!(common::main_tip(&fx.main), before);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    handle.join().expect("holder");
}

/// A scoped driver with its own state and worktree folders, over the shared repo at `main`.
fn scoped_driver(
    tmp: &std::path::Path,
    main: &std::path::Path,
    project: &str,
    subpath: &str,
) -> factory_engine::Driver {
    let mut layout = factory_core::Layout::in_repo(main);
    layout.state_dir = tmp.join(project).join("state");
    layout.jobs_dir = tmp.join(project).join("jobs");
    layout.worktrees_dir = tmp.join(project).join("worktrees");
    layout.scope = Some(factory_core::Scope {
        project: project.into(),
        subpath: std::path::PathBuf::from(subpath),
    });
    let runner = common::approving_runner(3);
    let factory: factory_engine::RunnerFactory = Box::new(move |_job_id| runner.clone());
    factory_engine::Driver::new(layout, factory_core::Config::default(), factory).expect("driver")
}

/// Gives the driver an approved job `001` that adds `file`, inside its project folder.
fn add_approved_job(driver: &factory_engine::Driver, project: &str, file: &str) {
    let wt = driver
        .repo()
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "hello".into(),
            project: Some(project.into()),
        })
        .expect("worktree");
    let head = common::commit_file(&wt.path, file, "x\n");
    let mut job = factory_core::Job::new(
        "001",
        "2026-10-13-001-hello",
        "hello",
        "2026-10-13",
        "say hello",
        0.0,
    );
    job.worktree = Some(wt.path.to_string_lossy().to_string());
    job.branch = wt.branch.clone();
    job.base = Some(wt.base.clone());
    job.head = Some(head);
    job.set_stage(factory_core::Stage::Approve, None, false, 1.0)
        .expect("stage");
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("job dir");
    std::fs::write(dir.join("plan.md"), "- [ ] Task 1: hello\n").expect("plan");
    driver.ledger(&job).create("# SDD ledger").expect("ledger");
    driver
        .store()
        .update(|b| {
            b.jobs.push(job);
            Ok(())
        })
        .expect("board");
}

#[test]
fn two_projects_merge_into_one_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    common::init_repo(&main);
    common::commit_file(&main, "apps/web/index.txt", "web\n");
    common::commit_file(&main, "apps/api/index.txt", "api\n");
    // Drivers are not Send, so each thread builds its own. A mutex keeps the setup serial.
    let setup = std::sync::Arc::new(std::sync::Mutex::new(()));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for (project, subpath, file) in [
        ("web", "apps/web", "apps/web/new.md"),
        ("api", "apps/api", "apps/api/new.md"),
    ] {
        let (tmp, main) = (tmp.path().to_path_buf(), main.clone());
        let (setup, barrier) = (setup.clone(), barrier.clone());
        handles.push(std::thread::spawn(move || {
            let driver = {
                let _serial = setup.lock().expect("setup");
                let driver = scoped_driver(&tmp, &main, project, subpath);
                add_approved_job(&driver, project, file);
                driver
            };
            barrier.wait();
            let r = merge_stage(&driver, "001").expect("stage");
            assert!(matches!(r, MergeResult::Merged(_)), "{project}: {r:?}");
            driver.job("001").expect("job").stage
        }));
    }
    for handle in handles {
        assert_eq!(handle.join().expect("thread"), factory_core::Stage::Merged);
    }
    // Both changes are in main's tree and both job commits are in its history.
    let repo = git2::Repository::open(&main).expect("open");
    let main_id = repo.revparse_single("main").expect("main").id();
    let tree = repo
        .find_commit(main_id)
        .expect("commit")
        .tree()
        .expect("tree");
    for file in ["apps/web/new.md", "apps/api/new.md"] {
        assert!(tree.get_path(std::path::Path::new(file)).is_ok(), "{file}");
    }
    for branch in ["factory/web/001-hello", "factory/api/001-hello"] {
        let tip = repo.revparse_single(branch).expect("branch").id();
        assert!(
            repo.graph_descendant_of(main_id, tip).expect("graph"),
            "{branch} was lost"
        );
    }
}
