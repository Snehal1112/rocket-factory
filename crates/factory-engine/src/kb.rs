//! The KB refresh step: run the KB refresher agent against the knowledge base folder and
//! check that it left the main checkout alone.

use std::path::Path;
use std::time::Duration;

use factory_agent::{AgentSpec, StopReason};
use factory_core::{blocked_reason, Config};

use crate::ports::git_err;
use crate::{Engine, EngineError};

/// The agent file the driver passes with `--agent`.
pub const KB_AGENT: &str = "factory-kb-refresher";
pub const KB_MAX_TURNS: u32 = 80;
/// The tools a KB run may use. It edits KB pages, so Edit and Write are on.
pub const KB_TOOLS: &str = "Read,Grep,Glob,Edit,Write,Skill,Agent,Bash";
/// Bash stays read only for git. Everything else is allowed by name.
pub const KB_ALLOWED_TOOLS: [&str; 16] = [
    "Read",
    "Grep",
    "Glob",
    "Edit",
    "Write",
    "Skill",
    "Agent",
    "Bash(git log:*)",
    "Bash(git diff:*)",
    "Bash(git show:*)",
    "Bash(git rev-parse:*)",
    "Bash(git merge-base:*)",
    "Bash(git status:*)",
    "Bash(git -C:*)",
    "Bash(date:*)",
    "Bash(ls:*)",
];

/// The prompt for one KB refresh run. `job` is the job id and slug when the run follows a merge.
pub fn kb_prompt(kb_dir: &Path, repo_root: &Path, tip: &str, job: Option<(&str, &str)>) -> String {
    let project = kb_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let trigger = match job {
        Some((id, slug)) => format!("Trigger: the merge of job {id} ({slug}).\n"),
        None => String::new(),
    };
    format!(
        "Refresh the knowledge base for a project.\n\n\
         Project: {project}\n\
         Repo: {repo}\n\
         KB path: {kb}\n\
         Main tip: {tip}\n\
         {trigger}\n\
         Run the `kb-refresh` skill for this project. Use the skill's own subagents through the Agent tool, never the Workflow tool.\n\
         Edit only files under the KB path. Change nothing in the repo. Git commands are read only (diff, log, show, rev-parse, merge-base, status), even with `-C`. Never run git commands that write, and do not commit.\n\
         Reply with the pages changed, one per line, or `No relevant changes`.",
        repo = repo_root.display(),
        kb = kb_dir.display(),
    )
}

/// The run spec for the KB refresher. It runs in the main checkout, with the KB folder added.
pub fn kb_spec(job_id: &str, config: &Config, repo_root: &Path, kb_dir: &Path, prompt: &str) -> AgentSpec {
    let mut spec = AgentSpec::new(job_id, "kb", repo_root, prompt);
    spec.agent = Some(KB_AGENT.to_string());
    spec.model = Some(config.kb_model.clone());
    spec.max_turns = Some(KB_MAX_TURNS);
    spec.budget_weighted = Some(config.kb_budget_weighted);
    spec.used_before = 0;
    spec.stall_after = Some(Duration::from_secs(u64::from(config.stall_minutes) * 60));
    spec.strict_mcp = true;
    spec.tools = Some(KB_TOOLS.to_string());
    spec.allowed_tools = KB_ALLOWED_TOOLS.iter().map(|t| t.to_string()).collect();
    spec.add_dirs = vec![kb_dir.to_path_buf()];
    spec
}

