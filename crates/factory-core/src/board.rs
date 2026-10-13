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
        crate::budget::weighted_tokens(self.input, self.cache_creation, self.output, self.cache_read)
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
        }
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
    pub fn set_stage(&mut self, to: Stage, reason: Option<&str>, force: bool, now: f64) -> Result<(), CoreError> {
        Stage::check_move(self.stage, to, force)?;
        if self.stage != to {
            self.stage = to;
            self.stage_since = now;
        }
        self.reason = if to == Stage::NeedsHuman { reason.map(str::to_string) } else { None };
        Ok(())
    }

    /// The base sha is recorded once. Changing it needs `force`.
    pub fn set_base(&mut self, sha: &str, force: bool) -> Result<(), CoreError> {
        match &self.base {
            Some(have) if have != sha && !force => {
                Err(CoreError::BaseChange { have: have.clone(), want: sha.to_string() })
            }
            _ => {
                self.base = Some(sha.to_string());
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub jobs: Vec<Job>,
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
        self.jobs.iter().find(|j| j.id == id).ok_or(CoreError::NoJob(id))
    }

    pub fn find_mut(&mut self, id: &str) -> Result<&mut Job, CoreError> {
        let id = crate::ids::normalize_id(id)?;
        self.jobs.iter_mut().find(|j| j.id == id).ok_or(CoreError::NoJob(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job::new("001", "2026-10-13-001-x", "x", "2026-10-13", "feature", 100.0)
    }

    #[test]
    fn set_stage_updates_since_only_on_change_and_manages_reason() {
        let mut j = job();
        j.set_stage(Stage::Build, None, false, 200.0).expect("spec to build");
        assert_eq!(j.stage_since, 200.0);
        j.set_stage(Stage::Build, None, false, 300.0).expect("same stage");
        assert_eq!(j.stage_since, 200.0);
        j.set_stage(Stage::NeedsHuman, Some("risky diff"), false, 400.0).expect("park");
        assert_eq!(j.reason.as_deref(), Some("risky diff"));
        j.set_stage(Stage::Review, Some("ignored"), false, 500.0).expect("resume");
        assert_eq!(j.reason, None);
    }

    #[test]
    fn refused_move_leaves_the_job_untouched() {
        let mut j = job();
        j.set_stage(Stage::Merged, None, false, 200.0).expect("forward");
        let before = j.clone();
        assert!(j.set_stage(Stage::Build, None, false, 300.0).is_err());
        assert_eq!(j, before);
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
