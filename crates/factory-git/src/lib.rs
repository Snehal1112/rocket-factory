//! Git layer for the factory driver. Built on git2, never shells out to git.

mod commits;
mod diff;
mod error;
mod merge;
mod repo;
mod stage;
mod state;
mod worktree;

pub use commits::CommitInfo;
pub use diff::DiffFiles;
pub use error::{GitError, Result};
pub use merge::MergeOutcome;
pub use repo::{Repo, MAIN_BRANCH};
pub use state::{BaseCheck, BaseState, Preflight};
pub use worktree::{JobRef, Worktree};
