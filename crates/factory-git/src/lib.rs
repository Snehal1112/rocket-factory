//! Git layer for the factory driver. Built on git2, never shells out to git.

mod diff;
mod error;
mod repo;

pub use diff::DiffFiles;
pub use error::{GitError, Result};
pub use repo::{Repo, MAIN_BRANCH};
