use std::path::{Path, PathBuf};

use factory_agent::StopReason;
use factory_core::{agent_files_rel_dir, is_agent_file_name, AGENT_EXCLUDE_PATTERN, ParkKind, ParkPart, Stage, TaskState};
use factory_git::{CommitInfo, JobRef, Worktree};

use crate::commit::{CommitKind, CommitOutcome, CommitRequest};
use crate::park::builder_stop_parts;
use crate::ports::{git_err, now, JobView};
use crate::tasks::{parse_tasks, plan_title, spec_goal, PlanTask};
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
        self.write_agent_files(&cwd)?;
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
        let tip_before = self.repo.tip(&branch).map_err(git_err)?;
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
            return self.park_with(ParkKind::BuilderStopped, format!("builder stopped: {:?}", run.stop_reason), builder_stop_parts(&run));
        }
        if self.repo.tip(&branch).map_err(git_err)? != tip_before {
            return self.park_with(
                ParkKind::BuilderCommitted,
                format!("builder committed on its own (the branch was at {} before the run)", tip_before.get(..7).unwrap_or(&tip_before)),
                Vec::new(),
            );
        }
        // The floor is the base in the first round and the last reviewed head in a fix round.
        let floor = previous_head.clone().unwrap_or_else(|| base.clone());
        let before = self.repo.commits_above(&floor, &tip_before).map_err(git_err)?;
        let wt = self.job_worktree(&job, &cwd, &base, &branch);
        let (kind, title) = if new_work {
            (CommitKind::Legacy, "build the plan")
        } else {
            (CommitKind::Fix, "address review findings")
        };
        let head = match self.commit_work(&wt, kind, title, &floor, &job.issues)? {
            CommitOutcome::Committed { tip, .. } => tip,
            CommitOutcome::NothingStaged => {
                let why = if new_work { "builder made no changes" } else { "builder made no changes on resume" };
                return self.park_with(ParkKind::NothingChanged, why.to_string(), Vec::new());
            }
            CommitOutcome::Park(reason) => return self.park_with(ParkKind::CommitFailed, reason, Vec::new()),
        };
        self.note_rewrites("build", &floor, &before, &head)?;
        self.finish_build(&job, &base, head, previous_head)
    }

    /// Creates the job worktree and records the branch and base. The git layer picks all three.
    fn start_worktree(&self, job: &JobView) -> Result<(PathBuf, String, String), EngineError> {
        // Refuse bad names before the worktree is created. The writer checks them again.
        self.check_agent_names()?;
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

    fn check_agent_names(&self) -> Result<(), EngineError> {
        for f in &self.agent_files {
            if !is_agent_file_name(&f.name) {
                return Err(EngineError::Msg(format!("agent file name {:?} is not allowed, use factory-<name>.md", f.name)));
            }
        }
        Ok(())
    }

    /// Writes the agent files into the job worktree. It runs on every build step, so a crash or a failed
    /// write on the first step is repaired on the next one. They are git-excluded, and a file that is tracked
    /// on the base branch is left alone. An existing file or a symlinked folder is never written through.
    fn write_agent_files(&self, wt: &Path) -> Result<(), EngineError> {
        if self.agent_files.is_empty() {
            return Ok(());
        }
        // All or nothing: no path is joined and nothing is created before every name is checked.
        self.check_agent_names()?;
        let rel_dir = agent_files_rel_dir();
        self.repo.exclude_paths(&[AGENT_EXCLUDE_PATTERN]).map_err(git_err)?;
        let dir = wt.join(&rel_dir);
        for p in [wt.join(rel_dir.components().next().map(|c| c.as_os_str()).unwrap_or_default()), dir.clone()] {
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(EngineError::Msg(format!("{} is a symlink, agent files are not written", p.display())));
            }
        }
        std::fs::create_dir_all(&dir)?;
        let mut shadowed = Vec::new();
        for f in &self.agent_files {
            let rel = rel_dir.join(&f.name);
            let rel_text = rel.to_string_lossy().replace('\\', "/");
            if self.repo.is_tracked_at_branch(self.repo.base_branch(), &rel_text).map_err(git_err)? {
                shadowed.push(f.name.as_str());
                continue;
            }
            match std::fs::OpenOptions::new().write(true).create_new(true).open(wt.join(&rel)) {
                Ok(mut file) => std::io::Write::write_all(&mut file, f.content.as_bytes())?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        if !shadowed.is_empty() {
            // Noted once per job, because this runs on every build step.
            let note = format!("repo agent files shadow the resolved ones: {}", shadowed.join(", "));
            let seen = std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default();
            if !seen.contains(&note) {
                self.ruling(&note)?;
            }
        }
        Ok(())
    }

    fn existing_worktree(&self, job: &JobView) -> Result<(PathBuf, String), EngineError> {
        let cwd = job.worktree.clone().ok_or_else(|| EngineError::Msg("job has no worktree".into()))?;
        let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        Ok((cwd, base))
    }

    /// The worktree handle the git layer needs, built from the job record.
    fn job_worktree(&self, job: &JobView, cwd: &Path, base: &str, branch: &str) -> Worktree {
        Worktree {
            name: JobRef { id: self.job_id.clone(), slug: job.slug.clone() }.dir_name(),
            path: cwd.to_path_buf(),
            branch: branch.to_string(),
            base: base.to_string(),
        }
    }

    /// Runs the committer step for the builder's uncommitted work.
    fn commit_work(
        &self,
        wt: &Worktree,
        kind: CommitKind,
        task_title: &str,
        floor: &str,
        issues: &[u32],
    ) -> Result<CommitOutcome, EngineError> {
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let spec = std::fs::read_to_string(self.job_dir.join("spec.md")).unwrap_or_default();
        let (plan_title, goal) = (plan_title(&plan), spec_goal(&spec));
        let req = CommitRequest { kind, plan_title: &plan_title, task_title, goal: &goal, floor, issues };
        self.commit_step(wt, &req)
    }

    /// Writes a ruling when the committer rewrote commits that were above the floor before the run.
    fn note_rewrites(&self, label: &str, floor: &str, before: &[CommitInfo], tip: &str) -> Result<(), EngineError> {
        let after = self.repo.commits_above(floor, tip).map_err(git_err)?;
        let gone: Vec<&str> = before
            .iter()
            .filter(|c| !after.iter().any(|a| a.sha == c.sha))
            .map(|c| c.sha.get(..7).unwrap_or(&c.sha))
            .collect();
        if gone.is_empty() {
            return Ok(());
        }
        let short = tip.get(..7).unwrap_or(tip);
        self.ruling(&format!("{label} commit rewrote earlier commits {}, the branch is now at {short}", gone.join(", ")))
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
        self.write_agent_files(&cwd)?;
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
            return self.park_with(ParkKind::BuilderStopped, format!("builder stopped in task {}: {:?}", task.n, run.stop_reason), builder_stop_parts(&run));
        }
        if self.repo.tip(&branch).map_err(git_err)? != tip {
            return self.park_with(
                ParkKind::BuilderCommitted,
                format!("builder committed on its own in task {} (the branch was at {} before the run)", task.n, tip.get(..7).unwrap_or(&tip)),
                Vec::new(),
            );
        }
        let before = self.repo.commits_above(&base, &tip).map_err(git_err)?;
        let wt = self.job_worktree(job, &cwd, &base, &branch);
        let new_tip = match self.commit_work(&wt, CommitKind::Task, &task.title, &base, &job.issues)? {
            CommitOutcome::Committed { tip, .. } => tip,
            CommitOutcome::NothingStaged => return self.park_with(ParkKind::NothingChanged, format!("task {} changed nothing", task.n), Vec::new()),
            CommitOutcome::Park(reason) => return self.park_with(ParkKind::CommitFailed, reason, Vec::new()),
        };
        self.note_rewrites(&format!("task {}", task.n), &base, &before, &new_tip)?;
        let short = new_tip.get(..7).unwrap_or(&new_tip).to_string();
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
        self.finish_build(job, &base, new_tip, job.head.clone())
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
            let reason = format!("diff has {} changed lines, the cap is {cap}", info.changed_lines());
            // The new head is not recorded yet, so the stat comes from the diff computed above.
            let mut parts = vec![ParkPart::new("Why it stopped", "driver", &reason)];
            if let Ok(stat) = std::fs::read_to_string(&info.stat) {
                parts.push(ParkPart::new("Changed files", "diff stat", &stat));
            }
            return self.park_with(ParkKind::DiffTooLarge, reason, parts);
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
