use crate::error::CoreError;
use crate::fsutil::write_atomic;
use crate::stage::Stage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewState {
    Pending,
    Running,
    Pass,
    Changes,
    Skipped,
}

/// Token totals of one job, summed over its agent runs. New in v3, absent in v1 boards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobTokens {
    pub input: u64,
    pub cache_creation: u64,
    pub output: u64,
    pub cache_read: u64,
}

impl JobTokens {
    pub fn is_zero(&self) -> bool {
        *self == JobTokens::default()
    }

    /// Adds one run. Saturates instead of overflowing.
    pub fn add(&mut self, input: u64, cache_creation: u64, output: u64, cache_read: u64) {
        self.input = self.input.saturating_add(input);
        self.cache_creation = self.cache_creation.saturating_add(cache_creation);
        self.output = self.output.saturating_add(output);
        self.cache_read = self.cache_read.saturating_add(cache_read);
    }

    /// The number the budget is measured in, see `weighted_tokens`.
    pub fn weighted(&self) -> u64 {
        crate::budget::weighted_tokens(
            self.input,
            self.cache_creation,
            self.output,
            self.cache_read,
        )
    }
}

/// One job in `board.json`. Field names match the v1 file so `factory-server` keeps working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub dir: String,
    pub slug: String,
    pub created: String,
    pub feature: String,
    pub branch: String,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub builder_id: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
    pub stage: Stage,
    pub round: u32,
    #[serde(default)]
    pub reason: Option<String>,
    pub stage_since: f64,
    pub reviews: BTreeMap<String, ReviewState>,
    /// Model override for this job. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The head before the latest fix round. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_head: Option<String>,
    /// Summed agent token usage. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "JobTokens::is_zero")]
    pub tokens: JobTokens,
    /// How many times main was merged into the job branch. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub syncs: u32,
    /// Position in a plan series. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<SeriesPos>,
    /// The driver that owns the job right now. New in v3, absent in v1 boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<Lease>,
    /// True once the job was cancelled by the user. New in v3.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
    /// True once the worktree and branch were removed. New in v3.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cleaned: bool,
    /// Every stage the job entered, in order. The dashboard reads it for durations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_history: Vec<StageEntry>,
    /// The weighted token budget this job was created with. The dashboard reads it for the budget bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_weighted: Option<u64>,
    /// GitHub issues this job works on, as numbers. New in this series, absent in older boards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<u32>,
    /// How many plan tasks have a finished, committed builder run. New in plan 09.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub task_index: u32,
    /// Why the job is parked, see `ParkKind::as_str`. New in the failure details series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park_kind: Option<String>,
    /// Unix seconds the job parked. New in the failure details series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park_at: Option<u64>,
    /// The tip of the snapshot commits while the batch commit of the job is pending. New in the cost reduction series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_snapshot: Option<String>,
    /// The cost estimate made when the job was created. New in the cost report series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<crate::cost::Estimate>,
    /// What started the latest fix round, `checks` or `review`. New in the approve review series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_source: Option<String>,
    /// A warning about the job's spec, for the dashboard. New in the draft cards series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_warning: Option<String>,
    /// The builder session of the running task, saved while the builder runs. New in the crash resume series, absent in older boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_session: Option<TaskSession>,
    /// The last finished task builder, used to decide a session carry. New in the builder task resume series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_last: Option<BuilderLast>,
}

/// The last finished task builder of the job, used to decide a carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuilderLast {
    /// The plan task number, as in `### Task N`.
    pub task: u32,
    /// The `claude` session id.
    pub id: String,
    /// Unix time in seconds when the builder run returned.
    pub ended_at: u64,
    /// The context size at the end of the run, in tokens.
    pub end_ctx: u64,
}

/// The builder session of the plan task that is running now. Only a crash leaves it on the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSession {
    /// The plan task number, as in `### Task N`.
    pub task: u32,
    /// The `claude` session id.
    pub id: String,
    /// The branch tip when the task started.
    pub tip: String,
}

/// One stage change, with the unix time in seconds it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageEntry {
    pub stage: Stage,
    pub at: f64,
}

