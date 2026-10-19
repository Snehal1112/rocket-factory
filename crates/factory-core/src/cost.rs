//! Job cost records, group statistics and the estimate for a new job.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::budget::weighted_tokens;
use crate::config::Config;
use crate::error::CoreError;
use crate::fsutil::write_atomic;

/// How big a job is, by the size of its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeGroup {
    Small,
    Medium,
    Complex,
}

impl SizeGroup {
    pub const ALL: [SizeGroup; 3] = [SizeGroup::Small, SizeGroup::Medium, SizeGroup::Complex];

    pub fn as_str(self) -> &'static str {
        match self {
            SizeGroup::Small => "small",
            SizeGroup::Medium => "medium",
            SizeGroup::Complex => "complex",
        }
    }
}

/// The group of a plan of `plan_bytes` bytes.
pub fn group_of(plan_bytes: u64, cfg: &Config) -> SizeGroup {
    if plan_bytes < cfg.size_small_max_bytes {
        SizeGroup::Small
    } else if plan_bytes <= cfg.size_medium_max_bytes {
        SizeGroup::Medium
    } else {
        SizeGroup::Complex
    }
}

/// The number of `### Task N` headings outside fenced code.
pub fn count_tasks(plan: &str) -> u32 {
    let mut in_fence = false;
    let mut count = 0u32;
    for line in plan.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = line.strip_prefix("### Task ") {
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                count = count.saturating_add(1);
            }
        }
    }
    count
}

/// What `usage.jsonl` says about one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub weighted: u64,
    pub raw: u64,
    pub runs: u32,
    /// True when any line predates the corrected output count (`usage_v` below 2).
    pub approx: bool,
    /// Weighted tokens per agent role.
    pub roles: BTreeMap<String, u64>,
}

/// Sums the lines of `job`. Lines of other jobs and lines that are not JSON are skipped.
pub fn usage_summary(text: &str, job: &str) -> UsageSummary {
    let mut s = UsageSummary::default();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["job"] != job {
            continue;
        }
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let (input, cc, out, cr) = (
            n("input"),
            n("cache_creation"),
            n("output"),
            n("cache_read"),
        );
        let weighted = weighted_tokens(input, cc, out, cr);
        let raw = input
            .saturating_add(cc)
            .saturating_add(out)
            .saturating_add(cr);
        s.weighted = s.weighted.saturating_add(weighted);
        s.raw = s.raw.saturating_add(raw);
        s.runs = s.runs.saturating_add(1);
        if n("usage_v") < 2 {
            s.approx = true;
        }
        let role = v["role"].as_str().unwrap_or("unknown").to_string();
        let slot = s.roles.entry(role).or_insert(0);
        *slot = slot.saturating_add(weighted);
    }
    s
}

/// The estimated cost of a new job, from the history at the time it was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estimate {
    /// `small`, `medium`, `complex`, or `all` when the group had no jobs.
    pub group: String,
    pub median: u64,
    pub low: u64,
    pub high: u64,
    pub n: u32,
    pub n_approx: u32,
}

/// The cost of one board job, derived from its folder and git.
#[derive(Debug, Clone, PartialEq)]
pub struct JobCost {
    pub id: String,
    pub slug: String,
    pub stage: String,
    pub group: Option<SizeGroup>,
    pub plan_bytes: u64,
    pub tasks: u32,
    pub lines: Option<u64>,
    pub files: Option<u64>,
    pub weighted: u64,
    pub raw: u64,
    pub runs: u32,
    pub approx: bool,
    pub roles: BTreeMap<String, u64>,
}

impl JobCost {
    /// True when the job ran at least once and has a plan to size it by.
    pub fn has_data(&self) -> bool {
        self.runs > 0 && self.group.is_some()
    }
}

/// Median and range over a set of jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spread {
    pub n: u32,
    pub median: u64,
    pub low: u64,
    pub high: u64,
}

/// The figures of one size group, or of all groups together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupStats {
    pub n: u32,
    pub n_approx: u32,
    pub median: u64,
    pub low: u64,
    pub high: u64,
    /// Share of the weighted tokens per role, in whole percent.
    pub roles: BTreeMap<String, u32>,
    /// The same figures over jobs that are not approximate, or `None` when there are none.
    pub corrected: Option<Spread>,
}

