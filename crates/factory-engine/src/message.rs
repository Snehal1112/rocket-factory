//! The message-only committer mode: the agent drafts the merge or sync message, the driver commits it.

use std::path::Path;
use std::time::Duration;

use factory_agent::AgentSpec;
use factory_core::{extract_message, validate_message, Config, MESSAGE_BEGIN, MESSAGE_END};
use factory_git::CommitInfo;

use crate::{Engine, EngineError, COMMITTER_AGENT, COMMITTER_MAX_TURNS};

/// The tools a message run may use. It reads files and runs the skill, nothing else.
pub const MESSAGE_TOOLS: &str = "Read,Skill";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Merge,
    Sync,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Merge => "merge",
            MessageKind::Sync => "sync",
        }
    }
}

/// What the driver tells the message run.
#[derive(Debug, Clone)]
pub struct MessageRequest<'a> {
    pub kind: MessageKind,
    pub job_id: &'a str,
    pub slug: &'a str,
    pub title: &'a str,
    /// The commit the job started from.
    pub base: &'a str,
    /// The commit that is merged: the reviewed head (merge) or main's tip (sync).
    pub tip: &'a str,
}

/// The message the driver used before the committer existed. It is the fallback.
pub fn fixed_message(kind: MessageKind, job_id: &str, slug: &str) -> String {
    match kind {
        MessageKind::Merge => format!("chore(factory): merge job {job_id} {slug}"),
        MessageKind::Sync => format!("chore(factory): sync main into job {job_id} {slug}"),
    }
}

/// The prompt of one message run. `commits` is the list above `base`, oldest first.
pub fn message_prompt(context: &str, req: &MessageRequest<'_>, commits: &[CommitInfo]) -> String {
    let mut listed = String::new();
    if commits.is_empty() {
        listed.push_str("(none)\n");
    }
    for commit in commits.iter().rev() {
        let short = commit.sha.get(..7).unwrap_or(&commit.sha);
        listed.push_str(&format!("{short} {}\n", commit.subject));
        for file in &commit.files {
            listed.push_str(&format!("  {file}\n"));
        }
    }
    let what = match req.kind {
        MessageKind::Merge => "the merge commit that brings the job branch into main",
        MessageKind::Sync => "the merge commit that brings main into the job branch",
    };
    let list_title = match req.kind {
        MessageKind::Merge => "Commits on the job branch above the base, newest first",
        MessageKind::Sync => "Commits that main gained since the base, newest first",
    };
    format!(
        "{context}\n\n\
         ## This run\n\n\
         Kind: {kind}\n\
         Job: {id} {slug}\n\
         Title: {title}\n\
         Base: {base}\n\
         Tip: {tip}\n\n\
         Write the message for {what}. This is a message-only run: there is no floor, no staged change and no issue footer, the driver adds any footer itself. \
         Do not run git commit, git add or any other git command, and do not change any file. Use the commit skill's style rules, answer its questions with the standing answers, and follow the Conventional Commits style from the context.\n\n\
         ## {list_title}\n\n{listed}\n\
         ## Reply\n\n\
         Reply with the message between {begin} and {end} and no other text.\n\n\
         {begin}\n<subject>\n\n<optional body>\n{end}\n",
        kind = req.kind.as_str(),
        id = req.job_id,
        slug = req.slug,
        title = req.title,
        base = req.base,
        tip = req.tip,
        begin = MESSAGE_BEGIN,
        end = MESSAGE_END,
    )
}

/// The run spec of a message run: read-only tools, a per-run cap, role `message`.
pub fn message_spec(job_id: &str, config: &Config, cwd: &Path, prompt: &str) -> AgentSpec {
    let mut spec = AgentSpec::new(job_id, "message", cwd, prompt);
    spec.agent = Some(COMMITTER_AGENT.to_string());
    spec.model = Some(config.commit_model.clone());
    spec.max_turns = Some(COMMITTER_MAX_TURNS);
    spec.budget_weighted = Some(config.commit_budget_weighted);
    spec.used_before = 0;
    spec.stall_after = Some(Duration::from_secs(u64::from(config.stall_minutes) * 60));
    spec.env = vec![("HARNESS_GATE".to_string(), "0".to_string())];
    spec.strict_mcp = true;
    spec.tools = Some(MESSAGE_TOOLS.to_string());
    spec.allowed_tools = MESSAGE_TOOLS.split(',').map(str::to_string).collect();
    spec
}

