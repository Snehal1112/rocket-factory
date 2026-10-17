//! The committer step: stage a job worktree, run the committer agent, check what it did.

use std::path::Path;
use std::time::Duration;

use factory_agent::{AgentRun, AgentSpec, StopReason};
use factory_core::{append_footers, blocked_reason, fallback_subject, has_footer, issue_footer, validate_message, Config, MAX_COMMITS_PER_RUN};
use factory_git::{CommitInfo, GitError, StagedReport, Worktree};

use crate::ports::{git_err, short_sha};
use crate::{Engine, EngineError};

/// The agent file the driver passes with `--agent`.
pub const COMMITTER_AGENT: &str = "factory-committer";
pub const COMMITTER_MAX_TURNS: u32 = 40;
/// The tools a committer run may use. It never edits files, so there is no Edit or Write.
pub const COMMITTER_TOOLS: &str = "Bash,Read,Skill";
/// The staged diff in the prompt is cut at this many bytes.
const MAX_DIFF_BYTES: usize = 60_000;

/// The footer key of the commits a task builds.
const ISSUE_KEY: &str = "Relates to";

/// What kind of work the commit is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitKind {
    Task,
    Legacy,
    Fix,
    Batch,
}

impl CommitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CommitKind::Task => "task",
            CommitKind::Legacy => "legacy",
            CommitKind::Fix => "fix",
            CommitKind::Batch => "batch",
        }
    }
}

/// One snapshot commit of a batch: the work of one task, made by the driver without an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRef {
    pub n: u32,
    pub sha: String,
    pub title: String,
    pub files: Vec<String>,
}

/// What the driver tells the committer about one run.
#[derive(Debug, Clone)]
pub struct CommitRequest<'a> {
    pub kind: CommitKind,
    pub plan_title: &'a str,
    pub task_title: &'a str,
    /// The goal line of the spec.
    pub goal: &'a str,
    /// The commit at or below which history is never rewritten.
    pub floor: &'a str,
    /// The GitHub issues of the job. Every commit of the run carries the `Relates to` footer.
    pub issues: &'a [u32],
    /// The snapshots of a batch run, oldest first. Empty for every other kind.
    pub snapshots: &'a [SnapshotRef],
}