/// A time-limited claim on a job, so two drivers never work on one job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub owner: String,
    pub expires: f64,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Where a job sits in a series of plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesPos {
    pub plan_file: String,
    pub index: u32,
    pub total: u32,
    /// The id of the series, `S1`, `S2` and so on. All jobs of one series share it. Absent in series
    /// made before the plans got their jobs up front.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Job {
    /// A fresh job in the `spec` stage. v3 has one reviewer, so only the `code` role exists.
    pub fn new(id: &str, dir: &str, slug: &str, created: &str, feature: &str, now: f64) -> Job {
        let mut reviews = BTreeMap::new();
        reviews.insert("code".to_string(), ReviewState::Pending);
        Job {
            id: id.to_string(),
            dir: dir.to_string(),
            slug: slug.to_string(),
            created: created.to_string(),
            feature: feature.to_string(),
            branch: format!("factory/{id}-{slug}"),
            worktree: None,
            builder_id: None,
            base: None,
            head: None,
            stage: Stage::Spec,
            round: 0,
            reason: None,
            stage_since: now,
            reviews,
            model: None,
            prev_head: None,
            tokens: JobTokens::default(),
            syncs: 0,
            series: None,
            lease: None,
            cancelled: false,
            cleaned: false,
            stage_history: vec![StageEntry {
                stage: Stage::Spec,
                at: now,
            }],
            budget_weighted: None,
            issues: Vec::new(),
            task_index: 0,
            park_kind: None,
            park_at: None,
            batch_snapshot: None,
            estimate: None,
            fix_source: None,
            spec_warning: None,
            task_session: None,
            builder_last: None,
        }
    }

    /// The refusal that approve gives for a needs-human job without a code review. CLI and API share it.
    pub fn review_missing_message(&self) -> String {
        format!(
            "job {} has no code review yet (the checks never passed or the reviewer never ran): use rework, or approve --without-review to merge anyway",
            self.id
        )
    }

    /// True when the `code` review has no verdict yet: pending, running or absent. Such a job was never reviewed.
    pub fn review_missing(&self) -> bool {
        !matches!(
            self.reviews.get("code"),
            Some(ReviewState::Pass | ReviewState::Changes | ReviewState::Skipped)
        )
    }

    pub fn set_review(&mut self, role: &str, state: ReviewState) {
        self.reviews.insert(role.to_string(), state);
    }

    pub fn reset_reviews(&mut self) {
        for state in self.reviews.values_mut() {
            *state = ReviewState::Pending;
        }
    }

    /// Moves the job to a stage. The reason is kept only for needs-human. Nothing changes when the move is refused.
    pub fn set_stage(
        &mut self,
        to: Stage,
        reason: Option<&str>,
        force: bool,
        now: f64,
    ) -> Result<(), CoreError> {
        Stage::check_move(self.stage, to, force)?;
        if self.stage != to {
            self.stage = to;
            self.stage_since = now;
            self.stage_history.push(StageEntry { stage: to, at: now });
        }
        self.reason = if to == Stage::NeedsHuman {
            reason.map(str::to_string)
        } else {
            None
        };
        if to != Stage::NeedsHuman {
            self.park_kind = None;
            self.park_at = None;
        }
        Ok(())
    }

    /// The base sha is recorded once. Changing it needs `force`.
    pub fn set_base(&mut self, sha: &str, force: bool) -> Result<(), CoreError> {
        match &self.base {
            Some(have) if have != sha && !force => Err(CoreError::BaseChange {
                have: have.clone(),
                want: sha.to_string(),
            }),
            _ => {
                self.base = Some(sha.to_string());
                Ok(())
            }
        }
    }
}

/// The stages a draft card moves through before it becomes a job.
pub const DRAFT_STAGES: [&str; 3] = ["interviewing", "writing-spec", "writing-plans"];

/// A feature that is still being specified. It has no job yet. New in the draft cards series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub slug: String,
    pub feature: String,
    /// One of `DRAFT_STAGES`.
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_file: Option<String>,
    pub created: String,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub jobs: Vec<Job>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drafts: Vec<Draft>,
}

