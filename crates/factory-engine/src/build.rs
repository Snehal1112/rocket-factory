use std::path::PathBuf;

use factory_agent::StopReason;
use factory_core::{Stage, TaskState};
use factory_git::JobRef;

use crate::ports::{git_err, now};
use crate::{Engine, EngineError, StageOutcome};

impl Engine<'_> {
    /// Runs the builder (first run) or resumes it (fix round), then records the new head.
    pub fn build_stage(&self) -> Result<StageOutcome, EngineError> {
        let job = self.job()?;
        let first = job.session_id.is_none();
        let (cwd, base, previous_head, branch) = if first {
            // The git layer picks the branch, the worktree path and the base (the main tip).
            let wt = self
                .repo
                .create_worktree(&JobRef { id: self.job_id.clone(), slug: job.slug.clone() })
                .map_err(git_err)?;
            let (path, base, branch) = (wt.path.to_string_lossy().to_string(), wt.base.clone(), wt.branch.clone());
            let (b, br) = (base.clone(), branch.clone());
            self.update(|j| {
                j.worktree = Some(path);
                j.branch = br;
                j.set_base(&b, false)
            })?;
            self.ledger_task(1, TaskState::Dispatched, &format!("BASE {base}, {}", self.config.builder_model))?;
            (wt.path, base, None, branch)
        } else {
            let cwd = job.worktree.clone().ok_or_else(|| EngineError::Msg("job has no worktree".into()))?;
            let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no base".into()))?;
            (cwd, base, job.head.clone(), job.branch.clone())
        };
        let spec = match (&job.session_id, first) {
            (Some(session), false) => {
                let findings = self.job_dir.join(format!("changes-r{}.md", job.round));
                if !findings.exists() {
                    return Err(EngineError::Msg(format!("missing findings file {}", findings.display())));
                }
                self.builder_resume_spec(&job, &cwd, &findings, session)
            }
            _ => self.builder_first_spec(&job, &cwd),
        };
        let run = self.run_agent(spec)?;
        let session = run.session_id.clone().or(job.session_id.clone());
        self.update(|j| {
            j.builder_id = session;
            Ok(())
        })?;
        if run.stop_reason != StopReason::Done {
            return self.park(format!("builder stopped: {:?}", run.stop_reason));
        }
        let head = self.repo.tip(&branch).map_err(git_err)?;
        if first && head == base {
            return self.park("builder made no commits".to_string());
        }
        if !first && Some(&head) == previous_head.as_ref() {
            return self.park("builder made no new commit on resume".to_string());
        }
        let info = self.repo.diff_to_files(&base, &head, &self.job_dir).map_err(git_err)?;
        let cap = self.config.diff_cap_lines as usize;
        if info.changed_lines() > cap {
            return self.park(format!("diff has {} changed lines, the cap is {cap}", info.changed_lines()));
        }
        let h = head.clone();
        self.update(|j| {
            j.prev_head = previous_head;
            j.head = Some(h);
            j.set_stage(Stage::Review, None, false, now())
        })?;
        self.ruling(&format!("head of round {} is {head}", job.round))?;
        Ok(StageOutcome::Advanced(Stage::Review))
    }

    /// The directory of the worktree, or an error when the job has none yet.
    pub(crate) fn worktree(&self) -> Result<PathBuf, EngineError> {
        self.job()?.worktree.ok_or_else(|| EngineError::Msg("job has no worktree".into()))
    }
}

pub fn build_stage(e: &Engine) -> Result<StageOutcome, EngineError> {
    e.build_stage()
}
