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
    assert_eq!(
        board.jobs[1].series.as_ref().and_then(|s| s.id.clone()),
        board.jobs[0].series.as_ref().and_then(|s| s.id.clone()),
        "one series id"
    );
    let stages: Vec<_> = board.jobs[1].stage_history.iter().map(|e| e.stage).collect();
    assert_eq!(stages[..2], [factory_core::Stage::Spec, factory_core::Stage::Build], "parked, then built");
    assert_ne!(board.jobs[0].base, board.jobs[1].base, "plan 2 builds on top of plan 1's merge");
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
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2, "plan 2 has its job from the start");
    assert_eq!(board.jobs[1].stage, factory_core::Stage::Spec, "and it never started");
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

#[test]
fn create_job_records_the_configured_budget_and_history() {
    let fx = common::SeriesFixture::new("x");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "new", "f", Some(&plan), None).expect("create");
    assert_eq!(job.budget_weighted, Some(fx.driver.config().budget_weighted));
    let stages: Vec<_> = job.stage_history.iter().map(|e| e.stage).collect();
    assert_eq!(stages, vec![factory_core::Stage::Spec, factory_core::Stage::Build]);
}

#[test]
fn every_agent_run_appends_a_usage_line_with_its_role() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    assert_eq!(run_job(&fx.driver, &job.id).expect("run"), RunEnd::Merged);
    let dir = fx.driver.job_dir(&job);
    let text = std::fs::read_to_string(dir.join("usage.jsonl")).expect("usage log");
    let roles: Vec<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["role"].as_str().map(str::to_string))
        .collect();
    assert_eq!(roles, vec!["builder", "committer", "reviewer", "message", "message"]);
}

#[test]
fn create_job_parks_later_plans_at_spec_with_one_series_id() {
    let fx = common::SeriesFixture::new("x");
    let a = fx.plan_file("plan-01-a.md", 2, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 3, "plan-03-c.md");
    fx.plan_file("plan-03-c.md", 1, "none");
    let first = create_job(&fx.driver, "feat", "three plans", Some(&a), None).expect("create");
    assert_eq!(first.id, "001");
    assert_eq!(first.stage, factory_core::Stage::Build);
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 3);
    let stages: Vec<_> = board.jobs.iter().map(|j| j.stage).collect();
    assert_eq!(stages, vec![factory_core::Stage::Build, factory_core::Stage::Spec, factory_core::Stage::Spec]);
    let ids: Vec<_> = board.jobs.iter().map(|j| j.series.as_ref().and_then(|s| s.id.clone())).collect();
    assert_eq!(ids, vec![Some("S1".to_string()); 3]);
    let places: Vec<_> = board.jobs.iter().map(|j| j.series.as_ref().map(|s| (s.index, s.total))).collect();
    assert_eq!(places, vec![Some((1, 3)), Some((2, 3)), Some((3, 3))]);
    for job in &board.jobs {
        assert!(fx.driver.job_dir(job).join("plan.md").exists(), "plan.md for job {}", job.id);
    }
    assert_eq!(board.jobs[1].slug, "plan-02-b");
}

#[test]
fn a_second_series_gets_the_next_series_id() {
    let fx = common::SeriesFixture::new("x");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    create_job(&fx.driver, "one", "f", Some(&a), None).expect("first series");
    create_job(&fx.driver, "two", "f", Some(&a), None).expect("second series");
    let board = fx.driver.store().load().expect("board");
    let ids: Vec<_> = board.jobs.iter().filter_map(|j| j.series.as_ref().and_then(|s| s.id.clone())).collect();
    assert_eq!(ids, vec!["S1", "S1", "S2", "S2"]);
}

#[test]
fn run_series_follows_its_own_series_when_plan_files_are_reused() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "plan-03-c.md");
    fx.plan_file("plan-03-c.md", 1, "none");
    create_job(&fx.driver, "one", "f", Some(&a), None).expect("first series");
    create_job(&fx.driver, "two", "f", Some(&a), None).expect("second series");
    assert_eq!(run_series(&fx.driver, "004").expect("series"), RunEnd::Merged);
    for id in ["004", "005", "006"] {
        assert_eq!(fx.driver.job(id).expect("job").stage, factory_core::Stage::Merged, "job {id}");
    }
    for id in ["002", "003"] {
        assert_eq!(fx.driver.job(id).expect("job").stage, factory_core::Stage::Spec, "job {id}");
    }
}