impl Board {
    /// A missing file is an empty board. Any other failure is an error, so a corrupt file is never overwritten by accident.
    pub fn load(path: &Path) -> Result<Board, CoreError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Board::default()),
            Err(e) => Err(CoreError::io(path, e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), CoreError> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(path, text.as_bytes())
    }

    pub fn find(&self, id: &str) -> Result<&Job, CoreError> {
        let id = crate::ids::normalize_id(id)?;
        self.jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or(CoreError::NoJob(id))
    }

    /// Takes a job out of the board and returns it. The job folder is not touched.
    pub fn remove_job(&mut self, id: &str) -> Result<Job, CoreError> {
        let id = crate::ids::normalize_id(id)?;
        let at = self
            .jobs
            .iter()
            .position(|j| j.id == id)
            .ok_or(CoreError::NoJob(id))?;
        Ok(self.jobs.remove(at))
    }

    /// The series id of a job that is one of a series of two or more plans and has another job
    /// of that series on the board. Removing such a job would break the chaining.
    pub fn series_with_siblings(&self, id: &str) -> Option<String> {
        let job = self.jobs.iter().find(|j| j.id == id)?;
        let pos = job.series.as_ref().filter(|p| p.total >= 2)?;
        let sid = pos.id.as_ref()?;
        let shared = self
            .jobs
            .iter()
            .any(|o| o.id != job.id && o.series.as_ref().and_then(|p| p.id.as_ref()) == Some(sid));
        shared.then(|| sid.clone())
    }

    pub fn find_mut(&mut self, id: &str) -> Result<&mut Job, CoreError> {
        let id = crate::ids::normalize_id(id)?;
        self.jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(CoreError::NoJob(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_series(id: &str, sid: &str, total: u32) -> Job {
        let mut j = Job::new(id, "d", "s", "2026-10-16", "f", 1.0);
        j.series = Some(SeriesPos {
            plan_file: "p.md".into(),
            index: 1,
            total,
            id: Some(sid.into()),
        });
        j
    }

    #[test]
    fn remove_job_takes_one_job_and_reports_a_missing_one() {
        let mut b = Board {
            jobs: vec![
                Job::new("001", "d", "s", "c", "f", 1.0),
                Job::new("002", "d", "s", "c", "f", 1.0),
            ],
            ..Default::default()
        };
        assert_eq!(b.remove_job("1").expect("removed").id, "001");
        assert_eq!(b.jobs.len(), 1);
        assert!(matches!(b.remove_job("1"), Err(CoreError::NoJob(_))));
    }

    #[test]
    fn series_with_siblings_needs_a_series_of_two_and_another_member() {
        let b = Board {
            jobs: vec![
                in_series("001", "S1", 2),
                in_series("002", "S1", 2),
                in_series("003", "S2", 2),
            ],
            ..Default::default()
        };
        assert_eq!(b.series_with_siblings("001"), Some("S1".to_string()));
        assert_eq!(b.series_with_siblings("003"), None);
        let single = Board {
            jobs: vec![in_series("001", "S1", 1), in_series("002", "S1", 1)],
            ..Default::default()
        };
        assert_eq!(single.series_with_siblings("001"), None);
        let plain = Board {
            jobs: vec![Job::new("001", "d", "s", "c", "f", 1.0)],
            ..Default::default()
        };
        assert_eq!(plain.series_with_siblings("001"), None);
    }

    #[test]
    fn the_estimate_field_is_optional_and_round_trips() {
        let mut job = Job::new("001", "d", "s", "2026-10-16", "f", 1.0);
        let json = serde_json::to_string(&job).expect("json");
        assert!(!json.contains("estimate"), "{json}");
        job.estimate = Some(crate::cost::Estimate {
            group: "small".into(),
            median: 250_000,
            low: 220_000,
            high: 310_000,
            n: 4,
            n_approx: 4,
        });
        let back: Job =
            serde_json::from_str(&serde_json::to_string(&job).expect("json")).expect("load");
        assert_eq!(back.estimate.as_ref().map(|e| e.median), Some(250_000));
    }

    #[test]
    fn series_pos_id_is_optional_and_left_out_when_none() {
        let old: SeriesPos =
            serde_json::from_str(r#"{"plan_file":"p.md","index":1,"total":2}"#).expect("old shape");
        assert_eq!(old.id, None);
        let text = serde_json::to_string(&old).expect("json");
        assert!(!text.contains("\"id\""), "{text}");
        let with = SeriesPos {
            id: Some("S2".into()),
            ..old
        };
        let text = serde_json::to_string(&with).expect("json");
        assert!(text.contains(r#""id":"S2""#), "{text}");
    }

    #[test]
    fn issues_are_optional_and_left_out_when_empty() {
        let mut j = job();
        let text = serde_json::to_string(&j).expect("json");
        assert!(!text.contains("\"issues\""), "{text}");
        let back: Job = serde_json::from_str(&text).expect("old shape");
        assert!(back.issues.is_empty());
        j.issues = vec![20, 21];
        let text = serde_json::to_string(&j).expect("json");
        assert!(text.contains(r#""issues":[20,21]"#), "{text}");
        let back: Job = serde_json::from_str(&text).expect("round trip");
        assert_eq!(back.issues, vec![20, 21]);
    }

    #[test]
    fn park_fields_are_set_kept_and_cleared_by_set_stage() {
        let mut job = Job::new("001", "d", "s", "2026-10-16", "f", 1.0);
        job.set_stage(Stage::Build, None, false, 2.0)
            .expect("build");
        job.park_kind = Some("checks-failed".to_string());
        job.park_at = Some(5);
        job.set_stage(Stage::NeedsHuman, Some("why"), false, 3.0)
            .expect("park");
        assert_eq!(job.park_kind.as_deref(), Some("checks-failed"));
        assert_eq!(job.park_at, Some(5));
        job.set_stage(Stage::Build, None, true, 4.0).expect("back");
        assert_eq!(job.park_kind, None);
        assert_eq!(job.park_at, None);
    }

    #[test]
    fn builder_last_round_trips_and_is_omitted_when_none() {
        let mut j = Job::new("001", "d", "s", "2026-10-20", "f", 1.0);
        assert!(j.builder_last.is_none());
        assert!(!serde_json::to_string(&j)
            .expect("json")
            .contains("builder_last"));
        j.builder_last = Some(BuilderLast {
            task: 1,
            id: "sess-1".into(),
            ended_at: 1_700_000_000,
            end_ctx: 31_000,
        });
        let text = serde_json::to_string(&j).expect("json");
        let back: Job = serde_json::from_str(&text).expect("back");
        assert_eq!(back.builder_last, j.builder_last);
    }

    #[test]
    fn task_session_round_trips() {
        let mut j = Job::new("001", "d", "s", "2026-10-20", "f", 1.0);
        assert!(j.task_session.is_none());
        assert!(!serde_json::to_string(&j)
            .expect("json")
            .contains("task_session"));
        j.task_session = Some(TaskSession {
            task: 2,
            id: "sess-9".to_string(),
            tip: "abc1234".to_string(),
        });
        let value = serde_json::to_value(&j).expect("json");
        assert_eq!(
            value["task_session"],
            serde_json::json!({"task": 2, "id": "sess-9", "tip": "abc1234"})
        );
        let back: Job = serde_json::from_value(value).expect("parse");
        assert_eq!(back.task_session, j.task_session);
    }

    #[test]
    fn batch_snapshot_is_optional_and_omitted_when_unset() {
        let job = Job::new(
            "001",
            "2026-10-17-001-demo",
            "demo",
            "2026-10-17",
            "demo",
            0.0,
        );
        assert!(job.batch_snapshot.is_none());
        let text = serde_json::to_string(&job).expect("json");
        assert!(!text.contains("batch_snapshot"), "{text}");
        let mut with = job.clone();
        with.batch_snapshot = Some("abc1234".to_string());
        let back: Job =
            serde_json::from_str(&serde_json::to_string(&with).expect("json")).expect("parse");
        assert_eq!(back.batch_snapshot.as_deref(), Some("abc1234"));
    }

    #[test]
    fn an_old_board_without_park_fields_loads_and_new_fields_are_skipped_when_empty() {
        let job = Job::new("001", "d", "s", "2026-10-16", "f", 1.0);
        let json = serde_json::to_string(&job).expect("json");
        assert!(
            !json.contains("park_kind") && !json.contains("park_at"),
            "{json}"
        );
        let back: Job = serde_json::from_str(&json).expect("load");
        assert_eq!(back.park_kind, None);
    }

    #[test]
    fn review_missing_is_true_until_the_code_review_has_a_verdict() {
        let mut j = Job::new("001", "d", "s", "2026-10-17", "f", 1.0);
        assert!(j.review_missing(), "a fresh job has a pending review");
        j.set_review("code", ReviewState::Running);
        assert!(j.review_missing());
        j.set_review("code", ReviewState::Pass);
        assert!(!j.review_missing());
        j.set_review("code", ReviewState::Changes);
        assert!(!j.review_missing());
        j.reset_reviews();
        assert!(j.review_missing(), "a fix round resets the verdict");
        j.reviews.clear();
        assert!(j.review_missing(), "an absent review is missing too");
    }

    #[test]
    fn the_review_missing_message_names_the_problem_and_the_options() {
        let j = Job::new("020", "d", "s", "2026-10-17", "f", 1.0);
        let m = j.review_missing_message();
        assert!(m.starts_with("job 020 has no code review yet"), "{m}");
        assert!(
            m.contains("use rework") && m.contains("approve --without-review"),
            "{m}"
        );
    }

    #[test]
    fn fix_source_is_optional_and_omitted_when_unset() {
        let mut j = Job::new("001", "d", "s", "2026-10-17", "f", 1.0);
        assert!(!serde_json::to_string(&j)
            .expect("json")
            .contains("fix_source"));
        j.fix_source = Some("checks".to_string());
        let back: Job =
            serde_json::from_str(&serde_json::to_string(&j).expect("json")).expect("parse");
        assert_eq!(back.fix_source.as_deref(), Some("checks"));
    }

    fn job() -> Job {
        Job::new(
            "001",
            "2026-10-13-001-x",
            "x",
            "2026-10-13",
            "feature",
            100.0,
        )
    }

    #[test]
    fn set_stage_updates_since_only_on_change_and_manages_reason() {
        let mut j = job();
        j.set_stage(Stage::Build, None, false, 200.0)
            .expect("spec to build");
        assert_eq!(j.stage_since, 200.0);
        j.set_stage(Stage::Build, None, false, 300.0)
            .expect("same stage");
        assert_eq!(j.stage_since, 200.0);
        j.set_stage(Stage::NeedsHuman, Some("risky diff"), false, 400.0)
            .expect("park");
        assert_eq!(j.reason.as_deref(), Some("risky diff"));
        j.set_stage(Stage::Review, Some("ignored"), false, 500.0)
            .expect("resume");
        assert_eq!(j.reason, None);
    }

    #[test]
    fn refused_move_leaves_the_job_untouched() {
        let mut j = job();
        j.set_stage(Stage::Merged, None, false, 200.0)
            .expect("forward");
        let before = j.clone();
        assert!(j.set_stage(Stage::Build, None, false, 300.0).is_err());
        assert_eq!(j, before);
    }

    #[test]
    fn set_stage_appends_history_in_order() {
        let mut j = job();
        assert_eq!(
            j.stage_history,
            vec![StageEntry {
                stage: Stage::Spec,
                at: 100.0
            }]
        );
        j.set_stage(Stage::Build, None, false, 200.0)
            .expect("build");
        j.set_stage(Stage::Build, None, false, 250.0)
            .expect("same stage");
        j.set_stage(Stage::Review, None, false, 300.0)
            .expect("review");
        let seen: Vec<(Stage, f64)> = j.stage_history.iter().map(|e| (e.stage, e.at)).collect();
        assert_eq!(
            seen,
            vec![
                (Stage::Spec, 100.0),
                (Stage::Build, 200.0),
                (Stage::Review, 300.0)
            ]
        );
    }

    #[test]
    fn refused_move_appends_no_history() {
        let mut j = job();
        j.set_stage(Stage::Merged, None, false, 200.0)
            .expect("forward");
        let n = j.stage_history.len();
        assert!(j.set_stage(Stage::Build, None, false, 300.0).is_err());
        assert_eq!(j.stage_history.len(), n);
    }

    #[test]
    fn history_and_budget_are_written_as_the_server_reads_them(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut j = job();
        j.budget_weighted = Some(3_000_000);
        let v = serde_json::to_value(&j)?;
        assert_eq!(v["stage_history"][0]["stage"], "spec");
        assert_eq!(v["stage_history"][0]["at"], 100.0);
        assert_eq!(v["budget_weighted"], 3_000_000);
        j.stage_history.clear();
        j.budget_weighted = None;
        let v = serde_json::to_value(&j)?;
        assert!(v.get("stage_history").is_none() && v.get("budget_weighted").is_none());
        Ok(())
    }

    #[test]
    fn base_is_set_once() {
        let mut j = job();
        j.set_base("aaa", false).expect("first");
        j.set_base("aaa", false).expect("same value is fine");
        assert!(j.set_base("bbb", false).is_err());
        j.set_base("bbb", true).expect("forced");
        assert_eq!(j.base.as_deref(), Some("bbb"));
    }
}
