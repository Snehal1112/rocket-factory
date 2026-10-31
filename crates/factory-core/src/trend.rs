//! The cost trend: median weighted tokens per changed line before and after each marker.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::CostMarker;
use crate::cost::{median_of, spread, JobCost, SizeGroup, Spread};

/// The fewest eligible jobs a side needs before a group shows numbers.
pub const MIN_JOBS_PER_SIDE: u32 = 3;
/// The dead band of the verdict, in percent.
pub const VERDICT_BAND_PCT: i64 = 10;

/// The trend of every marker, plus the markers that could not be placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendReport {
    pub markers: Vec<MarkerTrend>,
    pub warnings: Vec<String>,
}

/// The trend around one cut. Several markers on one job share a cut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerTrend {
    /// More than one name when markers collapse onto one cut.
    pub names: Vec<String>,
    /// The configured job ids, in the order of `names`.
    pub from: Vec<String>,
    /// The resolved job id.
    pub cut: String,
    /// The figures per size group name.
    pub groups: BTreeMap<String, GroupTrend>,
}

/// The change of one size group across a cut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupTrend {
    pub before: Side,
    pub after: Side,
    pub change_pct: Option<i64>,
    /// One of `fell`, `rose`, `flat`, `not-enough-data`.
    pub verdict: String,
}

/// The figures of one side of a cut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Side {
    pub n: u32,
    pub n_approx: u32,
    /// Weighted tokens per changed line over the jobs of the side.
    pub per_line: Option<Spread>,
    pub median_turns: Option<u64>,
    pub resumed_pct: Option<u32>,
    pub rework_pct: Option<u32>,
    pub park_pct: Option<u32>,
}

const NOT_ENOUGH: &str = "not-enough-data";

/// Builds the report, or `None` when there are no markers.
pub fn build_trend(jobs: &[JobCost], markers: &[CostMarker]) -> Option<TrendReport> {
    if markers.is_empty() {
        return None;
    }
    let mut board: Vec<u64> = jobs.iter().filter_map(|j| j.id.parse().ok()).collect();
    board.sort_unstable();
    let mut warnings = Vec::new();
    // Resolved cut number, then the marker name and configured id.
    let mut placed: Vec<(u64, &CostMarker)> = Vec::new();
    for m in markers {
        let Ok(wanted) = m.job.parse::<u64>() else {
            warnings.push(format!(
                "marker {} (job {}) is not a job number",
                m.name, m.job
            ));
            continue;
        };
        match board.iter().find(|&&id| id >= wanted) {
            Some(&cut) => placed.push((cut, m)),
            None => warnings.push(format!(
                "marker {} (job {}) is past the latest job",
                m.name, m.job
            )),
        }
    }
    placed.sort_by_key(|(cut, _)| *cut);
    let mut cuts: Vec<(u64, Vec<&CostMarker>)> = Vec::new();
    for (cut, m) in placed {
        match cuts.last_mut() {
            Some((last, group)) if *last == cut => group.push(m),
            _ => cuts.push((cut, vec![m])),
        }
    }
    let eligible: Vec<(u64, &JobCost)> = jobs
        .iter()
        .filter(|j| j.stage == "merged" && j.has_data() && j.lines.is_some_and(|l| l > 0))
        .filter_map(|j| Some((j.id.parse().ok()?, j)))
        .collect();
    let trends = cuts
        .iter()
        .enumerate()
        .map(|(i, (cut, ms))| {
            let next = cuts.get(i + 1).map_or(u64::MAX, |(n, _)| *n);
            MarkerTrend {
                names: ms.iter().map(|m| m.name.clone()).collect(),
                from: ms.iter().map(|m| m.job.clone()).collect(),
                cut: cut_id(jobs, *cut),
                groups: group_trends(&eligible, *cut, next),
            }
        })
        .collect();
    Some(TrendReport {
        markers: trends,
        warnings,
    })
}

/// The id text of the board job numbered `cut`.
fn cut_id(jobs: &[JobCost], cut: u64) -> String {
    jobs.iter()
        .find(|j| j.id.parse::<u64>().ok() == Some(cut))
        .map(|j| j.id.clone())
        .unwrap_or_else(|| cut.to_string())
}

fn group_trends(eligible: &[(u64, &JobCost)], cut: u64, next: u64) -> BTreeMap<String, GroupTrend> {
    let mut out = BTreeMap::new();
    for group in SizeGroup::ALL {
        let in_group = |lo: u64, hi: u64| -> Vec<&JobCost> {
            eligible
                .iter()
                .filter(|(n, j)| *n >= lo && *n < hi && j.group == Some(group))
                .map(|(_, j)| *j)
                .collect()
        };
        let before = in_group(0, cut);
        let after = in_group(cut, next);
        if before.is_empty() && after.is_empty() {
            continue;
        }
        out.insert(group.as_str().to_string(), group_trend(&before, &after));
    }
    out
}

