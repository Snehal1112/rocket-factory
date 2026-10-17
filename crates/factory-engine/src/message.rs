//! The message-only committer mode: the agent drafts the merge or sync message, the driver commits it.

use std::path::Path;

use factory_agent::AgentSpec;
use factory_core::{append_footers, extract_message, issue_footer, validate_message, Config, MESSAGE_BEGIN, MESSAGE_END};
use factory_git::CommitInfo;

use crate::commit::{committer_base_spec, render_commit_list};
use crate::{Engine, EngineError};

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
    /// The GitHub issues of the job, for the footer the driver adds.
    pub issues: &'a [u32],
    /// True when the job's spec lists follow-ups, so the merge footer says `Relates to` and does not close the ticket.
    pub partial: bool,
}

/// True when `spec.md` in the job folder has at least one `## Follow-ups` bullet. A missing file is false.
pub(crate) fn spec_has_follow_ups(job_dir: &Path) -> bool {
    std::fs::read_to_string(job_dir.join("spec.md")).map(|spec| !crate::follow_ups(&spec).is_empty()).unwrap_or(false)
}

/// The message the driver used before the committer existed. It is the fallback.
pub fn fixed_message(kind: MessageKind, job_id: &str, slug: &str, base: &str) -> String {
    match kind {
        MessageKind::Merge => format!("chore(factory): merge job {job_id} {slug}"),
        MessageKind::Sync => format!("chore(factory): sync {base} into job {job_id} {slug}"),
    }
}

/// The footer lines the driver adds: the issue line (`Fixes` for a merge, `Relates to` for a sync or a merge whose spec has follow-ups)
/// and then `Factory-Job`.
pub fn driver_footers(req: &MessageRequest<'_>) -> Vec<String> {
    let key = match req.kind {
        MessageKind::Merge if req.partial => "Relates to",
        MessageKind::Merge => "Fixes",
        MessageKind::Sync => "Relates to",
    };
    let mut lines: Vec<String> = issue_footer(key, req.issues).into_iter().collect();
    lines.push(format!("Factory-Job: {}", req.job_id));
    lines
}

/// The prompt of one message run. `commits` is the list above `base`, oldest first.
pub fn message_prompt(context: &str, req: &MessageRequest<'_>, commits: &[CommitInfo], base_branch: &str) -> String {
    let listed = render_commit_list(commits);
    let what = match req.kind {
        MessageKind::Merge => format!("the merge commit that brings the job branch into {base_branch}"),
        MessageKind::Sync => format!("the merge commit that brings {base_branch} into the job branch"),
    };
    let list_title = match req.kind {
        MessageKind::Merge => "Commits on the job branch above the base, newest first".to_string(),
        MessageKind::Sync => format!("Commits that {base_branch} gained since the base, newest first"),
    };
    let footers = driver_footers(req).join(", ");
    format!(
        "{context}\n\n\
         ## This run\n\n\
         Kind: {kind}\n\
         Job: {id} {slug}\n\
         Title: {title}\n\
         Base: {base}\n\
         Tip: {tip}\n\n\
         Write the message for {what}. This is a message-only run: there is no floor, no staged change and no issue footer. The driver appends these footer lines after your message: {footers}. Leave them out of your reply. \
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
        footers = footers,
        begin = MESSAGE_BEGIN,
        end = MESSAGE_END,
    )
}

/// The run spec of a message run: read-only tools, a per-run cap, role `message`.
pub fn message_spec(job_id: &str, config: &Config, cwd: &Path, prompt: &str) -> AgentSpec {
    let mut spec = committer_base_spec(job_id, "message", config, cwd, prompt, MESSAGE_TOOLS);
    spec.allowed_tools = MESSAGE_TOOLS.split(',').map(str::to_string).collect();
    spec
}

