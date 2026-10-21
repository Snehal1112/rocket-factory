mod common;

use factory_core::{Job, SizeGroup, Stage};
use factory_engine::{
    costs_path, create_job, estimate_for_plan, gather_costs, refresh_costs, run_job, RunEnd,
};

fn add_finished_job(
    fx: &common::SeriesFixture,
    id: &str,
    plan_text: &str,
    usage: Option<&str>,
) -> Job {
    let mut job = Job::new(
        id,
        &format!("2026-10-16-{id}-x"),
        "x",
        "2026-10-16",
        "feature",
        1.0,
    );
    job.set_stage(Stage::Build, None, false, 2.0)
        .expect("build");
    let dir = fx.driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("plan.md"), plan_text).expect("plan");
    if let Some(text) = usage {
        std::fs::write(dir.join("usage.jsonl"), text).expect("usage");
    }
    let added = job.clone();
    fx.driver
        .store()
        .update(|board| {
            board.jobs.push(added);
            Ok(())
        })
        .expect("add job");
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
fn gather_costs_carries_the_providers_of_each_job() {
    let fx = common::SeriesFixture::new("x");
    let usage = concat!(
        "{\"job\":\"105\",\"role\":\"builder\",\"input\":100,\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"usage_v\":2}\n",
        "{\"job\":\"105\",\"role\":\"builder\",\"provider\":\"codex\",\"input\":50,\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"usage_v\":2}\n",
    );
    add_finished_job(&fx, "105", "# P\n", Some(usage));
    let jobs = gather_costs(&fx.driver).expect("gather");
    let j = jobs.iter().find(|j| j.id == "105").expect("job 105");
    assert_eq!(j.providers.get("codex"), Some(&50));
    assert_eq!(j.providers.get("claude"), Some(&100));
}

#[test]
fn a_job_without_a_plan_or_usage_is_listed_without_data() {
    let fx = common::SeriesFixture::new("x");
    let mut job = Job::new("102", "2026-10-16-102-x", "x", "2026-10-16", "f", 1.0);
    job.set_stage(Stage::Build, None, false, 2.0)
        .expect("build");
    fx.driver
        .store()
        .update(|board| {
            board.jobs.push(job);
            Ok(())
        })
        .expect("add job");
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

fn make_merged(fx: &common::SeriesFixture, id: &str, plan_text: &str, weighted_input: u64) {
    let usage = format!(
        "{{\"job\":\"{id}\",\"role\":\"builder\",\"input\":{weighted_input},\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"usage_v\":2}}\n"
    );
    let job = add_finished_job(fx, id, plan_text, Some(&usage));
    fx.driver
        .store()
        .update_job(&job.id, |j| j.set_stage(Stage::Merged, None, true, 3.0))
        .expect("merged");
}

#[test]
fn refresh_costs_writes_groups_and_roles_but_no_job_list() {
    let fx = common::SeriesFixture::new("x");
    make_merged(&fx, "101", "# tiny\n", 200);
    refresh_costs(&fx.driver).expect("refresh");
    let raw = std::fs::read_to_string(costs_path(fx.driver.layout())).expect("costs.json");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(json["groups"]["small"]["n"], 1);
    assert_eq!(json["groups"]["small"]["median"], 200);
    assert_eq!(json["roles"]["builder"], 100);
    assert!(json.get("jobs").is_none(), "{raw}");
}

#[test]
fn estimate_for_plan_uses_history_and_is_none_without_it() {
    let fx = common::SeriesFixture::new("x");
    assert_eq!(estimate_for_plan(&fx.driver, 100), None);
    make_merged(&fx, "101", "# tiny\n", 200);
    make_merged(&fx, "102", "# tiny\n", 400);
    let e = estimate_for_plan(&fx.driver, 100).expect("estimate");
    assert_eq!(
        (e.group.as_str(), e.median, e.low, e.high, e.n),
        ("small", 300, 200, 400, 2)
    );
    // A complex plan has no complex history, so the overall figures are used.
    let all = estimate_for_plan(&fx.driver, 40_000).expect("fallback");
    assert_eq!((all.group.as_str(), all.n), ("all", 2));
}

#[test]
fn a_new_job_from_a_plan_gets_an_estimate_and_a_new_job_does_not_change_the_history() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    make_merged(&fx, "101", "# tiny\n", 200);
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "feat", "a feature", Some(&plan), None).expect("create");
    let est = job.estimate.as_ref().expect("estimate on the returned job");
    assert_eq!((est.group.as_str(), est.median, est.n), ("small", 200, 1));
    let on_board = fx.driver.job(&job.id).expect("job");
    assert_eq!(on_board.estimate, job.estimate);
    // The new job has no usage and is not merged, so the history is unchanged.
    let again = estimate_for_plan(&fx.driver, 100).expect("estimate");
    assert_eq!(again.n, 1);
}

#[test]
fn a_job_made_without_a_plan_has_no_estimate() {
    let fx = common::SeriesFixture::new("x");
    make_merged(&fx, "101", "# tiny\n", 200);
    let job = create_job(&fx.driver, "bare", "no plan", None, None).expect("create");
    assert_eq!(job.estimate, None);
}

#[test]
fn a_merge_refreshes_costs_json_and_a_failed_refresh_does_not_fail_the_merge() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "ok", "a feature", Some(&plan), None).expect("create");
    assert_eq!(run_job(&fx.driver, &job.id).expect("run"), RunEnd::Merged);
    let raw = std::fs::read_to_string(costs_path(fx.driver.layout()))
        .expect("costs.json after the merge");
    assert!(raw.contains("\"groups\""), "{raw}");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("costs.json parses");
    assert_eq!(
        value["groups"]["all"]["n"], 1,
        "the merged job is counted: {raw}"
    );
    let jobs = gather_costs(&fx.driver).expect("gather");
    let merged = jobs.iter().find(|j| j.id == job.id).expect("merged job");
    assert!(
        merged.lines.is_some_and(|n| n > 0),
        "lines: {:?}",
        merged.lines
    );
    assert!(
        merged.files.is_some_and(|n| n >= 1),
        "files: {:?}",
        merged.files
    );

    // A directory where the file goes makes the write fail.
    let fx2 = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    std::fs::create_dir_all(costs_path(fx2.driver.layout())).expect("blocker dir");
    let plan2 = fx2.plan_file("plan-01-a.md", 1, "none");
    let job2 = create_job(&fx2.driver, "blocked", "a feature", Some(&plan2), None).expect("create");
    assert_eq!(run_job(&fx2.driver, &job2.id).expect("run"), RunEnd::Merged);
}

#[test]
fn the_costs_refresh_after_a_merge_includes_the_kb_run_of_that_job() {
    let kb = tempfile::tempdir().expect("kb dir");
    let config = factory_core::Config {
        kb_path: Some(kb.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    let fx = common::SeriesFixture::with_config("fine\nVERDICT: APPROVE", config);
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "kb", "a feature", Some(&plan), None).expect("create");
    assert_eq!(run_job(&fx.driver, &job.id).expect("run"), RunEnd::Merged);
    let raw = std::fs::read_to_string(costs_path(fx.driver.layout())).expect("costs.json");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parses");
    assert!(
        value["roles"].to_string().contains("\"kb\""),
        "roles must list kb: {raw}"
    );
}
