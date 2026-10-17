//! Rules for commit messages written by the committer agent, checked by the driver.

/// Longest subject the driver accepts. The commit skill asks for 50, and the driver
/// allows slack so a 51 character subject does not fail a run.
pub const MAX_SUBJECT_CHARS: usize = 72;

/// Most commits one committer run may make.
pub const MAX_COMMITS_PER_RUN: usize = 4;

const TYPES: &[&str] = &["feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert"];

/// Checks a full commit message: a Conventional Commits subject of at most 72 characters
/// with no trailing period, and a blank line before any body. The error is a short reason.
pub fn validate_message(message: &str) -> Result<(), String> {
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or("");
    if subject.trim().is_empty() {
        return Err("the subject is empty".to_string());
    }
    let chars = subject.chars().count();
    if chars > MAX_SUBJECT_CHARS {
        return Err(format!("the subject is {chars} characters, the limit is {MAX_SUBJECT_CHARS}"));
    }
    if subject.ends_with(char::is_whitespace) {
        return Err("the subject ends with whitespace".to_string());
    }
    if subject.ends_with('.') {
        return Err("the subject ends with a period".to_string());
    }
    let (head, description) =
        subject.split_once(": ").ok_or_else(|| "the subject must look like `type(scope): description`".to_string())?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => {
            let scope = rest.strip_suffix(')').ok_or_else(|| "the scope has no closing parenthesis".to_string())?;
            (kind, Some(scope))
        }
        None => (head, None),
    };
    if !TYPES.contains(&kind) {
        return Err(format!("`{kind}` is not a lowercase Conventional Commits type"));
    }
    if let Some(scope) = scope {
        if scope.is_empty() || scope.chars().any(char::is_whitespace) {
            return Err("the scope must be one word".to_string());
        }
    }
    if description.trim().is_empty() {
        return Err("the description is empty".to_string());
    }
    if let Some(second) = lines.next() {
        if !second.trim().is_empty() {
            return Err("a blank line must follow the subject".to_string());
        }
    }
    Ok(())
}

/// One issue reference found in text. `repo` is `owner/repo` when the text was a GitHub issue URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IssueRef {
    pub number: u32,
    pub repo: Option<String>,
}

const GITHUB_PREFIX: &str = "https://github.com/";

/// True for a GitHub owner or repository name: letters, digits, `-`, `_` and `.`.
fn is_repo_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// The `#N` references. The `#` must start the text or follow a character that is not alphanumeric
/// or `_`, and N must be at least 1 and fit `u32`.
fn hash_refs(text: &str) -> Vec<IssueRef> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '#' {
            continue;
        }
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            continue;
        }
        let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).collect();
        if let Ok(number) = digits.parse::<u32>() {
            if number >= 1 {
                found.push(IssueRef { number, repo: None });
            }
        }
    }
    found
}

/// The `https://github.com/<owner>/<repo>/issues/N` references.
fn url_refs(text: &str) -> Vec<IssueRef> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(GITHUB_PREFIX) {
        let tail = &rest[at + GITHUB_PREFIX.len()..];
        let mut parts = tail.splitn(4, '/');
        let (owner, repo, kind, after) = (parts.next(), parts.next(), parts.next(), parts.next());
        if let (Some(owner), Some(repo), Some("issues"), Some(after)) = (owner, repo, kind, after) {
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(number) = digits.parse::<u32>() {
                if number >= 1 && is_repo_name(owner) && is_repo_name(repo) {
                    found.push(IssueRef { number, repo: Some(format!("{owner}/{repo}")) });
                }
            }
        }
        rest = tail;
    }
    found
}

/// Every issue reference in the text, `#N` and GitHub issue URLs, sorted and de-duplicated.
pub fn parse_issue_refs(text: &str) -> Vec<IssueRef> {
    let mut refs = hash_refs(text);
    refs.extend(url_refs(text));
    refs.sort();
    refs.dedup();
    refs
}

