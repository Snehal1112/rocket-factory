mod common;

use factory_core::{Job, SizeGroup, Stage};
use factory_engine::gather_costs;

fn add_finished_job(fx: &common::SeriesFixture, id: &str, plan_text: &str, usage: Option<&str>) -> Job {
    let mut job = Job::new(id, &format!("2026-10-16-{id}-x"), "x", "2026-10-16", "feature", 1.0);
    job.set_stage(Stage::Build, None, false, 2.0).expect("build");
    let dir = fx.driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("plan.md"), plan_text).expect("plan");
    if let Some(text) = usage {
        std::fs::write(dir.join("usage.jsonl"), text).expect("usage");
    }
    let added = job.clone();
    fx.driver.store().update(|board| { board.jobs.push(added); Ok(()) }).expect("add job");
    job
}

#[test]
fn gather_costs_builds_one_record_per_job_with_group_tasks_and_usage() {
    let fx = common::SeriesFixture::new("x");
    let plan = format!("# P\n### Task 1: a\n### Task 2: b\n{}", "x".repeat(13_000));
    let usage = "{\"job\":\"101\",\"role\":\"builder\",\"input\":100,\"cache_creation\":0,\"output\":0,\"cache_read\":1000,\"usage_v\":2}\n";
    add_finished_job(&fx, "101", &plan, Some(usage));
    let jobs = gather_costs(&fx.driver).expect("gather");
    let j = jobs.iter().find(|j| j.id == "101").expect("job 101");
    assert_eq!(j.group, Some(SizeGroup::Medium));
    assert_eq!(j.tasks, 2);
    assert_eq!(j.weighted, 200);
    assert_eq!(j.runs, 1);
    assert!(!j.approx);
    assert_eq!(j.lines, None, "no base and head on this job");
}

#[test]
fn a_job_without_a_plan_or_usage_is_listed_without_data() {
    let fx = common::SeriesFixture::new("x");
    let mut job = Job::new("102", "2026-10-16-102-x", "x", "2026-10-16", "f", 1.0);
    job.set_stage(Stage::Build, None, false, 2.0).expect("build");
    fx.driver.store().update(|board| { board.jobs.push(job); Ok(()) }).expect("add job");
    let jobs = gather_costs(&fx.driver).expect("gather");
    let j = jobs.iter().find(|j| j.id == "102").expect("job 102");
    assert_eq!(j.group, None);
    assert_eq!(j.runs, 0);
    assert!(!j.has_data());
}

#[test]
fn a_job_with_commits_that_no_longer_exist_still_gets_a_record() {
    let fx = common::SeriesFixture::new("x");
    let job = add_finished_job(&fx, "103", "# P\n", None);
    fx.driver
        .store()
        .update_job(&job.id, |j| {
            j.base = Some("1111111111111111111111111111111111111111".to_string());
            j.head = Some("2222222222222222222222222222222222222222".to_string());
            Ok(())
        })
        .expect("shas");
    let jobs = gather_costs(&fx.driver).expect("gather");
    let j = jobs.iter().find(|j| j.id == "103").expect("job 103");
    assert_eq!((j.lines, j.files), (None, None));
}

#[test]
fn old_usage_lines_make_a_job_approximate() {
    let fx = common::SeriesFixture::new("x");
    let usage = "{\"job\":\"104\",\"role\":\"builder\",\"input\":1,\"cache_creation\":0,\"output\":0,\"cache_read\":0}\n";
    add_finished_job(&fx, "104", "# P\n", Some(usage));
    let jobs = gather_costs(&fx.driver).expect("gather");
    assert!(jobs.iter().find(|j| j.id == "104").expect("job").approx);
}
