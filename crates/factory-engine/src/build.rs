use std::path::{Path, PathBuf};

use factory_agent::{SessionHook, StopReason};
use factory_core::{
    agent_files_rel_dir, builder_rulings, builder_status, is_agent_file_name, parse_builder_status,
    BuilderStatus, ParkKind, ParkPart, Stage, TaskSession, TaskState, AGENT_EXCLUDE_PATTERN,
};
use factory_git::{CommitInfo, Worktree};

use crate::carry::{carry_decision, carry_idle_limit, Carry, CarryInput};
use crate::commit::{CommitKind, CommitOutcome, CommitRequest, SnapshotRef};
use crate::park::{builder_blocked_parts, builder_stop_parts, part_from_file};
use crate::ports::{git_err, now, one_line, short_sha, JobView};
use crate::tasks::{parse_tasks, plan_title, spec_goal, PlanTask};
use crate::{BoardStore, Engine, EngineError, StageOutcome};

/// The most characters of a builder reply kept in `builder-r{round}.md`.
const REPLY_CAP: usize = 6000;

/// The subject of a task snapshot commit. The batch committer replaces it.
fn snapshot_subject(n: u32, title: &str) -> String {
    format!("wip: task {n} {title}")
}

/// The saved session id when it belongs to task `n` and the branch is still at the recorded tip.
fn saved_session(job: &JobView, n: u32, tip: &str) -> Option<String> {
    job.task_session
        .as_ref()
        .filter(|s| s.task == n && s.tip == tip)
        .map(|s| s.id.clone())
}

/// True when the job escalated to a model the stored builder session did not run on.
fn needs_fresh_session(job: &JobView) -> bool {
    match &job.escalated_model {
        Some(model) => job.builder_session_model.as_ref() != Some(model),
        None => false,
    }
}

/// The ruling when the last builder's session is cold and large, so a fix round should not resume it.
fn cold_session(
    last: Option<&factory_core::BuilderLast>,
    idle_limit: u64,
    max_ctx: u64,
    now: u64,
) -> Option<String> {
    let last = last?;
    let idle = now.saturating_sub(last.ended_at);
    (idle > idle_limit && last.end_ctx > max_ctx).then(|| {
        format!(
            "fresh fix builder (idle {idle}s over {idle_limit}s, context {} over {max_ctx})",
            last.end_ctx
        )
    })
}

/// True when the next build call runs a task agent. A job with a builder session but no task count is a
/// legacy whole-plan job and never does.
pub(crate) fn is_mid_task(job: &JobView, tasks: &[PlanTask]) -> bool {
    let legacy = job.session_id.is_some() && job.task_index == 0;
    !legacy && (job.task_index as usize) < tasks.len()
}

