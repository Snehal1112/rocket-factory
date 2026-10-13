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
