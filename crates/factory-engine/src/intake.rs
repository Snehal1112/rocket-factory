//! The issue queue: which labelled issue the factory takes next.

use crate::tracker::{IssueInfo, LabelEvent};
use factory_core::Config;

/// Set while an issue is being drafted and built.
pub const STATE_RUNNING: &str = "factory: running";
/// Set when the factory gave up on an issue. Only a person relabels it.
pub const STATE_NEEDS_HUMAN: &str = "factory: needs-human";
/// Set when the issue was merged.
pub const STATE_DONE: &str = "factory: done";

/// The next issue to work on, or None. A candidate is open, has the intake label and no state
/// label. Candidates sort by priority label, then age (a missing date last), then number. The
/// first one that is eligible wins: `labelled_by` names a trusted actor and nobody touched the
/// issue after that label event. A failed lookup, no event, an untrusted actor and an empty
/// `trusted` list all make an issue ineligible. Labels and logins compare without regard to case.
pub fn pick_next(
    issues: &[IssueInfo],
    cfg: &Config,
    labelled_by: &dyn Fn(u32) -> Result<Option<LabelEvent>, String>,
    trusted: &[String],
) -> Option<IssueInfo> {
    let has =
        |issue: &IssueInfo, label: &str| issue.labels.iter().any(|l| l.eq_ignore_ascii_case(label));
    let mut candidates: Vec<&IssueInfo> = issues
        .iter()
        .filter(|i| has(i, &cfg.intake_label))
        .filter(|i| {
            ![STATE_RUNNING, STATE_NEEDS_HUMAN, STATE_DONE]
                .iter()
                .any(|s| has(i, s))
        })
        .collect();
    candidates.sort_by_key(|i| {
        let rank = cfg
            .priority_labels
            .iter()
            .position(|p| has(i, p))
            .unwrap_or(cfg.priority_labels.len());
        (
            rank,
            i.created_at.is_empty(),
            i.created_at.as_str(),
            i.number,
        )
    });
    candidates
        .into_iter()
        .find(|i| eligible(i, labelled_by, trusted))
        .cloned()
}

