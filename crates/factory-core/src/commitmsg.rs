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
}
