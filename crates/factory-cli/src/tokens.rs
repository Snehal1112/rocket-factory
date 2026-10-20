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
    /// Part of `cache_creation` written with the 1 hour TTL, `None` on old lines.
    pub cache_creation_1h: Option<u64>,
    /// Run duration in seconds, `None` on old lines.
    pub secs: Option<u64>,
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

/// A resumed run counts as a cache rewrite at or above this creation share.
pub const REWRITE_SHARE: f64 = 0.25;
/// A gap strictly above this many seconds means the 5 minute cache had expired.
pub const CACHE_5M_SECS: u64 = 300;

/// Cache use of one role over the counted runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleCache {
    pub role: String,
    pub runs: u64,
    pub creation: u64,
    pub read: u64,
    /// Runs that carry `cache_creation_1h`.
    pub split_runs: u64,
    pub one_hour: u64,
    pub five_min: u64,
}

/// One resumed run with the idle time before it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumedRun {
    pub job: String,
    pub role: String,
    pub round: u64,
    pub gap: Option<u64>,
    pub creation: u64,
    pub read: u64,
}

impl ResumedRun {
    /// Creation share of creation plus read, `None` when both are 0.
    pub fn share(&self) -> Option<f64> {
        let total = self.creation.saturating_add(self.read);
        if total == 0 {
            None
        } else {
            Some(self.creation as f64 / total as f64)
        }
    }

    pub fn rewrote(&self) -> bool {
        self.share().is_some_and(|s| s >= REWRITE_SHARE)
    }