fn group_trend(before: &[&JobCost], after: &[&JobCost]) -> GroupTrend {
    let enough = |s: &[&JobCost]| s.len() as u32 >= MIN_JOBS_PER_SIDE;
    let full = enough(before) && enough(after);
    let (b, a) = (side(before, full), side(after, full));
    let medians = match (&b.per_line, &a.per_line) {
        (Some(x), Some(y)) if x.median > 0 => Some((x.median, y.median)),
        _ => None,
    };
    let Some((bm, am)) = medians else {
        return GroupTrend {
            before: side(before, false),
            after: side(after, false),
            change_pct: None,
            verdict: NOT_ENOUGH.to_string(),
        };
    };
    let change = ((am as f64 - bm as f64) * 100.0 / bm as f64).round() as i64;
    let verdict = if change <= -VERDICT_BAND_PCT {
        "fell"
    } else if change >= VERDICT_BAND_PCT {
        "rose"
    } else {
        "flat"
    };
    GroupTrend {
        before: b,
        after: a,
        change_pct: Some(change),
        verdict: verdict.to_string(),
    }
}

/// The figures of `jobs`, or only the counts when `full` is false.
fn side(jobs: &[&JobCost], full: bool) -> Side {
    let n = jobs.len() as u32;
    let n_approx = jobs.iter().filter(|j| j.approx).count() as u32;
    if !full {
        return Side {
            n,
            n_approx,
            per_line: None,
            median_turns: None,
            resumed_pct: None,
            rework_pct: None,
            park_pct: None,
        };
    }
    let mut ratios: Vec<u64> = jobs.iter().map(|j| ratio(j)).collect();
    let mut turns: Vec<u64> = jobs.iter().map(|j| u64::from(j.turns)).collect();
    turns.sort_unstable();
    let runs: u64 = jobs.iter().map(|j| u64::from(j.runs)).sum();
    let resumed: u64 = jobs.iter().map(|j| u64::from(j.resumed_runs)).sum();
    let count = |f: fn(&JobCost) -> bool| jobs.iter().filter(|j| f(j)).count() as u64;
    Side {
        n,
        n_approx,
        per_line: Some(spread(&mut ratios)),
        median_turns: Some(median_of(&turns)),
        resumed_pct: Some(pct(resumed, runs)),
        rework_pct: Some(pct(count(|j| j.round >= 2), u64::from(n))),
        park_pct: Some(pct(count(|j| j.parked), u64::from(n))),
    }
}

/// Weighted tokens per changed line, rounded half up. Eligible jobs have lines above 0.
fn ratio(job: &JobCost) -> u64 {
    let lines = u128::from(job.lines.unwrap_or(1).max(1));
    let r = (u128::from(job.weighted) + lines / 2) / lines;
    u64::try_from(r).unwrap_or(u64::MAX)
}