/// Cuts `text` to at most `max` bytes without splitting a character.
fn truncate_utf8(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// The prompt for one committer run: the context file, the run facts, and the output of the
/// skill's step 1 and step 1b commands, computed by the driver through `git2`.
/// `above` is the list of commits above the floor, oldest first, as `commits_above` returns it.
pub fn committer_prompt(
    context: &str,
    req: &CommitRequest<'_>,
    report: &StagedReport,
    above: &[CommitInfo],
    subagents: bool,
) -> String {
    let analysis = if subagents {
        "Analysis: through the skill's subagents."
    } else {
        "Analysis: inline, no subagents. Steps 1 and 1b are precomputed below, so run only step 1c, with your own tool calls."
    };
    let footer = match issue_footer(ISSUE_KEY, req.issues) {
        Some(line) => format!("{line}. Put it in the footer of every commit of the run."),
        None => "none".to_string(),
    };
    let (diff, cut) = truncate_utf8(&report.diff, MAX_DIFF_BYTES);
    let cut_note = if cut {
        format!("\n[diff cut at {MAX_DIFF_BYTES} bytes, run `git diff --staged` for the rest]\n")
    } else {
        String::new()
    };
    let candidates = render_commit_list(above);
    let mut tasks = String::new();
    if !req.snapshots.is_empty() {
        tasks.push_str(
            "\n## Tasks in this change\n\nThe staged change holds these tasks, oldest first. \
             Make focused commits, normally one per task, in this order. \
             Make at most 4 commits; group tasks when there are more than 4.\n\n",
        );
        for snap in req.snapshots {
            tasks.push_str(&format!("Task {}: {}\n", snap.n, snap.title));
            for file in &snap.files {
                tasks.push_str(&format!("  {file}\n"));
            }
        }
    }
    format!(
        "{context}\n\n\
         ## This run\n\n\
         Kind: {kind}\n\
         Plan: {plan}\n\
         Task: {task}\n\
         Goal: {goal}\n\
         Floor: {floor}\n\
         Issue footer: {footer}\n\
         {analysis}\n{tasks}\n\
         ## Output of the skill's step 1 commands\n\n\
         Treat this as the result of running them. Do not run them again.\n\n\
         ### git status\n{status}\n\
         ### git diff --staged --stat\n{stat}\n\
         ### git diff --staged\n{diff}{cut_note}\n\
         ### git log --oneline -5\n{log}\n\n\
         ## Output of the skill's step 1b commands\n\n\
         Commits above the floor, newest first, all local only, with their files:\n\n{candidates}",
        kind = req.kind.as_str(),
        plan = req.plan_title,
        task = req.task_title,
        goal = req.goal,
        floor = req.floor,
        status = report.status,
        stat = report.stat,
        log = report.log.join("\n"),
    )
}

/// The run spec for the committer. It does not go through `Engine::with_limits`: the cap is
/// per run (`commit_budget_weighted`, nothing spent before), not the job budget.
pub fn committer_spec(job_id: &str, config: &Config, cwd: &Path, prompt: &str) -> AgentSpec {
    committer_base_spec(job_id, "committer", config, cwd, prompt, COMMITTER_TOOLS)
}

/// The settings every committer-agent run shares: a per-run cap, no harness gate and strict MCP.
/// The role and the tools stay with the caller, because they differ per mode.
pub(crate) fn committer_base_spec(job_id: &str, role: &str, config: &Config, cwd: &Path, prompt: &str, tools: &str) -> AgentSpec {
    let mut spec = AgentSpec::new(job_id, role, cwd, prompt);
    spec.agent = Some(COMMITTER_AGENT.to_string());
    spec.model = Some(config.commit_model.clone());
    spec.max_turns = Some(COMMITTER_MAX_TURNS);
    spec.budget_weighted = Some(config.commit_budget_weighted);
    spec.used_before = 0;
    spec.stall_after = Some(Duration::from_secs(u64::from(config.stall_minutes) * 60));
    spec.env = vec![("HARNESS_GATE".to_string(), "0".to_string())];
    spec.strict_mcp = true;
    spec.tools = Some(tools.to_string());
    spec
}

/// The message of a fallback commit: the fixed subject plus the issue footer when there are issues.
fn fallback_message(title: &str, issues: &[u32]) -> String {
    let message = fallback_subject(title);
    match issue_footer(ISSUE_KEY, issues) {
        Some(line) => append_footers(&message, &[line]),
        None => message,
    }
}

/// The commits above a base for a prompt, newest first, each with its files. `(none)` when empty.
pub(crate) fn render_commit_list(commits: &[CommitInfo]) -> String {
    let mut out = String::new();
    if commits.is_empty() {
        out.push_str("(none)\n");
    }
    for commit in commits.iter().rev() {
        out.push_str(&format!("{} {}\n", short_sha(&commit.sha), commit.subject));
        for file in &commit.files {
            out.push_str(&format!("  {file}\n"));
        }
    }
    out
}

/// What a committer step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// The new tip, and whether the plain fallback commit was used.
    Committed { tip: String, fallback: bool },
    /// Nothing was staged, so no committer run was started.
    NothingStaged,
    /// The run broke a rule that stops the job. The caller parks it.
    Park(String),
}

/// The result of checking the worktree after one committer run.
enum Check {
    Good(String),
    Park(String),
    RunFault(String),
    MessageFault(Vec<String>),
}

impl Check {
    /// The findings text for a retry, when the check failed in a way a retry can fix.
    fn findings(&self) -> Option<String> {
        match self {
            Check::RunFault(reason) => Some(reason.clone()),
            Check::MessageFault(problems) => Some(problems.join("\n")),
            Check::Good(_) | Check::Park(_) => None,
        }
    }
}

