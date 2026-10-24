use std::path::{Path, PathBuf};
use std::time::Duration;

use factory_agent::AgentSpec;

use crate::ports::JobView;
use crate::tasks::{plan_header, spec_excerpt, task_section, PlanTask};
use crate::Engine;

/// The agent file the driver passes with `--agent`.
const BUILDER_AGENT: &str = "factory-builder";
/// The reviewer agent file the driver passes with `--agent`. It sets the read-only tools and the checks.
const REVIEWER_AGENT: &str = "factory-code-reviewer";
/// The turn cap of a builder run. The builder agent files carry the same number, and a test pins it.
pub const BUILDER_MAX_TURNS: u32 = 120;

/// The prompt of a resumed task builder after a crash. `{n}` is the task number.
pub(crate) const INTERRUPTED_PROMPT: &str = "You were interrupted. Run git status and git diff, check what is already done, continue task {n}, and do not redo finished work.";

/// The interrupted prompt with the task number filled in.
pub(crate) fn interrupted_prompt(n: u32) -> String {
    INTERRUPTED_PROMPT.replace("{n}", &n.to_string())
}
/// The turn cap of a reviewer run. The reviewer agent files carry the same number, and a test pins it.
pub const REVIEWER_MAX_TURNS: u32 = 40;
/// The tools a builder run may use. They are every tool the builder agent files tell it to use.
pub const BUILDER_TOOLS: &str = "Bash,Read,Edit,Write,Grep,Glob,Skill,Agent";

/// Keeps a builder that loads subagent-driven-development from acting as its controller and reviewing per task.
const NOT_CONTROLLER: &str =
    "You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review.";

/// The rules version this engine knows. Agent files carry `<!-- factory-rules: N -->` with the same number.
pub const RULES_VERSION: u32 = 1;

#[cfg(test)]
mod marker_tests {
    use super::rules_marker;

    #[test]
    fn rules_marker_reads_only_exact_lines() {
        assert_eq!(
            rules_marker("---\n---\n<!-- factory-rules: 3 -->\n"),
            Some(3)
        );
        assert_eq!(rules_marker("x <!-- factory-rules: 3 -->"), None);
        assert_eq!(rules_marker("<!-- factory-rules: x -->"), None);
        assert_eq!(rules_marker(""), None);
    }
}

/// The number of the first line that is exactly `<!-- factory-rules: N -->`.
pub(crate) fn rules_marker(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let n = line
            .strip_prefix("<!-- factory-rules: ")?
            .strip_suffix(" -->")?;
        n.parse::<u32>().ok()
    })
}