/// Every issue number in the text, sorted and de-duplicated. `#N` and GitHub issue URLs both count.
pub fn parse_issues(text: &str) -> Vec<u32> {
    let mut numbers: Vec<u32> = parse_issue_refs(text).into_iter().map(|r| r.number).collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// The footer line `<key>: #20, #21`, or `None` when there are no issues.
pub fn issue_footer(key: &str, issues: &[u32]) -> Option<String> {
    if issues.is_empty() {
        return None;
    }
    let list: Vec<String> = issues.iter().map(|n| format!("#{n}")).collect();
    Some(format!("{key}: {}", list.join(", ")))
}

/// True when there are no issues, or a line of the message equals the footer line exactly.
pub fn has_footer(message: &str, key: &str, issues: &[u32]) -> bool {
    match issue_footer(key, issues) {
        None => true,
        Some(line) => message.lines().any(|l| l == line),
    }
}

/// True when the line looks like `Key: value` with a key of letters, digits and dashes.
fn is_footer_line(line: &str) -> bool {
    match line.split_once(": ") {
        Some((key, value)) => {
            !key.is_empty()
                && !value.trim().is_empty()
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
                && !key.starts_with(' ')
                && !key.ends_with(' ')
        }
        None => false,
    }
}

/// Appends footer lines after the body. Lines the message already has are skipped. The lines join an
/// existing footer paragraph, or start a new one after a blank line.
pub fn append_footers(message: &str, lines: &[String]) -> String {
    let base = message.trim_end();
    let new: Vec<&String> = lines.iter().filter(|l| !base.lines().any(|m| m == l.as_str())).collect();
    if new.is_empty() {
        return base.to_string();
    }
    let mut out = base.to_string();
    let last_paragraph: Vec<&str> = base.rsplit("\n\n").next().unwrap_or("").lines().collect();
    let has_paragraph = base.contains("\n\n");
    let in_footer = has_paragraph && !last_paragraph.is_empty() && last_paragraph.iter().all(|l| is_footer_line(l));
    out.push_str(if in_footer { "\n" } else { "\n\n" });
    let joined: Vec<&str> = new.iter().map(|l| l.as_str()).collect();
    out.push_str(&joined.join("\n"));
    out
}

/// The subject of the plain fallback commit: `chore: <task title>`, with whitespace folded,
/// a trailing period removed and the whole subject cut to 72 characters.
pub fn fallback_subject(task_title: &str) -> String {
    let folded = task_title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = folded.trim_end_matches('.').trim();
    let title = if title.is_empty() { "commit factory changes" } else { title };
    let prefix = "chore: ";
    let room = MAX_SUBJECT_CHARS - prefix.chars().count();
    let cut: String = title.chars().take(room).collect();
    let cut = cut.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    let cut = if cut.is_empty() { "commit factory changes" } else { cut };
    format!("{prefix}{cut}")
}

/// The reason after `BLOCKED:` when the first line of a committer reply starts with it.
pub fn blocked_reason(reply: &str) -> Option<String> {
    let first = reply.lines().next()?.trim();
    first.strip_prefix("BLOCKED:").map(|reason| reason.trim().to_string())
}

/// What a builder reply says about how the task went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderStatus {
    Done,
    DoneWithConcerns(String),
    Blocked(String),
    NeedsContext(String),
}

/// The lines of the reply outside code fences.
fn unfenced_lines(reply: &str) -> impl Iterator<Item = &str> {
    let mut in_fence = false;
    reply.lines().filter(move |line| {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            return false;
        }
        !in_fence
    })
}

/// The value after a `Status:` key, or `None` when the line is not a status line.
/// Asterisks and backticks around the key and the value are ignored, so `**Status**: X`,
/// `**Status: X**` and ``Status: `X` `` all count. A leading `- ` is allowed.
fn status_value(line: &str) -> Option<&str> {
    const MARKS: [char; 2] = ['*', '`'];
    let t = line.trim();
    let t = t.strip_prefix("- ").unwrap_or(t).trim_start();
    let t = t.trim_start_matches(MARKS).strip_prefix("Status")?;
    let t = t.trim_start_matches(MARKS).strip_prefix(':')?;
    Some(t.trim_start_matches(|c: char| MARKS.contains(&c) || c.is_whitespace()))
}

