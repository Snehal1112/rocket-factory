//! Git layer for the factory driver. Built on git2, never shells out to git.

mod diff;
mod error;
mod merge;
mod repo;
mod state;

pub use diff::DiffFiles;
pub use error::{GitError, Result};
pub use merge::MergeOutcome;
pub use repo::{Repo, MAIN_BRANCH};
pub use state::{BaseCheck, BaseState, Preflight};