impl Engine<'_> {
    /// True when the worktree's agent file carries a rules marker at or above `RULES_VERSION`.
    fn agent_rules_current(&self, cwd: &Path, agent: &str) -> bool {
        let path = cwd
            .join(factory_core::agent_files_rel_dir())
            .join(format!("{agent}.md"));
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| rules_marker(&text))
            .is_some_and(|n| n >= RULES_VERSION)
    }

    /// True when the prompt for `agent` may drop the rules its file carries. A fallback is noted once per job and agent.
    pub(crate) fn slim_prompt(&self, cwd: &Path, agent: &str) -> bool {
        if self.agent_rules_current(cwd, agent) {
            return true;
        }
        let note = format!(
            "agent file {agent}.md has no current rules marker, prompts keep the full rules"
        );
        let seen = std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default();
        if !seen.contains(&note) {
            let _ = self.ruling(&note);
        }
        false
    }

    /// The prompt line that points an agent at the knowledge base. Empty when the KB is off.
    fn kb_line(&self) -> String {
        match self.config.kb_dir() {
            Some(dir) => {
                let dir = dir.display();
                format!(
                    "Knowledge base: {dir}. Read {dir}/README.md first, then only the topic pages that apply to your task. \
                     It is read-only context, never write to it. Source code and the spec win on any conflict, and say in your reply when a page is stale. \
                     If the folder does not exist, skip this.\n"
                )
            }
            None => String::new(),
        }
    }

    /// The KB folder as an extra readable directory. It sits outside the job worktree.
    fn kb_dirs(&self) -> Vec<std::path::PathBuf> {
        self.config.kb_dir().into_iter().collect()
    }

    /// The "Read ..." phrase for the builder prompt. It names the `read_first` entries and the plan.
    /// With a task excerpt the plan is only located, because the excerpt replaces reading it.
    fn read_line(&self, with_excerpt: bool) -> String {
        let plan = self.plan_file.display();
        match (with_excerpt, self.config.read_first.is_empty()) {
            (false, true) => format!("Read the plan at {plan}."),
            (false, false) => format!(
                "Read {}, and the plan at {plan}.",
                self.config.read_first.join(", ")
            ),
            (true, true) => format!("The plan is at {plan}."),
            (true, false) => format!(
                "Read {}. The plan is at {plan}.",
                self.config.read_first.join(", ")
            ),
        }
    }

    /// The "Rules:" sentence of the builder prompt. Empty when no rule is set.
    fn rules_line(&self) -> String {
        if self.config.builder_rules.is_empty() {
            String::new()
        } else {
            format!("Rules: {}\n", self.config.builder_rules.join(" "))
        }
    }

    /// The prompt line that names the job's spec, or nothing when the job has none.
    fn spec_line(&self) -> String {
        let spec_file = self.job_dir.join("spec.md");
        if spec_file.exists() {
            format!("The spec is at {}.\n", spec_file.display())
        } else {
            String::new()
        }
    }

    /// Sets the model, the budget and the stall limit from the config. Every agent spec goes through this.
    pub(crate) fn with_limits(&self, mut spec: AgentSpec, model: &str) -> AgentSpec {
        spec.model = Some(model.to_string());
        spec.budget_weighted = Some(self.config.budget_weighted);
        spec.used_before = self.job_weighted_tokens();
        spec.stall_after = Some(Duration::from_secs(
            u64::from(self.config.stall_minutes) * 60,
        ));
        spec
    }

    /// Gives a builder spec its permission mode, allowlist, deny rules and the job folder as an extra directory.
    /// The spec stays as it is when `builder_permissions` is off, or when the job folder path holds a glob character.
    pub(crate) fn with_builder_permissions(&self, mut spec: AgentSpec) -> AgentSpec {
        if !self.config.builder_permissions {
            return spec;
        }
        let job_dir = self
            .job_dir
            .canonicalize()
            .unwrap_or_else(|_| self.job_dir.clone());
        let read_only: Vec<PathBuf> = self
            .kb_dirs()
            .into_iter()
            .map(|d| d.canonicalize().unwrap_or(d))
            .collect();
        let guarded = ['*', '?', '[', '{', '(', ')', ',', '\\'];
        if std::iter::once(&job_dir)
            .chain(read_only.iter())
            .any(|d| d.to_string_lossy().contains(guarded))
        {
            return spec;
        }
        spec.permission_mode = Some("acceptEdits".to_string());
        spec.allowed_tools = crate::permissions::builder_allowed_tools(&self.config);
        spec.disallowed_tools = crate::permissions::builder_denied_tools(&job_dir, &read_only);
        spec.add_dirs.push(job_dir);
        spec
    }

    /// The headless builder for the first run of a job.
    pub(crate) fn builder_first_spec(&self, job: &JobView, cwd: &Path) -> AgentSpec {
        let spec_line = self.spec_line();
        let prompt = if self.slim_prompt(cwd, BUILDER_AGENT) {
            format!(
                "You are the builder in a software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
                 {read} Never edit the plan.\n{spec_line}\
                 {rules}{kb}",
                branch = job.branch,
                read = self.read_line(false),
                rules = self.rules_line(),
                kb = self.kb_line(),
            )
        } else {
            format!(
            "You are the builder in a software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
             {read} Never edit the plan.\n{spec_line}\
             Follow the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md), task by task. {NOT_CONTROLLER} \
             Write the failing test first, then the code, then run the checks.\n\
             {rules}\
             Do not run `git add` or `git commit`. Leave your changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. \
             Never push, never merge, never touch the board or the ledger.\n\
             {kb}Reply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line, then at most five lines of concerns or the blocker, then one Ruling: <what> — <why> — <cost if wrong> line per judgment call. Do not commit and do not write a report file. If you are blocked, stop and reply Status: BLOCKED with the reason.",
            branch = job.branch,
            read = self.read_line(false),
            rules = self.rules_line(),
            kb = self.kb_line(),
            )
        };
        let mut spec = AgentSpec::new(&self.job_id, "builder", self.repo.workdir_in(cwd), &prompt);
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.strict_mcp = true;
        spec.tools = Some(BUILDER_TOOLS.to_string());
        spec.add_dirs = self.kb_dirs();
        let spec = self.with_builder_permissions(spec);
        self.with_limits(spec, &self.config.builder_model)
    }

    /// The task excerpt for a builder prompt, or an empty string when the switch is off or the plan has no
    /// `### Task N:` section for the task. It replaces reading `plan.md` and `spec.md` in full.
    /// A fallback with the switch on is noted in the job's events.
    fn task_excerpt(&self, task: &PlanTask) -> String {
        if !self.config.builder_excerpt {
            return String::new();
        }
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let Some(section) = task_section(&plan, task.n) else {
            self.notice(&format!("no task excerpt for task {}: the plan has no `### Task {}:` section, the builder gets the full-file prompt", task.n, task.n));
            return String::new();
        };
        let header = plan_header(&plan);
        let spec = std::fs::read_to_string(self.job_dir.join("spec.md")).unwrap_or_default();
        let spec_part = spec_excerpt(&spec);
        let mut out = String::from(
            "Task excerpt: the plan header, your task, and the spec goal and acceptance criteria follow. \
             They replace reading the plan and the spec in full. Open those files only for something this excerpt refers to outside itself. \
             If a skill such as superpowers:subagent-driven-development tells you to read or load the plan file, use this excerpt instead and do not read the plan file in full.\n\
             --- plan header ---\n",
        );
        out.push_str(&header);
        out.push_str("\n--- your task ---\n");
        out.push_str(&section);
        if !spec_part.is_empty() {
            out.push_str("\n--- spec excerpt ---\n");
            out.push_str(&spec_part);
        }
        out.push_str("\n--- end of excerpt ---\n");
        out
    }

    /// Writes a driver note to the job's events. Never fails the caller.
    fn notice(&self, text: &str) {
        self.events.emit(&factory_agent::AgentEvent {
            kind: factory_agent::EventKind::Notice,
            agent_id: format!("driver-{}", self.job_id),
            agent_type: "driver".to_string(),
            tool_name: None,
            tool_target: None,
            cwd: None,
            last_message: Some(text.to_string()),
            tokens: self.job_weighted_tokens(),
        });
    }

    /// The headless builder for one plan task. It starts fresh and sees only the earlier commit subjects.
    pub(crate) fn builder_task_spec(
        &self,
        job: &JobView,
        cwd: &Path,
        task: &PlanTask,
        base: &str,
        tip: &str,
        earlier: &[String],
    ) -> AgentSpec {
        let spec_line = self.spec_line();
        let excerpt = self.task_excerpt(task);
        let done = if earlier.is_empty() {
            "No earlier task has a commit yet.".to_string()
        } else {
            format!(
                "Commits of earlier tasks, oldest first:\n{}",
                earlier.join("\n")
            )
        };
        let prompt = if self.slim_prompt(cwd, BUILDER_AGENT) {
            format!(
                "You are the builder in a software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
                 {read} Never edit the plan.\n{spec_line}{excerpt}\
                 Base commit: {base}. Current tip: {tip}.\n{done}\n\
                 Do only Task {n}: {title}. Earlier tasks are already committed, do not redo them. Later tasks belong to other agents.\n\
                 {rules}\
                 Never sleep or poll, never run a command in the background and wait for it; if a command needs more than ten minutes say so and stop.\n\
                 {kb}",
                branch = job.branch,
                read = self.read_line(!excerpt.is_empty()),
                rules = self.rules_line(),
                kb = self.kb_line(),
                n = task.n,
                title = task.title,
            )
        } else {
            format!(
            "You are the builder in a software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
             {read} Never edit the plan.\n{spec_line}{excerpt}\
             Base commit: {base}. Current tip: {tip}.\n{done}\n\
             Do only Task {n}: {title}. Earlier tasks are already committed, do not redo them. Later tasks belong to other agents.\n\
             Follow the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md) for this task. {NOT_CONTROLLER} \
             Write the failing test first, then the code, then run the checks the task names.\n\
             {rules}\
             Do not run `git add` or `git commit`. Leave this task's changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. \
             Never push, never merge, never touch the board or the ledger.\n\
             Never sleep or poll, never run a command in the background and wait for it; if a command needs more than ten minutes say so and stop.\n\
             {kb}Reply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line, then at most five lines of concerns or the blocker, then one Ruling: <what> — <why> — <cost if wrong> line per judgment call. Do not commit and do not write a report file. If you are blocked, stop and reply Status: BLOCKED with the reason.",
            branch = job.branch,
            read = self.read_line(!excerpt.is_empty()),
            rules = self.rules_line(),
            kb = self.kb_line(),
            n = task.n,
            title = task.title,
            )
        };
        let mut spec = AgentSpec::new(&self.job_id, "builder", self.repo.workdir_in(cwd), &prompt);
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.strict_mcp = true;
        spec.tools = Some(BUILDER_TOOLS.to_string());
        spec.add_dirs = self.kb_dirs();
        let spec = self.with_builder_permissions(spec);
        self.with_limits(spec, &self.config.builder_model)
    }

    /// The resumed builder of a task whose driver died while it ran.
    pub(crate) fn builder_task_resume_spec(
        &self,
        cwd: &Path,
        task: &PlanTask,
        session: &str,
    ) -> AgentSpec {
        let mut spec = AgentSpec::new(
            &self.job_id,
            "builder",
            self.repo.workdir_in(cwd),
            &interrupted_prompt(task.n),
        );
        spec.resume = Some(session.to_string());
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.strict_mcp = true;
        spec.tools = Some(BUILDER_TOOLS.to_string());
        spec.add_dirs = self.kb_dirs();
        let spec = self.with_builder_permissions(spec);
        self.with_limits(spec, &self.config.builder_model)
    }

    /// The builder of a task that continues the session of the previous task.
    pub(crate) fn builder_task_carry_spec(
        &self,
        cwd: &Path,
        task: &PlanTask,
        session: &str,
        base: &str,
        tip: &str,
    ) -> AgentSpec {
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let your_task = match task_section(&plan, task.n) {
            Some(section) => format!("--- your task ---\n{section}"),
            None => format!("Your task is in the plan at {}.", self.plan_file.display()),
        };
        let prompt = format!(
            "Task {prev} is done and committed as {sha}. Base {base7}, current tip {tip7}. \
             The driver now gives you Task {n} in this same session; this replaces the earlier line that later tasks belong to other agents. \
             Do only Task {n}: {title}. Do not redo or rework earlier tasks.\n{your_task}\n",
            prev = task.n.saturating_sub(1),
            sha = crate::ports::short_sha(tip),
            base7 = crate::ports::short_sha(base),
            tip7 = crate::ports::short_sha(tip),
            n = task.n,
            title = task.title,
        );
        let mut spec = AgentSpec::new(&self.job_id, "builder", self.repo.workdir_in(cwd), &prompt);
        spec.resume = Some(session.to_string());
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.strict_mcp = true;
        spec.tools = Some(BUILDER_TOOLS.to_string());
        spec.add_dirs = self.kb_dirs();
        let spec = self.with_builder_permissions(spec);
        self.with_limits(spec, &self.config.builder_model)
    }

    /// The resumed builder for a fix round.
    pub(crate) fn builder_resume_spec(
        &self,
        job: &JobView,
        cwd: &Path,
        findings: &Path,
        session: &str,
    ) -> AgentSpec {
        let hints = if self.config.check_hints.is_empty() {
            String::new()
        } else {
            let list: Vec<String> = self
                .config
                .check_hints
                .iter()
                .map(|h| format!("`{h}`"))
                .collect();
            format!(" (for example {})", list.join(", "))
        };
        let prompt = if self.slim_prompt(cwd, BUILDER_AGENT) {
            format!(
                "{kb}Fix only the problems listed in {findings}. You are on branch `{branch}`. \
                 Follow `superpowers:receiving-code-review` for the findings. \
                 Read all the findings first, then verify each against the code. \
                 Work one item at a time. If a finding is wrong, push back with a reason in your reply and do not change the code for it. \
                 If an item is unclear, reply with `Status: NEEDS_CONTEXT` and the question instead of guessing. \
                 Run the relevant checks{hints} before you finish.",
                findings = findings.display(),
                branch = job.branch,
                hints = hints,
                kb = self.kb_line(),
            )
        } else {
            format!(
            "{kb}Fix only the problems listed in {findings}. You are on branch `{branch}`. \
             Do not run `git add` or `git commit`: leave the fixes in the worktree and the driver commits them. \
             Do not amend or rewrite earlier commits. \
             Follow `superpowers:receiving-code-review` for the findings, and the implementer rules of `superpowers:subagent-driven-development`. {NOT_CONTROLLER} \
             Read all the findings first, then verify each against the code. \
             Work one item at a time. If a finding is wrong, push back with a reason in your reply and do not change the code for it. \
             If an item is unclear, reply with `Status: NEEDS_CONTEXT` and the question instead of guessing. \
             Run the relevant checks{hints} before you finish. \
             Never push, never merge.",
            findings = findings.display(),
            branch = job.branch,
            hints = hints,
            kb = self.kb_line(),
            )
        };
        let mut spec = AgentSpec::new(&self.job_id, "builder", self.repo.workdir_in(cwd), &prompt);
        spec.resume = Some(session.to_string());
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.strict_mcp = true;
        spec.tools = Some(BUILDER_TOOLS.to_string());
        spec.add_dirs = self.kb_dirs();
        let spec = self.with_builder_permissions(spec);
        self.with_limits(spec, &self.config.builder_model)
    }
}

