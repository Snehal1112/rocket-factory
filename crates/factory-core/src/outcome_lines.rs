//! The last stdout line of `factory run` and `factory approve`. The CLI prints these and the supervisor
//! reads them, so both sides build and test them from here and the text cannot drift.

/// `merged`, or `merged <sha>` after an approve.
pub fn merged_line(sha: Option<&str>) -> String {
    match sha {
        Some(sha) => format!("merged {sha}"),
        None => "merged".to_string(),
    }
}

pub fn needs_human_line(reason: &str) -> String {
    format!("needs-human: {reason}")
}

pub fn parked_line(reason: &str) -> String {
    format!("parked: {reason}")
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
}