/// Whether a trusted user added the intake label and nobody touched the issue since. Every
/// refusal is logged.
fn eligible(
    issue: &IssueInfo,
    labelled_by: &dyn Fn(u32) -> Result<Option<LabelEvent>, String>,
    trusted: &[String],
) -> bool {
    let skip = |msg: &str, actor: &str| {
        tracing::warn!(event = "intake.skip", issue = issue.number, actor, msg);
        false
    };
    match labelled_by(issue.number) {
        Err(e) => skip(&format!("the label lookup failed: {e}"), "none"),
        Ok(None) => skip("no label event found", "none"),
        Ok(Some(event)) if !trusted.iter().any(|t| t.eq_ignore_ascii_case(&event.actor)) => skip(
            "the intake label was not set by a trusted user",
            &event.actor,
        ),
        Ok(Some(event)) if issue.updated_at > event.at => skip(
            "the issue changed after the intake label was set",
            &event.actor,
        ),
        Ok(Some(_)) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const LABELLED_AT: &str = "2026-11-05T10:00:00Z";

    fn issue(number: u32, labels: &[&str], created: &str) -> IssueInfo {
        IssueInfo {
            number,
            title: format!("issue {number}"),
            body: String::new(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            created_at: created.to_string(),
            updated_at: String::new(),
        }
    }

    fn event(actor: &str) -> LabelEvent {
        LabelEvent {
            actor: actor.to_string(),
            at: LABELLED_AT.to_string(),
        }
    }

    fn alice(_: u32) -> Result<Option<LabelEvent>, String> {
        Ok(Some(event("alice")))
    }

    fn trusted() -> Vec<String> {
        vec!["alice".to_string()]
    }

    fn pick(issues: &[IssueInfo]) -> Option<u32> {
        pick_next(issues, &Config::default(), &alice, &trusted()).map(|i| i.number)
    }

    #[test]
    fn pick_next_prefers_the_higher_priority_then_the_older_issue() {
        let mut issues = vec![
            issue(3, &["factory", "priority: P1"], "2026-11-01"),
            issue(4, &["factory", "priority: P0"], "2026-11-03"),
            issue(5, &["factory", "priority: P1"], "2026-11-02"),
        ];
        assert_eq!(pick(&issues), Some(4));
        issues.remove(1);
        assert_eq!(pick(&issues), Some(3));
    }

    #[test]
    fn pick_next_sorts_unprioritised_issues_last() {
        let issues = vec![
            issue(3, &["factory"], "2026-11-01"),
            issue(4, &["factory", "priority: P2"], "2026-11-05"),
        ];
        assert_eq!(pick(&issues), Some(4));
    }

    #[test]
    fn pick_next_skips_state_labelled_issues() {
        for state in [STATE_RUNNING, STATE_NEEDS_HUMAN, STATE_DONE] {
            let issues = vec![issue(3, &["factory", state], "2026-11-01")];
            assert_eq!(pick(&issues), None, "{state}");
        }
    }

    #[test]
    fn pick_next_skips_untrusted_labellers() {
        let issues = vec![
            issue(3, &["factory"], "2026-11-01"),
            issue(4, &["factory"], "2026-11-02"),
        ];
        let by = |n: u32| Ok(Some(event(if n == 3 { "mallory" } else { "alice" })));
        let got = pick_next(&issues, &Config::default(), &by, &trusted());
        assert_eq!(got.map(|i| i.number), Some(4));
    }

    #[test]
    fn pick_next_skips_a_null_actor() {
        let issues = vec![issue(3, &["factory"], "2026-11-01")];
        let none = |_: u32| Ok(None);
        assert_eq!(
            pick_next(&issues, &Config::default(), &none, &trusted()),
            None
        );
    }

    #[test]
    fn pick_next_skips_an_issue_whose_lookup_fails() {
        let issues = vec![
            issue(3, &["factory"], "2026-11-01"),
            issue(4, &["factory"], "2026-11-02"),
        ];
        let by = |n: u32| {
            if n == 3 {
                Err("boom".to_string())
            } else {
                Ok(Some(event("alice")))
            }
        };
        let got = pick_next(&issues, &Config::default(), &by, &trusted());
        assert_eq!(got.map(|i| i.number), Some(4));
        let fails = |_: u32| Err("boom".to_string());
        assert_eq!(
            pick_next(&issues, &Config::default(), &fails, &trusted()),
            None
        );
    }

    #[test]
    fn pick_next_returns_none_for_an_empty_trusted_list() {
        let issues = vec![issue(3, &["factory"], "2026-11-01")];
        assert_eq!(pick_next(&issues, &Config::default(), &alice, &[]), None);
    }

    #[test]
    fn pick_next_skips_an_issue_edited_after_labelling() {
        let mut edited = issue(3, &["factory"], "2026-11-01");
        edited.updated_at = "2026-11-05T10:00:01Z".to_string();
        assert_eq!(pick(std::slice::from_ref(&edited)), None);
        edited.updated_at = LABELLED_AT.to_string();
        assert_eq!(pick(&[edited]), Some(3));
    }

    #[test]
    fn pick_next_ties_break_by_issue_number() {
        let issues = vec![
            issue(9, &["factory", "priority: P1"], "2026-11-01"),
            issue(7, &["factory", "priority: P1"], "2026-11-01"),
        ];
        assert_eq!(pick(&issues), Some(7));
    }

    #[test]
    fn pick_next_ranks_a_missing_date_last() {
        let issues = vec![
            issue(3, &["factory"], ""),
            issue(4, &["factory"], "2026-11-01"),
        ];
        assert_eq!(pick(&issues), Some(4));
    }

    #[test]
    fn pick_next_compares_labels_without_regard_to_case() {
        let running = vec![issue(3, &["factory", "Factory: Running"], "2026-11-01")];
        assert_eq!(pick(&running), None);
        let by = |_: u32| Ok(Some(event("Alice")));
        let plain = vec![issue(3, &["factory"], "2026-11-01")];
        let got = pick_next(&plain, &Config::default(), &by, &trusted());
        assert_eq!(got.map(|i| i.number), Some(3));
    }

    #[test]
    fn pick_next_stops_asking_after_the_first_eligible_issue() {
        let issues = vec![
            issue(3, &["factory", "priority: P0"], "2026-11-01"),
            issue(4, &["factory"], "2026-11-01"),
            issue(5, &["factory"], "2026-11-02"),
        ];
        let calls = Cell::new(0);
        let by = |_: u32| {
            calls.set(calls.get() + 1);
            Ok(Some(event("alice")))
        };
        let got = pick_next(&issues, &Config::default(), &by, &trusted());
        assert_eq!(got.map(|i| i.number), Some(3));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn pick_next_ignores_issues_without_the_intake_label() {
        let issues = vec![issue(3, &["priority: P0"], "2026-11-01")];
        assert_eq!(pick(&issues), None);
    }
}