/// `num` over `den` as a whole percent rounded half up, 0 when `den` is 0.
fn pct(num: u64, den: u64) -> u32 {
    if den == 0 {
        return 0;
    }
    let p = (u128::from(num) * 100 + u128::from(den) / 2) / u128::from(den);
    u32::try_from(p).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jc(id: &str, group: SizeGroup, weighted: u64, lines: u64) -> JobCost {
        JobCost {
            id: id.into(),
            stage: "merged".into(),
            group: Some(group),
            lines: Some(lines),
            weighted,
            runs: 2,
            turns: 10,
            resumed_runs: 1,
            ..JobCost::default()
        }
    }

    fn marker(name: &str, job: &str) -> CostMarker {
        CostMarker {
            name: name.into(),
            job: job.into(),
        }
    }

    /// Jobs 001 to 006 in the small group, lines 1, so weighted is the ratio.
    fn six(a: u64, b: u64) -> Vec<JobCost> {
        (1..=6)
            .map(|i| {
                jc(
                    &format!("{i:03}"),
                    SizeGroup::Small,
                    if i <= 3 { a } else { b },
                    1,
                )
            })
            .collect()
    }

    fn small(report: &TrendReport) -> &GroupTrend {
        &report.markers[0].groups["small"]
    }

    #[test]
    fn before_is_everything_below_the_cut_and_after_stops_at_the_next_cut() {
        let jobs: Vec<JobCost> = (1..=9)
            .map(|i| {
                let w = match i {
                    1..=3 => 100_000,
                    4..=6 => 50_000,
                    _ => 25_000,
                };
                jc(&format!("{i:03}"), SizeGroup::Small, w, 100)
            })
            .collect();
        let report =
            build_trend(&jobs, &[marker("m2", "007"), marker("m1", "004")]).expect("report");
        let cuts: Vec<&str> = report.markers.iter().map(|m| m.cut.as_str()).collect();
        assert_eq!(cuts, vec!["004", "007"]);
        let g = &report.markers[0].groups["small"];
        assert_eq!((g.before.n, g.after.n), (3, 3));
        assert_eq!(g.before.per_line.as_ref().expect("before").median, 1000);
        assert_eq!(g.after.per_line.as_ref().expect("after").median, 500);
        assert_eq!((g.change_pct, g.verdict.as_str()), (Some(-50), "fell"));
        let g = &report.markers[1].groups["small"];
        assert_eq!((g.before.n, g.after.n), (6, 3));
        assert_eq!(g.before.per_line.as_ref().expect("before").median, 750);
        assert_eq!(g.after.per_line.as_ref().expect("after").median, 250);
        assert_eq!(g.change_pct, Some(-67));
    }

    #[test]
    fn no_markers_gives_no_report() {
        assert!(build_trend(&six(100, 50), &[]).is_none());
    }

    #[test]
    fn a_marker_on_a_missing_job_cuts_at_the_next_board_job() {
        let ids = ["001", "002", "003", "005", "006", "007"];
        let jobs: Vec<JobCost> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| jc(id, SizeGroup::Small, if i < 3 { 100 } else { 50 }, 1))
            .collect();
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        assert_eq!(report.markers[0].cut, "005");
        assert_eq!(report.markers[0].from, vec!["004"]);
    }

    #[test]
    fn marker_ids_compare_as_numbers() {
        let ids = ["001", "002", "003", "1000", "1001", "1002"];
        let jobs: Vec<JobCost> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| jc(id, SizeGroup::Small, if i < 3 { 100 } else { 50 }, 1))
            .collect();
        let report = build_trend(&jobs, &[marker("m", "999")]).expect("report");
        assert_eq!(report.markers[0].cut, "1000");
        let g = small(&report);
        assert_eq!((g.before.n, g.after.n), (3, 3));
    }

    #[test]
    fn markers_are_sorted_by_resolved_id() {
        let mut jobs = six(100, 50);
        jobs.extend((7..=9).map(|i| jc(&format!("{i:03}"), SizeGroup::Small, 25, 1)));
        let report =
            build_trend(&jobs, &[marker("late", "007"), marker("early", "004")]).expect("report");
        assert_eq!(report.markers[0].names, vec!["early"]);
        assert_eq!(report.markers[1].names, vec!["late"]);
    }

    #[test]
    fn a_marker_past_the_latest_job_warns_and_is_skipped() {
        let report = build_trend(&six(100, 50), &[marker("m", "099")]).expect("report");
        assert!(report.markers.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("past the latest job"));
        assert!(report.warnings[0].contains('m'));
    }

    #[test]
    fn a_marker_that_is_not_a_number_warns_and_is_skipped() {
        let report = build_trend(&six(100, 50), &[marker("m", "abc")]).expect("report");
        assert!(report.markers.is_empty());
        assert!(report.warnings[0].contains("not a job number"));
    }

    #[test]
    fn two_markers_on_one_job_share_a_block() {
        let report =
            build_trend(&six(100, 50), &[marker("a", "004"), marker("b", "004")]).expect("report");
        assert_eq!(report.markers.len(), 1);
        assert_eq!(report.markers[0].names, vec!["a", "b"]);
        assert_eq!(report.markers[0].from, vec!["004", "004"]);
    }

    #[test]
    fn non_numeric_job_ids_are_ignored_for_cuts() {
        let mut jobs = six(100, 50);
        let mut pseudo = jc("spec-review", SizeGroup::Small, 10, 1);
        pseudo.stage = "spec-review".into();
        jobs.push(pseudo);
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        assert_eq!(report.markers[0].cut, "004");
    }

    #[test]
    fn per_line_is_the_median_of_job_ratios() {
        let mut jobs: Vec<JobCost> = (1..=3)
            .map(|i| jc(&format!("{i:03}"), SizeGroup::Small, 100, 1))
            .collect();
        jobs.extend(
            [100, 200, 300, 400]
                .iter()
                .enumerate()
                .map(|(i, w)| jc(&format!("{:03}", i + 4), SizeGroup::Small, *w, 1)),
        );
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        let after = small(&report).after.per_line.as_ref().expect("after");
        assert_eq!((after.median, after.low, after.high), (250, 100, 400));
    }

    #[test]
    fn the_job_ratio_rounds_half_up() {
        let jobs: Vec<JobCost> = (1..=6)
            .map(|i| jc(&format!("{i:03}"), SizeGroup::Small, 3, 2))
            .collect();
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        let g = small(&report);
        assert_eq!(g.before.per_line.as_ref().expect("before").median, 2);
        assert_eq!(g.after.per_line.as_ref().expect("after").median, 2);
    }

    #[test]
    fn huge_weighted_values_do_not_overflow_the_ratio() {
        let report = build_trend(&six(100, u64::MAX), &[marker("m", "004")]).expect("report");
        let after = small(&report).after.per_line.as_ref().expect("after");
        assert_eq!(after.median, u64::MAX);
    }

    #[test]
    fn a_side_with_two_jobs_is_not_enough_data() {
        let jobs: Vec<JobCost> = (1..=5)
            .map(|i| jc(&format!("{i:03}"), SizeGroup::Small, 100, 1))
            .collect();
        let report = build_trend(&jobs, &[marker("m", "003")]).expect("report");
        let g = small(&report);
        assert_eq!(g.verdict, "not-enough-data");
        assert!(g.before.per_line.is_none() && g.after.per_line.is_none());
        assert_eq!(g.change_pct, None);
        assert_eq!(g.before.n, 2);
    }

    #[test]
    fn a_group_with_no_eligible_job_on_either_side_is_left_out() {
        let report = build_trend(&six(100, 50), &[marker("m", "004")]).expect("report");
        assert!(!report.markers[0].groups.contains_key("medium"));
    }

    #[test]
    fn a_group_with_jobs_on_one_side_only_is_not_enough_data() {
        let mut jobs = six(100, 50);
        jobs.extend((4..=6).map(|i| jc(&format!("{i:03}"), SizeGroup::Medium, 10, 1)));
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        let g = &report.markers[0].groups["medium"];
        assert_eq!(g.verdict, "not-enough-data");
        assert_eq!((g.before.n, g.after.n), (0, 3));
    }

    #[test]
    fn jobs_without_lines_or_data_are_left_out() {
        let mut jobs = six(100, 50);
        let mut no_lines = jc("007", SizeGroup::Small, 50, 1);
        no_lines.lines = None;
        let mut zero_lines = jc("008", SizeGroup::Small, 50, 1);
        zero_lines.lines = Some(0);
        let mut parked = jc("009", SizeGroup::Small, 50, 1);
        parked.stage = "needs-human".into();
        let mut no_runs = jc("010", SizeGroup::Small, 50, 1);
        no_runs.runs = 0;
        jobs.extend([no_lines, zero_lines, parked, no_runs]);
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        assert_eq!(small(&report).after.n, 3);
    }

    #[test]
    fn a_job_without_a_size_group_is_left_out() {
        let mut jobs = six(100, 50);
        let mut ungrouped = jc("007", SizeGroup::Small, 50, 1);
        ungrouped.group = None;
        jobs.push(ungrouped);
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        assert_eq!(small(&report).after.n, 3);
    }

    #[test]
    fn change_pct_and_verdict_use_the_ten_percent_band() {
        let cases = [
            (90, -10, "fell"),
            (91, -9, "flat"),
            (109, 9, "flat"),
            (110, 10, "rose"),
        ];
        for (x, pct, verdict) in cases {
            let report = build_trend(&six(100, x), &[marker("m", "004")]).expect("report");
            let g = small(&report);
            assert_eq!((g.change_pct, g.verdict.as_str()), (Some(pct), verdict));
        }
    }

    #[test]
    fn an_after_median_of_zero_is_a_drop_of_one_hundred_percent() {
        let report = build_trend(&six(100, 0), &[marker("m", "004")]).expect("report");
        let g = small(&report);
        assert_eq!((g.change_pct, g.verdict.as_str()), (Some(-100), "fell"));
    }

    #[test]
    fn a_zero_before_median_is_not_enough_data() {
        let report = build_trend(&six(0, 50), &[marker("m", "004")]).expect("report");
        let g = small(&report);
        assert_eq!(g.verdict, "not-enough-data");
        assert_eq!(g.change_pct, None);
    }

    #[test]
    fn turns_resumed_rework_and_park_shares() {
        let mut jobs: Vec<JobCost> = (1..=11)
            .map(|i| jc(&format!("{i:03}"), SizeGroup::Small, 100, 1))
            .collect();
        jobs[0].round = 2;
        jobs[8].parked = true;
        let report = build_trend(&jobs, &[marker("m", "009")]).expect("report");
        let g = small(&report);
        assert_eq!(g.before.rework_pct, Some(13));
        assert_eq!(g.after.park_pct, Some(33));
        assert_eq!(g.before.resumed_pct, Some(50));
        assert_eq!(g.before.median_turns, Some(10));
    }

    #[test]
    fn a_side_with_no_runs_has_a_zero_resumed_share() {
        assert_eq!(pct(0, 0), 0);
    }

    #[test]
    fn approximate_jobs_are_counted_per_side() {
        let mut jobs = six(100, 50);
        jobs[4].approx = true;
        let report = build_trend(&jobs, &[marker("m", "004")]).expect("report");
        let g = small(&report);
        assert_eq!((g.before.n_approx, g.after.n_approx), (0, 1));
    }
}