/// The status the builder reported, or `None` when it reported none or an unknown one.
/// A first line `BLOCKED:` wins. Else the first `Status:` line outside code fences with a known
/// value counts (see `status_value` for the accepted forms); a line with an unknown value is skipped.
/// The text is the rest of that line and the following unfenced lines up to the first `Ruling:` line.
pub fn parse_builder_status(reply: &str) -> Option<BuilderStatus> {
    if let Some(reason) = blocked_reason(reply) {
        return Some(BuilderStatus::Blocked(reason));
    }
    let lines: Vec<&str> = unfenced_lines(reply).collect();
    for (at, line) in lines.iter().enumerate() {
        let Some(value) = status_value(line) else {
            continue;
        };
        let word_end = value.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(value.len());
        let (word, tail) = value.split_at(word_end);
        let make: fn(String) -> BuilderStatus = match word.to_ascii_uppercase().as_str() {
            "DONE" => |_| BuilderStatus::Done,
            "DONE_WITH_CONCERNS" => BuilderStatus::DoneWithConcerns,
            "BLOCKED" => BuilderStatus::Blocked,
            "NEEDS_CONTEXT" => BuilderStatus::NeedsContext,
            _ => continue,
        };
        let mut text = tail.trim_start_matches(|c: char| c == '*' || c == '`' || c == ':' || c == '-' || c.is_whitespace()).to_string();
        for rest in lines[at + 1..].iter().take_while(|l| !l.trim().starts_with("Ruling:")) {
            text.push('\n');
            text.push_str(rest);
        }
        return Some(make(text.trim().to_string()));
    }
    None
}

/// Like `parse_builder_status`, with `Done` when the reply has no usable status line.
pub fn builder_status(reply: &str) -> BuilderStatus {
    parse_builder_status(reply).unwrap_or(BuilderStatus::Done)
}

/// The bodies of the `Ruling:` lines outside code fences, trimmed, empty bodies dropped.
pub fn builder_rulings(reply: &str) -> Vec<String> {
    unfenced_lines(reply)
        .filter_map(|line| line.trim().strip_prefix("Ruling:"))
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .collect()
}

pub const MESSAGE_BEGIN: &str = "---BEGIN MESSAGE---";
pub const MESSAGE_END: &str = "---END MESSAGE---";