/// What the reviewer gets besides the diff: the plan's Review Focus, the builder's rulings and concerns,
/// and the plan header with its task headings.
#[derive(Debug, Clone, Default)]
pub struct ReviewerExtra {
    pub focus: Option<String>,
    pub notes: Vec<String>,
    pub plan_outline: String,
    /// True when a code diff adds no test path and no test marker.
    pub no_tests: bool,
}

impl Engine<'_> {
    /// The combined reviewer. It is read-only and pages the diff to stay cheap. The agent file carries the checks.
    pub fn reviewer_spec(
        &self,
        cwd: &Path,
        diff: &Path,
        names: &Path,
        round: u32,
        rules: &[String],
        extra: &ReviewerExtra,
    ) -> AgentSpec {
        self.reviewer_spec_with_model(
            cwd,
            diff,
            names,
            round,
            rules,
            extra,
            &self.config.reviewer_model,
        )
    }

    /// The model for a review: the large one when it is set and the diff is risky or over the line threshold.
    pub(crate) fn reviewer_model_for(&self, changed_lines: usize, risky: bool) -> &str {
        match &self.config.reviewer_model_large {
            Some(large)
                if risky || changed_lines > self.config.reviewer_large_over_lines as usize =>
            {
                large
            }
            _ => &self.config.reviewer_model,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reviewer_spec_with_model(
        &self,
        cwd: &Path,
        diff: &Path,
        names: &Path,
        round: u32,
        rules: &[String],
        extra: &ReviewerExtra,
        model: &str,
    ) -> AgentSpec {
        let spec_line = self.spec_line();
        // A later round also gets the findings of the round before, so the reviewer can check they were fixed.
        let earlier = self
            .job_dir
            .join(format!("review-code-r{}.md", round.saturating_sub(1)));
        let earlier_line = if round > 1 && earlier.exists() {
            format!(
                "The findings of round {} are at {}. Check that each was fixed.\n",
                round - 1,
                earlier.display()
            )
        } else {
            String::new()
        };
        // Round 2 and later review only the fix, so the reviewer does not reopen settled ground.
        // It needs earlier findings to judge, so a round after a checks-only fix is a normal review.
        let rereview = if round > 1 && earlier.exists() {
            "This is a scoped re-review. Give a verdict for each earlier finding: fixed, not fixed, or wrong. \
             A finding that is not fixed keeps its original `[blocker]` or `[minor]` tag. \
             Inspect the fix diff only. Report anything you notice outside it under `Out-of-scope observations`, without changing the verdict.\n"
        } else {
            ""
        };
        // The builder's reply to the findings, when it wrote one, holds its push-back reasons.
        let builder_reply = self.job_dir.join(format!("builder-r{round}.md"));
        let reply_line = if rereview.is_empty() || !builder_reply.exists() {
            String::new()
        } else {
            format!("The builder's reply to those findings is at {}. It may explain why a finding was not changed.\n", builder_reply.display())
        };
        let rules_line = if rules.is_empty() {
            String::new()
        } else {
            format!("{}\n", rules.join(" "))
        };
        let mut context = String::new();
        if let Some(focus) = &extra.focus {
            context.push_str(&format!("Review focus from the plan:\n{focus}\n"));
        }
        if !extra.notes.is_empty() {
            context.push_str(&format!(
                "Builder notes, rulings and concerns recorded while building:\n{}\n",
                extra.notes.join("\n")
            ));
        }
        if !extra.plan_outline.is_empty() {
            context.push_str(&format!(
                "Plan outline (header and task headings):\n{}\n",
                extra.plan_outline
            ));
        }
        if extra.no_tests {
            context.push_str("No test was added in this job. Judge whether the new behavior is covered, and treat missing tests as a blocker.\n");
        }
        let prompt = if self.slim_prompt(cwd, REVIEWER_AGENT) {
            // The agent file carries the paging, check, grading and verdict rules.
            format!(
                "Review this round's diff for the job.\n\
                 Inputs: the diff at {diff}, the list of changed files at {names}, the plan at {plan} (read it only to look up one step), and the worktree at {cwd}. \
                 Round {round}.\n{spec_line}{earlier_line}{reply_line}{rereview}{context}\
                 {rules_line}{kb}",
                diff = diff.display(),
                names = names.display(),
                plan = self.plan_file.display(),
                rules_line = rules_line,
                cwd = cwd.display(),
                kb = self.kb_line(),
            )
        } else {
            format!(
                "Review this round's diff for the job.\n\
                 Inputs: the diff at {diff}, the list of changed files at {names}, the plan at {plan} (read it only to look up one step), and the worktree at {cwd}. \
                 Round {round}.\n{spec_line}{earlier_line}{reply_line}{rereview}{context}\
                 Read the diff in pages: use the Read tool with offset and limit 1500 lines per call. Do not read the whole file at once. \
                 {rules_line}\
                 Check: the diff does what the plan asks and nothing else; correctness and edge cases; tests that would fail without the change; no debug leftovers. \
                 The spec and plan are a vision document: where they are silent, what a reasonable person would expect counts. \
                 You are read-only and you do not dispatch subagents.\n\
                 {kb}Reply in at most 60 lines. List findings as a numbered list with file:line, severity [blocker] or [minor], and a concrete fix. \
                 Grade each finding: Critical and Important findings carry [blocker], Minor findings carry [minor]. \
                 Before the verdict list anything you could not judge under `Declined to judge`, with the reason. \
                 Any blocker means CHANGES. Your last line, alone, must be exactly `VERDICT: APPROVE` or `VERDICT: CHANGES`.",
                diff = diff.display(),
                names = names.display(),
                plan = self.plan_file.display(),
                rules_line = rules_line,
                cwd = cwd.display(),
                kb = self.kb_line(),
            )
        };
        let mut spec = AgentSpec::new(&self.job_id, "reviewer", self.repo.workdir_in(cwd), &prompt);
        spec.agent = Some(REVIEWER_AGENT.to_string());
        spec.max_turns = Some(REVIEWER_MAX_TURNS);
        spec.strict_mcp = true;
        // The reviewer has no Skill tool, so it needs no skill list.
        spec.disable_slash_commands = true;
        spec.allowed_tools = vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()];
        // The tool list itself is limited too, so the reviewer cannot even see a write tool.
        spec.tools = Some("Read,Grep,Glob".to_string());
        spec.add_dirs = self.kb_dirs();
        self.with_limits(spec, model)
    }
}