/// Reads the reply. An empty list means `No relevant changes`, an error is a `BLOCKED:` reply.
pub fn parse_kb_reply(reply: &str) -> Result<Vec<String>, String> {
    let reply = reply.trim();
    if let Some(reason) = blocked_reason(reply) {
        return Err(format!("the KB refresher replied BLOCKED: {reason}"));
    }
    let first = reply.lines().next().unwrap_or("").trim().to_lowercase();
    if first.starts_with("no relevant changes") || first.starts_with("kb is up to date") {
        return Ok(Vec::new());
    }
    Ok(reply.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// What a KB refresh step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbOutcome {
    /// The pages the agent says it changed.
    Refreshed { pages: Vec<String> },
    UpToDate,
    /// The run did not finish. The caller records it and carries on.
    Failed(String),
    /// The main checkout changed during the run. The caller records it and stops later refreshes.
    Tampered(String),
}

impl Engine<'_> {
    /// Runs the KB refresher once. The caller checks `Config::kb_dir` first and writes the rulings.
    /// A changed main checkout fingerprint after the run is `Tampered`, whatever the reply says.
    pub fn kb_refresh_step(&self, job: Option<(&str, &str)>) -> Result<KbOutcome, EngineError> {
        let Some(kb_dir) = self.config.kb_dir() else {
            return Ok(KbOutcome::Failed("kb_path is not set".to_string()));
        };
        if !kb_dir.is_dir() {
            return Ok(KbOutcome::Failed(format!("the KB folder {} does not exist", kb_dir.display())));
        }
        let tip = self.repo.tip(factory_git::MAIN_BRANCH).map_err(git_err)?;
        let before = self.repo.checkout_fingerprint().map_err(git_err)?;
        let prompt = kb_prompt(&kb_dir, self.repo.root(), &tip, job);
        let run = self.run_agent(kb_spec(&self.job_id, &self.config, self.repo.root(), &kb_dir, &prompt))?;
        let after = self.repo.checkout_fingerprint().map_err(git_err)?;
        if before != after {
            return Ok(KbOutcome::Tampered("the main checkout changed during the KB refresh".to_string()));
        }
        if run.stop_reason != StopReason::Done {
            let detail = run.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default();
            return Ok(KbOutcome::Failed(format!("the run ended with {:?}{detail}", run.stop_reason)));
        }
        match parse_kb_reply(&run.result_text) {
            Err(reason) => Ok(KbOutcome::Failed(reason)),
            Ok(pages) if pages.is_empty() => Ok(KbOutcome::UpToDate),
            Ok(pages) => Ok(KbOutcome::Refreshed { pages }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kb() -> PathBuf {
        PathBuf::from("/kb/rocketapi")
    }

    #[test]
    fn prompt_names_the_project_paths_tip_and_rules() {
        let prompt = kb_prompt(&kb(), Path::new("/repo/rocket"), "abc1234", None);
        for want in ["Project: rocketapi", "/repo/rocket", "/kb/rocketapi", "abc1234", "Workflow", "Agent", "No relevant changes", "kb-refresh"] {
            assert!(prompt.contains(want), "missing {want}");
        }
        assert!(!prompt.contains("Trigger:"));
    }

    #[test]
    fn prompt_names_the_job_when_given() {
        let prompt = kb_prompt(&kb(), Path::new("/repo/rocket"), "abc1234", Some(("007", "demo")));
        assert!(prompt.contains("job 007 (demo)"), "{prompt}");
    }

    #[test]
    fn spec_carries_the_fixed_fields() {
        let config = Config::default();
        let spec = kb_spec("001", &config, Path::new("/repo/rocket"), &kb(), "p");
        assert_eq!(spec.role, "kb");
        assert_eq!(spec.agent.as_deref(), Some(KB_AGENT));
        assert_eq!(spec.model.as_deref(), Some(config.kb_model.as_str()));
        assert_eq!(spec.budget_weighted, Some(config.kb_budget_weighted));
        assert_eq!(spec.used_before, 0);
        assert_eq!(spec.max_turns, Some(KB_MAX_TURNS));
        assert_eq!(spec.stall_after, Some(Duration::from_secs(u64::from(config.stall_minutes) * 60)));
        assert!(spec.strict_mcp);
        assert_eq!(spec.tools.as_deref(), Some(KB_TOOLS));
        assert_eq!(spec.allowed_tools.len(), KB_ALLOWED_TOOLS.len());
        assert!(spec.allowed_tools.contains(&"Bash(git status:*)".to_string()));
        assert!(spec.allowed_tools.contains(&"Bash(git -C:*)".to_string()));
        assert!(spec.allowed_tools.contains(&"Bash(date:*)".to_string()));
        assert_eq!(spec.add_dirs, vec![kb()]);
        assert_eq!(spec.cwd, PathBuf::from("/repo/rocket"));
        assert!(spec.env.is_empty());
    }

    #[test]
    fn reply_parsing() {
        assert_eq!(parse_kb_reply("No relevant changes"), Ok(vec![]));
        assert_eq!(parse_kb_reply("  no relevant changes \n"), Ok(vec![]));
        assert!(parse_kb_reply("BLOCKED: x").unwrap_err().contains("x"));
        assert_eq!(parse_kb_reply("No relevant changes since 2026-10-01 (abc1234) in the KB scope.\nNothing to do."), Ok(vec![]));
        assert_eq!(parse_kb_reply("\nKB is up to date"), Ok(vec![]));
        assert!(parse_kb_reply("\n\nBLOCKED: no skill").is_err());
        assert_eq!(parse_kb_reply(" a.md \n\nb.md\n"), Ok(vec!["a.md".to_string(), "b.md".to_string()]));
    }
}
