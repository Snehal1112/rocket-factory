use factory_core::{classify_paths, BudgetState, Config, DiffClass, ParkKind, Verdict};

/// What to do with a job after the reviewer ran.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    AutoMerge,
    FixRound,
    NeedsHuman { kind: ParkKind, reason: String },
}

pub struct DecideInput<'a> {
    pub checks_ok: bool,
    pub verdict: &'a Verdict,
    pub class: DiffClass,
    pub risky_summary: String,
    pub diff_lines: usize,
    pub diff_cap: usize,
    pub budget: BudgetState,
    pub round: u32,
    /// The fix-round limit for this diff's class.
    pub fix_rounds: u32,
    /// The round count at the last rework, so a rework gets fresh fix rounds.
    pub rework_base: u32,
    /// True when a reviewer run ended failed or stalled, so its reply is missing for an environment reason.
    pub role_failed: bool,
}

/// Implements the decision order of the review-findings spec. The order of the checks matters.
/// The risk gate comes last, so a risky diff gets its fix rounds before it parks for a human.
pub fn decide(i: &DecideInput) -> Decision {
    if i.budget == BudgetState::Stop {
        return Decision::NeedsHuman {
            kind: ParkKind::BudgetStopped,
            reason: "budget is stopped".to_string(),
        };
    }
    if i.diff_lines > i.diff_cap {
        return Decision::NeedsHuman {
            kind: ParkKind::DiffTooLarge,
            reason: format!(
                "diff has {} changed lines, the cap is {}",
                i.diff_lines, i.diff_cap
            ),
        };
    }
    let used = fix_rounds_used(i.round, i.rework_base);
    let rounds_left = used < i.fix_rounds;
    if !i.checks_ok {
        return if rounds_left {
            Decision::FixRound
        } else {
            Decision::NeedsHuman {
                kind: ParkKind::ChecksFailed,
                reason: format!("checks failed after {used} fix rounds"),
            }
        };
    }
    match i.verdict {
        // A risky diff never merges without a human, even when approved.
        Verdict::Approve if i.class == DiffClass::Risky => Decision::NeedsHuman {
            kind: ParkKind::RiskyDiff,
            reason: format!("risky diff: {}", i.risky_summary),
        },
        Verdict::Approve => Decision::AutoMerge,
        // A reviewer that never finished says nothing about the diff, so a fix round cannot help.
        Verdict::Missing if i.role_failed => Decision::NeedsHuman {
            kind: ParkKind::ReviewFailed,
            reason: "review failed: a reviewer run did not finish".into(),
        },
        // A missing verdict is never a pass.
        Verdict::Changes | Verdict::Missing => {
            if rounds_left {
                Decision::FixRound
            } else {
                Decision::NeedsHuman {
                    kind: ParkKind::ChangesRemain,
                    reason: format!("changes remain after {used} fix rounds"),
                }
            }
        }
    }
}

/// The fix rounds spent since the last rework, when the job is in `round`.
/// Round 1 is the first build.
pub(crate) fn fix_rounds_used(round: u32, rework_base: u32) -> u32 {
    round.saturating_sub(1).saturating_sub(rework_base)
}

/// Names the risky paths for the human, at most five, with the count of the rest.
pub fn risk_summary(paths: &[String], cfg: &Config) -> String {
    let risky: Vec<&String> = paths
        .iter()
        .filter(|p| classify_paths(std::slice::from_ref(*p), cfg) == DiffClass::Risky)
        .collect();
    let shown: Vec<&str> = risky.iter().take(5).map(|s| s.as_str()).collect();
    let more = risky.len().saturating_sub(shown.len());
    let tail = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    format!(
        "risky paths ({}): {}{}",
        risky.len(),
        shown.join(", "),
        tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(verdict: &Verdict) -> DecideInput<'_> {
        DecideInput {
            checks_ok: true,
            verdict,
            class: DiffClass::Normal,
            risky_summary: String::new(),
            diff_lines: 100,
            diff_cap: 4000,
            budget: BudgetState::Ok,
            round: 1,
            fix_rounds: 2,
            rework_base: 0,
            role_failed: true,
        }
    }

    #[test]
    fn a_failed_role_with_missing_verdict_parks_review_failed() {
        match decide(&input(&Verdict::Missing)) {
            Decision::NeedsHuman { kind, reason } => {
                assert_eq!(kind, ParkKind::ReviewFailed);
                assert!(reason.starts_with("review failed"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_failed_review_on_failing_checks_still_starts_a_fix_round() {
        let mut i = input(&Verdict::Missing);
        i.checks_ok = false;
        assert_eq!(decide(&i), Decision::FixRound);
    }

    #[test]
    fn budget_stop_wins_over_a_failed_review() {
        let mut i = input(&Verdict::Missing);
        i.budget = BudgetState::Stop;
        match decide(&i) {
            Decision::NeedsHuman { kind, .. } => assert_eq!(kind, ParkKind::BudgetStopped),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_changes_verdict_with_a_failed_role_starts_a_fix_round() {
        assert_eq!(decide(&input(&Verdict::Changes)), Decision::FixRound);
    }
}
