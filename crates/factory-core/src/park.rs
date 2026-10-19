//! The park record: why a job stopped in `needs-human`, the evidence and the next commands.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fsutil::write_atomic;

/// Largest evidence text kept per part, in bytes.
pub const PART_CAP: usize = 200 * 1024;

/// The file name inside the job folder.
pub const PARK_FILE: &str = "park.json";

/// Why a job parked. The names are stable, the dashboard reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParkKind {
    ChecksFailed,
    ChangesRemain,
    RiskyDiff,
    DiffTooLarge,
    BudgetStopped,
    BuilderStopped,
    BuilderBlocked,
    BuilderCommitted,
    NothingChanged,
    CommitFailed,
    NoChecks,
    NoTests,
    Other,
}

impl ParkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ParkKind::ChecksFailed => "checks-failed",
            ParkKind::ChangesRemain => "changes-remain",
            ParkKind::RiskyDiff => "risky-diff",
            ParkKind::DiffTooLarge => "diff-too-large",
            ParkKind::BudgetStopped => "budget-stopped",
            ParkKind::BuilderStopped => "builder-stopped",
            ParkKind::BuilderBlocked => "builder-blocked",
            ParkKind::BuilderCommitted => "builder-committed",
            ParkKind::NothingChanged => "nothing-changed",
            ParkKind::CommitFailed => "commit-failed",
            ParkKind::NoChecks => "no-checks",
            ParkKind::NoTests => "no-tests",
            ParkKind::Other => "other",
        }
    }

    /// Like `next`, but a park that offers approve while the code review has not run offers
    /// `approve --without-review` instead, since a plain approve would be refused.
    pub fn next_for(self, id: &str, review_missing: bool) -> Vec<String> {
        let next = self.next(id);
        let approve = format!("factory approve {id}");
        if !review_missing || !next.contains(&approve) {
            return next;
        }
        vec![
            format!("factory rework {id} <note>"),
            format!("factory cancel {id}"),
            format!("factory approve {id} --without-review (the review has not run)"),
        ]
    }

    /// The commands worth running next, with the job id filled in.
    pub fn next(self, id: &str) -> Vec<String> {
        let approve = format!("factory approve {id}");
        let rework = format!("factory rework {id} <note>");
        let cancel = format!("factory cancel {id}");
        match self {
            ParkKind::ChecksFailed
            | ParkKind::BuilderStopped
            | ParkKind::BuilderBlocked
            | ParkKind::BuilderCommitted
            | ParkKind::CommitFailed => {
                vec![rework]
            }
            ParkKind::ChangesRemain
            | ParkKind::RiskyDiff
            | ParkKind::DiffTooLarge
            | ParkKind::NoChecks
            | ParkKind::NoTests => vec![approve, rework],
            ParkKind::BudgetStopped => vec![approve, cancel],
            ParkKind::NothingChanged => vec![rework, cancel],
            ParkKind::Other => vec![approve, rework, cancel],
        }
    }
}

/// One block of evidence, such as the failing check output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkPart {
    pub title: String,
    pub source: String,
    pub text: String,
    pub truncated: bool,
}

impl ParkPart {
    /// Keeps at most `PART_CAP` bytes of `text`, the end of it, cut on a character boundary.
    pub fn new(title: &str, source: &str, text: &str) -> ParkPart {
        let (text, truncated) = if text.len() <= PART_CAP {
            (text.to_string(), false)
        } else {
            let mut start = text.len() - PART_CAP;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            (text[start..].to_string(), true)
        };
        ParkPart {
            title: title.to_string(),
            source: source.to_string(),
            text,
            truncated,
        }
    }
}

/// The content of `park.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParkRecord {
    pub kind: ParkKind,
    pub reason: String,
    /// Unix seconds.
    pub at: u64,
    pub round: u32,
    pub next: Vec<String>,
    pub parts: Vec<ParkPart>,
}

