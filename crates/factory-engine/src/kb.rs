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
/// The read only git subcommands a KB run may use.
const KB_GIT_SUBS: [&str; 6] = ["log", "diff", "show", "rev-parse", "merge-base", "status"];

/// The allowlist of a KB run. Edit and Write are scoped to the KB folder (`//` marks an absolute
/// path in a permission rule). Git is read only, and `git -C` is tied to the repo root.
pub fn kb_allowed_tools(repo_root: &Path, kb_dir: &Path) -> Vec<String> {
    let mut tools: Vec<String> = ["Read", "Grep", "Glob", "Skill", "Agent"]
        .iter()
        .map(|t| t.to_string())
        .collect();
    let kb = kb_dir.display().to_string();
    let kb = kb.trim_start_matches('/');
    tools.push(format!("Edit(//{kb}/**)"));
    tools.push(format!("Write(//{kb}/**)"));
    for sub in KB_GIT_SUBS {
        tools.push(format!("Bash(git {sub}:*)"));
    }
    for sub in KB_GIT_SUBS {
        tools.push(format!("Bash(git -C {} {sub}:*)", repo_root.display()));
    }
    tools.push("Bash(date:*)".to_string());
    tools.push("Bash(ls:*)".to_string());
    tools
}

/// The prompt for one KB refresh run. `job` is the job id and slug when the run follows a merge.
pub fn kb_prompt(kb_dir: &Path, repo_root: &Path, tip: &str, job: Option<(&str, &str)>) -> String {
    let project = kb_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
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
         The project is `{project}`, already resolved. Use the repo path and the KB path above as given. Step 1 of the skill reads `projects.json` one folder above the KB path. Read it only if you are allowed to. If you cannot, continue with the given paths.\n\
         Run the `kb-refresh` skill for this project. Use the skill's own subagents through the Agent tool, never the Workflow tool.\n\
         Edit only files under the KB path. Change nothing in the repo. Git is read only (diff, log, show, rev-parse, merge-base, status). To run git elsewhere, use `git -C {repo}` with the repo path exactly as given. Never run git commands that write, and do not commit.\n\
         Reply with one changed page path per line and nothing else, or `No relevant changes`.",
        repo = repo_root.display(),
        kb = kb_dir.display(),
    )
}

/// The run spec for the KB refresher. It runs in the main checkout, with the KB folder added.
pub fn kb_spec(
    job_id: &str,
    config: &Config,
    repo_root: &Path,
    kb_dir: &Path,
    prompt: &str,
) -> AgentSpec {
    let mut spec = AgentSpec::new(job_id, "kb", repo_root, prompt);
    spec.agent = Some(KB_AGENT.to_string());
    spec.model = Some(config.kb_model.clone());
    spec.max_turns = Some(KB_MAX_TURNS);
    spec.budget_weighted = Some(config.kb_budget_weighted);
    spec.used_before = 0;
    spec.stall_after = Some(Duration::from_secs(u64::from(config.stall_minutes) * 60));
    spec.strict_mcp = true;
    spec.tools = Some(KB_TOOLS.to_string());
    spec.allowed_tools = kb_allowed_tools(repo_root, kb_dir);
    // Auto mode ignores the allowlist, so dontAsk makes it binding.
    spec.permission_mode = Some("dontAsk".to_string());
    spec.add_dirs = vec![kb_dir.to_path_buf()];
    spec
}

/// The page path on a reply line, or `None` when the line does not look like a page.
fn page_of(line: &str) -> Option<String> {
    let line = line
        .trim_start_matches(['-', '*'])
        .trim()
        .trim_matches('`')
        .trim();
    if line.to_lowercase().starts_with("skipped") {
        return None;
    }
    // A skill-style line reads `Refreshed: <page> - <reason>`.
    let line = match line.split_once(':') {
        Some((head, rest)) if head.eq_ignore_ascii_case("refreshed") => rest.trim(),
        _ => line,
    };
    let line = line
        .split(" \u{2014} ")
        .next()
        .unwrap_or(line)
        .split(" - ")
        .next()
        .unwrap_or(line)
        .trim();
    line.ends_with(".md").then(|| line.to_string())
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
    let lines: Vec<&str> = reply
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // A skill-style reply may carry prose, so keep only the lines that look like a page.
    let pages: Vec<String> = lines.iter().filter_map(|l| page_of(l)).collect();
    if pages.is_empty() {
        return Ok(lines.iter().map(|l| l.to_string()).collect());
    }
    Ok(pages)
}