    pub fn idle(&self) -> bool {
        self.gap.is_some_and(|g| g > CACHE_5M_SECS)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheReport {
    pub runs: u64,
    pub days: Option<u32>,
    pub roles: Vec<RoleCache>,
    pub resumed: Vec<ResumedRun>,
}

/// Builds the cache report. Gaps use all rows of a job, `days` only limits what is counted.
pub fn cache_report(jobs: &[Vec<UsageRecord>], days: Option<u32>, now: u64) -> CacheReport {
    let cutoff = days.map(|d| now.saturating_sub(u64::from(d) * 86_400));
    let mut report = CacheReport {
        days,
        ..Default::default()
    };
    let mut roles: BTreeMap<String, RoleCache> = BTreeMap::new();
    for rows in jobs {
        for (i, r) in rows.iter().enumerate() {
            if cutoff.is_some_and(|c| r.ts < c) {
                continue;
            }
            report.runs += 1;
            let role = roles.entry(r.role.clone()).or_insert_with(|| RoleCache {
                role: r.role.clone(),
                ..Default::default()
            });
            role.runs += 1;
            role.creation = role.creation.saturating_add(r.cache_creation);
            role.read = role.read.saturating_add(r.cache_read);
            if let Some(h) = r.cache_creation_1h {
                role.split_runs += 1;
                role.one_hour = role.one_hour.saturating_add(h);
                role.five_min = role
                    .five_min
                    .saturating_add(r.cache_creation.saturating_sub(h));
            }
            if r.resumed {
                let prev = i.checked_sub(1).and_then(|p| rows.get(p));
                let gap = match (r.secs, prev) {
                    (Some(s), Some(p)) => Some(r.ts.saturating_sub(s).saturating_sub(p.ts)),
                    _ => None,
                };
                report.resumed.push(ResumedRun {
                    job: r.job.clone(),
                    role: r.role.clone(),
                    round: r.round.max(1),
                    gap,
                    creation: r.cache_creation,
                    read: r.cache_read,
                });
            }
        }
    }
    report.roles = roles.into_values().collect();
    report
}

fn percent(creation: u64, read: u64) -> String {
    let total = creation.saturating_add(read);
    if total == 0 {
        "-".to_string()
    } else {
        format!("{:.1}%", creation as f64 * 100.0 / total as f64)
    }
}

pub fn render_cache(r: &CacheReport) -> String {
    let span = r
        .days
        .map_or("all time".to_string(), |d| format!("last {d} days"));
    let mut out = format!("cache report ({} runs, {span})\n", r.runs);
    if r.runs == 0 {
        out.push_str("no runs\n");
        return out;
    }
    out.push_str(&format!(
        "{:<10} {:<5} {:<10} {:<10} {:<10} {:<10} {}\n",
        "ROLE", "RUNS", "CREATION", "READ", "CREATION%", "5M", "1H"
    ));
    let mut split_total = 0;
    for role in &r.roles {
        let (five, hour) = if role.split_runs == 0 {
            ("n/a".to_string(), "n/a".to_string())
        } else {
            (role.five_min.to_string(), role.one_hour.to_string())
        };
        split_total += role.split_runs;
        out.push_str(&format!(
            "{:<10} {:<5} {:<10} {:<10} {:<10} {:<10} {}\n",
            role.role,
            role.runs,
            role.creation,
            role.read,
            percent(role.creation, role.read),
            five,
            hour
        ));
    }
    if split_total > 0 && split_total < r.runs {
        out.push_str(&format!("split covers {split_total} of {} runs\n", r.runs));
    }
    out.push('\n');
    if r.resumed.is_empty() {
        out.push_str("no resumed runs\n");
        return out;
    }
    out.push_str("RESUMED RUNS\n");
    out.push_str(&format!(
        "{:<5} {:<10} {:<6} {:<8} {:<10} {:<10} {:<6} {}\n",
        "JOB", "ROLE", "ROUND", "GAP", "CREATION", "READ", "SHARE", "REWROTE"
    ));
    for m in &r.resumed {
        let gap = m.gap.map_or("?".to_string(), |g| format!("{g}s"));
        let share = m
            .share()
            .map_or("-".to_string(), |s| format!("{:.1}%", s * 100.0));
        out.push_str(&format!(
            "{:<5} {:<10} {:<6} {:<8} {:<10} {:<10} {:<6} {}\n",
            m.job,
            m.role,
            m.round,
            gap,
            m.creation,
            m.read,
            share,
            if m.rewrote() { "yes" } else { "no" }
        ));
    }
    let idle = r.resumed.iter().filter(|m| m.idle()).count();
    let idle_rewrote = r.resumed.iter().filter(|m| m.idle() && m.rewrote()).count();
    let unknown = r
        .resumed
        .iter()
        .filter(|m| m.gap.is_none() && m.rewrote())
        .count();
    out.push_str(&format!(
        "resumed runs: {}, idle over 5m: {idle}, of those rewrote: {idle_rewrote}, rewrote with gap unknown: {unknown}\n",
        r.resumed.len()
    ));
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
            cache_creation_1h: v["cache_creation_1h"].as_u64(),
            secs: v["secs"].as_u64(),
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

    #[allow(clippy::too_many_arguments)]
    fn run(
        job: &str,
        role: &str,
        resumed: bool,
        ts: u64,
        secs: Option<u64>,
        cc: u64,
        cc1h: Option<u64>,
        cr: u64,
    ) -> UsageRecord {
        UsageRecord {
            job: job.into(),
            role: role.into(),
            resumed,
            ts,
            secs,
            cache_creation: cc,
            cache_creation_1h: cc1h,
            cache_read: cr,
            usage_v: 2,
            ..Default::default()
        }
    }

    fn resumed_pair(cc: u64, cr: u64) -> CacheReport {
        let first = run("020", "builder", false, 1_000, Some(50), 1, Some(0), 1);
        let second = run("020", "builder", true, 1_400, Some(100), cc, Some(0), cr);
        cache_report(&[vec![first, second]], None, 2_000)
    }

    #[test]
    fn load_usage_reads_the_split_and_the_duration_when_present(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("usage.jsonl");
        std::fs::write(
            &log,
            "{\"job\":\"001\",\"role\":\"builder\",\"cache_creation_1h\":5,\"secs\":9}\n",
        )?;
        let records = load_usage(&log, "001")?;
        assert_eq!(records[0].cache_creation_1h, Some(5));
        assert_eq!(records[0].secs, Some(9));
        Ok(())
    }

    #[test]
    fn load_usage_leaves_split_and_duration_none_on_old_lines(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("usage.jsonl");
        std::fs::write(&log, "{\"job\":\"001\",\"role\":\"builder\"}\n")?;
        let records = load_usage(&log, "001")?;
        assert_eq!(records[0].cache_creation_1h, None);
        assert_eq!(records[0].secs, None);
        Ok(())
    }

    #[test]
    fn cache_roles_sum_creation_read_and_split() {
        let rows = vec![
            run("1", "builder", false, 1, None, 30, Some(10), 5),
            run("1", "builder", false, 2, None, 50, Some(20), 7),
        ];
        let r = cache_report(&[rows], None, 10);
        assert_eq!(r.roles.len(), 1);
        let b = &r.roles[0];
        assert_eq!((b.runs, b.creation, b.read), (2, 80, 12));
        assert_eq!((b.one_hour, b.five_min, b.split_runs), (30, 50, 2));
    }

    #[test]
    fn cache_split_is_n_a_when_no_row_has_it() {
        let rows = vec![run("1", "builder", false, 1, None, 30, None, 5)];
        let text = render_cache(&cache_report(&[rows], None, 10));
        let line = text
            .lines()
            .find(|l| l.starts_with("builder"))
            .expect("role row");
        assert_eq!(line.matches("n/a").count(), 2, "{text}");
        assert!(!text.contains("split covers"), "{text}");
    }

    #[test]
    fn cache_footnote_shows_split_coverage() {
        let rows = vec![
            run("1", "builder", false, 1, None, 30, Some(1), 5),
            run("1", "builder", false, 2, None, 30, Some(1), 5),
            run("1", "builder", false, 3, None, 30, None, 5),
        ];
        let text = render_cache(&cache_report(&[rows], None, 10));
        assert!(text.contains("split covers 2 of 3 runs"), "{text}");
    }

    #[test]
    fn resumed_gap_is_ts_minus_secs_minus_previous_ts() {
        let first = run("020", "builder", false, 1_000, Some(50), 100, Some(0), 900);
        let second = run(
            "020",
            "builder",
            true,
            1_400,
            Some(100),
            3_415,
            Some(0),
            118_160,
        );
        let r = cache_report(&[vec![first, second]], None, 2_000);
        assert_eq!(r.resumed.len(), 1);
        assert_eq!(r.resumed[0].gap, Some(300));
    }

    #[test]
    fn resumed_gap_is_unknown_without_secs_or_predecessor() {
        let first = run("1", "builder", true, 1_000, Some(5), 1, None, 1);
        let second = run("1", "builder", true, 1_400, None, 1, None, 1);
        let r = cache_report(&[vec![first, second]], None, 2_000);
        assert_eq!(r.resumed.len(), 2);
        assert_eq!(r.resumed[0].gap, None);
        assert_eq!(r.resumed[1].gap, None);
        let text = render_cache(&r);
        assert!(text.contains('?'), "{text}");
    }

    #[test]
    fn resumed_gap_floors_at_zero() {
        let first = run("1", "builder", false, 1_000, Some(5), 1, None, 1);
        let second = run("1", "builder", true, 1_050, Some(100), 1, None, 1);
        let r = cache_report(&[vec![first, second]], None, 2_000);
        assert_eq!(r.resumed[0].gap, Some(0));
    }

    #[test]
    fn rewrite_threshold_is_25_percent_inclusive() {
        assert!(!resumed_pair(249, 751).resumed[0].rewrote());
        assert!(resumed_pair(250, 750).resumed[0].rewrote());
    }

    #[test]
    fn zero_creation_and_read_is_never_a_rewrite() {
        let r = resumed_pair(0, 0);
        assert_eq!(r.resumed[0].share(), None);
        assert!(!r.resumed[0].rewrote());
        let text = render_cache(&r);
        let line = text
            .lines()
            .find(|l| l.starts_with("020"))
            .expect("resumed row");
        assert!(line.contains(" - "), "{line}");
    }

    #[test]
    fn summary_counts_idle_rewrites_and_unknown_gaps() {
        let rows = vec![
            run("1", "builder", false, 1_000, Some(1), 1, None, 1),
            // Idle rewrite: the gap is 1000 seconds.
            run("1", "builder", true, 2_100, Some(100), 500, None, 500),
            // Idle non-rewrite: gap 1000.
            run("1", "builder", true, 3_200, Some(100), 1, None, 999),
            // Unknown gap rewrite.
            run("1", "builder", true, 3_300, None, 600, None, 400),
        ];
        let text = render_cache(&cache_report(&[rows], None, 4_000));
        assert!(
            text.contains(
                "resumed runs: 3, idle over 5m: 2, of those rewrote: 1, rewrote with gap unknown: 1"
            ),
            "{text}"
        );
    }

    #[test]
    fn days_filter_lists_recent_runs_but_keeps_the_predecessor_gap() {
        let now = 1_000_000;
        let old = run("1", "builder", false, 100, Some(1), 1, None, 1);
        let recent = run("1", "builder", true, now - 100, Some(50), 1, None, 1);
        let r = cache_report(&[vec![old, recent]], Some(1), now);
        assert_eq!(r.runs, 1);
        assert_eq!(r.resumed.len(), 1);
        assert!(r.resumed[0].gap.is_some());
    }

    #[test]
    fn days_cutoff_is_inclusive() {
        let now = 1_000_000;
        let edge = run("1", "builder", false, now - 86_400, None, 1, None, 1);
        let older = run("1", "reviewer", false, now - 86_401, None, 1, None, 1);
        let r = cache_report(&[vec![older, edge]], Some(1), now);
        assert_eq!(r.runs, 1);
        assert_eq!(r.roles[0].role, "builder");
    }

    #[test]
    fn no_resumed_runs_prints_the_empty_line() {
        let rows = vec![run("1", "builder", false, 1, None, 1, None, 1)];
        let text = render_cache(&cache_report(&[rows], None, 10));
        assert!(text.contains("no resumed runs"), "{text}");
        assert!(!text.contains("resumed runs:"), "{text}");
    }

    #[test]
    fn header_names_all_time_or_the_day_window_and_empty_prints_no_runs() {
        let all = render_cache(&cache_report(&[], None, 10));
        assert!(all.starts_with("cache report (0 runs, all time)"), "{all}");
        assert!(all.contains("no runs"), "{all}");
        let week = render_cache(&cache_report(&[], Some(7), 10));
        assert!(
            week.contains("cache report (0 runs, last 7 days)"),
            "{week}"
        );
        let rows = vec![run("1", "builder", false, 5, None, 1, None, 1)];
        let some = render_cache(&cache_report(&[rows], None, 10));
        assert!(some.contains("cache report (1 runs, all time)"), "{some}");
    }

    #[test]
    fn creation_percent_is_one_decimal_and_dash_when_empty() {
        let rows = vec![
            run("1", "builder", false, 1, None, 1, None, 3),
            run("1", "reviewer", false, 2, None, 0, None, 0),
        ];
        let text = render_cache(&cache_report(&[rows], None, 10));
        assert!(text.contains("25.0%"), "{text}");
        let line = text
            .lines()
            .find(|l| l.starts_with("reviewer"))
            .expect("reviewer row");
        assert!(line.contains(" - "), "{line}");
    }

    #[test]
    fn roles_are_sorted_by_name() {
        let rows = vec![
            run("1", "reviewer", false, 1, None, 1, None, 1),
            run("1", "builder", false, 2, None, 1, None, 1),
        ];
        let r = cache_report(&[rows], None, 10);
        assert_eq!(r.roles[0].role, "builder");
        assert_eq!(r.roles[1].role, "reviewer");
    }

    #[test]
    fn five_m_split_saturates() {
        let rows = vec![run("1", "builder", false, 1, None, 5, Some(9), 1)];
        let r = cache_report(&[rows], None, 10);
        assert_eq!(r.roles[0].five_min, 0);
    }
}
