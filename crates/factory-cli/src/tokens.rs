//! Token report: raw and weighted totals per agent role and per job.

use std::collections::BTreeMap;

/// Job 001 measured baseline from the spec: raw tokens, weighted tokens and orchestrator share.
pub const BASELINE: Baseline = Baseline {
    raw: 12_750_000,
    weighted: 2_170_000,
    orchestrator_percent: 53.6,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    pub raw: u64,
    pub weighted: u64,
    pub orchestrator_percent: f64,
}

/// One agent run as logged by the engine in `usage.jsonl`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UsageRecord {
    pub job: String,
    pub role: String,
    pub input: u64,
    pub cache_creation: u64,
    pub output: u64,
    pub cache_read: u64,
    /// 2 for lines with the corrected output count, 0 for old lines.
    pub usage_v: u64,
    pub model: String,
    pub turns: u64,
    pub resumed: bool,
    pub stop: String,
    pub round: u64,
    pub tasks_done: Option<u64>,
    pub start_ctx: u64,
    pub ts: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub raw: u64,
    pub weighted: u64,
    pub runs: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenReport {
    pub by_role: BTreeMap<String, Totals>,
    pub total: Totals,
}

/// Weighted tokens, using the one weighting rule of factory-core.
pub fn weighted(r: &UsageRecord) -> u64 {
    factory_core::weighted_tokens(r.input, r.cache_creation, r.output, r.cache_read)
}

fn raw(r: &UsageRecord) -> u64 {
    r.input
        .saturating_add(r.cache_creation)
        .saturating_add(r.output)
        .saturating_add(r.cache_read)
}

pub fn aggregate(records: &[UsageRecord]) -> TokenReport {
    let mut report = TokenReport::default();
    for r in records {
        let role = report.by_role.entry(r.role.clone()).or_default();
        role.raw += raw(r);
        role.weighted += weighted(r);
        role.runs += 1;
        report.total.raw += raw(r);
        report.total.weighted += weighted(r);
        report.total.runs += 1;
    }
    report
}

pub fn render(job: &str, report: &TokenReport) -> String {
    let mut out = format!("tokens for job {job}\n");
    for (role, t) in &report.by_role {
        out.push_str(&format!(
            "  {role:<10} runs {:<3} raw {:<10} weighted {}\n",
            t.runs, t.raw, t.weighted
        ));
    }
    out.push_str(&format!(
        "  total      runs {:<3} raw {} weighted {}\n",
        report.total.runs, report.total.raw, report.total.weighted
    ));
    let percent = report.total.weighted as f64 * 100.0 / BASELINE.weighted as f64;
    out.push_str(&format!(
        "  {percent:.1}% of job 001 baseline weighted ({} weighted, {} raw, orchestrator {}%)\n",
        BASELINE.weighted, BASELINE.raw, BASELINE.orchestrator_percent
    ));
    out
}

/// True when any line predates the corrected output count.
pub fn under_counted(records: &[UsageRecord]) -> bool {
    records.iter().any(|r| r.usage_v < 2)
}

/// One row per run in log order, so resumes, fix rounds and tasks show up as separate costs.
pub fn render_detail(records: &[UsageRecord]) -> String {
    if records.is_empty() {
        return String::new();
    }
    let mut out = String::from("  runs\n");
    for (i, r) in records.iter().enumerate() {
        let task = r.tasks_done.map_or(String::new(), |n| format!(" done {n}"));
        let resumed = if r.resumed { " resumed" } else { "" };
        out.push_str(&format!(
            "  {:<3} {:<10} round {}{task}{resumed} turns {} weighted {}\n",
            i + 1,
            r.role,
            r.round.max(1),
            r.turns,
            weighted(r)
        ));
    }
    out
}