/// What a KB refresh step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbOutcome {
    /// The pages the agent says it changed.
    Refreshed {
        pages: Vec<String>,
    },
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
            return Ok(KbOutcome::Failed(format!(
                "the KB folder {} does not exist",
                kb_dir.display()
            )));
        }
        let tip = self.repo.tip(self.repo.base_branch()).map_err(git_err)?;
        let before = self.repo.checkout_fingerprint().map_err(git_err)?;
        let prompt = kb_prompt(&kb_dir, self.repo.root(), &tip, job);
        let run = self.run_agent(kb_spec(
            &self.job_id,
            &self.config,
            self.repo.root(),
            &kb_dir,
            &prompt,
        ))?;
        let after = self.repo.checkout_fingerprint().map_err(git_err)?;
        if before != after {
            return Ok(KbOutcome::Tampered(
                "the main checkout changed during the KB refresh".to_string(),
            ));
        }
        if run.stop_reason != StopReason::Done {
            let detail = run
                .error
                .as_deref()
                .map(|e| format!(": {e}"))
                .unwrap_or_default();
            return Ok(KbOutcome::Failed(format!(
                "the run ended with {:?}{detail}",
                run.stop_reason
            )));
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
        for want in [
            "Project: rocketapi",
            "/repo/rocket",
            "/kb/rocketapi",
            "abc1234",
            "Workflow",
            "Agent",
            "No relevant changes",
            "kb-refresh",
        ] {
            assert!(prompt.contains(want), "missing {want}");
        }
        assert!(!prompt.contains("Trigger:"));
    }

    #[test]
    fn prompt_names_the_job_when_given() {
        let prompt = kb_prompt(
            &kb(),
            Path::new("/repo/rocket"),
            "abc1234",
            Some(("007", "demo")),
        );
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
        assert_eq!(
            spec.stall_after,
            Some(Duration::from_secs(u64::from(config.stall_minutes) * 60))
        );
        assert!(spec.strict_mcp);
        assert_eq!(spec.tools.as_deref(), Some(KB_TOOLS));
        assert_eq!(spec.permission_mode.as_deref(), Some("dontAsk"));
        assert_eq!(
            spec.allowed_tools,
            kb_allowed_tools(Path::new("/repo/rocket"), &kb())
        );
        let has = |t: &str| spec.allowed_tools.contains(&t.to_string());
        assert!(has("Bash(git status:*)"));
        assert!(has("Bash(git -C /repo/rocket status:*)"));
        assert!(has("Bash(git -C /repo/rocket merge-base:*)"));
        assert!(!has("Bash(git -C:*)"));
        assert!(has("Edit(//kb/rocketapi/**)"));
        assert!(has("Write(//kb/rocketapi/**)"));
        assert!(!has("Edit") && !has("Write"));
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
        assert_eq!(
            parse_kb_reply(
                "No relevant changes since 2026-10-01 (abc1234) in the KB scope.\nNothing to do."
            ),
            Ok(vec![])
        );
        assert_eq!(parse_kb_reply("\nKB is up to date"), Ok(vec![]));
        assert!(parse_kb_reply("\n\nBLOCKED: no skill").is_err());
        assert_eq!(
            parse_kb_reply(" a.md \n\nb.md\n"),
            Ok(vec!["a.md".to_string(), "b.md".to_string()])
        );
    }

    #[test]
    fn reply_parsing_keeps_only_page_lines() {
        let reply = "Refreshed: a.md \u{2014} reason\nSkipped: b.md";
        assert_eq!(parse_kb_reply(reply), Ok(vec!["a.md".to_string()]));
        assert_eq!(
            parse_kb_reply("- docs/a.md\n* `docs/b.md`\nDone."),
            Ok(vec!["docs/a.md".to_string(), "docs/b.md".to_string()])
        );
        // Without any page-like line, every non-empty line stays.
        assert_eq!(
            parse_kb_reply("one\ntwo"),
            Ok(vec!["one".to_string(), "two".to_string()])
        );
    }
}
