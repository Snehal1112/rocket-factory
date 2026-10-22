//! Text for `factory costs` and the estimate lines of `factory new`.

use std::collections::BTreeMap;

use factory_core::{Board, Estimate, GroupStats, Job, JobCost, SizeGroup};

use crate::status::human;

const FEW_JOBS: u32 = 3;

fn opt(n: Option<u64>) -> String {
    n.map_or("-".to_string(), |v| v.to_string())
}

/// The per-job table, the per-group table, the corrected figures and the role split.
pub fn render_costs(jobs: &[JobCost], groups: &BTreeMap<String, GroupStats>) -> String {
    if jobs.is_empty() {
        return "no jobs\n".to_string();
    }
    let mut out = String::from(
        "JOB  STAGE        GROUP    TASKS  LINES  FILES  WEIGHTED     RAW       RUNS\n",
    );
    for j in jobs {
        let group = j.group.map_or("-", SizeGroup::as_str);
        let weighted = if j.runs == 0 {
            "no data".to_string()
        } else if j.approx {
            format!("{} ~", human(j.weighted))
        } else {
            human(j.weighted)
        };
        out.push_str(&format!(
            "{:<4} {:<12} {:<8} {:<6} {:<6} {:<6} {:<12} {:<9} {}\n",
            j.id,
            j.stage,
            group,
            j.tasks,
            opt(j.lines),
            opt(j.files),
            weighted,
            human(j.raw),
            j.runs
        ));
    }
    out.push('\n');
    if groups.is_empty() {
        out.push_str("no merged jobs with data yet\n");
        return out;
    }
    out.push_str("GROUP    JOBS  MEDIAN     RANGE                NOTES\n");
    let mut corrected_lines = Vec::new();
    let order = ["small", "medium", "complex", "all"];
    for name in order {
        let Some(g) = groups.get(name) else { continue };
        let mut notes = Vec::new();
        if g.n < FEW_JOBS {
            notes.push("few jobs".to_string());
        }
        if g.n_approx > 0 {
            notes.push(format!("{} approx", g.n_approx));
        }
        out.push_str(&format!(
            "{:<8} {:<5} {:<10} {:<20} {}\n",
            name,
            g.n,
            human(g.median),
            format!("{}-{}", human(g.low), human(g.high)),
            notes.join(", ")
        ));
        if let Some(c) = &g.corrected {
            let plural = if c.n == 1 { "job" } else { "jobs" };
            corrected_lines.push(format!(
                "  {name} corrected: {} {plural}, median {} ({}-{})",
                c.n,
                human(c.median),
                human(c.low),
                human(c.high)
            ));
        }
    }
    if !corrected_lines.is_empty() {
        out.push_str("\nCORRECTED (jobs with exact output counts only)\n");
        for l in corrected_lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    if let Some(all) = groups.get("all") {
        if all.providers.len() >= 2 {
            out.push_str("\nROLES (share of weighted tokens by provider)\n");
            let mut by_provider: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for (pair, percent) in &all.provider_roles {
                if let Some((provider, role)) = pair.split_once(':') {
                    by_provider
                        .entry(provider)
                        .or_default()
                        .push(format!("{role} {percent}%"));
                }
            }
            for (provider, roles) in by_provider {
                out.push_str(&format!("  {provider}: {}\n", roles.join("  ")));
            }
            let providers: Vec<String> = all
                .providers
                .iter()
                .map(|(r, p)| format!("{r} {p}%"))
                .collect();
            out.push_str(&format!("PROVIDERS  {}\n", providers.join("  ")));
        } else {
            let roles: Vec<String> = all.roles.iter().map(|(r, p)| format!("{r} {p}%")).collect();
            out.push_str(&format!(
                "\nROLES (share of weighted tokens)\n  {}\n",
                roles.join("  ")
            ));
        }
    }
    out
}

/// One line for an estimate.
pub fn format_estimate(e: &Estimate) -> String {
    let group = if e.group == "all" {
        "all sizes".to_string()
    } else {
        e.group.clone()
    };
    let jobs = if e.n == 1 {
        "1 job".to_string()
    } else {
        format!("{} jobs", e.n)
    };
    let mut notes = vec![group, jobs];
    if e.n < FEW_JOBS {
        notes.push("few jobs".to_string());
    }
    if e.n_approx > 0 {
        notes.push(format!("{} approximate", e.n_approx));
    }
    format!(
        "estimate {} to {} weighted, median {} ({})",
        human(e.low),
        human(e.high),
        human(e.median),
        notes.join(", ")
    )
}

/// The estimate line for the first job and for every later job of its series.
pub fn estimate_lines(board: &Board, first: &Job) -> String {
    let series_id = first.series.as_ref().and_then(|s| s.id.clone());
    let mut out = String::new();
    for j in &board.jobs {
        let same = j.id == first.id
            || (series_id.is_some() && j.series.as_ref().and_then(|s| s.id.clone()) == series_id);
        if !same {
            continue;
        }
        match &j.estimate {
            Some(e) => out.push_str(&format!("\njob {}: {}", j.id, format_estimate(e))),
            None => out.push_str(&format!(
                "\njob {}: no estimate yet (no merged jobs with data)",
                j.id
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::{stats, JobCost, SizeGroup};
    fn cost(
        id: &str,
        group: Option<SizeGroup>,
        stage: &str,
        weighted: u64,
        approx: bool,
        runs: u32,
    ) -> JobCost {
        let mut roles = BTreeMap::new();
        roles.insert("builder".to_string(), weighted * 6 / 10);
        roles.insert("reviewer".to_string(), weighted - weighted * 6 / 10);
        JobCost {
            id: id.into(),
            slug: "s".into(),
            stage: stage.into(),
            group,
            plan_bytes: 5000,
            tasks: 2,
            lines: Some(300),
            files: Some(4),
            weighted,
            raw: weighted * 4,
            runs,
            approx,
            roles,
            providers: BTreeMap::from([("claude".to_string(), weighted)]),
            provider_roles: BTreeMap::from([
                ("claude:builder".to_string(), weighted * 6 / 10),
                ("claude:reviewer".to_string(), weighted - weighted * 6 / 10),
            ]),
        }
    }

    #[test]
    fn costs_with_two_providers_prints_a_providers_line() {
        let mut a = cost("1", Some(SizeGroup::Small), "merged", 700, false, 1);
        a.providers = BTreeMap::from([("claude".to_string(), 700)]);
        let mut b = cost("2", Some(SizeGroup::Small), "merged", 300, false, 1);
        b.providers = BTreeMap::from([("codex".to_string(), 300)]);
        b.provider_roles = BTreeMap::from([
            ("codex:builder".to_string(), 180),
            ("codex:reviewer".to_string(), 120),
        ]);
        let jobs = vec![a, b];
        let text = render_costs(&jobs, &stats(&jobs));
        assert!(text.contains("PROVIDERS  claude 70%  codex 30%"), "{text}");
        assert!(
            text.contains("ROLES (share of weighted tokens by provider)"),
            "{text}"
        );
        assert!(text.contains("claude: builder 42%  reviewer 28%"), "{text}");
        assert!(text.contains("codex: builder 18%  reviewer 12%"), "{text}");
    }

    #[test]
    fn costs_with_only_claude_prints_no_providers_line() {
        let jobs = vec![cost("1", Some(SizeGroup::Small), "merged", 1000, false, 1)];
        let text = render_costs(&jobs, &stats(&jobs));
        assert!(!text.contains("PROVIDERS"), "{text}");
    }

    #[test]
    fn the_per_job_table_marks_approximate_and_no_data_jobs() {
        let jobs = vec![
            cost("003", Some(SizeGroup::Small), "merged", 300_000, true, 2),
            cost("004", Some(SizeGroup::Small), "merged", 250_000, false, 3),
            cost("005", None, "build", 0, false, 0),
        ];
        let text = render_costs(&jobs, &stats(&jobs));
        let line = |id: &str| {
            text.lines()
                .find(|l| l.starts_with(id))
                .unwrap_or("")
                .to_string()
        };
        assert!(line("003").contains("300.0K ~"), "{text}");
        assert!(!line("004").contains('~'), "{text}");
        assert!(line("005").contains("no data"), "{text}");
        assert!(
            line("003").contains("small") && line("003").contains("300"),
            "{text}"
        );
    }

    #[test]
    fn a_group_table_shows_jobs_median_range_and_notes() {
        let jobs = vec![
            cost("1", Some(SizeGroup::Small), "merged", 200_000, true, 1),
            cost("2", Some(SizeGroup::Small), "merged", 400_000, false, 1),
        ];
        let text = render_costs(&jobs, &stats(&jobs));
        let small = text
            .lines()
            .find(|l| l.trim_start().starts_with("small"))
            .expect("small row");
        assert!(small.contains("300.0K"), "{small}");
        assert!(small.contains("200.0K-400.0K"), "{small}");
        assert!(small.contains("few jobs"), "{small}");
        assert!(small.contains("1 approx"), "{small}");
        assert!(text.contains("corrected: 1 job, median 400.0K"), "{text}");
    }

    #[test]
    fn the_role_split_is_printed() {
        let jobs = vec![cost(
            "1",
            Some(SizeGroup::Small),
            "merged",
            1_000_000,
            false,
            1,
        )];
        let text = render_costs(&jobs, &stats(&jobs));
        assert!(
            text.contains("builder 60%") && text.contains("reviewer 40%"),
            "{text}"
        );
    }

    #[test]
    fn no_history_says_so() {
        let jobs = vec![cost("1", Some(SizeGroup::Small), "build", 0, false, 0)];
        let text = render_costs(&jobs, &stats(&jobs));
        assert!(text.contains("no merged jobs with data yet"), "{text}");
    }

    #[test]
    fn estimates_read_as_one_line() {
        let e = factory_core::Estimate {
            group: "small".into(),
            median: 280_000,
            low: 220_000,
            high: 370_000,
            n: 4,
            n_approx: 4,
        };
        assert_eq!(
            format_estimate(&e),
            "estimate 220.0K to 370.0K weighted, median 280.0K (small, 4 jobs, 4 approximate)"
        );
        let few = factory_core::Estimate {
            group: "all".into(),
            median: 10,
            low: 10,
            high: 10,
            n: 1,
            n_approx: 0,
        };
        assert_eq!(
            format_estimate(&few),
            "estimate 10 to 10 weighted, median 10 (all sizes, 1 job, few jobs)"
        );
    }
}
