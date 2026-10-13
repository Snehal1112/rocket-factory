use std::path::PathBuf;

use factory_agent::StopReason;
use factory_core::{Stage, TaskState};
use factory_git::JobRef;

use crate::ports::{git_err, now, JobView};
use crate::tasks::{parse_tasks, PlanTask};
use crate::{Engine, EngineError, StageOutcome};

impl Engine<'_> {
    /// Runs the builder, then records the new head. With a task list in `plan.md` it runs one task agent per
    /// call and stays in Build until the last task has a commit. Fix rounds resume the last session.
    pub fn build_stage(&self) -> Result<StageOutcome, EngineError> {
        let job = self.job()?;
        let tasks = parse_tasks(&std::fs::read_to_string(&self.plan_file).unwrap_or_default());
        // A job that already has a builder session but no task count is a legacy whole-plan job.
        let legacy = job.session_id.is_some() && job.task_index == 0;
        if !legacy && (job.task_index as usize) < tasks.len() {
            return self.build_task(&job, &tasks);
        }
        let fresh = job.worktree.is_none();
        let new_work = fresh || job.session_id.is_none();
        let (cwd, base, previous_head, branch) = if fresh {
            let (cwd, base, branch) = self.start_worktree(&job)?;
            self.ledger_task(1, TaskState::Dispatched, &format!("BASE {base}, {}", self.config.builder_model))?;
            (cwd, base, None, branch)
        } else {
            let (cwd, base) = self.existing_worktree(&job)?;
            (cwd, base, job.head.clone(), job.branch.clone())
        };
        let spec = match &job.session_id {
            Some(session) if !fresh => {
                // A rework note from the user wins over a round findings file.
                let note = self.job_dir.join("rework-note.md");
                let findings =
                    if note.exists() { note } else { self.job_dir.join(format!("changes-r{}.md", job.round)) };
                if !findings.exists() {
                    return Err(EngineError::Msg(format!("missing findings file {}", findings.display())));
                }
                self.builder_resume_spec(&job, &cwd, &findings, session)
            }
            _ => self.builder_first_spec(&job, &cwd),
        };
        let run = self.run_agent(spec)?;
        if !new_work {
            // The builder has seen the note, so it must not be sent again. A missing file is fine.
            let _ = std::fs::remove_file(self.job_dir.join("rework-note.md"));
        }
        let session = run.session_id.clone().or(job.session_id.clone());
        self.update(|j| {
            j.builder_id = session;
            Ok(())
        })?;
        if run.stop_reason != StopReason::Done {
            return self.park(format!("builder stopped: {:?}", run.stop_reason));
        }
        let head = self.repo.tip(&branch).map_err(git_err)?;
        if new_work && head == base {
            return self.park("builder made no commits".to_string());
        }
        if !new_work && Some(&head) == previous_head.as_ref() {
            return self.park("builder made no new commit on resume".to_string());
        }
        self.finish_build(&job, &base, head, previous_head)
    }

    /// Creates the job worktree and records the branch and base. The git layer picks all three.
    fn start_worktree(&self, job: &JobView) -> Result<(PathBuf, String, String), EngineError> {
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
        Ok((wt.path, base, branch))
    }

    fn existing_worktree(&self, job: &JobView) -> Result<(PathBuf, String), EngineError> {
        let cwd = job.worktree.clone().ok_or_else(|| EngineError::Msg("job has no worktree".into()))?;
        let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        Ok((cwd, base))
    }

    /// Runs the agent of the next plan task. After the last task it hands over to review.
    fn build_task(&self, job: &JobView, tasks: &[PlanTask]) -> Result<StageOutcome, EngineError> {
        let Some(task) = tasks.get(job.task_index as usize) else {
            return Err(EngineError::Msg("task index is past the plan".into()));
        };
        let fresh = job.worktree.is_none();
        let (cwd, base, branch) = if fresh {
            self.start_worktree(job)?
        } else {
            let (cwd, base) = self.existing_worktree(job)?;
            (cwd, base, job.branch.clone())
        };
        if fresh {
            self.ruling(&format!("per-task builders, {} tasks", tasks.len()))?;
        }
        let tip = self.repo.tip(&branch).map_err(git_err)?;
        let earlier = self.repo.log_oneline(&base, &tip).map_err(git_err)?;
        self.ledger_task(task.n, TaskState::Dispatched, &format!("BASE {base}, {}", self.config.builder_model))?;
        let mut spec = self.builder_task_spec(job, &cwd, task, &base, &tip, &earlier);
        let note_path = self.job_dir.join("rework-note.md");
        let note = std::fs::read_to_string(&note_path).ok();
        if let Some(text) = &note {
            spec.prompt.push_str(&format!("\nA note from the user for this task:\n{text}\n"));
        }
        let run = self.run_agent(spec)?;
        if note.is_some() {
            // The builder has seen the note, so it must not be sent again.
            let _ = std::fs::remove_file(&note_path);
        }
        if run.stop_reason != StopReason::Done {
            return self.park(format!("builder stopped in task {}: {:?}", task.n, run.stop_reason));
        }
        let after = self.repo.tip(&branch).map_err(git_err)?;
        if after == tip {
            return self.park(format!("task {} made no commit", task.n));
        }
        let short = after.get(..7).unwrap_or(&after).to_string();
        self.ledger_task(task.n, TaskState::ImplementerDone, &short)?;
        let session = run.session_id.clone().or(job.session_id.clone());
        let next = job.task_index + 1;
        self.update(|j| {
            j.builder_id = session;
            j.task_index = next;
            Ok(())
        })?;
        if (next as usize) < tasks.len() {
            return Ok(StageOutcome::Advanced(Stage::Build));
        }
        self.finish_build(job, &base, after, job.head.clone())
    }

    /// Checks the diff cap, records the head and moves the job to review.
    fn finish_build(
        &self,
        job: &JobView,
        base: &str,
        head: String,
        previous_head: Option<String>,
    ) -> Result<StageOutcome, EngineError> {
        let info = self.repo.diff_to_files(base, &head, &self.job_dir).map_err(git_err)?;
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
