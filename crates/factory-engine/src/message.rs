//! The kinds, fixed texts and footers of the merge and sync messages the driver writes.

use factory_core::issue_footer;

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

/// What the driver needs to write a merge or sync message.
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
    /// True when this merge is the last plan of its job, so the footer says `Fixes`.
    pub closes_ticket: bool,
}

/// The message the driver used before the committer existed. It is the fallback.
pub fn fixed_message(kind: MessageKind, job_id: &str, slug: &str, base: &str) -> String {
    match kind {
        MessageKind::Merge => format!("chore(factory): merge job {job_id} {slug}"),
        MessageKind::Sync => format!("chore(factory): sync {base} into job {job_id} {slug}"),
    }
}

/// The footer lines the driver adds: the issue line (`Fixes` for a merge that closes the ticket, `Relates to` for any other merge or a sync)
/// and then `Factory-Job`.
pub fn driver_footers(req: &MessageRequest<'_>) -> Vec<String> {
    let key = match req.kind {
        MessageKind::Merge if req.closes_ticket => "Fixes",
        MessageKind::Merge => "Relates to",
        MessageKind::Sync => "Relates to",
    };
    let mut lines: Vec<String> = issue_footer(key, req.issues).into_iter().collect();
    lines.push(format!("Factory-Job: {}", req.job_id));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn footer_req(kind: MessageKind, closes_ticket: bool) -> MessageRequest<'static> {
        MessageRequest {
            kind,
            job_id: "001",
            slug: "s",
            title: "t",
            base: "a",
            tip: "b",
            issues: &[76],
            closes_ticket,
        }
    }

    #[test]
    fn a_merge_that_closes_the_ticket_says_fixes() {
        let req = footer_req(MessageKind::Merge, true);
        assert_eq!(
            driver_footers(&req),
            vec!["Fixes: #76".to_string(), "Factory-Job: 001".to_string()]
        );
    }

    #[test]
    fn a_merge_that_does_not_close_the_ticket_says_relates_to() {
        let req = footer_req(MessageKind::Merge, false);
        assert_eq!(
            driver_footers(&req),
            vec![
                "Relates to: #76".to_string(),
                "Factory-Job: 001".to_string()
            ]
        );
    }

    #[test]
    fn a_sync_is_always_relates_to() {
        let req = footer_req(MessageKind::Sync, true);
        assert_eq!(
            driver_footers(&req),
            vec![
                "Relates to: #76".to_string(),
                "Factory-Job: 001".to_string()
            ]
        );
    }

    #[test]
    fn fixed_messages_match_the_old_driver_text() {
        assert_eq!(
            fixed_message(MessageKind::Merge, "001", "hello", "main"),
            "chore(factory): merge job 001 hello"
        );
        assert_eq!(
            fixed_message(MessageKind::Sync, "001", "hello", "main"),
            "chore(factory): sync main into job 001 hello"
        );
    }
}