/// Reads usage lines for one job. Unreadable lines are skipped, a missing file is empty.
pub fn load_usage(log: &std::path::Path, job: &str) -> Result<Vec<UsageRecord>, std::io::Error> {
    let text = match std::fs::read_to_string(log) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["job"] != job {
            continue;
        }
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        out.push(UsageRecord {
            job: job.to_string(),
            role: v["role"].as_str().unwrap_or("unknown").to_string(),
            input: n("input"),
            cache_creation: n("cache_creation"),
            output: n("output"),
            cache_read: n("cache_read"),
            usage_v: n("usage_v"),
            model: v["model"].as_str().unwrap_or("").to_string(),
            turns: n("turns"),
            resumed: v["resumed"].as_bool().unwrap_or(false),
            stop: v["stop"].as_str().unwrap_or("").to_string(),
            round: n("round"),
            tasks_done: v["tasks_done"].as_u64(),
            start_ctx: n("start_ctx"),
            ts: n("ts"),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(role: &str, input: u64, cc: u64, out: u64, cr: u64) -> UsageRecord {
        UsageRecord {
            job: "001".into(),
            role: role.into(),
            input,
            cache_creation: cc,
            output: out,
            cache_read: cr,
            usage_v: 2,
            ..Default::default()
        }
    }

    #[test]
    fn old_lines_are_flagged_as_under_counted() {
        let old = UsageRecord {
            usage_v: 0,
            ..rec("builder", 1, 1, 1, 1)
        };
        assert!(under_counted(&[rec("reviewer", 1, 1, 1, 1), old]));
        assert!(!under_counted(&[rec("reviewer", 1, 1, 1, 1)]));
        assert!(!under_counted(&[]));
    }

    #[test]
    fn load_reads_old_new_and_garbage_lines_together() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("usage.jsonl");
        std::fs::write(
            &log,
            concat!(
                "{\"job\":\"001\",\"role\":\"builder\",\"input\":1,\"cache_creation\":2,\"output\":3,\"cache_read\":40}\n",
                "garbage line\n",
                "{\"job\":\"001\",\"role\":\"builder\",\"input\":1,\"cache_creation\":2,\"output\":300,\"cache_read\":40,",
                "\"ts\":5,\"model\":\"sonnet\",\"turns\":9,\"resumed\":true,\"stop\":\"done\",\"round\":2,\"tasks_done\":1,\"start_ctx\":77,\"usage_v\":2}\n"
            ),
        )?;
        let records = load_usage(&log, "001")?;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].usage_v, 0);
        assert_eq!(records[0].tasks_done, None);
        assert_eq!(records[1].usage_v, 2);
        assert_eq!(records[1].turns, 9);
        assert!(records[1].resumed);
        assert_eq!(records[1].tasks_done, Some(1));
        assert_eq!(records[1].start_ctx, 77);
        assert!(under_counted(&records));
        Ok(())
    }

    #[test]
    fn detail_lists_one_row_per_run_with_its_weighted_cost() {
        let r = UsageRecord {
            turns: 9,
            resumed: true,
            round: 2,
            tasks_done: Some(1),
            stop: "done".into(),
            ..rec("builder", 100, 200, 300, 1_000)
        };
        let text = render_detail(&[r, rec("reviewer", 1, 0, 0, 0)]);
        assert!(text.contains("builder"), "{text}");
        assert!(text.contains("resumed"), "{text}");
        assert!(text.contains("turns 9"), "{text}");
        assert!(text.contains("weighted 700"), "{text}");
        assert!(text.contains("reviewer"), "{text}");
    }

    #[test]
    fn detail_of_nothing_is_empty() {
        assert_eq!(render_detail(&[]), "");
    }

    #[test]
    fn weighted_counts_cache_reads_at_one_tenth() {
        let r = rec("builder", 100, 200, 300, 1_000);
        assert_eq!(weighted(&r), 100 + 200 + 300 + 100);
    }

    #[test]
    fn aggregate_groups_by_role_and_sums_totals() {
        let report = aggregate(&[
            rec("builder", 10, 0, 10, 0),
            rec("builder", 5, 0, 5, 0),
            rec("reviewer", 1, 1, 1, 10),
        ]);
        assert_eq!(report.by_role["builder"].runs, 2);
        assert_eq!(report.by_role["builder"].raw, 30);
        assert_eq!(report.by_role["reviewer"].raw, 13);
        assert_eq!(report.by_role["reviewer"].weighted, 4);
        assert_eq!(report.total.raw, 43);
        assert_eq!(report.total.runs, 3);
    }

    #[test]
    fn aggregate_lists_kb_and_message_roles() {
        let report = aggregate(&[rec("kb", 100, 0, 50, 0), rec("message", 10, 0, 5, 0)]);
        assert_eq!(report.by_role["kb"].weighted, 150);
        assert_eq!(report.by_role["message"].runs, 1);
        let text = render("001", &report);
        assert!(text.contains("kb ") && text.contains("message"), "{text}");
    }

    #[test]
    fn aggregate_of_nothing_is_zero() {
        let report = aggregate(&[]);
        assert_eq!(report.total, Totals::default());
        assert!(report.by_role.is_empty());
    }

    #[test]
    fn render_shows_roles_totals_and_baseline_percent() {
        let report = aggregate(&[rec("builder", 217_000, 0, 0, 0)]);
        let text = render("001", &report);
        assert!(text.contains("job 001"));
        assert!(text.contains("builder"));
        assert!(text.contains("raw 217000"));
        assert!(
            text.contains("10.0% of job 001 baseline weighted"),
            "{text}"
        );
    }

    #[test]
    fn render_of_an_empty_report_does_not_divide_by_zero() {
        let text = render("002", &aggregate(&[]));
        assert!(text.contains("raw 0"));
        assert!(text.contains("0.0% of job 001 baseline weighted"));
    }

    #[test]
    fn load_reads_only_lines_for_the_job() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("usage.jsonl");
        std::fs::write(
            &log,
            concat!(
                "{\"job\":\"001\",\"role\":\"builder\",\"input\":1,\"cache_creation\":2,\"output\":3,\"cache_read\":40}\n",
                "{\"job\":\"002\",\"role\":\"builder\",\"input\":9,\"cache_creation\":9,\"output\":9,\"cache_read\":9}\n",
                "garbage line\n"
            ),
        )?;
        let records = load_usage(&log, "001")?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].cache_read, 40);
        Ok(())
    }

    #[test]
    fn load_of_a_missing_log_is_empty() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        assert!(load_usage(&dir.path().join("none.jsonl"), "001")?.is_empty());
        Ok(())
    }
}
