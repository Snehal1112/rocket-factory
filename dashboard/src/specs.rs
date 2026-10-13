//! Spec cards: one card per series, built from the plan jobs that share a series id.

use serde::Serialize;

use crate::model::Job;

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
}
