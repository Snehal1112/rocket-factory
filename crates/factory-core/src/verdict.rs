#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Changes,
    Missing,
}

impl Verdict {
    /// The word stored on the board and in the park record.
    pub fn as_word(self) -> &'static str {
        match self {
            Verdict::Approve => "approve",
            Verdict::Changes => "changes",
            Verdict::Missing => "missing",
        }
    }
}

/// The review roles a job can hold, in the order they run.
pub const REVIEW_ROLES: [&str; 2] = ["code", "security"];

/// `Approve` for a non-empty all-approve list, `Changes` when any entry is `Changes`, else `Missing`.
pub fn combine_verdicts(verdicts: &[Verdict]) -> Verdict {
    if verdicts.contains(&Verdict::Changes) {
        Verdict::Changes
    } else if !verdicts.is_empty() && verdicts.iter().all(|v| *v == Verdict::Approve) {
        Verdict::Approve
    } else {
        Verdict::Missing
    }
}

/// The trimmed lines of a review that carry `[blocker]` and not `[fixed]`, in order.
pub fn open_blockers(review_text: &str) -> Vec<String> {
    review_text
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("[blocker]") && !l.contains("[fixed]"))
        .map(str::to_string)
        .collect()
}

/// The trimmed lines of a review that carry `[minor]` and not `[fixed]`, in order.
pub fn open_minors(review_text: &str) -> Vec<String> {
    review_text
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("[minor]") && !l.contains("[fixed]"))
        .map(str::to_string)
        .collect()
}

/// Only an exact last non-empty line counts. Trailing whitespace and CRLF are tolerated, nothing else.
pub fn parse_verdict(text: &str) -> Verdict {
    let last = text
        .lines()
        .rev()
        .map(|l| l.trim_end())
        .find(|l| !l.is_empty());
    match last {
        Some("VERDICT: APPROVE") => Verdict::Approve,
        Some("VERDICT: CHANGES") => Verdict::Changes,
        _ => Verdict::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_blockers_keeps_tagged_lines_in_order() {
        let text = "a\n- [blocker] one\n- [minor] two\n- [blocker] three";
        assert_eq!(
            open_blockers(text),
            vec!["- [blocker] one", "- [blocker] three"]
        );
    }

    #[test]
    fn open_minors_skips_fixed_and_blockers() {
        let text = "- [minor] one\n- [blocker] two\n  - [minor] [fixed] three\n- [minor] four";
        assert_eq!(open_minors(text), vec!["- [minor] one", "- [minor] four"]);
    }

    #[test]
    fn fixed_lines_are_not_open_blockers() {
        let text = "- [blocker] [fixed] one\n- [fixed] two\n- [blocker] three";
        assert_eq!(open_blockers(text), vec!["- [blocker] three"]);
    }

    #[test]
    fn combine_all_approve_is_approve() {
        assert_eq!(
            combine_verdicts(&[Verdict::Approve, Verdict::Approve]),
            Verdict::Approve
        );
    }

    #[test]
    fn combine_any_changes_is_changes() {
        assert_eq!(
            combine_verdicts(&[Verdict::Approve, Verdict::Changes, Verdict::Missing]),
            Verdict::Changes
        );
    }

    #[test]
    fn combine_missing_without_changes_is_missing() {
        assert_eq!(
            combine_verdicts(&[Verdict::Approve, Verdict::Missing]),
            Verdict::Missing
        );
    }

    #[test]
    fn combine_empty_is_missing() {
        assert_eq!(combine_verdicts(&[]), Verdict::Missing);
    }

    #[test]
    fn as_word_names_each_verdict() {
        assert_eq!(Verdict::Approve.as_word(), "approve");
        assert_eq!(Verdict::Changes.as_word(), "changes");
        assert_eq!(Verdict::Missing.as_word(), "missing");
    }

    #[test]
    fn exact_last_line_wins() {
        assert_eq!(
            parse_verdict("Looks fine.\nVERDICT: APPROVE"),
            Verdict::Approve
        );
        assert_eq!(
            parse_verdict("Issues below.\nVERDICT: CHANGES\n\n\n"),
            Verdict::Changes
        );
        assert_eq!(parse_verdict("VERDICT: APPROVE\r\n"), Verdict::Approve);
        assert_eq!(
            parse_verdict("text\r\nVERDICT: CHANGES   \r\n"),
            Verdict::Changes
        );
        assert_eq!(parse_verdict("VERDICT: APPROVE"), Verdict::Approve);
    }

    #[test]
    fn anything_else_is_missing() {
        assert_eq!(parse_verdict(""), Verdict::Missing);
        assert_eq!(parse_verdict("   \n \n"), Verdict::Missing);
        assert_eq!(parse_verdict("**VERDICT: APPROVE**"), Verdict::Missing);
        assert_eq!(parse_verdict("VERDICT: approve"), Verdict::Missing);
        assert_eq!(parse_verdict("VERDICT: APPROVE please"), Verdict::Missing);
        assert_eq!(parse_verdict("  VERDICT: APPROVE"), Verdict::Missing);
        assert_eq!(
            parse_verdict("VERDICT: APPROVE\nbut see the notes above"),
            Verdict::Missing
        );
        assert_eq!(
            parse_verdict("VERDICT: CHANGES\nVERDICT: MAYBE"),
            Verdict::Missing
        );
    }
}