impl Engine<'_> {
    /// Asks the committer for a merge or sync message. A missing or invalid message gets one
    /// retry by resuming the session, then the fixed message. This never parks and never fails
    /// on a message problem, only on a board or git error.
    pub fn message_step(&self, cwd: &Path, req: &MessageRequest<'_>) -> Result<String, EngineError> {
        let fixed = fixed_message(req.kind, req.job_id, req.slug);
        let context = match std::fs::read_to_string(self.repo.root().join(crate::COMMIT_CONTEXT_FILE)) {
            Ok(text) => text,
            Err(e) => {
                self.ruling(&format!("{} message: commit context unreadable ({e}), fixed message used", req.kind.as_str()))?;
                return Ok(fixed);
            }
        };
        let commits = self.repo.commits_above(req.base, req.tip).map_err(crate::ports::git_err)?;
        let prompt = message_prompt(&context, req, &commits);

        let run = self.run_agent(message_spec(&self.job_id, &self.config, cwd, &prompt))?;
        let first = pick(&run.result_text);
        let problem = match first {
            Ok(message) => return Ok(message),
            Err(problem) => problem,
        };
        let retry = format!(
            "The previous reply cannot be used: {problem}\nReply again with only the message between {MESSAGE_BEGIN} and {MESSAGE_END}. The subject is type(scope): description, lowercase type, 72 characters or fewer, no trailing period."
        );
        let text = if run.session_id.is_some() { retry } else { format!("{prompt}\n\n{retry}") };
        let mut spec = message_spec(&self.job_id, &self.config, cwd, &text);
        spec.resume = run.session_id.clone();
        let run = self.run_agent(spec)?;
        match pick(&run.result_text) {
            Ok(message) => Ok(message),
            Err(problem) => {
                self.ruling(&format!("{} message failed twice ({problem}), fixed message used", req.kind.as_str()))?;
                Ok(fixed)
            }
        }
    }
}

/// The valid message in a reply, or the reason there is none.
fn pick(reply: &str) -> Result<String, String> {
    let message = extract_message(reply).ok_or_else(|| "no message between the markers".to_string())?;
    validate_message(&message)?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::Config;

    #[test]
    fn fixed_messages_match_the_old_driver_text() {
        assert_eq!(fixed_message(MessageKind::Merge, "001", "hello"), "chore(factory): merge job 001 hello");
        assert_eq!(fixed_message(MessageKind::Sync, "001", "hello"), "chore(factory): sync main into job 001 hello");
    }

    #[test]
    fn prompt_says_no_floor_and_no_commit_and_lists_the_commits() {
        let req = MessageRequest { kind: MessageKind::Merge, job_id: "001", slug: "hello", title: "hello feature", base: "abc1234", tip: "def5678" };
        let commits = vec![CommitInfo { sha: "aaaaaaa1111".into(), subject: "feat(x): one".into(), message: "feat(x): one".into(), files: vec!["a.txt".into()] }];
        let prompt = message_prompt("CONTEXT", &req, &commits);
        assert!(prompt.starts_with("CONTEXT"));
        for want in ["Kind: merge", "Job: 001 hello", "Title: hello feature", "aaaaaaa feat(x): one", "  a.txt", "no floor", "Do not run git commit", "---BEGIN MESSAGE---", "---END MESSAGE---", "Commits on the job branch above the base, newest first"] {
            assert!(prompt.contains(want), "missing {want}");
        }
        let sync = MessageRequest { kind: MessageKind::Sync, ..req };
        assert!(message_prompt("c", &sync, &commits).contains("Commits that main gained since the base, newest first"));
    }

    #[test]
    fn prompt_with_no_commits_says_none() {
        let req = MessageRequest { kind: MessageKind::Sync, job_id: "001", slug: "hello", title: "t", base: "a", tip: "b" };
        assert!(message_prompt("c", &req, &[]).contains("(none)"));
    }

    #[test]
    fn spec_is_read_only_and_capped_per_run() {
        let mut config = Config::default();
        config.commit_model = "opus".into();
        config.commit_budget_weighted = 777;
        let spec = message_spec("001", &config, Path::new("/wt"), "p");
        assert_eq!(spec.role, "message");
        assert_eq!(spec.agent.as_deref(), Some("factory-committer"));
        assert_eq!(spec.model.as_deref(), Some("opus"));
        assert_eq!(spec.budget_weighted, Some(777));
        assert_eq!(spec.used_before, 0);
        assert_eq!(spec.tools.as_deref(), Some("Read,Skill"));
        assert_eq!(spec.allowed_tools, vec!["Read".to_string(), "Skill".to_string()]);
        assert_eq!(spec.env, vec![("HARNESS_GATE".to_string(), "0".to_string())]);
        assert!(spec.strict_mcp);
    }
}
