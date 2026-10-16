//! Spec cards: one card per series, built from the plan jobs that share a series id.

use std::path::Path;

use serde::Serialize;

use crate::model::Job;
use crate::plan::load_spec;
use crate::state::JobView;

/// One plan of a series, as a row of the spec card.
#[derive(Debug, Serialize, PartialEq)]
pub struct SpecPlanRow {
    pub job_id: String,
    /// 1-based position in the series.
    pub index: u32,
    pub title: Option<String>,
    /// "waiting", "building", "review", "needs-human", "merged" or "cancelled".
    pub state: String,
}

/// The card of one spec and the plans that belong to it.
#[derive(Debug, Serialize, PartialEq)]
pub struct SpecCard {
    pub series_id: String,
    pub title: Option<String>,
    pub goal: Option<String>,
    /// The column the card sits in: "spec", "needs-human" or "merged".
    pub stage: String,
    pub plans: Vec<SpecPlanRow>,
}

/// The row state of one plan job. A cancelled flag wins over the stage.
pub fn plan_state(job: &Job) -> &'static str {
    if job.cancelled == Some(true) {
        return "cancelled";
    }
    match job.stage.as_str() {
        "merged" => "merged",
        "needs-human" => "needs-human",
        "review" | "approve" => "review",
        "build" => "building",
        _ => "waiting",
    }
}

/// Merged once every plan merged, needs-human when a plan is stuck or cancelled, spec otherwise.
pub fn card_stage(rows: &[SpecPlanRow]) -> &'static str {
    if !rows.is_empty() && rows.iter().all(|row| row.state == "merged") {
        "merged"
    } else if rows.iter().any(|row| row.state == "cancelled" || row.state == "needs-human") {
        "needs-human"
    } else {
        "spec"
    }
}

fn index_of(view: &JobView) -> u32 {
    view.job.series.as_ref().map_or(0, |series| series.index)
}

