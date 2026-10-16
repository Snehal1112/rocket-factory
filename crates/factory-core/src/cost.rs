//! Job cost records, group statistics and the estimate for a new job.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::budget::weighted_tokens;
use crate::config::Config;

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
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v["job"] != job {
            continue;
        }
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let (input, cc, out, cr) = (n("input"), n("cache_creation"), n("output"), n("cache_read"));
        let weighted = weighted_tokens(input, cc, out, cr);
        let raw = input.saturating_add(cc).saturating_add(out).saturating_add(cr);
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
}