fn median_of(sorted: &[u64]) -> u64 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        return sorted[n / 2];
    }
    let (a, b) = (sorted[n / 2 - 1], sorted[n / 2]);
    // Half of each first keeps the sum from overflowing.
    a / 2 + b / 2 + (a % 2 + b % 2) / 2
}

fn spread(values: &mut [u64]) -> Spread {
    values.sort_unstable();
    Spread {
        n: values.len() as u32,
        median: median_of(values),
        low: values.first().copied().unwrap_or(0),
        high: values.last().copied().unwrap_or(0),
    }
}

fn group_stats(jobs: &[&JobCost]) -> GroupStats {
    let mut all: Vec<u64> = jobs.iter().map(|j| j.weighted).collect();
    let mut fixed: Vec<u64> = jobs
        .iter()
        .filter(|j| !j.approx)
        .map(|j| j.weighted)
        .collect();
    let s = spread(&mut all);
    let corrected = if fixed.is_empty() {
        None
    } else {
        Some(spread(&mut fixed))
    };
    let mut role_totals: BTreeMap<String, u128> = BTreeMap::new();
    let mut total: u128 = 0;
    for j in jobs {
        for (role, w) in &j.roles {
            *role_totals.entry(role.clone()).or_insert(0) += u128::from(*w);
            total += u128::from(*w);
        }
    }
    let roles = role_totals
        .into_iter()
        .map(|(role, w)| {
            (
                role,
                (w * 100 + total / 2)
                    .checked_div(total)
                    .map_or(0, |share| share as u32),
            )
        })
        .collect();
    GroupStats {
        n: s.n,
        n_approx: jobs.iter().filter(|j| j.approx).count() as u32,
        median: s.median,
        low: s.low,
        high: s.high,
        roles,
        corrected,
    }
}

/// Statistics per size group over merged jobs with data, plus an `all` entry.
pub fn stats(jobs: &[JobCost]) -> BTreeMap<String, GroupStats> {
    let feeding: Vec<&JobCost> = jobs
        .iter()
        .filter(|j| j.stage == "merged" && j.has_data())
        .collect();
    let mut out = BTreeMap::new();
    if feeding.is_empty() {
        return out;
    }
    for group in SizeGroup::ALL {
        let members: Vec<&JobCost> = feeding
            .iter()
            .copied()
            .filter(|j| j.group == Some(group))
            .collect();
        if !members.is_empty() {
            out.insert(group.as_str().to_string(), group_stats(&members));
        }
    }
    out.insert("all".to_string(), group_stats(&feeding));
    out
}

/// The estimate for a plan of `plan_bytes` bytes. It uses the plan's group and falls back to all sizes.
pub fn estimate_for(
    plan_bytes: u64,
    stats: &BTreeMap<String, GroupStats>,
    cfg: &Config,
) -> Option<Estimate> {
    let group = group_of(plan_bytes, cfg).as_str();
    let (name, g) = match stats.get(group) {
        Some(g) => (group, g),
        None => ("all", stats.get("all")?),
    };
    Some(Estimate {
        group: name.to_string(),
        median: g.median,
        low: g.low,
        high: g.high,
        n: g.n,
        n_approx: g.n_approx,
    })
}

/// The size thresholds the figures were made with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thresholds {
    pub small_max_bytes: u64,
    pub medium_max_bytes: u64,
}

/// The content of `costs.json`: the group figures and the role split, never the per-job list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostsFile {
    pub generated_at: f64,
    pub thresholds: Thresholds,
    pub groups: BTreeMap<String, GroupStats>,
    pub roles: BTreeMap<String, u32>,
}