/// Builds one card per series id. Only a series of two or more plans gets a card, and jobs without a
/// series id (made before plans got series ids) are left out. The title and goal come from the spec
/// copy in plan 1's job folder.
pub fn build_spec_cards(jobs: &[JobView], root: &Path) -> Vec<SpecCard> {
    let mut groups: Vec<(String, Vec<&JobView>)> = Vec::new();
    for view in jobs {
        let Some(series) = view.job.series.as_ref() else { continue };
        let Some(id) = series.id.as_ref() else { continue };
        if series.total < 2 {
            continue;
        }
        match groups.iter_mut().find(|(group_id, _)| group_id == id) {
            Some((_, members)) => members.push(view),
            None => groups.push((id.clone(), vec![view])),
        }
    }
    groups
        .into_iter()
        .map(|(series_id, mut members)| {
            members.sort_by_key(|view| index_of(view));
            let spec = members
                .iter()
                .find(|view| index_of(view) == 1)
                .and_then(|view| load_spec(root, &view.job.dir));
            let plans: Vec<SpecPlanRow> = members
                .iter()
                .map(|view| SpecPlanRow {
                    job_id: view.job.id.clone(),
                    index: index_of(view),
                    title: view.plan.as_ref().and_then(|plan| plan.title.clone()),
                    state: plan_state(&view.job).to_string(),
                })
                .collect();
            let stage = card_stage(&plans).to_string();
            SpecCard {
                series_id,
                title: spec.as_ref().and_then(|info| info.title.clone()),
                goal: spec.and_then(|info| info.goal),
                stage,
                plans,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_json(stage: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut value = serde_json::json!({
            "id": "001", "dir": "d", "slug": "s", "created": "2026-10-13", "feature": "f",
            "branch": "b", "stage": stage, "round": 0, "stage_since": 0.0, "reviews": {},
        });
        if let (Some(base), Some(more)) = (value.as_object_mut(), extra.as_object()) {
            base.extend(more.clone());
        }
        value
    }

    fn job(stage: &str, cancelled: bool) -> Job {
        serde_json::from_value(job_json(stage, serde_json::json!({ "cancelled": cancelled }))).expect("job")
    }

    fn row(state: &str) -> SpecPlanRow {
        SpecPlanRow { job_id: "001".into(), index: 1, title: None, state: state.into() }
    }

    #[test]
    fn a_plan_row_state_follows_the_stage_and_the_cancelled_flag() {
        assert_eq!(plan_state(&job("spec", false)), "waiting");
        assert_eq!(plan_state(&job("build", false)), "building");
        assert_eq!(plan_state(&job("review", false)), "review");
        assert_eq!(plan_state(&job("approve", false)), "review");
        assert_eq!(plan_state(&job("needs-human", false)), "needs-human");
        assert_eq!(plan_state(&job("merged", false)), "merged");
        assert_eq!(plan_state(&job("needs-human", true)), "cancelled");
        assert_eq!(plan_state(&job("surprise", false)), "waiting");
    }

    #[test]
    fn the_card_stage_is_merged_when_all_merged_needs_human_when_stuck_and_spec_otherwise() {
        assert_eq!(card_stage(&[row("merged"), row("merged")]), "merged");
        assert_eq!(card_stage(&[row("merged"), row("building"), row("waiting")]), "spec");
        assert_eq!(card_stage(&[row("merged"), row("needs-human"), row("waiting")]), "needs-human");
        assert_eq!(card_stage(&[row("building"), row("cancelled")]), "needs-human");
        assert_eq!(card_stage(&[row("waiting"), row("waiting")]), "spec");
    }

    #[test]
    fn the_series_id_and_cancelled_flag_are_read() {
        let extra = serde_json::json!({ "series": { "index": 2, "total": 5, "id": "S2" }, "cancelled": true });
        let parsed: Job = serde_json::from_value(job_json("build", extra)).expect("job");
        assert_eq!(parsed.series.and_then(|s| s.id), Some("S2".to_string()));
        assert_eq!(parsed.cancelled, Some(true));
    }

    #[test]
    fn bad_series_id_and_cancelled_values_are_dropped_not_fatal() {
        let extra = serde_json::json!({ "series": { "index": 2, "total": 5, "id": 7 }, "cancelled": "yes" });
        let parsed: Job = serde_json::from_value(job_json("build", extra)).expect("job still parses");
        let series = parsed.series.expect("the series stays");
        assert_eq!((series.index, series.total, series.id), (2, 5, None));
        assert_eq!(parsed.cancelled, None);
    }

    #[test]
    fn an_old_job_has_no_series_id_and_no_cancelled_flag() {
        let extra = serde_json::json!({ "series": { "plan_file": "p.md", "index": 1, "total": 3 } });
        let parsed: Job = serde_json::from_value(job_json("merged", extra)).expect("job");
        assert_eq!(parsed.series.and_then(|s| s.id), None);
        assert_eq!(parsed.cancelled, None);
    }

    use crate::plan::PlanView;
    use crate::state::JobView;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn view(id: &str, stage: &str, index: u32, total: u32, series_id: Option<&str>, title: Option<&str>) -> JobView {
        let extra = serde_json::json!({
            "id": id,
            "dir": format!("2026-10-13-{id}-x"),
            "series": { "index": index, "total": total, "id": series_id },
        });
        JobView {
            job: serde_json::from_value(job_json(stage, extra)).expect("job"),
            agents: Vec::new(),
            seconds_in_stage: 0.0,
            plan: title.map(|t| PlanView { title: Some(t.to_string()), goal: None, tasks: Vec::new() }),
            stage_durations: Vec::new(),
            token_view: None,
            has_park: false,
        }
    }

    #[test]
    fn one_card_per_series_with_plans_in_index_order() {
        let jobs = vec![
            view("003", "spec", 3, 3, Some("S2"), Some("Third")),
            view("001", "merged", 1, 3, Some("S2"), Some("First")),
            view("002", "build", 2, 3, Some("S2"), Some("Second")),
            view("004", "build", 1, 2, Some("S10"), None),
            view("005", "spec", 2, 2, Some("S10"), None),
        ];
        let cards = build_spec_cards(&jobs, std::path::Path::new("/nonexistent"));
        let ids: Vec<_> = cards.iter().map(|c| c.series_id.as_str()).collect();
        assert_eq!(ids, vec!["S2", "S10"], "groups keep first-appearance order");
        let rows: Vec<_> = cards[0].plans.iter().map(|r| (r.index, r.state.as_str(), r.title.as_deref())).collect();
        assert_eq!(rows, vec![(1, "merged", Some("First")), (2, "building", Some("Second")), (3, "waiting", Some("Third"))]);
        assert_eq!(cards[0].stage, "spec");
        assert_eq!(cards[1].plans.len(), 2);
    }

    #[test]
    fn jobs_without_a_series_id_or_in_a_one_plan_series_make_no_card() {
        let jobs = vec![
            view("001", "merged", 1, 3, None, None),
            view("002", "build", 1, 1, Some("S1"), None),
        ];
        assert!(build_spec_cards(&jobs, std::path::Path::new("/nonexistent")).is_empty());
    }

    #[test]
    fn the_title_and_goal_come_from_spec_md_of_plan_one() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("spec.md"), "# Spec: Demo spec\n\n## Goal\n\nShow the plans.\n")?;
        let jobs = vec![view("001", "build", 1, 2, Some("S1"), None), view("002", "spec", 2, 2, Some("S1"), None)];
        let cards = build_spec_cards(&jobs, root.path());
        assert_eq!(cards[0].title.as_deref(), Some("Demo spec"));
        assert_eq!(cards[0].goal.as_deref(), Some("Show the plans."));
        Ok(())
    }

    #[test]
    fn a_series_without_its_plan_one_job_still_gets_a_card_without_a_title() {
        let jobs = vec![view("002", "build", 2, 3, Some("S1"), None), view("003", "spec", 3, 3, Some("S1"), None)];
        let cards = build_spec_cards(&jobs, std::path::Path::new("/nonexistent"));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, None);
        assert_eq!(cards[0].plans.len(), 2);
    }
}