#[test]
fn a_single_plan_gets_no_series_id_and_no_extra_jobs() {
    let fx = common::SeriesFixture::new("x");
    let a = fx.plan_file("plan-01-a.md", 1, "none");
    create_job(&fx.driver, "solo", "f", Some(&a), None).expect("create");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 1);
    assert_eq!(board.jobs[0].series.as_ref().and_then(|s| s.id.clone()), None);
}

#[test]
fn a_broken_chain_creates_no_jobs() {
    let fx = common::SeriesFixture::new("x");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-missing.md");
    assert!(create_job(&fx.driver, "bad", "f", Some(&a), None).is_err());
    assert!(fx.driver.store().load().expect("board").jobs.is_empty());
}

#[test]
fn spec_md_is_copied_into_plan_one_only_and_only_for_a_folder() {
    let fx = common::SeriesFixture::new("x");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let folder = a.parent().expect("plans folder").to_path_buf();
    std::fs::write(folder.join("spec.md"), "# Spec: Demo\n\n## Goal\n\nDo it.\n").expect("spec");
    // A single file as the argument has no folder, so nothing is copied and nothing fails.
    create_job(&fx.driver, "file", "f", Some(&a), None).expect("file argument");
    let board = fx.driver.store().load().expect("board");
    assert!(!fx.driver.job_dir(&board.jobs[0]).join("spec.md").exists());
    // A folder copies the spec into plan 1 and not into plan 2.
    create_job(&fx.driver, "folder", "f", Some(&folder), None).expect("folder argument");
    let board = fx.driver.store().load().expect("board");
    assert!(fx.driver.job_dir(&board.jobs[2]).join("spec.md").exists());
    assert!(!fx.driver.job_dir(&board.jobs[3]).join("spec.md").exists());
}

#[test]
fn an_old_series_without_an_id_still_gets_its_next_job_after_the_merge() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "old", "two plans", Some(&a), None).expect("create");
    // Make the board look like one written before the plans got their jobs up front.
    fx.driver
        .store()
        .update(|board| {
            board.jobs.truncate(1);
            if let Some(series) = board.jobs[0].series.as_mut() {
                series.id = None;
            }
            Ok(())
        })
        .expect("rewrite board");
    assert_eq!(run_series(&fx.driver, &job.id).expect("series"), RunEnd::Merged);
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert!(board.jobs.iter().all(|j| j.stage == factory_core::Stage::Merged));
    assert_eq!(board.jobs[1].series.as_ref().and_then(|s| s.id.clone()), None);
}

#[test]
fn a_cancelled_plan_stops_the_series_and_later_plans_stay_at_spec() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "plan-03-c.md");
    fx.plan_file("plan-03-c.md", 1, "none");
    let job = create_job(&fx.driver, "cancel", "three plans", Some(&a), None).expect("create");
    fx.driver
        .store()
        .update_job("002", |j| {
            j.set_stage(factory_core::Stage::NeedsHuman, Some("cancelled: test"), false, 1.0)?;
            j.cancelled = true;
            Ok(())
        })
        .expect("cancel plan 2");
    assert!(matches!(run_series(&fx.driver, &job.id).expect("series"), RunEnd::NeedsHuman(_)));
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs[0].stage, factory_core::Stage::Merged);
    assert_eq!(board.jobs[2].stage, factory_core::Stage::Spec);
}

#[test]
fn run_job_moves_a_waiting_plan_to_build_by_itself() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    create_job(&fx.driver, "waiting", "two plans", Some(&a), None).expect("create");
    assert_eq!(run_job(&fx.driver, "002").expect("run plan 2 alone"), RunEnd::Merged);
    assert_eq!(fx.driver.job("002").expect("job").stage, factory_core::Stage::Merged);
}

#[test]
fn create_job_records_issue_numbers_from_the_feature() {
    let fx = common::SeriesFixture::new("x");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let feature = "GitHub issue #20 (https://github.com/o/r/issues/20)";
    let job = create_job(&fx.driver, "new", feature, Some(&plan), None).expect("create");
    assert_eq!(job.issues, vec![20]);
    assert_eq!(fx.driver.job(&job.id).expect("job").issues, vec![20]);
}

#[test]
fn create_job_without_issue_numbers_leaves_the_key_out_of_the_board() {
    let fx = common::SeriesFixture::new("x");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "new", "no refs here", Some(&plan), None).expect("create");
    assert!(job.issues.is_empty());
    let board = fx.driver.store().load().expect("board");
    let text = serde_json::to_string(&board).expect("json");
    assert!(!text.contains("\"issues\""), "{text}");
}