impl ParkRecord {
    pub fn new(
        kind: ParkKind,
        reason: &str,
        id: &str,
        round: u32,
        at: u64,
        parts: Vec<ParkPart>,
    ) -> ParkRecord {
        ParkRecord {
            kind,
            reason: reason.to_string(),
            at,
            round,
            next: kind.next(id),
            parts,
        }
    }

    pub fn write(&self, dir: &Path) -> Result<(), CoreError> {
        // `CoreError::Json` converts from a serde_json error, so `?` is enough.
        let bytes = serde_json::to_vec_pretty(self)?;
        write_atomic(&dir.join(PARK_FILE), &bytes)
    }

    /// A missing or unreadable file is `None`, because old jobs have no record.
    pub fn read(dir: &Path) -> Option<ParkRecord> {
        let text = std::fs::read_to_string(dir.join(PARK_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_are_stable_kebab_case() {
        assert_eq!(ParkKind::ChecksFailed.as_str(), "checks-failed");
        assert_eq!(ParkKind::ChangesRemain.as_str(), "changes-remain");
        assert_eq!(ParkKind::RiskyDiff.as_str(), "risky-diff");
        assert_eq!(ParkKind::DiffTooLarge.as_str(), "diff-too-large");
        assert_eq!(ParkKind::BudgetStopped.as_str(), "budget-stopped");
        assert_eq!(ParkKind::BuilderStopped.as_str(), "builder-stopped");
        assert_eq!(ParkKind::BuilderBlocked.as_str(), "builder-blocked");
        assert_eq!(ParkKind::BuilderCommitted.as_str(), "builder-committed");
        assert_eq!(ParkKind::NothingChanged.as_str(), "nothing-changed");
        assert_eq!(ParkKind::CommitFailed.as_str(), "commit-failed");
        assert_eq!(ParkKind::Other.as_str(), "other");
    }

    #[test]
    fn next_commands_carry_the_job_id() {
        assert_eq!(
            ParkKind::ChecksFailed.next("012"),
            vec!["factory rework 012 <note>"]
        );
        assert_eq!(
            ParkKind::RiskyDiff.next("012"),
            vec!["factory approve 012", "factory rework 012 <note>"]
        );
        assert_eq!(
            ParkKind::BudgetStopped.next("012"),
            vec!["factory approve 012", "factory cancel 012"]
        );
        assert_eq!(
            ParkKind::NothingChanged.next("012"),
            vec!["factory rework 012 <note>", "factory cancel 012"]
        );
        assert_eq!(
            ParkKind::Other.next("012"),
            vec![
                "factory approve 012",
                "factory rework 012 <note>",
                "factory cancel 012"
            ]
        );
    }

    #[test]
    fn next_without_a_review_offers_approve_without_review() {
        let wr = "factory approve 012 --without-review (the review has not run)";
        let want = vec![
            "factory rework 012 <note>".to_string(),
            "factory cancel 012".to_string(),
            wr.to_string(),
        ];
        for kind in [ParkKind::Other, ParkKind::BudgetStopped] {
            assert_eq!(kind.next_for("012", true), want, "{kind:?}");
        }
        // Parks without an approve option, and parks with a review, stay as they were.
        for kind in [
            ParkKind::ChecksFailed,
            ParkKind::NothingChanged,
            ParkKind::BuilderStopped,
            ParkKind::BuilderBlocked,
        ] {
            assert_eq!(kind.next_for("012", true), kind.next("012"), "{kind:?}");
        }
        for kind in [
            ParkKind::Other,
            ParkKind::RiskyDiff,
            ParkKind::BudgetStopped,
        ] {
            assert_eq!(kind.next_for("012", false), kind.next("012"), "{kind:?}");
        }
    }

    #[test]
    fn no_checks_and_no_tests_have_stable_names() {
        assert_eq!(ParkKind::NoChecks.as_str(), "no-checks");
        assert_eq!(ParkKind::NoTests.as_str(), "no-tests");
    }

    #[test]
    fn no_checks_and_no_tests_offer_approve_and_rework() {
        for kind in [ParkKind::NoChecks, ParkKind::NoTests] {
            assert_eq!(
                kind.next("012"),
                vec![
                    "factory approve 012".to_string(),
                    "factory rework 012 <note>".to_string()
                ],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_park_before_the_review_offers_approve_without_review() {
        let next = ParkKind::NoChecks.next_for("012", true);
        assert!(
            next.iter()
                .any(|n| n.contains("factory approve 012 --without-review")),
            "{next:?}"
        );
        assert!(
            !next.contains(&"factory approve 012".to_string()),
            "{next:?}"
        );
    }

    #[test]
    fn an_old_park_record_still_parses() {
        let json = r#"{"kind":"checks-failed","reason":"r","at":1,"round":1,"next":[],"parts":[]}"#;
        let rec: ParkRecord = serde_json::from_str(json).expect("old record parses");
        assert_eq!(rec.kind, ParkKind::ChecksFailed);
    }

    #[test]
    fn the_new_kinds_round_trip_through_park_json() {
        for kind in [ParkKind::NoChecks, ParkKind::NoTests] {
            let rec = ParkRecord::new(kind, "why", "012", 5, 1, Vec::new());
            let text = serde_json::to_string(&rec).expect("serialize");
            assert!(text.contains(kind.as_str()), "{text}");
            let back: ParkRecord = serde_json::from_str(&text).expect("parse");
            assert_eq!(back.kind, kind);
        }
    }

    #[test]
    fn a_short_part_is_kept_whole() {
        let p = ParkPart::new("Checks", "checks.md", "FAIL cargo check");
        assert_eq!(p.text, "FAIL cargo check");
        assert!(!p.truncated);
    }

    #[test]
    fn a_long_part_keeps_its_end_and_is_marked_truncated() {
        let text = format!("{}END", "x".repeat(PART_CAP + 10));
        let p = ParkPart::new("Log", "log", &text);
        assert!(p.truncated);
        assert!(p.text.len() <= PART_CAP);
        assert!(p.text.ends_with("END"));
    }

    #[test]
    fn the_cap_cuts_on_a_character_boundary() {
        // Each char is 3 bytes, so a byte cut in the middle of one must move forward.
        let text = "€".repeat(PART_CAP / 3 + 5);
        let p = ParkPart::new("Log", "log", &text);
        assert!(p.truncated);
        assert!(p.text.chars().all(|c| c == '€'));
    }

    #[test]
    fn a_record_round_trips_through_park_json() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let rec = ParkRecord::new(
            ParkKind::ChecksFailed,
            "checks failed after 2 fix rounds",
            "012",
            3,
            1_760_000_000,
            vec![ParkPart::new(
                "Failing checks",
                "checks.md",
                "FAIL cargo check",
            )],
        );
        rec.write(dir.path())?;
        let back = ParkRecord::read(dir.path());
        assert_eq!(back, Some(rec));
        let raw = std::fs::read_to_string(dir.path().join("park.json"))?;
        assert!(raw.contains("\"kind\": \"checks-failed\""), "{raw}");
        Ok(())
    }

    #[test]
    fn read_of_a_missing_or_broken_file_is_none() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        assert_eq!(ParkRecord::read(dir.path()), None);
        std::fs::write(dir.path().join("park.json"), "not json")?;
        assert_eq!(ParkRecord::read(dir.path()), None);
        Ok(())
    }

    #[test]
    fn write_into_a_missing_unwritable_place_is_an_error_not_a_panic() {
        let rec = ParkRecord::new(ParkKind::Other, "x", "001", 1, 1, Vec::new());
        // A path under a regular file cannot be created.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("f");
        std::fs::write(&file, "x").expect("file");
        assert!(rec.write(&file.join("sub")).is_err());
    }
}
