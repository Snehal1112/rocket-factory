//! The gate record: one JSON line per gate result in a job's `gates.jsonl`, read by the dashboard.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// The file name inside the job folder.
pub const GATES_FILE: &str = "gates.jsonl";

/// Most characters kept in the detail text of one line.
pub const MAX_DETAIL: usize = 300;

/// The version written into every line.
const LINE_VERSION: u32 = 1;

/// One of the 18 gates of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateKey {
    SpecCheck,
    SpecReview,
    Series,
    Machine,
    BuilderGuard,
    Builder,
    Checks,
    Format,
    Lint,
    NoChecks,
    NoTests,
    Budget,
    DiffCap,
    Review,
    Risk,
    FinalGate,
    Merge,
    Approve,
}

impl GateKey {
    /// Every gate, in the order of the dashboard cards.
    pub const ALL: [GateKey; 18] = [
        GateKey::SpecCheck,
        GateKey::SpecReview,
        GateKey::Series,
        GateKey::Machine,
        GateKey::BuilderGuard,
        GateKey::Builder,
        GateKey::Checks,
        GateKey::Format,
        GateKey::Lint,
        GateKey::NoChecks,
        GateKey::NoTests,
        GateKey::Budget,
        GateKey::DiffCap,
        GateKey::Review,
        GateKey::Risk,
        GateKey::FinalGate,
        GateKey::Merge,
        GateKey::Approve,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GateKey::SpecCheck => "spec-check",
            GateKey::SpecReview => "spec-review",
            GateKey::Series => "series",
            GateKey::Machine => "machine",
            GateKey::BuilderGuard => "builder-guard",
            GateKey::Builder => "builder",
            GateKey::Checks => "checks",
            GateKey::Format => "format",
            GateKey::Lint => "lint",
            GateKey::NoChecks => "no-checks",
            GateKey::NoTests => "no-tests",
            GateKey::Budget => "budget",
            GateKey::DiffCap => "diff-cap",
            GateKey::Review => "review",
            GateKey::Risk => "risk",
            GateKey::FinalGate => "final-gate",
            GateKey::Merge => "merge",
            GateKey::Approve => "approve",
        }
    }

    /// The stage group the gate belongs to on the dashboard.
    pub fn stage(self) -> &'static str {
        match self {
            GateKey::SpecCheck | GateKey::SpecReview => "spec",
            GateKey::Series | GateKey::Machine => "start",
            GateKey::BuilderGuard | GateKey::Builder => "build",
            GateKey::Checks
            | GateKey::Format
            | GateKey::Lint
            | GateKey::NoChecks
            | GateKey::NoTests => "checks",
            GateKey::Budget | GateKey::DiffCap | GateKey::Review | GateKey::Risk => "review",
            GateKey::FinalGate | GateKey::Merge | GateKey::Approve => "merge",
        }
    }

    /// True for a gate that runs once per job, not once per round.
    pub fn once(self) -> bool {
        matches!(
            self,
            GateKey::SpecCheck | GateKey::SpecReview | GateKey::Series
        )
    }
}

/// The result of a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateResult {
    Pass,
    Fail,
    Note,
    Waived,
    Skipped,
    Running,
    Waiting,
}

/// One line of `gates.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateLine {
    pub v: u32,
    pub gate: GateKey,
    pub result: GateResult,
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub at: u64,
    pub detail: String,
}

impl GateLine {
    /// A line of the current version. Round 0 is written as 1 and the detail is cut.
    pub fn new(
        gate: GateKey,
        result: GateResult,
        round: u32,
        head: Option<String>,
        at: u64,
        detail: &str,
    ) -> GateLine {
        GateLine {
            v: LINE_VERSION,
            gate,
            result,
            round: round.max(1),
            head,
            at,
            detail: cut_detail(detail),
        }
    }
}

/// At most `MAX_DETAIL` characters, the last one `…` when cut. Newlines become spaces.
pub fn cut_detail(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_DETAIL {
        return flat;
    }
    let mut out: String = flat.chars().take(MAX_DETAIL - 1).collect();
    out.push('…');
    out
}

/// Appends one line. The job folder must exist. A file that ends in a cut line gets a newline
/// first, so the new line stands alone.
pub fn append(dir: &Path, line: &GateLine) -> Result<(), CoreError> {
    let path = dir.join(GATES_FILE);
    let mut text = serde_json::to_string(line)?;
    text.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
        .map_err(|e| CoreError::io(&path, e))?;
    let len = file.metadata().map_err(|e| CoreError::io(&path, e))?.len();
    if len > 0 {
        let mut last = [0u8; 1];
        file.seek(SeekFrom::End(-1))
            .and_then(|_| file.read_exact(&mut last))
            .map_err(|e| CoreError::io(&path, e))?;
        if last[0] != b'\n' {
            text.insert(0, '\n');
        }
    }
    file.write_all(text.as_bytes())
        .map_err(|e| CoreError::io(&path, e))
}

