//! Git layer for the factory driver. Built on git2, never shells out to git.

mod commits;
mod diff;
mod error;
mod exclude;
mod fingerprint;
mod merge;
mod repo;
mod report;
mod reset;
mod stage;
mod state;
mod tip;
mod worktree;

pub use commits::CommitInfo;
pub use diff::{DiffCounts, DiffFiles};
pub use error::{GitError, Result};
pub use merge::MergeOutcome;
pub use repo::{Repo, DEFAULT_BASE_BRANCH};
pub use report::StagedReport;
pub use state::{BaseCheck, BaseState, Preflight};
pub use worktree::{JobRef, PruneReport, Worktree};
