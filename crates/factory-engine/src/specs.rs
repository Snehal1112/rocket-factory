use std::path::Path;
use std::time::Duration;

use factory_agent::AgentSpec;

use crate::ports::JobView;
use crate::Engine;

impl Engine<'_> {
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
             Commit each task with the dev-workflow-skills:1-git-commit skill and a pathspec commit (`git add <paths> && git commit --only -m \"...\" -- <paths>`). \
             Never use `git add -A`, `--all` or `.`. Never push, never merge, never touch the board or the ledger.\n\
             If you are blocked, say why in your final reply and stop. Your last line must be `HEAD: <sha of your last commit>`.",
            branch = job.branch,
            plan = self.plan_file.display(),
        );
        self.with_limits(AgentSpec::new(&self.job_id, "builder", cwd, &prompt), &self.config.builder_model)
    }

    /// The resumed builder for a fix round.
    pub(crate) fn builder_resume_spec(&self, job: &JobView, cwd: &Path, findings: &Path, session: &str) -> AgentSpec {
        let prompt = format!(
            "Fix only the problems listed in {findings}. Make new commits on branch `{branch}`. Do not amend or rewrite earlier commits. \
             Run the relevant checks (`cargo check -j4`, targeted `cargo test -j4 -p <crate> <test_name>`, yarn checks) before each commit. \
             Never push, never merge. Your last line must be `HEAD: <sha of your last commit>`.",
            findings = findings.display(),
            branch = job.branch,
        );
        let mut spec = AgentSpec::new(&self.job_id, "builder", cwd, &prompt);
        spec.resume = Some(session.to_string());
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
             Reply in at most 60 lines. List findings as a numbered list with file:line, severity [blocker] or [minor], and a concrete fix. \
             Any blocker means CHANGES. Your last line, alone, must be exactly `VERDICT: APPROVE` or `VERDICT: CHANGES`.",
            diff = diff.display(),
            names = names.display(),
            plan = self.plan_file.display(),
            cwd = cwd.display(),
        );
        let mut spec = AgentSpec::new(&self.job_id, "reviewer", cwd, &prompt);
        spec.allowed_tools = vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()];
        self.with_limits(spec, &self.config.reviewer_model)
    }
}
