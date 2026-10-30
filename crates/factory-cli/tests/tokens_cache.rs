mod common;
use common::Fixture;
use std::path::{Path, PathBuf};
use std::time::Duration;

const USAGE: &str = concat!(
    r#"{"job":"001","role":"builder","input":10,"cache_creation":1000,"output":50,"cache_read":9000,"usage_v":2,"resumed":false,"round":1,"ts":1000,"secs":20}"#,
    "\n",
    r#"{"job":"001","role":"builder","input":10,"cache_creation":5000,"output":50,"cache_read":1000,"usage_v":2,"resumed":true,"round":2,"ts":2000,"secs":20}"#,
    "\n",
);

fn new_job(fx: &Fixture, slug: &str) {
    let plans = fx.write_plan("plan-01-a.md", 1, "none");
    fx.run(&[
        "new",
        slug,
        "a job",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
}

fn job_folder(fx: &Fixture, id: &str) -> PathBuf {
    let layout = factory_core::Layout::in_repo(&fx.root);
    let store = factory_engine::BoardStore::new(&layout.board_file(), Duration::from_secs(15));
    let board = store.load().expect("load board");
    let job = board.find(id).expect("job");
    layout.job_dir(&job.dir)
}

fn files_under(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
    for e in std::fs::read_dir(dir).expect("read dir") {
        let p = e.expect("entry").path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            let len = std::fs::metadata(&p).expect("meta").len();
            out.push((p, len));
        }
    }
}

fn snapshot(fx: &Fixture) -> Vec<(PathBuf, u64)> {
    let mut v = Vec::new();
    files_under(&factory_core::Layout::in_repo(&fx.root).state_dir, &mut v);
    v.sort();
    v
}

#[test]
fn cache_report_counts_spec_review_usage() {
    let fx = Fixture::new();
    let layout = factory_core::Layout::in_repo(&fx.root);
    std::fs::create_dir_all(layout.spec_reviews_dir()).expect("dir");
    let line = concat!(
        r#"{"job":"spec-review","role":"spec-reviewer","input":10,"cache_creation":1000,"output":50,"cache_read":9000,"usage_v":2,"resumed":false,"round":1,"ts":1000,"secs":20}"#,
        "\n",
    );
    std::fs::write(layout.spec_review_usage_file(), line).expect("usage");
    let out = fx.run(&["tokens", "--cache"]).expect("cache report");
    assert!(out.contains("spec-reviewer"), "{out}");
    assert!(out.contains("(1 runs"), "{out}");
}

#[test]
fn cache_report_reads_every_job_of_the_project() {
    let fx = Fixture::new();
    new_job(&fx, "a");
    new_job(&fx, "b");
    // Only the first job has a usage.jsonl, the second has none.
    let codex = concat!(
        r#"{"job":"001","role":"builder","provider":"codex","input":10,"cache_creation":10,"output":5,"cache_read":10,"usage_v":2,"resumed":false,"round":1,"ts":3000,"secs":20}"#,
        "\n",
    );
    std::fs::write(
        job_folder(&fx, "001").join("usage.jsonl"),
        format!("{USAGE}{codex}"),
    )
    .expect("write usage");
    let out = fx.run(&["tokens", "--cache"]).expect("cache report");
    assert!(out.contains("builder"), "{out}");
    assert!(out.contains("claude"), "{out}");
    assert!(out.contains("codex"), "{out}");
    assert!(out.contains("resumed runs: 1"), "{out}");
}

#[test]
fn tokens_cache_prints_the_cold_starts_section() {
    let fx = Fixture::new();
    new_job(&fx, "a");
    let line = concat!(
        r#"{"job":"001","role":"builder","input":10,"cache_creation":1000,"output":50,"cache_read":9000,"usage_v":3,"resumed":false,"round":1,"ts":1000,"secs":20,"start_cc":400,"start_cr":100}"#,
        "\n",
    );
    std::fs::write(job_folder(&fx, "001").join("usage.jsonl"), line).expect("write usage");
    let out = fx.run(&["tokens", "--cache"]).expect("cache report");
    assert!(out.contains("COLD STARTS"), "{out}");
    assert!(out.contains("builder"), "{out}");
}

#[test]
fn tokens_cache_prints_avg_end() {
    let fx = Fixture::new();
    new_job(&fx, "a");
    let line = concat!(
        r#"{"job":"001","role":"builder","input":10,"cache_creation":1000,"output":50,"cache_read":9000,"usage_v":4,"resumed":false,"round":1,"ts":1000,"secs":20,"start_cc":400,"start_cr":100,"end_ctx":31000}"#,
        "\n",
    );
    std::fs::write(job_folder(&fx, "001").join("usage.jsonl"), line).expect("write usage");
    let out = fx.run(&["tokens", "--cache"]).expect("cache report");
    assert!(out.contains("AVG-END"), "{out}");
}

#[test]
fn cache_report_writes_no_file() {
    let fx = Fixture::new();
    new_job(&fx, "a");
    std::fs::write(job_folder(&fx, "001").join("usage.jsonl"), USAGE).expect("write usage");
    let before = snapshot(&fx);
    fx.run(&["tokens", "--cache"]).expect("cache report");
    assert_eq!(before, snapshot(&fx));
}

#[test]
fn tokens_for_one_job_is_unchanged() {
    let fx = Fixture::new();
    new_job(&fx, "a");
    std::fs::write(job_folder(&fx, "001").join("usage.jsonl"), USAGE).expect("write usage");
    let out = fx.run(&["tokens", "1"]).expect("tokens");
    let expected = "tokens for job 001\n  totals\n    input           0\n    cache_creation  0\n    output          0\n    cache_read      0\n    weighted        0 (0% of 1000000)\n\n  by role\n    builder    runs 2   raw 16120      weighted 7120\n    total      runs 2   raw 16120      weighted 7120\n  0.3% of job 001 baseline weighted (2170000 weighted, 12750000 raw, orchestrator 53.6%)\n\n  runs\n    #   role       provider round turns  weighted  details\n    1   builder    claude   round 1  turns 0    weighted 1960      \n    2   builder    claude   round 2  turns 0    weighted 5160      resumed";
    assert_eq!(out, expected);
}