/// The text between the first begin marker and the first end marker after it, trimmed.
/// `None` when a marker is missing or the text is empty.
pub fn extract_message(reply: &str) -> Option<String> {
    let start = reply.find(MESSAGE_BEGIN)? + MESSAGE_BEGIN.len();
    let rest = &reply[start..];
    let end = rest.find(MESSAGE_END)?;
    let text = rest[..end].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_scoped_and_breaking_subjects() {
        assert!(validate_message("feat: add a thing").is_ok());
        assert!(validate_message("fix(infra): stop the leak").is_ok());
        assert!(validate_message("feat(x)!: drop the old flag").is_ok());
        assert!(validate_message("chore: tidy\n\nBody paragraph.\nMore body.").is_ok());
    }

    #[test]
    fn subject_length_limit_is_72_characters() {
        let ok = format!("feat: {}", "a".repeat(72 - 6));
        assert_eq!(ok.chars().count(), 72);
        assert!(validate_message(&ok).is_ok());
        let long = format!("feat: {}", "a".repeat(72 - 5));
        assert_eq!(long.chars().count(), 73);
        assert!(validate_message(&long).is_err());
    }

    #[test]
    fn rejects_bad_subjects() {
        for bad in [
            "",
            "Added a thing",
            "feat add a thing",
            "feat:add a thing",
            "Feat: add a thing",
            "wip: add a thing",
            "feat(): add a thing",
            "feat(two words): add a thing",
            "feat(x: add a thing",
            "feat: ",
            "feat: add a thing.",
        ] {
            assert!(validate_message(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn a_body_needs_a_blank_line_after_the_subject() {
        assert!(validate_message("feat: add a thing\nbody right away").is_err());
        assert!(validate_message("feat: add a thing\n\nbody after a blank line").is_ok());
    }

    #[test]
    fn counts_characters_and_not_bytes() {
        let subject = format!("feat: {}", "é".repeat(66));
        assert_eq!(subject.chars().count(), 72);
        assert!(validate_message(&subject).is_ok());
    }

    #[test]
    fn fallback_subject_is_a_valid_chore_subject() {
        assert_eq!(fallback_subject("Add the thing."), "chore: Add the thing");
        assert_eq!(fallback_subject("  Add   the\nthing  "), "chore: Add the thing");
        assert_eq!(fallback_subject(""), "chore: commit factory changes");
        let long = fallback_subject(&"word ".repeat(40));
        assert!(long.chars().count() <= MAX_SUBJECT_CHARS);
        assert!(validate_message(&long).is_ok());
        let multibyte = fallback_subject(&"é".repeat(100));
        assert!(multibyte.chars().count() <= MAX_SUBJECT_CHARS);
        assert!(validate_message(&multibyte).is_ok());
    }

    #[test]
    fn fallback_subject_is_valid_for_dots_and_blank_titles() {
        assert_eq!(fallback_subject(". ."), "chore: commit factory changes");
        assert_eq!(fallback_subject("abc ."), "chore: abc");
        assert_eq!(fallback_subject("   "), "chore: commit factory changes");
        for title in [". .", "abc .", "   "] {
            assert!(validate_message(&fallback_subject(title)).is_ok());
        }
    }

    #[test]
    fn rejects_a_subject_ending_with_whitespace() {
        assert!(validate_message("feat: x ").is_err());
    }

    #[test]
    fn blocked_reason_reads_only_the_first_line() {
        assert_eq!(blocked_reason("BLOCKED: no skill\nmore"), Some("no skill".to_string()));
        assert_eq!(blocked_reason("  BLOCKED:   spaced  "), Some("spaced".to_string()));
        assert_eq!(blocked_reason("abc1234 feat: x\nBLOCKED: later line"), None);
        assert_eq!(blocked_reason(""), None);
    }

    #[test]
    fn builder_status_reads_a_first_line_blocked() {
        assert_eq!(builder_status("BLOCKED: no schema\nmore"), BuilderStatus::Blocked("no schema".to_string()));
    }

    #[test]
    fn builder_status_reads_a_status_line_in_a_longer_reply() {
        let reply = "I did the work.\n\n- **Status:** NEEDS_CONTEXT the API is unclear\nWhich endpoint?";
        assert_eq!(
            builder_status(reply),
            BuilderStatus::NeedsContext("the API is unclear\nWhich endpoint?".to_string())
        );
        assert_eq!(builder_status("ok\nStatus: DONE_WITH_CONCERNS\nslow"), BuilderStatus::DoneWithConcerns("slow".to_string()));
        assert_eq!(builder_status("ok\nStatus: BLOCKED: no db"), BuilderStatus::Blocked("no db".to_string()));
        assert_eq!(builder_status("Status: DONE"), BuilderStatus::Done);
    }

    #[test]
    fn builder_status_ignores_blocked_inside_a_fence() {
        let reply = "Done.\n```\nBLOCKED: quoted\nStatus: BLOCKED\n```\nThe text above is a sample.\nBLOCKED: mid text";
        assert_eq!(builder_status(reply), BuilderStatus::Done);
        assert_eq!(parse_builder_status(reply), None);
    }

    #[test]
    fn builder_status_defaults_to_done_without_a_status_line() {
        assert_eq!(builder_status(""), BuilderStatus::Done);
        assert_eq!(builder_status("all good"), BuilderStatus::Done);
        assert_eq!(parse_builder_status("all good"), None);
        assert_eq!(builder_status("Status: MAYBE"), BuilderStatus::Done);
        assert_eq!(parse_builder_status("Status: MAYBE"), None);
    }

    #[test]
    fn builder_status_accepts_marked_up_forms() {
        for reply in [
            "Status: **BLOCKED**",
            "Status: `BLOCKED`",
            "**Status**: BLOCKED",
            "**Status: BLOCKED**",
            "- **Status:** BLOCKED",
            "`Status: BLOCKED`",
        ] {
            assert_eq!(parse_builder_status(reply), Some(BuilderStatus::Blocked(String::new())), "{reply}");
        }
        assert_eq!(
            parse_builder_status("ok\n**Status**: `NEEDS_CONTEXT` which table?"),
            Some(BuilderStatus::NeedsContext("which table?".to_string()))
        );
        assert_eq!(parse_builder_status("**Status: DONE_WITH_CONCERNS**\nslow"), Some(BuilderStatus::DoneWithConcerns("slow".to_string())));
    }

    #[test]
    fn builder_status_skips_an_unknown_value_and_keeps_searching() {
        let reply = "Status: tests green\nwork done\nStatus: BLOCKED: no db";
        assert_eq!(parse_builder_status(reply), Some(BuilderStatus::Blocked("no db".to_string())));
        assert_eq!(parse_builder_status("Status: tests green\nStatus: maybe"), None);
    }

    #[test]
    fn concern_text_stops_at_the_first_ruling_line_and_skips_fences() {
        let reply = "Status: DONE_WITH_CONCERNS\nThe cap is tight.\n```\nquoted\n```\nIt may be slow.\n\
                     Ruling: kept the name \u{2014} the plan says so \u{2014} a rename later\nRuling: second \u{2014} why \u{2014} cost";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::DoneWithConcerns("The cap is tight.\nIt may be slow.".to_string()))
        );
        assert_eq!(builder_rulings(reply).len(), 2);
    }

    #[test]
    fn a_blocked_reason_stops_at_the_first_ruling_line() {
        let reply = "Status: BLOCKED\nThe schema file is gone.\nRuling: stopped \u{2014} nothing to build on \u{2014} a retry";
        assert_eq!(parse_builder_status(reply), Some(BuilderStatus::Blocked("The schema file is gone.".to_string())));
        assert_eq!(builder_rulings(reply), vec!["stopped \u{2014} nothing to build on \u{2014} a retry".to_string()]);
        let reply = "Status: NEEDS_CONTEXT which table?\nRuling: a \u{2014} b \u{2014} c";
        assert_eq!(parse_builder_status(reply), Some(BuilderStatus::NeedsContext("which table?".to_string())));
    }

    #[test]
    fn builder_rulings_strips_prefix_and_drops_empty_bodies() {
        let reply = "x\nRuling: kept the old name\n  Ruling:   \nRuling:\n```\nRuling: fenced\n```\nRuling: second one  ";
        assert_eq!(builder_rulings(reply), vec!["kept the old name".to_string(), "second one".to_string()]);
    }

    #[test]
    fn extract_message_takes_the_first_marker_pair() {
        let reply = "noise\n---BEGIN MESSAGE---\nfeat(x): add y\n\nbody line\n---END MESSAGE---\n---BEGIN MESSAGE---\nother\n---END MESSAGE---";
        assert_eq!(extract_message(reply).as_deref(), Some("feat(x): add y\n\nbody line"));
    }

    #[test]
    fn extract_message_needs_both_markers_and_text() {
        assert_eq!(extract_message("feat(x): add y"), None);
        assert_eq!(extract_message("---BEGIN MESSAGE---\nfeat(x): y"), None);
        assert_eq!(extract_message("---BEGIN MESSAGE---\n  \n---END MESSAGE---"), None);
    }

    #[test]
    fn refs_read_hash_numbers_and_urls() {
        let refs = parse_issue_refs("fix #20 and https://github.com/Snehal1112/rocket/issues/76 now");
        assert_eq!(
            refs,
            vec![
                IssueRef { number: 20, repo: None },
                IssueRef { number: 76, repo: Some("Snehal1112/rocket".to_string()) },
            ]
        );
    }

    #[test]
    fn url_followed_by_punctuation() {
        let refs = parse_issue_refs("see (https://github.com/o/r/issues/76).");
        assert_eq!(refs, vec![IssueRef { number: 76, repo: Some("o/r".to_string()) }]);
        let refs = parse_issue_refs("https://github.com/o/r/issues/76.");
        assert_eq!(refs, vec![IssueRef { number: 76, repo: Some("o/r".to_string()) }]);
    }

    #[test]
    fn url_with_fragment_counts_once() {
        let refs = parse_issue_refs("https://github.com/o/r/issues/76#issuecomment-5");
        assert_eq!(refs, vec![IssueRef { number: 76, repo: Some("o/r".to_string()) }]);
    }

    #[test]
    fn pull_request_url_is_not_an_issue() {
        assert!(parse_issue_refs("https://github.com/o/r/pull/9").is_empty());
        assert!(parse_issue_refs("https://example.com/o/r/issues/9").is_empty());
    }

    #[test]
    fn refs_are_sorted_and_deduplicated() {
        let refs = parse_issue_refs("#9 #3 #9 https://github.com/o/r/issues/3");
        assert_eq!(
            refs,
            vec![
                IssueRef { number: 3, repo: None },
                IssueRef { number: 3, repo: Some("o/r".to_string()) },
                IssueRef { number: 9, repo: None },
            ]
        );
    }

    #[test]
    fn parse_issues_keeps_hash_behavior_and_adds_url_numbers() {
        assert_eq!(parse_issues("fix #20, #21 and #20"), vec![20, 21]);
        assert_eq!(parse_issues("no issues here"), Vec::<u32>::new());
        assert_eq!(parse_issues("abc#5 and #0 and #x"), Vec::<u32>::new());
        assert_eq!(parse_issues("https://github.com/o/r/issues/76 and #5"), vec![5, 76]);
    }

    #[test]
    fn parse_issues_reads_hash_numbers_only() {
        assert_eq!(parse_issues("GitHub issue #20 (https://github.com/o/r/issues/20)"), vec![20]);
        assert_eq!(parse_issues("fix #21, #20 and #21"), vec![20, 21]);
        assert_eq!(parse_issues("C# and a#1 and #abc and #0 and #99999999999"), Vec::<u32>::new());
        assert_eq!(parse_issues("(#7)"), vec![7]);
        assert_eq!(parse_issues("#3\n#4"), vec![3, 4]);
    }

    #[test]
    fn footers_are_built_and_checked() {
        assert_eq!(issue_footer("Relates to", &[]), None);
        assert_eq!(issue_footer("Fixes", &[20, 21]).as_deref(), Some("Fixes: #20, #21"));
        assert!(has_footer("feat: x", "Relates to", &[]));
        assert!(has_footer("feat: x\n\nRelates to: #20", "Relates to", &[20]));
        assert!(!has_footer("feat: x\n\nRelates to: #20", "Relates to", &[20, 21]));
        assert!(!has_footer("feat: x", "Relates to", &[20]));
    }

    #[test]
    fn append_footers_adds_each_line_once() {
        let lines = vec!["Fixes: #20".to_string(), "Factory-Job: 001".to_string()];
        assert_eq!(append_footers("feat: x\n", &lines), "feat: x\n\nFixes: #20\nFactory-Job: 001");
        assert_eq!(append_footers("feat: x\n\nFixes: #20", &lines), "feat: x\n\nFixes: #20\nFactory-Job: 001");
        assert_eq!(append_footers("feat: x", &[]), "feat: x");
    }

    #[test]
    fn append_footers_joins_an_existing_footer_block() {
        let lines = vec!["Fixes: #20".to_string()];
        assert_eq!(
            append_footers("feat: x\n\nbody\n\nSigned-off-by: A", &lines),
            "feat: x\n\nbody\n\nSigned-off-by: A\nFixes: #20"
        );
    }
}
