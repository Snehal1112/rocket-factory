//! The last stdout line of `factory run` and `factory approve`. The CLI prints these and the supervisor
//! reads them, so both sides build and test them from here and the text cannot drift.

/// The exit code of `factory run` and `factory approve` when the project is busy at the start and
/// nothing was done. It is the sysexits `EX_TEMPFAIL`: the supervisor keeps the request and tries again.
pub const BUSY_EXIT_CODE: i32 = 75;

/// Folds a reason to one line. A reason can hold a file name or a message with a newline, and the
/// supervisor reads only the last line, so a second line must never look like a result.
fn one_line(reason: &str) -> String {
    reason
        .split(|c: char| c.is_control())
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `merged`, or `merged <sha>` after an approve.
pub fn merged_line(sha: Option<&str>) -> String {
    match sha {
        Some(sha) => format!("merged {sha}"),
        None => "merged".to_string(),
    }
}

pub fn needs_human_line(reason: &str) -> String {
    format!("needs-human: {}", one_line(reason))
}

pub fn parked_line(reason: &str) -> String {
    format!("parked: {}", one_line(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_have_a_fixed_shape() {
        assert_eq!(merged_line(None), "merged");
        assert_eq!(merged_line(Some("abc1234")), "merged abc1234");
        assert_eq!(needs_human_line("risky"), "needs-human: risky");
        assert_eq!(parked_line("x"), "parked: x");
    }

    #[test]
    fn a_reason_with_newlines_stays_on_one_line() {
        let line = parked_line("builder changed migrations/x\nmerged\r\n");
        assert_eq!(line, "parked: builder changed migrations/x merged");
        assert_eq!(line.lines().count(), 1);
        assert_eq!(needs_human_line("a\n\nb"), "needs-human: a b");
    }
}