/// A short note on why the run itself looks wrong, or an empty string.
fn run_note(run: &AgentRun) -> String {
    if run.stop_reason != StopReason::Done {
        let detail = run.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default();
        return format!("the run ended with {:?}{detail}", run.stop_reason);
    }
    if let Some(reason) = blocked_reason(&run.result_text) {
        return format!("the committer replied BLOCKED: {reason}");
    }
    if run.result_text.contains("Unknown skill") {
        return "the commit skill is not available in the run".to_string();
    }
    String::new()
}

/// The text of the retry run. Without a session to resume it carries the whole prompt.
fn retry_text(prompt: &str, retry: &str, has_session: bool) -> String {
    if has_session {
        retry.to_string()
    } else {
        format!(
            "{prompt}\n\n(The analysis above was computed before the first run. Check git status and git log for the current state.)\n\n{retry}"
        )
    }
}

fn retry_prompt(findings: &str) -> String {
    format!(
        "The previous committer run did not leave the worktree in the expected state.\n\
         Findings:\n{findings}\n\
         Fix this with the commit skill, using the same standing answers. Never rewrite the floor."
    )
}

impl Engine<'_> {
    /// The run spec for one committer run. A batch run sees the whole job diff, so its cap is doubled.
    fn committer_run_spec(&self, wt: &Worktree, req: &CommitRequest<'_>, text: &str) -> AgentSpec {
        let mut spec = committer_spec(&self.job_id, &self.config, &wt.path, text);
        if req.kind == CommitKind::Batch {
            spec.budget_weighted = spec.budget_weighted.map(|b| b.saturating_mul(2));
        }
        spec
    }

    /// Stages the job worktree, runs the committer agent, and checks the result against the
    /// rules in the spec. A failed run gets one retry by resuming the session, then a plain
    /// `git2` fallback commit. Rewriting the floor is returned as `Park`, never retried.
    /// The step does not change the job stage: the caller parks on `Park`.
    pub fn commit_step(&self, wt: &Worktree, req: &CommitRequest<'_>) -> Result<CommitOutcome, EngineError> {
        let staged = self.repo.stage_all(wt).map_err(git_err)?;
        if staged.is_empty() {
            return Ok(CommitOutcome::NothingStaged);
        }
        let tip = self.repo.tip(&wt.branch).map_err(git_err)?;
        let above = self.repo.commits_above(req.floor, &tip).map_err(git_err)?;
        let before = above.clone();
        let context = match self.commit_context_text() {
            Ok(text) => text,
            Err(e) => return self.fallback_commit(wt, req, &format!("commit context unreadable: {e}")),
        };
        let report = self.repo.staged_report(wt, 5).map_err(git_err)?;
        let prompt = committer_prompt(&context, req, &report, &above, self.config.commit_subagents);

        let mut run = self.run_agent(self.committer_run_spec(wt, req, &prompt))?;
        let mut check = self.verify_commit_run(wt, req, &before, &run)?;
        if let Some(findings) = check.findings() {
            let retry = retry_prompt(&findings);
            let text = retry_text(&prompt, &retry, run.session_id.is_some());
            let mut spec = self.committer_run_spec(wt, req, &text);
            spec.resume = run.session_id.clone();
            run = self.run_agent(spec)?;
            check = self.verify_commit_run(wt, req, &before, &run)?;
        }
        match check {
            Check::Good(tip) => Ok(CommitOutcome::Committed { tip, fallback: false }),
            Check::Park(reason) => Ok(CommitOutcome::Park(reason)),
            Check::RunFault(reason) => self.fallback_commit(wt, req, &reason),
            Check::MessageFault(problems) => {
                for problem in &problems {
                    self.ruling(&format!("commit message check failed, {problem}"))?;
                }
                let tip = self.repo.tip(&wt.branch).map_err(git_err)?;
                Ok(CommitOutcome::Committed { tip, fallback: false })
            }
        }
    }

    /// Checks the worktree after a run. Git state decides: a valid commit with a clean tree is
    /// accepted even when the reply says BLOCKED. The driver never trusts the prompt for the floor.
    fn verify_commit_run(
        &self,
        wt: &Worktree,
        req: &CommitRequest<'_>,
        before: &[CommitInfo],
        run: &AgentRun,
    ) -> Result<Check, EngineError> {
        if !self.repo.head_is_on_branch(wt).map_err(git_err)? {
            return Ok(Check::Park("committer moved HEAD off the job branch".to_string()));
        }
        let tip = match self.repo.tip(&wt.branch) {
            Ok(tip) => tip,
            Err(GitError::NotFound(_)) => return Ok(Check::Park("the job branch is gone".to_string())),
            Err(e) => return Err(git_err(e)),
        };
        if !self.repo.is_ancestor(req.floor, &tip).map_err(git_err)? {
            return Ok(Check::Park("committer rewrote history at or below the floor".to_string()));
        }
        let dirty = !self.repo.worktree_is_clean(wt).map_err(git_err)?;
        let now = self.repo.commits_above(req.floor, &tip).map_err(git_err)?;
        let changed: Vec<&CommitInfo> = now.iter().filter(|c| !before.iter().any(|b| b.sha == c.sha)).collect();
        if dirty || changed.is_empty() {
            let mut notes = Vec::new();
            let note = run_note(run);
            if !note.is_empty() {
                notes.push(note);
            }
            if dirty {
                notes.push("changes are still uncommitted in the worktree".to_string());
            }
            if changed.is_empty() {
                notes.push("no commit was made".to_string());
            }
            return Ok(Check::RunFault(notes.join("; ")));
        }
        let mut problems = Vec::new();
        // A rebase rewrites every later commit, so only the net growth counts as new commits.
        let net = now.len().saturating_sub(before.len());
        if net > MAX_COMMITS_PER_RUN {
            problems.push(format!("{net} commits were made, the limit is {MAX_COMMITS_PER_RUN}"));
        }
        // A rewritten commit keeps its message, so only messages not seen before are checked.
        for commit in changed.iter().filter(|c| !before.iter().any(|b| b.message == c.message)) {
            let short = short_sha(&commit.sha);
            if let Err(reason) = validate_message(&commit.message) {
                problems.push(format!("commit {short}: {reason}"));
            }
            if !has_footer(&commit.message, ISSUE_KEY, req.issues) {
                if let Some(line) = issue_footer(ISSUE_KEY, req.issues) {
                    problems.push(format!("commit {short}: missing footer `{line}`"));
                }
            }
        }
        if problems.is_empty() {
            Ok(Check::Good(tip))
        } else {
            Ok(Check::MessageFault(problems))
        }
    }

    /// The fallback of a batch run: one `chore: <title>` commit per snapshot, rebuilt from the snapshot trees.
    fn fallback_batch(&self, wt: &Worktree, req: &CommitRequest<'_>, why: &str) -> Result<CommitOutcome, EngineError> {
        self.repo.reset_hard(wt, req.floor).map_err(git_err)?;
        let mut tip: Option<String> = None;
        for snap in req.snapshots {
            self.repo.restore_tree(wt, &snap.sha).map_err(git_err)?;
            let message = fallback_message(&snap.title, req.issues);
            match self.repo.commit_staged(wt, &message) {
                Ok(sha) => tip = Some(sha),
                // A task whose tree equals the one before it has nothing to commit.
                Err(GitError::Refused(_)) => {}
                Err(e) => return Err(git_err(e)),
            }
        }
        let Some(sha) = tip else {
            return Ok(CommitOutcome::Park(format!("the batch fallback made no commit: {why}")));
        };
        let short = short_sha(&sha);
        self.ruling(&format!("committer failed ({why}), batch fallback commits up to {short}"))?;
        Ok(CommitOutcome::Committed { tip: sha, fallback: true })
    }

    /// The plain `git2` commit used when the committer failed twice: `chore: <task title>`.
    /// Parks when there is nothing left to commit, because then the run lost the changes.
    fn fallback_commit(&self, wt: &Worktree, req: &CommitRequest<'_>, why: &str) -> Result<CommitOutcome, EngineError> {
        if req.kind == CommitKind::Batch && !req.snapshots.is_empty() {
            return self.fallback_batch(wt, req, why);
        }
        let staged = self.repo.stage_all(wt).map_err(git_err)?;
        if staged.is_empty() {
            return Ok(CommitOutcome::Park(format!("the committer left nothing to commit: {why}")));
        }
        let message = fallback_message(req.task_title, req.issues);
        let sha = self.repo.commit_staged(wt, &message).map_err(git_err)?;
        let short = short_sha(&sha);
        self.ruling(&format!("committer failed ({why}), fallback commit {short}"))?;
        Ok(CommitOutcome::Committed { tip: sha, fallback: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request<'a>() -> CommitRequest<'a> {
        CommitRequest {
            kind: CommitKind::Task,
            plan_title: "Demo plan",
            task_title: "Add the thing",
            goal: "add the thing",
            floor: "abc1234def",
            issues: &[],
            snapshots: &[],
        }
    }

    fn report(diff: &str) -> StagedReport {
        StagedReport {
            status: "On branch factory/001-demo\nChanges to be committed:\n\tnew file:   a.txt\n".to_string(),
            stat: " a.txt | 1 +\n".to_string(),
            diff: diff.to_string(),
            log: vec!["1111111 feat(x): earlier".to_string()],
        }
    }

    fn info(sha: &str, subject: &str, files: &[&str]) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            subject: subject.to_string(),
            message: subject.to_string(),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn prompt_carries_the_context_the_run_and_the_precomputed_analysis() {
        let above = vec![info("aaaaaaa1111", "feat(x): first", &["one.txt"]), info("bbbbbbb2222", "feat(x): second", &["two.txt"])];
        let prompt = committer_prompt("CONTEXT TEXT", &request(), &report("+++ b/a.txt\n+hi\n"), &above, false);
        assert!(prompt.starts_with("CONTEXT TEXT"));
        for want in ["Kind: task", "Plan: Demo plan", "Task: Add the thing", "Goal: add the thing", "Floor: abc1234def"] {
            assert!(prompt.contains(want), "missing {want}");
        }
        for want in ["### git status", "new file:   a.txt", "### git diff --staged --stat", "### git diff --staged", "+hi", "### git log --oneline -5", "1111111 feat(x): earlier"] {
            assert!(prompt.contains(want), "missing {want}");
        }
        // Newest first, with the files of each candidate.
        let second = prompt.find("bbbbbbb feat(x): second").expect("second listed");
        let first = prompt.find("aaaaaaa feat(x): first").expect("first listed");
        assert!(second < first);
        assert!(prompt.contains("  two.txt"));
    }

    #[test]
    fn prompt_names_the_issue_footer_or_none() {
        let with = CommitRequest { issues: &[20, 21], ..request() };
        let prompt = committer_prompt("c", &with, &report(""), &[], false);
        assert!(prompt.contains("Issue footer: Relates to: #20, #21"), "{prompt}");
        assert!(prompt.contains("every commit of the run"));
        let none = committer_prompt("c", &request(), &report(""), &[], false);
        assert!(none.contains("Issue footer: none"));
    }

    #[test]
    fn prompt_says_inline_only_when_subagents_are_off() {
        let off = committer_prompt("c", &request(), &report(""), &[], false);
        let on = committer_prompt("c", &request(), &report(""), &[], true);
        assert!(off.contains("inline, no subagents"));
        assert!(!on.contains("inline, no subagents"));
        assert!(on.contains("through the skill's subagents"));
    }

    #[test]
    fn retry_text_carries_the_prompt_only_without_a_session() {
        assert_eq!(retry_text("PROMPT", "RETRY", true), "RETRY");
        let text = retry_text("PROMPT", "RETRY", false);
        assert!(text.starts_with("PROMPT\n\n(The analysis above was computed before the first run."));
        assert!(text.ends_with("\n\nRETRY"));
    }

    #[test]
    fn prompt_lists_no_candidates_when_nothing_is_above_the_floor() {
        let prompt = committer_prompt("c", &request(), &report(""), &[], false);
        assert!(prompt.contains("(none)"));
    }

    #[test]
    fn a_huge_diff_is_cut_at_a_character_boundary() {
        let huge = "é".repeat(40_000);
        assert!(huge.len() > MAX_DIFF_BYTES);
        let prompt = committer_prompt("c", &request(), &report(&huge), &[], false);
        assert!(prompt.contains("diff cut at 60000 bytes"));
        assert!(prompt.len() < huge.len());
    }

    #[test]
    fn spec_is_trimmed_gated_and_capped_per_run() {
        let config = Config {
            commit_model: "opus".to_string(),
            commit_budget_weighted: 12_345,
            stall_minutes: 7,
            ..Config::default()
        };
        let spec = committer_spec("001", &config, Path::new("/wt"), "the prompt");
        assert_eq!(spec.role, "committer");
        assert_eq!(spec.agent.as_deref(), Some("factory-committer"));
        assert_eq!(spec.model.as_deref(), Some("opus"));
        assert_eq!(spec.budget_weighted, Some(12_345));
        assert_eq!(spec.used_before, 0);
        assert_eq!(spec.max_turns, Some(40));
        assert_eq!(spec.stall_after, Some(Duration::from_secs(420)));
        assert_eq!(spec.env, vec![("HARNESS_GATE".to_string(), "0".to_string())]);
        assert!(spec.strict_mcp);
        assert_eq!(spec.tools.as_deref(), Some("Bash,Read,Skill"));
        assert_eq!(spec.prompt, "the prompt");
    }

    fn snap(n: u32, title: &str, files: &[&str]) -> SnapshotRef {
        SnapshotRef { n, sha: format!("{n}{n}{n}{n}{n}{n}{n}"), title: title.to_string(), files: files.iter().map(|f| f.to_string()).collect() }
    }

    #[test]
    fn a_batch_prompt_lists_the_tasks_with_their_files() {
        let snaps = vec![snap(1, "first thing", &["a.txt"]), snap(2, "second thing", &["b.txt", "c.txt"])];
        let req = CommitRequest { kind: CommitKind::Batch, snapshots: &snaps, ..request() };
        let prompt = committer_prompt("CONTEXT", &req, &report("+x\n"), &[], false);
        assert!(prompt.contains("Kind: batch"), "{prompt}");
        assert!(prompt.contains("## Tasks in this change"), "{prompt}");
        assert!(prompt.contains("Make at most 4 commits; group tasks when there are more than 4."), "{prompt}");
        assert!(prompt.contains("Task 1: first thing\n  a.txt\n"), "{prompt}");
        assert!(prompt.contains("Task 2: second thing\n  b.txt\n  c.txt\n"), "{prompt}");
        let tasks = prompt.find("## Tasks in this change").expect("tasks");
        let step1 = prompt.find("## Output of the skill's step 1 commands").expect("step 1");
        assert!(tasks < step1);
    }

    #[test]
    fn a_prompt_without_snapshots_has_no_tasks_section() {
        let prompt = committer_prompt("c", &request(), &report(""), &[], false);
        assert!(!prompt.contains("## Tasks in this change"));
    }

    #[test]
    fn shipped_context_file_has_the_style_and_the_standing_answers() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.claude/factory/commit-context.md");
        let text = std::fs::read_to_string(path).expect("shipped context file");
        for want in [
            "Conventional Commits",
            "lowercase type",
            "## Standing answers",
            "at most 4 commits per run",
            "above the floor and not pushed",
            "GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash",
            "Never push",
            "issue footer",
        ] {
            assert!(text.contains(want), "context file is missing {want}");
        }
    }
}
