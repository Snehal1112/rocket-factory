mod common;
use factory_engine::{create_job, run_job, run_series, RunEnd};

#[test]
fn run_job_goes_from_build_to_merged_in_one_call() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    assert_eq!(run_job(&fx.driver, &job.id).expect("run"), RunEnd::Merged);
    assert_eq!(fx.driver.job(&job.id).expect("job").stage, factory_core::Stage::Merged);
}

#[test]
fn a_crash_after_the_merge_commit_resumes_without_a_second_merge() {
    let fx = common::approved_job();
    // Simulate the crash: main already has the head, the board still says approve.
    let sha = common::merge_by_hand(&fx);
    assert_eq!(common::main_tip(&fx.main), sha);
    assert_eq!(run_job(&fx.driver, &fx.id).expect("run"), RunEnd::Merged);
    assert_eq!(common::main_tip(&fx.main), sha, "no second merge commit");
    let job = fx.driver.job(&fx.id).expect("job");
    let ledger = std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains(&format!("(merge {sha})")));
}

#[test]
fn run_series_chains_plans_and_each_job_writes_its_own_job_id_into_events() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "series", "two plans", Some(&a), None).expect("create");
    assert_eq!(run_series(&fx.driver, &job.id).expect("series"), RunEnd::Merged);
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert!(board.jobs.iter().all(|j| j.stage == factory_core::Stage::Merged));
    assert_eq!(board.jobs[1].series.as_ref().map(|s| (s.index, s.total)), Some((2, 2)));
    // The second job must not carry the first job's id.
    let events = std::fs::read_to_string(fx.driver.events_path()).expect("events");
    let mut seen = std::collections::BTreeSet::new();
    for line in events.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("json line");
        let job_id = v["job_id"].as_str().expect("job_id").to_string();
        let agent_id = v["agent_id"].as_str().expect("agent_id");
        assert!(agent_id.starts_with(&job_id), "{agent_id} under job {job_id}");
        seen.insert(job_id);
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec!["001".to_string(), "002".to_string()]);
}

#[test]
fn run_series_stops_when_a_plan_needs_a_human() {
    let fx = common::SeriesFixture::new("no\nVERDICT: CHANGES");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "stuck", "x", Some(&a), None).expect("create");
    assert!(matches!(run_series(&fx.driver, &job.id).expect("series"), RunEnd::NeedsHuman(_)));
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 1);
}

#[test]
fn create_job_avoids_ids_of_existing_branches() {
    let fx = common::SeriesFixture::new("x");
    let repo = git2::Repository::open(&fx.main).expect("open");
    let main = repo.revparse_single("main").expect("main").peel_to_commit().expect("commit");
    repo.branch("factory/004-old", &main, false).expect("branch");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "new", "f", Some(&plan), None).expect("create");
    assert_eq!(job.id, "005");
}