impl Engine<'_> {
    /// Asks the committer for a merge or sync message. A missing or invalid message gets one
    /// retry by resuming the session, then the fixed message. This never parks and never fails
    /// on a message problem, only on a board or git error.
    pub fn message_step(&self, cwd: &Path, req: &MessageRequest<'_>) -> Result<String, EngineError> {
        let footers = driver_footers(req);
        let fixed = append_footers(&fixed_message(req.kind, req.job_id, req.slug, self.repo.base_branch()), &footers);
        let context = match self.commit_context_text() {
            Ok(text) => text,
            Err(e) => {
                self.ruling(&format!("{} message: commit context unreadable ({e}), fixed message used", req.kind.as_str()))?;
                return Ok(fixed);
            }
        };
        let commits = self.repo.commits_above(req.base, req.tip).map_err(crate::ports::git_err)?;
        let prompt = message_prompt(&context, req, &commits, self.repo.base_branch());

        let run = self.run_agent(message_spec(&self.job_id, &self.config, cwd, &prompt))?;
        let first = pick(&run.result_text);
        let problem = match first {
            Ok(message) => return Ok(append_footers(&message, &footers)),
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
            Ok(message) => Ok(append_footers(&message, &footers)),
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
    fn merge_footer_relates_to_when_the_spec_has_follow_ups() {
        let req = MessageRequest { kind: MessageKind::Merge, job_id: "001", slug: "s", title: "t", base: "a", tip: "b", issues: &[76], partial: true };
        assert_eq!(driver_footers(&req), vec!["Relates to: #76".to_string(), "Factory-Job: 001".to_string()]);
        let full = MessageRequest { partial: false, ..req };
        assert_eq!(driver_footers(&full), vec!["Fixes: #76".to_string(), "Factory-Job: 001".to_string()]);
    }

    #[test]
    fn spec_has_follow_ups_reads_spec_md_of_the_job_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!spec_has_follow_ups(dir.path()), "no spec.md means no follow-ups");
        std::fs::write(dir.path().join("spec.md"), "## Goal\nx\n").expect("write");
        assert!(!spec_has_follow_ups(dir.path()));
        std::fs::write(dir.path().join("spec.md"), "## Follow-ups\n- One\n").expect("write");
        assert!(spec_has_follow_ups(dir.path()));
    }

    #[test]
    fn fixed_messages_match_the_old_driver_text() {
        assert_eq!(fixed_message(MessageKind::Merge, "001", "hello", "main"), "chore(factory): merge job 001 hello");
        assert_eq!(fixed_message(MessageKind::Sync, "001", "hello", "main"), "chore(factory): sync main into job 001 hello");
    }

    #[test]
    fn prompt_says_no_floor_and_no_commit_and_lists_the_commits() {
        let req = MessageRequest { kind: MessageKind::Merge, job_id: "001", slug: "hello", title: "hello feature", base: "abc1234", tip: "def5678", issues: &[], partial: false };
        let commits = vec![CommitInfo { sha: "aaaaaaa1111".into(), subject: "feat(x): one".into(), message: "feat(x): one".into(), files: vec!["a.txt".into()] }];
        let prompt = message_prompt("CONTEXT", &req, &commits, "main");
        assert!(prompt.starts_with("CONTEXT"));
        for want in ["Kind: merge", "Job: 001 hello", "Title: hello feature", "aaaaaaa feat(x): one", "  a.txt", "no floor", "Do not run git commit", "---BEGIN MESSAGE---", "---END MESSAGE---", "Commits on the job branch above the base, newest first"] {
            assert!(prompt.contains(want), "missing {want}");
        }
        let sync = MessageRequest { kind: MessageKind::Sync, ..req };
        assert!(message_prompt("c", &sync, &commits, "main").contains("Commits that main gained since the base, newest first"));
    }

    #[test]
    fn prompt_names_the_footer_lines_the_driver_adds() {
        let req = MessageRequest { kind: MessageKind::Merge, job_id: "001", slug: "hello", title: "t", base: "a", tip: "b", issues: &[20], partial: false };
        let prompt = message_prompt("c", &req, &[], "main");
        assert!(prompt.contains("Fixes: #20, Factory-Job: 001"), "{prompt}");
        assert!(!prompt.contains("the driver adds any footer itself"));
        let sync = MessageRequest { kind: MessageKind::Sync, issues: &[], ..req };
        assert!(message_prompt("c", &sync, &[], "main").contains("footer lines after your message: Factory-Job: 001."));
    }

    #[test]
    fn prompt_with_no_commits_says_none() {
        let req = MessageRequest { kind: MessageKind::Sync, job_id: "001", slug: "hello", title: "t", base: "a", tip: "b", issues: &[], partial: false };
        assert!(message_prompt("c", &req, &[], "main").contains("(none)"));
    }

    #[test]
    fn spec_is_read_only_and_capped_per_run() {
        let config = Config { commit_model: "opus".into(), commit_budget_weighted: 777, ..Config::default() };
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
