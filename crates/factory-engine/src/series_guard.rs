use std::path::Path;

use factory_core::{Board, ParkKind, Stage};
use factory_git::GitError;

use crate::ports::{core_err, git_err, short_sha, JobView};
use crate::tasks::parse_tasks;
use crate::{Engine, EngineError, StageOutcome};

impl Engine<'_> {
    /// Checks a job before its builder runs. A later plan of a series parks until the plan before it
    /// merged, and a job with no commits of its own gets its branch cut again from the current base
    /// branch. `Some` means the job parked. `None` means build on, after reloading the job.
    pub(crate) fn series_guard(&self, job: &JobView) -> Result<Option<StageOutcome>, EngineError> {
        // A crash between the removal and the board write leaves fields that name nothing.
        let gone = job
            .worktree
            .as_ref()
            .is_some_and(|wt| !wt.exists() && !self.repo.branch_exists(&job.branch));
        let reloaded;
        let job = if gone {
            self.ruling("finished an interrupted re-cut")?;
            self.clear_tree_fields()?;
            reloaded = self.job()?;
            &reloaded
        } else {
            job
        };
        // A job with commits of its own is past the guard: fix rounds, later tasks and older jobs.
        if let (Some(_), Some(base)) = (&job.worktree, &job.base) {
            if self.repo.tip(&job.branch).map_err(git_err)? != *base {
                return Ok(None);
            }
        }
        if let Some(parked) = self.wait_for_earlier_plan()? {
            return Ok(Some(parked));
        }
        self.recut_stale(job)?;
        Ok(None)
    }

    /// Parks a later plan of a series while the plan before it is not merged. Legacy series have no id
    /// and are never matched.
    fn wait_for_earlier_plan(&self) -> Result<Option<StageOutcome>, EngineError> {
        let board = Board::load(&self.board_path).map_err(core_err)?;
        let me = board.find(&self.job_id).map_err(core_err)?;
        let Some(pos) = me.series.as_ref().filter(|s| s.index > 1 && s.id.is_some()) else {
            return Ok(None);
        };
        let n = pos.index - 1;
        let earlier = board.jobs.iter().find(|j| {
            j.series
                .as_ref()
                .is_some_and(|s| s.id == pos.id && s.index == n)
        });
        let Some(earlier) = earlier else {
            return Ok(None);
        };
        if earlier.stage == Stage::Merged {
            return Ok(None);
        }
        let reason = if earlier.cancelled {
            format!(
                "plan {n} (job {}) was cancelled; cancel this plan too, or plan the work again",
                earlier.id
            )
        } else {
            format!("plan {n} (job {}) is not merged yet", earlier.id)
        };
        self.park_with(ParkKind::SeriesWaiting, reason, vec![])
            .map(Some)
    }

    /// Cuts the branch of a job with no commits again when the base branch moved since it was cut.
    /// A worktree with uncommitted work is kept, because the git layer refuses to remove it.
    fn recut_stale(&self, job: &JobView) -> Result<(), EngineError> {
        let (Some(cwd), Some(base)) = (&job.worktree, &job.base) else {
            return Ok(());
        };
        // Clearing the session of a legacy whole-plan job would switch it to per-task builders.
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        if job.session_id.is_some() && job.task_index == 0 && !parse_tasks(&plan).is_empty() {
            return Ok(());
        }
        let main = self.repo.base_branch().to_string();
        let tip = self.repo.tip(&main).map_err(git_err)?;
        if *base == tip {
            return Ok(());
        }
        let wt = self.job_worktree(job, Path::new(cwd), base, &job.branch);
        match self.repo.remove_worktree(&wt, false) {
            Ok(()) => {}
            Err(GitError::Refused(why)) => {
                return self.ruling(&format!("kept the stale worktree: {why}"));
            }
            Err(e) => return Err(git_err(e)),
        }
        self.ruling(&format!(
            "re-cut the branch from {main} {}, the job had no commits",
            short_sha(&tip)
        ))?;
        self.clear_tree_fields()
    }

    /// Forgets the worktree and the builder sessions, so the next build starts on a fresh worktree.
    /// The round, the fix source, the reviews, the escalation and the branch name stay.
    fn clear_tree_fields(&self) -> Result<(), EngineError> {
        self.update(|j| {
            j.worktree = None;
            j.base = None;
            j.head = None;
            j.prev_head = None;
            j.builder_id = None;
            j.task_session = None;
            j.builder_last = None;
            j.checks_pass = None;
            j.builder_session_model = None;
            Ok(())
        })
    }
}