/// The good lines of the file. Blank, broken, newer-version and unknown-gate lines are skipped.
/// A missing file is an empty list.
pub fn read(dir: &Path) -> Vec<GateLine> {
    let Ok(bytes) = std::fs::read(dir.join(GATES_FILE)) else {
        return Vec::new();
    };
    // A lossy decode keeps the good lines when a torn write cuts a multi-byte character.
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .filter_map(|l| serde_json::from_str::<GateLine>(l).ok())
        .filter(|l| l.v <= LINE_VERSION)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_skips_broken_blank_newer_and_unknown_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = GateLine::new(
            GateKey::NoChecks,
            GateResult::Pass,
            2,
            Some("b83bfbc".into()),
            7,
            "all 7 code paths covered",
        );
        let text = format!(
            "{}\n\n{{not json\n{}\n{}\n",
            serde_json::to_string(&good).expect("json"),
            r#"{"v":2,"gate":"checks","result":"pass","round":1,"at":1,"detail":""}"#,
            r#"{"v":1,"gate":"no-such-gate","result":"pass","round":1,"at":1,"detail":""}"#,
        );
        std::fs::write(dir.path().join(GATES_FILE), text).expect("write");
        assert_eq!(read(dir.path()), vec![good]);
    }

    #[test]
    fn read_keeps_good_lines_before_a_line_cut_inside_a_character() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = GateLine::new(GateKey::Lint, GateResult::Pass, 1, None, 3, "clean");
        let mut bytes = serde_json::to_string(&good).expect("json").into_bytes();
        bytes.extend_from_slice(b"\n{\"v\":1,\"detail\":\"");
        // The first byte of the two-byte character é.
        bytes.push("é".as_bytes()[0]);
        std::fs::write(dir.path().join(GATES_FILE), bytes).expect("write");
        assert_eq!(read(dir.path()), vec![good]);
    }

    #[test]
    fn a_line_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let line = GateLine::new(
            GateKey::Review,
            GateResult::Fail,
            3,
            None,
            9,
            "code CHANGES, 2 blockers",
        );
        append(dir.path(), &line).expect("append");
        assert_eq!(read(dir.path()), vec![line]);
        let text = std::fs::read_to_string(dir.path().join(GATES_FILE)).expect("read");
        assert!(text.contains("\"gate\":\"review\""));
        assert!(!text.contains("\"head\""));
    }

    #[test]
    fn append_after_a_cut_line_starts_a_new_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(GATES_FILE);
        std::fs::write(&path, r#"{"v":1,"gate":"chec"#).expect("write");
        let line = GateLine::new(GateKey::Merge, GateResult::Pass, 1, None, 4, "merged");
        append(dir.path(), &line).expect("append");
        assert_eq!(read(dir.path()), vec![line]);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn append_into_a_missing_folder_fails_and_creates_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let line = GateLine::new(GateKey::Merge, GateResult::Pass, 1, None, 4, "merged");
        append(&tmp.path().join("gone"), &line).expect_err("missing folder");
        assert!(!tmp.path().join("gone").exists());
    }

    #[test]
    fn cut_detail_keeps_300_characters() {
        let cut = cut_detail(&"é".repeat(400));
        assert_eq!(cut.chars().count(), 300);
        assert!(cut.ends_with('…'));
        assert_eq!(cut_detail("a\nb"), "a b");
    }

    #[test]
    fn round_zero_is_written_as_one() {
        let line = GateLine::new(GateKey::Builder, GateResult::Pass, 0, None, 1, "");
        assert_eq!(line.round, 1);
    }

    #[test]
    fn all_keys_are_in_card_order_with_their_stages() {
        let names = GateKey::ALL.map(GateKey::as_str);
        assert_eq!(
            names,
            [
                "spec-check",
                "spec-review",
                "series",
                "machine",
                "builder-guard",
                "builder",
                "checks",
                "format",
                "lint",
                "no-checks",
                "no-tests",
                "budget",
                "diff-cap",
                "review",
                "risk",
                "final-gate",
                "merge",
                "approve",
            ]
        );
        let count = |stage: &str| GateKey::ALL.iter().filter(|k| k.stage() == stage).count();
        assert_eq!(
            [
                count("spec"),
                count("start"),
                count("build"),
                count("checks"),
                count("review"),
                count("merge"),
            ],
            [2, 2, 2, 5, 4, 3]
        );
        let once: Vec<&str> = GateKey::ALL
            .iter()
            .filter(|k| k.once())
            .map(|k| k.as_str())
            .collect();
        assert_eq!(once, ["spec-check", "spec-review", "series"]);
        for key in GateKey::ALL {
            let json = serde_json::to_string(&key).expect("json");
            assert_eq!(json, format!("\"{}\"", key.as_str()));
        }
    }
}
