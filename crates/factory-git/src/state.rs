use crate::error::{GitError, Result};
use crate::repo::Repo;
use git2::{RepositoryState, StatusOptions};

/// Result of the pre-merge check on the main checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    Ready { main_sha: String },
    Busy { reason: String },
}

/// Where a job branch stands relative to main.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseState {
    /// Base is on main, head contains it, and head contains all of main.
    Current,
    /// Main has commits the job branch does not have yet.
    MainMoved,
    /// The job base is not on main, or the head does not contain it. Never sync this.
    BaseNotMain,
    /// The head is already on main.
    Merged,
}

impl BaseState {
    /// The token used on the board and in the CLI.
    pub fn as_token(&self) -> &'static str {
        match self {
            BaseState::Current => "ok",
            BaseState::MainMoved => "main-moved",
            BaseState::BaseNotMain => "base-not-main",
            BaseState::Merged => "merged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseCheck {
    pub state: BaseState,
    pub main_sha: String,
}

impl Repo {
    /// Checks that main can take a merge: it is checked out, has no tracked
    /// changes (untracked files are fine) and no merge or other operation is
    /// in progress. Reports the main sha so the caller can detect a race.
    pub fn preflight_main(&self) -> Result<Preflight> {
        let main_sha = self.tip(&self.base)?;
        let busy = |reason: &str| Ok(Preflight::Busy { reason: reason.to_string() });
        let head = self.inner.head()?;
        if !head.is_branch() || head.shorthand() != Some(self.base.as_str()) {
            return busy(&format!("the main checkout is not on {}", self.base));
        }
        if self.inner.state() != RepositoryState::Clean {
            return busy(&format!("a merge or another operation is in progress on {}", self.base));
        }
        let mut opts = StatusOptions::new();
        opts.include_untracked(false).include_ignored(false);
        if !self.inner.statuses(Some(&mut opts))?.is_empty() {
            return busy("the main checkout has tracked changes");
        }
        Ok(Preflight::Ready { main_sha })
    }

    /// Classifies a job branch against main. A job with no commits is refused.
    pub fn base_state(&self, base: &str, head: &str) -> Result<BaseCheck> {
        if self.commit_of(base)?.id() == self.commit_of(head)?.id() {
            return Err(GitError::Refused("job has no commits, head equals base".into()));
        }
        let main_sha = self.tip(&self.base)?;
        let state = if self.is_ancestor(head, &main_sha)? {
            BaseState::Merged
        } else if !self.is_ancestor(base, &main_sha)? || !self.is_ancestor(base, head)? {
            BaseState::BaseNotMain
        } else if self.is_ancestor(&main_sha, head)? {
            BaseState::Current
        } else {
            BaseState::MainMoved
        };
        Ok(BaseCheck { state, main_sha })
    }
}
