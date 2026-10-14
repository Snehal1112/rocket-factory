//! The committer step: stage a job worktree, run the committer agent, check what it did.

use std::path::Path;
use std::time::Duration;

use factory_agent::AgentSpec;
use factory_core::Config;
use factory_git::{CommitInfo, StagedReport};

/// The agent file the driver passes with `--agent`.
pub const COMMITTER_AGENT: &str = "factory-committer";
pub const COMMITTER_MAX_TURNS: u32 = 40;
/// The tools a committer run may use. It never edits files, so there is no Edit or Write.
pub const COMMITTER_TOOLS: &str = "Bash,Read,Skill";
/// Repo relative path of the prompt text, read from the main checkout like `config.toml`.
pub const COMMIT_CONTEXT_FILE: &str = ".claude/factory/commit-context.md";
/// The staged diff in the prompt is cut at this many bytes.
const MAX_DIFF_BYTES: usize = 60_000;

/// What kind of work the commit is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitKind {
    Task,
    Legacy,
    Fix,
}

impl CommitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CommitKind::Task => "task",
            CommitKind::Legacy => "legacy",
            CommitKind::Fix => "fix",
        }
    }
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
        "Analysis: inline, no subagents. Run the skill's steps 1, 1b and 1c with your own tool calls."
    };
    let (diff, cut) = truncate_utf8(&report.diff, MAX_DIFF_BYTES);
    let cut_note = if cut {
        format!("\n[diff cut at {MAX_DIFF_BYTES} bytes, run `git diff --staged` for the rest]\n")
    } else {
        String::new()
    };
    let mut candidates = String::new();
    if above.is_empty() {
        candidates.push_str("(none)\n");
    }
    for commit in above.iter().rev() {
        let short = commit.sha.get(..7).unwrap_or(&commit.sha);
        candidates.push_str(&format!("{short} {}\n", commit.subject));
        for file in &commit.files {
            candidates.push_str(&format!("  {file}\n"));
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
         {analysis}\n\n\
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
    let mut spec = AgentSpec::new(job_id, "committer", cwd, prompt);
    spec.agent = Some(COMMITTER_AGENT.to_string());
    spec.model = Some(config.commit_model.clone());
    spec.max_turns = Some(COMMITTER_MAX_TURNS);
    spec.budget_weighted = Some(config.commit_budget_weighted);
    spec.used_before = 0;
    spec.stall_after = Some(Duration::from_secs(u64::from(config.stall_minutes) * 60));
    spec.env = vec![("HARNESS_GATE".to_string(), "0".to_string())];
    spec.strict_mcp = true;
    spec.tools = Some(COMMITTER_TOOLS.to_string());
    spec
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
    fn prompt_says_inline_only_when_subagents_are_off() {
        let off = committer_prompt("c", &request(), &report(""), &[], false);
        let on = committer_prompt("c", &request(), &report(""), &[], true);
        assert!(off.contains("inline, no subagents"));
        assert!(!on.contains("inline, no subagents"));
        assert!(on.contains("through the skill's subagents"));
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
        let mut config = Config::default();
        config.commit_model = "opus".to_string();
        config.commit_budget_weighted = 12_345;
        config.stall_minutes = 7;
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

    #[test]
    fn shipped_context_file_has_the_style_and_the_standing_answers() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../.claude/factory/commit-context.md");
        let text = std::fs::read_to_string(path).expect("shipped context file");
        for want in [
            "Conventional Commits",
            "lowercase type",
            "## Standing answers",
            "at most 4 commits per run",
            "above the floor and not pushed",
            "GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash",
            "Never push",
        ] {
            assert!(text.contains(want), "context file is missing {want}");
        }
    }
}