impl Engine<'_> {
    /// Runs the builder, then records the new head. With a task list in `plan.md` it runs one task agent per
    /// call and stays in Build until the last task has a commit. Fix rounds resume the last session unless it is
    /// escalated, or cold and large.
    pub fn build_stage(&self) -> Result<StageOutcome, EngineError> {
        // Provider changes must clear old session ids before this stage copies them into specs.
        self.sync_provider()?;
        let job = self.job()?;
        let tasks = parse_tasks(&std::fs::read_to_string(&self.plan_file).unwrap_or_default());
        // A pending batch commit resumes before any builder runs.
        if job.batch_snapshot.is_some() {
            return self.resume_batch(&job, &tasks);
        }
        if is_mid_task(&job, &tasks) {
            return self.build_task(&job, &tasks);
        }
        let fresh = job.worktree.is_none();
        // A rework of a job parked mid-task leaves a note and keeps the round, so a note marks a fix round too.
        let fixing = job.round > 1 || self.job_dir.join("rework-note.md").exists();
        // A fix round or rework on a model other than the stored session's starts a fresh session.
        let escalated = !fresh && fixing && needs_fresh_session(&job);
        // A fix round or rework that would resume a cold, large session starts a fresh session too. The idle
        // limit is the Anthropic cache TTL, so Codex sessions always resume.
        let codex = self.config.agent_cli() == "codex";
        let cold = if !fresh && !escalated && !codex && fixing && job.session_id.is_some() {
            cold_session(
                job.builder_last.as_ref(),
                carry_idle_limit(self.config.cache_ttl.as_deref()),
                self.config.fix_fresh_over_ctx,
                now().max(0.0) as u64,
            )
        } else {
            None
        };
        let fresh_fix = escalated || cold.is_some();
        let new_work = !fresh_fix && (fresh || job.session_id.is_none());
        let (cwd, base, previous_head, branch) = if fresh {
            let (cwd, base, branch) = self.start_worktree(&job)?;
            self.ledger_task(
                1,
                TaskState::Dispatched,
                &format!("BASE {base}, {}", self.builder_model()),
            )?;
            (cwd, base, None, branch)
        } else {
            let (cwd, base) = self.existing_worktree(&job)?;
            (cwd, base, job.head.clone(), job.branch.clone())
        };
        self.write_agent_files(&cwd)?;
        if let Some(text) = &cold {
            self.ruling(text)?;
        }
        let spec = match &job.session_id {
            _ if fresh_fix => {
                let findings = self.fix_findings(&job)?;
                self.builder_fix_fresh_spec(&job, &cwd, &findings)
            }
            Some(session) if !fresh => {
                let findings = self.fix_findings(&job)?;
                self.builder_resume_spec(&job, &cwd, &findings, session)
            }
            _ => self.builder_first_spec(&job, &cwd),
        };
        let tip_before = self.repo.tip(&branch).map_err(git_err)?;
        let run = match self.run_builder_guarded(spec)? {
            Ok(run) => run,
            Err(parked) => return Ok(parked),
        };
        if !new_work {
            // The builder has seen the note, so it must not be sent again. A missing file is fine.
            let _ = std::fs::remove_file(self.job_dir.join("rework-note.md"));
        }
        // The time is read when the builder run returns, so the next round can tell if its cache expired.
        let ended_at = now().max(0.0) as u64;
        let session = run.session_id.clone().or(job.session_id.clone());
        let builder_last = session.clone().map(|id| factory_core::BuilderLast {
            task: job.builder_last.as_ref().map_or(job.task_index, |l| l.task),
            id,
            ended_at,
            end_ctx: run.end_ctx,
        });
        let session_model = self.session_model(&run, job.builder_session_model.clone());
        self.update(|j| {
            j.builder_last = builder_last;
            j.builder_id = session;
            j.builder_session_model = session_model;
            Ok(())
        })?;
        // The reply to the findings may hold push-back reasons, so keep it for the reviewer and the human.
        let reply = if new_work {
            None
        } else {
            self.save_builder_reply(job.round, &run.result_text)
        };
        if run.stop_reason != StopReason::Done {
            return self.park_with(
                ParkKind::BuilderStopped,
                format!("builder stopped: {:?}", run.stop_reason),
                builder_stop_parts(&run),
            );
        }
        if let Some(out) = self.park_if_blocked(&run, None)? {
            return Ok(out);
        }
        if self.repo.tip(&branch).map_err(git_err)? != tip_before {
            return self.park_with(
                ParkKind::BuilderCommitted,
                format!(
                    "builder committed on its own (the branch was at {} before the run)",
                    short_sha(&tip_before)
                ),
                Vec::new(),
            );
        }
        // The floor is the base in the first round and the last reviewed head in a fix round.
        let floor = previous_head.clone().unwrap_or_else(|| base.clone());
        let before = self
            .repo
            .commits_above(&floor, &tip_before)
            .map_err(git_err)?;
        let wt = self.job_worktree(&job, &cwd, &base, &branch);
        let (kind, title) = if new_work {
            (CommitKind::Legacy, "build the plan")
        } else {
            (CommitKind::Fix, "address review findings")
        };
        let head = match self.commit_work(&wt, kind, title, &floor, &job.issues)? {
            CommitOutcome::Committed { tip, .. } => tip,
            CommitOutcome::NothingStaged => {
                if !new_work && job.fix_source.as_deref() == Some("checks") {
                    return self.nothing_changed_after_checks(
                        &job,
                        &base,
                        tip_before,
                        previous_head,
                    );
                }
                let why = if new_work {
                    "builder made no changes"
                } else {
                    "builder made no changes on resume"
                };
                let mut parts = if matches!(job.fix_source.as_deref(), Some("review" | "rework")) {
                    self.lead_parts(&self.job_changed_paths().unwrap_or_default())
                } else {
                    Vec::new()
                };
                parts.push(match reply {
                    Some(text) => ParkPart::new("Builder reply", "builder", &text),
                    None => {
                        ParkPart::new("Builder reply", "builder", "The builder wrote no reply.")
                    }
                });
                return self.park_with(ParkKind::NothingChanged, why.to_string(), parts);
            }
            CommitOutcome::Park(reason) => {
                return self.park_with(ParkKind::CommitFailed, reason, Vec::new())
            }
        };
        self.note_rewrites("build", &floor, &before, &head)?;
        self.finish_build(&job, &base, head, previous_head)
    }

    /// The findings file of a fix round. A rework note from the user wins over the round findings file.
    fn fix_findings(&self, job: &JobView) -> Result<PathBuf, EngineError> {
        let note = self.job_dir.join("rework-note.md");
        let findings = if note.exists() {
            note
        } else {
            self.job_dir.join(format!("changes-r{}.md", job.round))
        };
        if !findings.exists() {
            return Err(EngineError::Msg(format!(
                "missing findings file {}",
                findings.display()
            )));
        }
        Ok(findings)
    }

    /// The model of the builder session stored after a run: this run's model when it reported a session,
    /// else the model of the earlier session it keeps.
    pub(crate) fn session_model(
        &self,
        run: &factory_agent::AgentRun,
        earlier: Option<String>,
    ) -> Option<String> {
        if run.session_id.is_some() {
            Some(self.builder_model())
        } else {
            earlier
        }
    }

    /// Writes the builder's reply to a fix round as `builder-r{round}.md` in the job folder.
    /// Returns the saved text, or `None` when the reply is empty or the file cannot be written.
    fn save_builder_reply(&self, round: u32, reply: &str) -> Option<String> {
        let text: String = reply.trim().chars().take(REPLY_CAP).collect();
        if text.is_empty() {
            return None;
        }
        std::fs::write(self.job_dir.join(format!("builder-r{round}.md")), &text).ok()?;
        Some(text)
    }

    /// A fix round that started from failed checks changed nothing. The checks run again once: when they pass,
    /// the earlier failure did not reproduce and the unchanged head goes to the reviewer, else the job parks.
    fn nothing_changed_after_checks(
        &self,
        job: &JobView,
        base: &str,
        head: String,
        previous_head: Option<String>,
    ) -> Result<StageOutcome, EngineError> {
        let report = self.run_checks_now()?;
        if report.all_passed() {
            self.ruling(&format!(
                "fix round {} changed nothing and the checks now pass, the earlier failure did not reproduce",
                job.round
            ))?;
            return self.finish_build(job, base, head, previous_head);
        }
        let first = report
            .failures()
            .first()
            .map_or_else(|| "a check".to_string(), |f| f.command.clone());
        let parts = part_from_file(&self.job_dir, "checks.md", "Checks")
            .into_iter()
            .collect();
        self.park_with(
            ParkKind::ChecksFailed,
            format!(
                "fix round {} changed nothing and the checks still fail: {first}",
                job.round
            ),
            parts,
        )
    }

    /// Creates the job worktree and records the branch and base. The git layer picks all three.
    fn start_worktree(&self, job: &JobView) -> Result<(PathBuf, String, String), EngineError> {
        // Refuse bad names before the worktree is created. The writer checks them again.
        self.check_agent_names()?;
        let wt = self
            .repo
            .create_worktree(&self.repo.job_ref(&self.job_id, &job.slug))
            .map_err(git_err)?;
        let (path, base, branch) = (
            wt.path.to_string_lossy().to_string(),
            wt.base.clone(),
            wt.branch.clone(),
        );
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
                return Err(EngineError::Msg(format!(
                    "agent file name {:?} is not allowed, use factory-<name>.md",
                    f.name
                )));
            }
        }
        Ok(())
    }

    /// Writes the agent files into the job worktree, and restores any that changed. It runs before every build
    /// step and every agent run in the worktree, so a crash or an edit by an earlier agent is repaired. The files
    /// are git-excluded, and a file that is tracked on the base branch is left alone. A symlinked folder is never
    /// written through.
    pub(crate) fn write_agent_files(&self, wt: &Path) -> Result<(), EngineError> {
        if self.agent_files.is_empty() {
            return Ok(());
        }
        // All or nothing: no path is joined and nothing is created before every name is checked.
        self.check_agent_names()?;
        // A folder that is not a git checkout would be created or filled with debris, so refuse it.
        if !wt.join(".git").exists() {
            return Err(EngineError::Msg(format!(
                "{} is not a git checkout, agent files are not written",
                wt.display()
            )));
        }
        let rel_dir = agent_files_rel_dir();
        self.repo
            .exclude_paths(&[AGENT_EXCLUDE_PATTERN])
            .map_err(git_err)?;
        let dir = wt.join(&rel_dir);
        for p in [
            wt.join(
                rel_dir
                    .components()
                    .next()
                    .map(|c| c.as_os_str())
                    .unwrap_or_default(),
            ),
            dir.clone(),
        ] {
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(EngineError::Msg(format!(
                    "{} is a symlink, agent files are not written",
                    p.display()
                )));
            }
        }
        std::fs::create_dir_all(&dir)?;
        let mut shadowed = Vec::new();
        for f in &self.agent_files {
            let rel = rel_dir.join(&f.name);
            let rel_text = rel.to_string_lossy().replace('\\', "/");
            if self
                .repo
                .is_tracked_at_branch(self.repo.base_branch(), &rel_text)
                .map_err(git_err)?
            {
                shadowed.push(f.name.as_str());
                continue;
            }
            self.restore_agent_file(&wt.join(&rel), &f.content)?;
        }
        if !shadowed.is_empty() {
            // Noted once per job, because this runs on every build step.
            let note = format!(
                "repo agent files shadow the resolved ones: {}",
                shadowed.join(", ")
            );
            let seen =
                std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default();
            if !seen.contains(&note) {
                self.ruling(&note)?;
            }
        }
        Ok(())
    }

    /// Makes the file at `path` hold `content`. A missing file is created, a symlink is removed (never followed)
    /// and replaced, and a file that differs is removed and created anew (a new inode), so an agent cannot weaken a later agent's file.
    fn restore_agent_file(&self, path: &Path, content: &str) -> Result<(), EngineError> {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(path)?,
            Ok(m) if m.is_file() => {
                if std::fs::read_to_string(path).is_ok_and(|t| t == content) {
                    return Ok(());
                }
                // Remove first, so a hard link to a file outside is never written through.
                std::fs::remove_file(path)?;
            }
            Ok(_) => {
                return Err(EngineError::Msg(format!(
                    "{} is not a regular file",
                    path.display()
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        std::io::Write::write_all(&mut file, content.as_bytes())?;
        Ok(())
    }

    fn existing_worktree(&self, job: &JobView) -> Result<(PathBuf, String), EngineError> {
        let cwd = job
            .worktree
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no worktree".into()))?;
        let base = job
            .base
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        Ok((cwd, base))
    }

    /// The worktree handle the git layer needs, built from the job record.
    fn job_worktree(&self, job: &JobView, cwd: &Path, base: &str, branch: &str) -> Worktree {
        Worktree {
            name: self.repo.job_ref(&self.job_id, &job.slug).dir_name(),
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
        let req = CommitRequest {
            kind,
            plan_title: &plan_title,
            task_title,
            goal: &goal,
            floor,
            issues,
            snapshots: &[],
        };
        self.commit_step(wt, &req)
    }

    /// Writes a ruling when the committer rewrote commits that were above the floor before the run.
    fn note_rewrites(
        &self,
        label: &str,
        floor: &str,
        before: &[CommitInfo],
        tip: &str,
    ) -> Result<(), EngineError> {
        let after = self.repo.commits_above(floor, tip).map_err(git_err)?;
        let gone: Vec<&str> = before
            .iter()
            .filter(|c| !after.iter().any(|a| a.sha == c.sha))
            .map(|c| short_sha(&c.sha))
            .collect();
        if gone.is_empty() {
            return Ok(());
        }
        let short = short_sha(tip);
        self.ruling(&format!(
            "{label} commit rewrote earlier commits {}, the branch is now at {short}",
            gone.join(", ")
        ))
    }

    /// Parks the job when the builder reply says BLOCKED or NEEDS_CONTEXT, before anything is committed.
    /// A reply without a usable status line counts as done, and a task run notes that in the ledger.
    fn park_if_blocked(
        &self,
        run: &factory_agent::AgentRun,
        task: Option<u32>,
    ) -> Result<Option<StageOutcome>, EngineError> {
        let status = match parse_builder_status(&run.result_text) {
            Some(status) => status,
            None => {
                if let Some(n) = task {
                    self.ruling(&format!(
                        "builder task {n}: no status line, treated as done"
                    ))?;
                }
                return Ok(None);
            }
        };
        let (what, why) = match &status {
            BuilderStatus::Blocked(why) => ("blocked", why),
            BuilderStatus::NeedsContext(why) => ("needs context", why),
            BuilderStatus::Done | BuilderStatus::DoneWithConcerns(_) => return Ok(None),
        };
        let first = why.split_whitespace().collect::<Vec<_>>().join(" ");
        let first: String = first.chars().take(300).collect();
        let place = task.map_or(String::new(), |n| format!(" in task {n}"));
        let reason = format!("builder {what}{place}: {first}");
        self.park_with(
            ParkKind::BuilderBlocked,
            reason,
            builder_blocked_parts(&status, run),
        )
        .map(Some)
    }

    /// Writes the builder's `Ruling:` lines and its concerns to the ledger, so the reviewer can read them.
    fn note_builder_words(&self, n: u32, reply: &str) -> Result<(), EngineError> {
        for text in builder_rulings(reply) {
            self.ruling(&format!("builder task {n}: {text}"))?;
        }
        if let BuilderStatus::DoneWithConcerns(text) = builder_status(reply) {
            let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let folded: String = folded.chars().take(600).collect();
            if !folded.is_empty() {
                self.ruling(&format!("builder task {n} concerns: {folded}"))?;
            }
        }
        Ok(())
    }

    /// Runs the checks over one task's diff and notes the result in the ledger. It is informational only:
    /// a failure or an error never parks the job and never starts a fix round.
    fn note_task_checks(&self, n: u32, from: &str, to: &str) -> Result<(), EngineError> {
        match self.run_checks_between(from, to, &format!("checks-task{n}.md")) {
            Ok(None) => Ok(()),
            Ok(Some(report)) => match report.failures().first() {
                None => self.ruling(&format!("task {n} checks: pass")),
                Some(f) => self.ruling(&format!("task {n} checks: fail {}", f.command)),
            },
            Err(e) => self.ruling(&format!("task {n} checks: not run, {e}")),
        }
    }

    /// A hook that saves the builder session of task `task` on the board while the run goes on.
    /// It uses its own store, because the stream thread calls it. A failed write is logged and dropped.
    fn session_hook(&self, task: u32, tip: &str) -> SessionHook {
        let board_path = self.board_path.clone();
        let job_id = self.job_id.clone();
        let wait = self.lock_wait;
        let tip = tip.to_string();
        SessionHook::new(move |id| {
            let saved = BoardStore::new(&board_path, wait).update_job(&job_id, |j| {
                j.task_session = Some(TaskSession {
                    task,
                    id: id.to_string(),
                    tip: tip.clone(),
                });
                Ok(())
            });
            if let Err(e) = saved {
                tracing::warn!(event = "builder.session_save_failed", job_id = %job_id, task, msg = %e);
            }
        })
    }

    /// Forgets the saved builder session once a builder run has returned.
    fn clear_task_session(&self) -> Result<(), EngineError> {
        self.update(|j| {
            j.task_session = None;
            Ok(())
        })
    }

    /// Runs the agent of the next plan task. After the last task it hands over to review.
    fn build_task(&self, job: &JobView, tasks: &[PlanTask]) -> Result<StageOutcome, EngineError> {
        let Some(task) = tasks.get(job.task_index as usize) else {
            return Err(EngineError::Msg("task index is past the plan".into()));
        };
        let started = std::time::Instant::now();
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
        let saved = saved_session(job, task.n, &tip);
        let note_path = self.job_dir.join("rework-note.md");
        let note = std::fs::read_to_string(&note_path).ok();
        let carry = carry_decision(&CarryInput {
            enabled: self.config.builder_carry_session,
            max_ctx: self.config.builder_carry_max_ctx,
            idle_limit: carry_idle_limit(self.config.cache_ttl.as_deref()),
            last: job.builder_last.as_ref(),
            task_n: task.n,
            now: now().max(0.0) as u64,
            rework_note: note.is_some(),
            saved: saved.is_some(),
            codex: self.config.agent_cli() == "codex",
        });
        let how = match (&saved, &carry) {
            (Some(_), _) => "resumed".to_string(),
            (None, Carry::Yes { .. }) => "continued".to_string(),
            (None, Carry::No { reason }) if self.config.builder_carry_session => {
                format!("fresh ({reason})")
            }
            (None, Carry::No { .. }) => "fresh".to_string(),
        };
        self.ledger_task(
            task.n,
            TaskState::Dispatched,
            &format!("BASE {base}, {}, {how}", self.builder_model()),
        )?;
        tracing::info!(
            event = "task.start",
            task = task.n,
            title = %factory_telemetry::cap_message(&one_line(&task.title)),
            how = %how,
            model = %self.builder_model(),
            of = tasks.len() as u32,
            msg = %format!("task {} started", task.n)
        );
        let carried = match &carry {
            Carry::Yes { id } if saved.is_none() => Some(id.clone()),
            _ => None,
        };
        let mut spec = match (&saved, &carried) {
            (Some(id), _) => self.builder_task_resume_spec(&cwd, task, id),
            (None, Some(id)) => self.builder_task_carry_spec(&cwd, task, id, &base, &tip),
            (None, None) => self.builder_task_spec(job, &cwd, task, &base, &tip, &earlier),
        };
        if let Some(text) = &note {
            spec.prompt
                .push_str(&format!("\nA note from the user for this task:\n{text}\n"));
        }
        spec.on_session = Some(self.session_hook(task.n, &tip));
        let drop_saved = saved.is_none() && job.task_session.is_some();
        if drop_saved || job.builder_last.is_some() {
            // A saved session of another task or an older tip is dropped, and the last
            // builder record is cleared because a new builder is about to run.
            self.update(|j| {
                if drop_saved {
                    j.task_session = None;
                }
                j.builder_last = None;
                Ok(())
            })?;
        }
        let first = self.run_builder_guarded(spec);
        let mut run = match first {
            Ok(Ok(run)) => run,
            Ok(Err(parked)) => {
                self.clear_task_session()?;
                return Ok(parked);
            }
            Err(e) => return Err(e),
        };
        if (saved.is_some() || carried.is_some()) && run.stop_reason == StopReason::Failed {
            // A failed resume or carry, such as a missing transcript, gets one fresh builder on the worktree as it is.
            self.clear_task_session()?;
            let again = if saved.is_some() {
                "fresh"
            } else {
                "fresh (carry failed)"
            };
            self.ledger_task(
                task.n,
                TaskState::Dispatched,
                &format!("BASE {base}, {}, {again}", self.builder_model()),
            )?;
            let mut spec = self.builder_task_spec(job, &cwd, task, &base, &tip, &earlier);
            if let Some(text) = &note {
                spec.prompt
                    .push_str(&format!("\nA note from the user for this task:\n{text}\n"));
            }
            spec.on_session = Some(self.session_hook(task.n, &tip));
            run = match self.run_builder_guarded(spec)? {
                Ok(run) => run,
                Err(parked) => {
                    self.clear_task_session()?;
                    return Ok(parked);
                }
            };
        }
        // The time is read when the last builder run returns, before checks and the snapshot.
        let ended_at = crate::ports::now() as u64;
        self.clear_task_session()?;
        if note.is_some() {
            // The builder has seen the note, so it must not be sent again.
            let _ = std::fs::remove_file(&note_path);
        }
        if run.stop_reason != StopReason::Done {
            return self.park_with(
                ParkKind::BuilderStopped,
                format!("builder stopped in task {}: {:?}", task.n, run.stop_reason),
                builder_stop_parts(&run),
            );
        }
        if let Some(out) = self.park_if_blocked(&run, Some(task.n))? {
            return Ok(out);
        }
        if self.repo.tip(&branch).map_err(git_err)? != tip {
            return self.park_with(
                ParkKind::BuilderCommitted,
                format!(
                    "builder committed on its own in task {} (the branch was at {} before the run)",
                    task.n,
                    short_sha(&tip)
                ),
                Vec::new(),
            );
        }
        let wt = self.job_worktree(job, &cwd, &base, &branch);
        let new_tip = if self.config.commit_batch {
            if !self.repo.head_is_on_branch(&wt).map_err(git_err)? {
                let short = short_sha(&tip);
                let at = self.repo.head_sha(&wt).map_or(String::new(), |id| {
                    format!(", HEAD is at {}", short_sha(&id))
                });
                return self.park_with(
                    ParkKind::BuilderCommitted,
                    format!(
                        "builder moved HEAD off the job branch in task {} (the branch was at {short} before the run{at}); if the detached HEAD has commits, note its sha with `git rev-parse HEAD` first, then run `git checkout {branch}` in the job worktree (uncommitted changes stay), then `factory rework {}`",
                        task.n, self.job_id
                    ),
                    Vec::new(),
                );
            }
            let staged = self.repo.stage_all(&wt).map_err(git_err)?;
            if staged.is_empty() {
                return self.park_with(
                    ParkKind::NothingChanged,
                    format!("task {} changed nothing", task.n),
                    Vec::new(),
                );
            }
            let sha = self
                .repo
                .commit_staged(&wt, &snapshot_subject(task.n, &task.title))
                .map_err(git_err)?;
            tracing::info!(
                event = "commit.done",
                kind = "snapshot",
                sha = %sha,
                fallback = false,
                "snapshot commit made"
            );
            sha
        } else {
            let before = self.repo.commits_above(&base, &tip).map_err(git_err)?;
            let new_tip =
                match self.commit_work(&wt, CommitKind::Task, &task.title, &base, &job.issues)? {
                    CommitOutcome::Committed { tip, .. } => tip,
                    CommitOutcome::NothingStaged => {
                        return self.park_with(
                            ParkKind::NothingChanged,
                            format!("task {} changed nothing", task.n),
                            Vec::new(),
                        )
                    }
                    CommitOutcome::Park(reason) => {
                        return self.park_with(ParkKind::CommitFailed, reason, Vec::new())
                    }
                };
            self.note_rewrites(&format!("task {}", task.n), &base, &before, &new_tip)?;
            new_tip
        };
        let short = short_sha(&new_tip).to_string();
        self.note_task_checks(task.n, &tip, &new_tip)?;
        self.ledger_task(task.n, TaskState::ImplementerDone, &short)?;
        tracing::info!(
            event = "task.end",
            task = task.n,
            sha = %new_tip,
            of = tasks.len() as u32,
            duration_ms = started.elapsed().as_millis() as u64,
            msg = %format!("task {} done", task.n)
        );
        self.note_builder_words(task.n, &run.result_text)?;
        let session = run.session_id.clone().or(job.session_id.clone());
        let builder_last = session.clone().map(|id| factory_core::BuilderLast {
            task: task.n,
            id,
            ended_at,
            end_ctx: run.end_ctx,
        });
        let next = job.task_index + 1;
        let last = next as usize >= tasks.len();
        let snapshot = (self.config.commit_batch && last).then(|| new_tip.clone());
        let session_model = self.session_model(&run, job.builder_session_model.clone());
        self.update(|j| {
            j.builder_last = builder_last;
            j.builder_id = session;
            j.builder_session_model = session_model;
            j.task_index = next;
            j.batch_snapshot = snapshot;
            j.task_session = None;
            Ok(())
        })?;
        if !last {
            return Ok(StageOutcome::Advanced(Stage::Build));
        }
        if self.config.commit_batch {
            let job = self.job()?;
            return self.resume_batch(&job, tasks);
        }
        self.finish_build(job, &base, new_tip, job.head.clone())
    }

    /// Runs the one committer of a job after its last task. It is safe to call again after a park:
    /// the branch is either still at the snapshot tip or already reset to the base.
    fn resume_batch(&self, job: &JobView, tasks: &[PlanTask]) -> Result<StageOutcome, EngineError> {
        let snapshot = job
            .batch_snapshot
            .clone()
            .ok_or_else(|| EngineError::Msg("no pending batch commit".into()))?;
        let (cwd, base) = self.existing_worktree(job)?;
        let branch = job.branch.clone();
        let wt = self.job_worktree(job, &cwd, &base, &branch);
        let tip = self.repo.tip(&branch).map_err(git_err)?;
        if tip == snapshot {
            self.repo.reset_soft(&wt, &base).map_err(git_err)?;
        } else if tip != base {
            if self.batch_already_committed(&base, &tip, &snapshot)? {
                // The committer finished and the driver stopped before it cleared the flag.
                return self.finish_batch(job, &base, tip, 0, &snapshot);
            }
            let short = short_sha(&base);
            let reason = format!(
                "the branch moved during the batch commit; run `git reset --soft {short}` in the job worktree, then `factory rework {}`",
                self.job_id
            );
            return self.park_with(ParkKind::CommitFailed, reason, Vec::new());
        }
        let snaps = self.snapshot_refs(&base, &snapshot, tasks)?;
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let spec = std::fs::read_to_string(self.job_dir.join("spec.md")).unwrap_or_default();
        let (title, goal) = (plan_title(&plan), spec_goal(&spec));
        let req = CommitRequest {
            kind: CommitKind::Batch,
            plan_title: &title,
            task_title: &title,
            goal: &goal,
            floor: &base,
            issues: &job.issues,
            snapshots: &snaps,
        };
        let head = match self.commit_step(&wt, &req)? {
            CommitOutcome::Committed { tip, .. } => tip,
            CommitOutcome::NothingStaged => {
                return self.park_with(
                    ParkKind::NothingChanged,
                    "the batch commit found nothing staged".to_string(),
                    Vec::new(),
                )
            }
            CommitOutcome::Park(reason) => {
                return self.park_with(ParkKind::CommitFailed, reason, Vec::new())
            }
        };
        self.finish_batch(job, &base, head, snaps.len(), &snapshot)
    }

    /// True when the tip holds the snapshot work as real commits: above the base, no `wip:` subject, same tree.
    fn batch_already_committed(
        &self,
        base: &str,
        tip: &str,
        snapshot: &str,
    ) -> Result<bool, EngineError> {
        if !self.repo.is_ancestor(base, tip).map_err(git_err)? {
            return Ok(false);
        }
        let commits = self.repo.commits_above(base, tip).map_err(git_err)?;
        if commits.iter().any(|c| c.subject.starts_with("wip:")) {
            return Ok(false);
        }
        self.repo.same_tree(tip, snapshot).map_err(git_err)
    }

    /// A rework note that the batch commit left unread must not displace the findings of a later fix round.
    /// Its text goes to the ledger and the file is removed.
    fn keep_unsent_note(&self) -> Result<(), EngineError> {
        let path = self.job_dir.join("rework-note.md");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let kept: String = one_line.chars().take(300).collect();
        self.ruling(&format!(
            "rework note not sent to a builder because the batch commit ran no builder: {kept}"
        ))?;
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    /// Records the end of a batch commit: a ruling, a cleared flag, then the usual build finish.
    fn finish_batch(
        &self,
        job: &JobView,
        base: &str,
        head: String,
        count: usize,
        snapshot: &str,
    ) -> Result<StageOutcome, EngineError> {
        let short = short_sha(&head).to_string();
        let was = short_sha(snapshot);
        let what = if count == 0 {
            format!("batch commit already done, replaced the snapshots up to {was}")
        } else {
            format!("batch commit replaced {count} snapshots")
        };
        self.ruling(&format!("{what}, the branch is at {short}"))?;
        self.keep_unsent_note()?;
        self.update(|j| {
            j.batch_snapshot = None;
            Ok(())
        })?;
        self.finish_build(job, base, head, job.head.clone())
    }

    /// The snapshot commits above the base, oldest first, matched to the plan tasks by position.
    fn snapshot_refs(
        &self,
        base: &str,
        snapshot: &str,
        tasks: &[PlanTask],
    ) -> Result<Vec<SnapshotRef>, EngineError> {
        let commits = self.repo.commits_above(base, snapshot).map_err(git_err)?;
        Ok(commits
            .into_iter()
            .enumerate()
            .map(|(i, c)| SnapshotRef {
                n: tasks.get(i).map_or(i as u32 + 1, |t| t.n),
                sha: c.sha,
                title: tasks
                    .get(i)
                    .map_or_else(|| c.subject.clone(), |t| t.title.clone()),
                files: c.files,
            })
            .collect())
    }

    /// Checks the diff cap, records the head and moves the job to review.
    fn finish_build(
        &self,
        job: &JobView,
        base: &str,
        head: String,
        previous_head: Option<String>,
    ) -> Result<StageOutcome, EngineError> {
        let from = self.range_start(base, &head)?;
        let info = self
            .repo
            .diff_to_files(&from, &head, &self.job_dir)
            .map_err(git_err)?;
        let cap = self.config.diff_cap_lines as usize;
        if info.changed_lines() > cap {
            let reason = format!(
                "diff has {} changed lines, the cap is {cap}",
                info.changed_lines()
            );
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
            crate::ports::set_stage_logged(j, Stage::Review, None, false, now())
        })?;
        self.ruling(&format!("head of round {} is {head}", job.round))?;
        Ok(StageOutcome::Advanced(Stage::Review))
    }

    /// The directory of the worktree, or an error when the job has none yet.
    pub(crate) fn worktree(&self) -> Result<PathBuf, EngineError> {
        self.job()?
            .worktree
            .ok_or_else(|| EngineError::Msg("job has no worktree".into()))
    }
}

pub fn build_stage(e: &Engine) -> Result<StageOutcome, EngineError> {
    e.build_stage()
}
