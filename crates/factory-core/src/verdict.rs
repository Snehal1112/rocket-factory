#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Changes,
    Missing,
}

/// Only an exact last non-empty line counts. Trailing whitespace and CRLF are tolerated, nothing else.
pub fn parse_verdict(text: &str) -> Verdict {
    let last = text.lines().rev().map(|l| l.trim_end()).find(|l| !l.is_empty());
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
    fn exact_last_line_wins() {
        assert_eq!(parse_verdict("Looks fine.\nVERDICT: APPROVE"), Verdict::Approve);
        assert_eq!(parse_verdict("Issues below.\nVERDICT: CHANGES\n\n\n"), Verdict::Changes);
        assert_eq!(parse_verdict("VERDICT: APPROVE\r\n"), Verdict::Approve);
        assert_eq!(parse_verdict("text\r\nVERDICT: CHANGES   \r\n"), Verdict::Changes);
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
        assert_eq!(parse_verdict("VERDICT: APPROVE\nbut see the notes above"), Verdict::Missing);
        assert_eq!(parse_verdict("VERDICT: CHANGES\nVERDICT: MAYBE"), Verdict::Missing);
    }
}
