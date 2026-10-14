use std::path::Path;
use std::time::Duration;

use factory_agent::AgentSpec;

use crate::ports::JobView;
use crate::tasks::PlanTask;
use crate::Engine;

/// The agent file the driver passes with `--agent`.
const BUILDER_AGENT: &str = "factory-builder";
const BUILDER_MAX_TURNS: u32 = 120;

impl Engine<'_> {
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

    /// Sets the model, the budget and the stall limit from the config. Every agent spec goes through this.
    pub(crate) fn with_limits(&self, mut spec: AgentSpec, model: &str) -> AgentSpec {
        spec.model = Some(model.to_string());
        spec.budget_weighted = Some(self.config.budget_weighted);
        spec.used_before = self.job_weighted_tokens();
        spec.stall_after = Some(Duration::from_secs(u64::from(self.config.stall_minutes) * 60));
        spec
    }

    /// The headless builder for the first run of a job.
    pub(crate) fn builder_first_spec(&self, job: &JobView, cwd: &Path) -> AgentSpec {
        let prompt = format!(
            "You are the builder in Rocket's software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
             Read CLAUDE.md, the rule files for the areas you touch under .claude/rules/, and the plan at {plan}. Never edit the plan.\n\
             Do the plan task by task: write the failing test first, then the code, then run the checks.\n\
             Rules: run `cargo check -j4` and `cargo test -j4 -p <crate> <test_name>`; never use --workspace or --all; never set CARGO_TARGET_DIR; \
             run `yarn install --frozen-lockfile` once before the first yarn check; no unwrap in production Rust; no git CLI in Rust code; \
             shadcn/ui, lucide-react, SingleLineEditor and Monaco rules apply.\n\
             Do not run `git add` or `git commit`. Leave your changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. \
             Never push, never merge, never touch the board or the ledger.\n\
             {kb}If you are blocked, say why in your final reply and stop.",
            branch = job.branch,
            plan = self.plan_file.display(),
            kb = self.kb_line(),
        );
        let mut spec = AgentSpec::new(&self.job_id, "builder", cwd, &prompt);
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.add_dirs = self.kb_dirs();
        self.with_limits(spec, &self.config.builder_model)
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
        let spec_file = self.job_dir.join("spec.md");
        let spec_line =
            if spec_file.exists() { format!("The spec is at {}.\n", spec_file.display()) } else { String::new() };
        let done = if earlier.is_empty() {
            "No earlier task has a commit yet.".to_string()
        } else {
            format!("Commits of earlier tasks, oldest first:\n{}", earlier.join("\n"))
        };
        let prompt = format!(
            "You are the builder in Rocket's software factory. Your working directory is the job worktree, already on branch `{branch}`.\n\
             The plan is at {plan}. Never edit the plan.\n{spec_line}\
             Base commit: {base}. Current tip: {tip}.\n{done}\n\
             Do only Task {n}: {title}. Earlier tasks are already committed, do not redo them. Later tasks belong to other agents.\n\
             Write the failing test first, then the code, then run the checks the task names.\n\
             Do not run `git add` or `git commit`. Leave this task's changes in the worktree: the driver commits them for you after you finish. Skip every commit step of the plan. \
             Never push, never merge, never touch the board or the ledger.\n\
             Never sleep or poll, never run a command in the background and wait for it; if a command needs more than ten minutes say so and stop.\n\
             {kb}If you are blocked, say why in your final reply and stop. Reply with one line for Task {n}.",
            branch = job.branch,
            plan = self.plan_file.display(),
            kb = self.kb_line(),
            n = task.n,
            title = task.title,
        );
        let mut spec = AgentSpec::new(&self.job_id, "builder", cwd, &prompt);
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.add_dirs = self.kb_dirs();
        self.with_limits(spec, &self.config.builder_model)
    }

    /// The resumed builder for a fix round.
    pub(crate) fn builder_resume_spec(&self, job: &JobView, cwd: &Path, findings: &Path, session: &str) -> AgentSpec {
        let prompt = format!(
            "{kb}Fix only the problems listed in {findings}. You are on branch `{branch}`. \
             Do not run `git add` or `git commit`: leave the fixes in the worktree and the driver commits them. \
             Do not amend or rewrite earlier commits. \
             Run the relevant checks (`cargo check -j4`, targeted `cargo test -j4 -p <crate> <test_name>`, yarn checks) before you finish. \
             Never push, never merge.",
            findings = findings.display(),
            branch = job.branch,
            kb = self.kb_line(),
        );
        let mut spec = AgentSpec::new(&self.job_id, "builder", cwd, &prompt);
        spec.resume = Some(session.to_string());
        spec.agent = Some(BUILDER_AGENT.to_string());
        spec.max_turns = Some(BUILDER_MAX_TURNS);
        spec.add_dirs = self.kb_dirs();
        self.with_limits(spec, &self.config.builder_model)
    }
}

impl Engine<'_> {
    /// The combined reviewer. It is read-only and pages the diff to stay cheap.
    pub(crate) fn reviewer_spec(&self, cwd: &Path, diff: &Path, names: &Path, round: u32) -> AgentSpec {
        let prompt = format!(
            "You are the code reviewer in Rocket's software factory. You are read-only.\n\
             Inputs: the diff at {diff}, the list of changed files at {names}, the plan at {plan}, and the worktree at {cwd}. \
             Round {round}.\n\
             Read the diff in pages: use the Read tool with offset and limit 1500 lines per call. Do not read the whole file at once. \
             Read .claude/rules/rust-ddd-boundaries.md, .claude/rules/tauri-ipc-boundaries.md and .claude/rules/frontend-component-guardrails.md only for the areas the diff touches.\n\
             Check: the diff does what the plan asks and nothing else; correctness and edge cases; no unwrap in production Rust; \
             DDD placement; camelCase serde renames on IPC DTOs only; tests that would fail without the change; \
             hard rules (shadcn/ui, lucide-react, SingleLineEditor, Monaco, narrow Zustand selectors, no git CLI in Rust); no debug leftovers.\n\
             {kb}Reply in at most 60 lines. List findings as a numbered list with file:line, severity [blocker] or [minor], and a concrete fix. \
             Any blocker means CHANGES. Your last line, alone, must be exactly `VERDICT: APPROVE` or `VERDICT: CHANGES`.",
            diff = diff.display(),
            names = names.display(),
            plan = self.plan_file.display(),
            cwd = cwd.display(),
            kb = self.kb_line(),
        );
        let mut spec = AgentSpec::new(&self.job_id, "reviewer", cwd, &prompt);
        spec.allowed_tools = vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()];
        spec.add_dirs = self.kb_dirs();
        self.with_limits(spec, &self.config.reviewer_model)
    }
}
