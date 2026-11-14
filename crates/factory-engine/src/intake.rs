//! The issue queue: which labelled issue the factory takes next.

use crate::tracker::IssueInfo;
use factory_core::Config;

/// Set while an issue is being drafted and built.
pub const STATE_RUNNING: &str = "factory: running";
/// Set when the factory gave up on an issue. Only a person relabels it.
pub const STATE_NEEDS_HUMAN: &str = "factory: needs-human";
/// Set when the issue was merged.
pub const STATE_DONE: &str = "factory: done";

/// The next issue to work on, or None. An issue is eligible when it has the intake label, no
/// state label, and `labelled_by` names a trusted user for it. An untrusted or unknown labeller
/// skips the issue. The first by priority label, then age, then number wins.
pub fn pick_next(
    issues: &[IssueInfo],
    cfg: &Config,
    labelled_by: &dyn Fn(u32) -> Option<String>,
    trusted: &[String],
) -> Option<IssueInfo> {
    let has = |issue: &IssueInfo, label: &str| issue.labels.iter().any(|l| l == label);
    issues
        .iter()
        .filter(|i| has(i, &cfg.intake_label))
        .filter(|i| {
            ![STATE_RUNNING, STATE_NEEDS_HUMAN, STATE_DONE]
                .iter()
                .any(|s| has(i, s))
        })
        .filter(|i| match labelled_by(i.number) {
            Some(login) if trusted.contains(&login) => true,
            actor => {
                tracing::warn!(
                    event = "intake.skip",
                    issue = i.number,
                    actor = actor.as_deref().unwrap_or("none"),
                    msg = "the intake label was not set by a trusted user"
                );
                false
            }
        })
        .min_by_key(|i| {
            let rank = cfg
                .priority_labels
                .iter()
                .position(|p| has(i, p))
                .unwrap_or(cfg.priority_labels.len());
            (rank, i.created_at.as_str(), i.number)
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u32, labels: &[&str], created: &str) -> IssueInfo {
        IssueInfo {
            number,
            title: format!("issue {number}"),
            body: String::new(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            created_at: created.to_string(),
        }
    }

    fn alice(_: u32) -> Option<String> {
        Some("alice".to_string())
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
        let by = |n: u32| Some(if n == 3 { "mallory" } else { "alice" }.to_string());
        let got = pick_next(&issues, &Config::default(), &by, &trusted());
        assert_eq!(got.map(|i| i.number), Some(4));
    }

    #[test]
    fn pick_next_skips_an_issue_with_no_label_event() {
        let issues = vec![issue(3, &["factory"], "2026-11-01")];
        let none = |_: u32| None;
        assert_eq!(
            pick_next(&issues, &Config::default(), &none, &trusted()),
            None
        );
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
    fn pick_next_ignores_issues_without_the_intake_label() {
        let issues = vec![issue(3, &["priority: P0"], "2026-11-01")];
        assert_eq!(pick(&issues), None);
    }
}