impl CostsFile {
    pub fn build(jobs: &[JobCost], cfg: &Config, now: f64) -> CostsFile {
        let groups = stats(jobs);
        let roles = groups
            .get("all")
            .map(|g| g.roles.clone())
            .unwrap_or_default();
        CostsFile {
            generated_at: now,
            thresholds: Thresholds {
                small_max_bytes: cfg.size_small_max_bytes,
                medium_max_bytes: cfg.size_medium_max_bytes,
            },
            groups,
            roles,
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), CoreError> {
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn group_edges_follow_the_thresholds() {
        let cfg = Config::default();
        assert_eq!(group_of(0, &cfg), SizeGroup::Small);
        assert_eq!(group_of(12_287, &cfg), SizeGroup::Small);
        assert_eq!(group_of(12_288, &cfg), SizeGroup::Medium);
        assert_eq!(group_of(25_600, &cfg), SizeGroup::Medium);
        assert_eq!(group_of(25_601, &cfg), SizeGroup::Complex);
        assert_eq!(group_of(u64::MAX, &cfg), SizeGroup::Complex);
    }

    #[test]
    fn group_names_are_stable() {
        let names: Vec<&str> = SizeGroup::ALL.iter().map(|g| g.as_str()).collect();
        assert_eq!(names, vec!["small", "medium", "complex"]);
    }

    #[test]
    fn count_tasks_counts_headings_outside_code_fences() {
        let plan = "# Plan\n\n### Task 1: One\n\ntext\n\n```markdown\n### Task 9: quoted\n```\n\n### Task 2: Two\n### Task Z: not a task\n";
        assert_eq!(count_tasks(plan), 2);
        assert_eq!(count_tasks(""), 0);
    }

    #[test]
    fn usage_summary_sums_this_jobs_lines_and_weights_cache_reads() {
        let text = concat!(
            "{\"job\":\"001\",\"role\":\"builder\",\"input\":100,\"cache_creation\":200,\"output\":300,\"cache_read\":1000,\"usage_v\":2}\n",
            "{\"job\":\"002\",\"role\":\"builder\",\"input\":9,\"cache_creation\":9,\"output\":9,\"cache_read\":9,\"usage_v\":2}\n",
            "garbage line\n",
            "{\"job\":\"001\",\"role\":\"reviewer\",\"input\":10,\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"usage_v\":2}\n",
        );
        let s = usage_summary(text, "001");
        assert_eq!(s.runs, 2);
        assert_eq!(s.weighted, (100 + 200 + 300 + 100) + 10);
        assert_eq!(s.raw, (100 + 200 + 300 + 1000) + 10);
        assert!(!s.approx);
        assert_eq!(s.roles.get("builder"), Some(&700));
        assert_eq!(s.roles.get("reviewer"), Some(&10));
    }

    #[test]
    fn a_line_without_usage_v_makes_the_summary_approximate() {
        let text = "{\"job\":\"001\",\"role\":\"builder\",\"input\":1,\"cache_creation\":1,\"output\":1,\"cache_read\":10}\n\
                    {\"job\":\"001\",\"role\":\"builder\",\"input\":1,\"cache_creation\":1,\"output\":1,\"cache_read\":10,\"usage_v\":2}\n";
        assert!(usage_summary(text, "001").approx);
    }

    #[test]
    fn usage_summary_of_nothing_is_empty_and_not_approximate() {
        let s = usage_summary("", "001");
        assert_eq!(s.runs, 0);
        assert_eq!(s.weighted, 0);
        assert!(!s.approx);
    }

    #[test]
    fn huge_counts_do_not_overflow() {
        let text = format!(
            "{{\"job\":\"001\",\"role\":\"builder\",\"input\":{m},\"cache_creation\":{m},\"output\":{m},\"cache_read\":{m},\"usage_v\":2}}\n",
            m = u64::MAX
        );
        let s = usage_summary(&text, "001");
        assert_eq!(s.raw, u64::MAX);
        assert_eq!(s.weighted, u64::MAX);
    }

    fn job(id: &str, group: SizeGroup, stage: &str, weighted: u64, approx: bool) -> JobCost {
        let mut roles = BTreeMap::new();
        let builder = (u128::from(weighted) * 7 / 10) as u64;
        roles.insert("builder".to_string(), builder);
        roles.insert("reviewer".to_string(), weighted - builder);
        JobCost {
            id: id.into(),
            slug: "s".into(),
            stage: stage.into(),
            group: Some(group),
            plan_bytes: 1000,
            tasks: 2,
            lines: Some(100),
            files: Some(3),
            weighted,
            raw: weighted.saturating_mul(4),
            runs: 3,
            approx,
            roles,
        }
    }

    #[test]
    fn stats_median_low_high_for_odd_and_even_counts() {
        let odd = [
            job("1", SizeGroup::Small, "merged", 300, false),
            job("2", SizeGroup::Small, "merged", 100, false),
            job("3", SizeGroup::Small, "merged", 200, false),
        ];
        let s = stats(&odd);
        let small = &s["small"];
        assert_eq!(
            (small.n, small.median, small.low, small.high),
            (3, 200, 100, 300)
        );
        let even = [
            job("1", SizeGroup::Medium, "merged", 100, false),
            job("2", SizeGroup::Medium, "merged", 201, false),
        ];
        let m = &stats(&even)["medium"];
        assert_eq!((m.n, m.median, m.low, m.high), (2, 150, 100, 201));
    }

    #[test]
    fn only_merged_jobs_with_data_feed_the_stats() {
        let mut no_data = job("3", SizeGroup::Small, "merged", 999, false);
        no_data.runs = 0;
        let mut no_group = job("4", SizeGroup::Small, "merged", 999, false);
        no_group.group = None;
        let jobs = [
            job("1", SizeGroup::Small, "merged", 100, false),
            job("2", SizeGroup::Small, "needs-human", 900, false),
            no_data,
            no_group,
        ];
        let s = stats(&jobs);
        assert_eq!(s["small"].n, 1);
        assert_eq!(s["all"].n, 1);
        assert!(!s.contains_key("medium"));
    }

    #[test]
    fn approximate_jobs_are_counted_and_corrected_figures_exclude_them() {
        let jobs = [
            job("1", SizeGroup::Small, "merged", 100, true),
            job("2", SizeGroup::Small, "merged", 300, false),
            job("3", SizeGroup::Small, "merged", 500, false),
        ];
        let small = &stats(&jobs)["small"];
        assert_eq!((small.n, small.n_approx), (3, 1));
        let c = small.corrected.as_ref().expect("corrected");
        assert_eq!((c.n, c.median, c.low, c.high), (2, 400, 300, 500));
        let only_old = [job("1", SizeGroup::Small, "merged", 100, true)];
        assert!(stats(&only_old)["small"].corrected.is_none());
    }

    #[test]
    fn role_shares_add_up_to_one_hundred_within_rounding() {
        let jobs = [
            job("1", SizeGroup::Small, "merged", 1000, false),
            job("2", SizeGroup::Small, "merged", 1000, false),
        ];
        let roles = &stats(&jobs)["small"].roles;
        assert_eq!(roles["builder"], 70);
        assert_eq!(roles["reviewer"], 30);
        let total: u32 = roles.values().sum();
        assert!((99..=101).contains(&total), "{total}");
    }

    #[test]
    fn empty_input_gives_no_stats_and_no_estimate() {
        let s = stats(&[]);
        assert!(s.is_empty());
        assert_eq!(estimate_for(5000, &s, &Config::default()), None);
    }

    #[test]
    fn huge_weighted_values_do_not_overflow_the_median() {
        let jobs = [
            job("1", SizeGroup::Complex, "merged", u64::MAX, false),
            job("2", SizeGroup::Complex, "merged", u64::MAX, false),
        ];
        assert_eq!(stats(&jobs)["complex"].median, u64::MAX);
    }

    #[test]
    fn the_estimate_uses_the_group_of_the_plan_and_falls_back_to_all() {
        let jobs = [
            job("1", SizeGroup::Small, "merged", 200, false),
            job("2", SizeGroup::Complex, "merged", 400, true),
        ];
        let s = stats(&jobs);
        let cfg = Config::default();
        let small = estimate_for(5_000, &s, &cfg).expect("small");
        assert_eq!(
            (small.group.as_str(), small.median, small.n, small.n_approx),
            ("small", 200, 1, 0)
        );
        let complex = estimate_for(40_000, &s, &cfg).expect("complex");
        assert_eq!((complex.group.as_str(), complex.n_approx), ("complex", 1));
        // No medium jobs, so the overall figures are used.
        let medium = estimate_for(15_000, &s, &cfg).expect("fallback");
        assert_eq!(
            (medium.group.as_str(), medium.n, medium.low, medium.high),
            ("all", 2, 200, 400)
        );
    }

    #[test]
    fn the_costs_file_has_groups_roles_and_thresholds_and_writes_atomically(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let jobs = [job("1", SizeGroup::Small, "merged", 1000, false)];
        let file = CostsFile::build(&jobs, &Config::default(), 42.0);
        assert_eq!(file.thresholds.small_max_bytes, 12_288);
        assert_eq!(file.roles["builder"], 70);
        assert!(file.groups.contains_key("small") && file.groups.contains_key("all"));
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sub").join("costs.json");
        file.write(&path)?;
        let raw = std::fs::read_to_string(&path)?;
        assert!(raw.contains("\"generated_at\": 42.0"), "{raw}");
        assert!(!raw.contains("\"jobs\""), "the per-job list is not written");
        Ok(())
    }
}
