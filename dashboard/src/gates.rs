//! Reads the driver's `gates.jsonl` for the dashboard. The server never writes it.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Job;
use crate::paths::Paths;

/// The file the driver writes into a job folder.
pub const GATES_FILE: &str = "gates.jsonl";

/// Most bytes read from the end of the file.
pub const TAIL_BYTES: u64 = 512 * 1024;

/// The newest line version this reader knows.
const LINE_VERSION: u32 = 1;

/// The 18 gates with their stage groups, in card order.
const KEYS: [(&str, &str); 18] = [
    ("spec-check", "spec"),
    ("spec-review", "spec"),
    ("series", "start"),
    ("machine", "start"),
    ("builder-guard", "build"),
    ("builder", "build"),
    ("checks", "checks"),
    ("format", "checks"),
    ("lint", "checks"),
    ("no-checks", "checks"),
    ("no-tests", "checks"),
    ("budget", "review"),
    ("diff-cap", "review"),
    ("review", "review"),
    ("risk", "review"),
    ("final-gate", "merge"),
    ("merge", "merge"),
    ("approve", "merge"),
];

/// The gates that run once per job, not once per round.
const ONCE: [&str; 3] = ["spec-check", "spec-review", "series"];

/// The result strings the driver writes.
const RESULTS: [&str; 7] = [
    "pass", "fail", "note", "waived", "skipped", "running", "waiting",
];

/// One gate as the dashboard gets it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GateView {
    pub gate: String,
    pub stage: String,
    pub state: String,
    pub detail: String,
    pub round: u32,
    pub history: String,
}

/// All 18 gates of a job, plus the gate the job stopped at.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GatesView {
    pub list: Vec<GateView>,
    pub stopped_at: Option<String>,
}

/// One good line of the file. Fields the dashboard does not show are dropped.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RawLine {
    pub v: u32,
    pub gate: String,
    pub result: String,
    pub round: u32,
    #[serde(default)]
    pub detail: String,
}

/// `<sdd>/<dir>/gates.jsonl`, or `None` when `dir` could point outside the sdd folder.
pub fn gates_path(paths: &Paths, dir: &str) -> Option<PathBuf> {
    crate::park::park_path(paths, dir).map(|p| p.with_file_name(GATES_FILE))
}

/// The last `TAIL_BYTES` of the file as text. A cut drops the first partial line.
fn read_tail(path: &std::path::Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let cut = len > TAIL_BYTES;
    if cut {
        file.seek(SeekFrom::Start(len - TAIL_BYTES)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    // A lossy decode keeps the good lines when the cut lands inside a character.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !cut {
        return Some(text);
    }
    Some(match text.split_once('\n') {
        Some((_, rest)) => rest.to_string(),
        None => String::new(),
    })
}

/// The good lines of a text. Broken, newer-version, unknown-gate and unknown-result lines are skipped.
fn parse_lines(text: &str) -> Vec<RawLine> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<RawLine>(l).ok())
        .filter(|l| l.v <= LINE_VERSION)
        .filter(|l| KEYS.iter().any(|(k, _)| *k == l.gate))
        .filter(|l| RESULTS.contains(&l.result.as_str()))
        .collect()
}

/// The gates of a job, or `None` when it has no record.
pub fn load_gates(paths: &Paths, job: &Job) -> Option<GatesView> {
    let path = gates_path(paths, &job.dir)?;
    let text = read_tail(&path)?;
    Some(latest(&parse_lines(&text), job))
}

/// True when a `running` or `waiting` result no longer describes a live job.
fn is_stale(gate: &str, result: &str, job: &Job) -> bool {
    let ended = job.stage == "merged" || job.cancelled == Some(true);
    match result {
        "running" => ended || job.stage == "needs-human",
        "waiting" => {
            let live = matches!(gate, "series" | "machine");
            let working = matches!(job.stage.as_str(), "spec" | "build" | "review");
            ended || (working && !live)
        }
        _ => false,
    }
}

/// The state of every gate for the job's current round.
pub fn latest(lines: &[RawLine], job: &Job) -> GatesView {
    let round = job.round.max(1);
    let list = KEYS
        .iter()
        .map(|(key, stage)| {
            let once = ONCE.contains(key);
            let own = || lines.iter().filter(|l| l.gate == *key);
            let current = if once {
                own().next_back()
            } else {
                own().rfind(|l| l.round == round)
            };
            let mut history = Vec::new();
            if !once {
                // The last line of each earlier round, oldest first.
                let mut earlier: Vec<&RawLine> = Vec::new();
                for line in own().filter(|l| l.round < round) {
                    match earlier.iter().position(|e| e.round == line.round) {
                        Some(i) => earlier[i] = line,
                        None => earlier.push(line),
                    }
                }
                earlier.sort_by_key(|l| l.round);
                history = earlier
                    .iter()
                    .map(|l| format!("{} r{}", l.result, l.round))
                    .collect();
            }
            let (state, detail, line_round) = match current {
                Some(l) if is_stale(key, &l.result, job) => {
                    ("interrupted".to_string(), l.detail.clone(), l.round)
                }
                Some(l) => (l.result.clone(), l.detail.clone(), l.round),
                None => ("not-reached".to_string(), String::new(), 0),
            };
            GateView {
                gate: (*key).to_string(),
                stage: (*stage).to_string(),
                state,
                detail,
                round: line_round,
                history: history.join(", "),
            }
        })
        .collect();
    let stopped_at = lines
        .iter()
        .rfind(|l| l.round == round && l.result == "fail")
        .map(|l| l.gate.clone());
    GatesView { list, stopped_at }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn job_at(stage: &str, round: u32) -> Job {
        serde_json::from_value(serde_json::json!({
            "id": "001",
            "dir": "2026-11-22-001-x",
            "slug": "x",
            "created": "2026-11-22",
            "feature": "feature",
            "branch": "factory/001-x",
            "stage": stage,
            "round": round,
            "stage_since": 0.0,
            "reviews": {},
        }))
        .expect("job")
    }

    fn raw(gate: &str, result: &str, round: u32) -> RawLine {
        RawLine {
            v: 1,
            gate: gate.to_string(),
            result: result.to_string(),
            round,
            detail: String::new(),
        }
    }

    fn state_of(view: &GatesView, gate: &str) -> String {
        view.list
            .iter()
            .find(|g| g.gate == gate)
            .expect("row")
            .state
            .clone()
    }

    #[test]
    fn first_round_lines_count_for_a_board_round_of_zero() {
        let job = job_at("build", 0);
        let lines = vec![raw("builder-guard", "pass", 1)];
        let view = latest(&lines, &job);
        let guard = view
            .list
            .iter()
            .find(|g| g.gate == "builder-guard")
            .expect("row");
        assert_eq!(guard.state, "pass");
        assert_eq!(view.list.len(), 18);
    }

    #[test]
    fn once_gates_keep_their_last_line_across_rounds() {
        let view = latest(&[raw("series", "pass", 1)], &job_at("build", 3));
        assert_eq!(state_of(&view, "series"), "pass");
    }

    #[test]
    fn a_round_gate_from_an_earlier_round_is_not_reached() {
        let view = latest(&[raw("checks", "fail", 1)], &job_at("build", 2));
        let checks = view.list.iter().find(|g| g.gate == "checks").expect("row");
        assert_eq!(checks.state, "not-reached");
        assert_eq!(checks.history, "fail r1");
    }

    #[test]
    fn the_last_line_of_the_round_wins() {
        let lines = vec![raw("checks", "fail", 2), raw("checks", "pass", 2)];
        let view = latest(&lines, &job_at("build", 2));
        assert_eq!(state_of(&view, "checks"), "pass");
    }

    #[test]
    fn waiting_in_review_is_stale_and_checks_is_no_stage() {
        assert!(is_stale("checks", "waiting", &job_at("review", 1)));
        assert!(!is_stale("series", "waiting", &job_at("review", 1)));
        assert!(!is_stale("checks", "waiting", &job_at("checks", 1)));
    }

    #[test]
    fn stale_running_and_waiting_are_interrupted() {
        let mut cancelled = job_at("build", 1);
        cancelled.cancelled = Some(true);
        let cases = [
            (raw("review", "running", 1), job_at("merged", 1)),
            (raw("review", "running", 1), cancelled),
            (raw("review", "running", 1), job_at("needs-human", 1)),
            (raw("approve", "waiting", 1), job_at("build", 1)),
        ];
        for (line, job) in cases {
            let gate = line.gate.clone();
            let view = latest(&[line], &job);
            assert_eq!(
                state_of(&view, &gate),
                "interrupted",
                "{gate} {}",
                job.stage
            );
        }
    }

    #[test]
    fn live_waiting_states_stay() {
        let build = job_at("build", 1);
        for gate in ["machine", "series"] {
            let view = latest(&[raw(gate, "waiting", 1)], &build);
            assert_eq!(state_of(&view, gate), "waiting");
        }
        let view = latest(&[raw("approve", "waiting", 1)], &job_at("needs-human", 1));
        assert_eq!(state_of(&view, "approve"), "waiting");
    }

    #[test]
    fn stopped_at_is_the_last_fail_of_the_round() {
        let lines = vec![raw("machine", "fail", 2), raw("checks", "fail", 2)];
        let view = latest(&lines, &job_at("needs-human", 2));
        assert_eq!(view.stopped_at.as_deref(), Some("checks"));
        let none = latest(&[raw("checks", "fail", 1)], &job_at("build", 2));
        assert_eq!(none.stopped_at, None);
    }

    #[test]
    fn no_file_gives_null() -> TestResult {
        let root = tempfile::tempdir()?;
        let paths = Paths::for_root(root.path());
        let job = job_at("build", 1);
        assert!(load_gates(&paths, &job).is_none());
        let view = crate::state::build_state(
            &crate::model::Board {
                jobs: vec![job],
                ..Default::default()
            },
            &crate::events::Registry::new(),
            10.0,
            None,
            &paths,
        );
        let json = serde_json::to_string(&view)?;
        assert!(json.contains("\"gates\":null"), "{json}");
        Ok(())
    }

    #[test]
    fn a_bad_dir_reads_nothing() {
        let paths = Paths::for_root(std::path::Path::new("/tmp/none"));
        assert!(gates_path(&paths, "../x").is_none());
        assert!(gates_path(&paths, "a/b").is_none());
        assert!(gates_path(&paths, "").is_none());
        assert!(gates_path(&paths, "ok-dir").is_some());
    }

    #[test]
    fn unknown_versions_gates_and_results_are_skipped() -> TestResult {
        let root = tempfile::tempdir()?;
        let paths = Paths::for_root(root.path());
        let job = job_at("build", 1);
        let dir = paths.sdd.join(&job.dir);
        std::fs::create_dir_all(&dir)?;
        let text = concat!(
            "{not json\n",
            "{\"v\":2,\"gate\":\"checks\",\"result\":\"fail\",\"round\":1,\"at\":1,\"detail\":\"\"}\n",
            "{\"v\":1,\"gate\":\"no-such\",\"result\":\"fail\",\"round\":1,\"at\":1,\"detail\":\"\"}\n",
            "{\"v\":1,\"gate\":\"lint\",\"result\":\"odd\",\"round\":1,\"at\":1,\"detail\":\"\"}\n",
            "{\"v\":1,\"gate\":\"format\",\"result\":\"pass\",\"round\":1,\"at\":1,\"detail\":\"clean\"}\n",
        );
        std::fs::write(dir.join(GATES_FILE), text)?;
        let view = load_gates(&paths, &job).expect("view");
        assert_eq!(state_of(&view, "format"), "pass");
        assert_eq!(state_of(&view, "checks"), "not-reached");
        assert_eq!(state_of(&view, "lint"), "not-reached");
        let format = view.list.iter().find(|g| g.gate == "format").expect("row");
        assert_eq!(format.detail, "clean");
        Ok(())
    }

    #[test]
    fn only_the_tail_is_read() -> TestResult {
        let root = tempfile::tempdir()?;
        let paths = Paths::for_root(root.path());
        let job = job_at("build", 1);
        let dir = paths.sdd.join(&job.dir);
        std::fs::create_dir_all(&dir)?;
        let mut file = std::fs::File::create(dir.join(GATES_FILE))?;
        let filler = "{\"v\":1,\"gate\":\"lint\",\"result\":\"fail\",\"round\":1,\"at\":1,\"detail\":\"filler filler filler filler\"}\n";
        for _ in 0..(600 * 1024 / filler.len() + 1) {
            file.write_all(filler.as_bytes())?;
        }
        file.write_all(b"{\"v\":1,\"gate\":\"merge\",\"result\":\"pass\",\"round\":1,\"at\":2,\"detail\":\"merged\"}\n")?;
        drop(file);
        let view = load_gates(&paths, &job).expect("view");
        assert_eq!(state_of(&view, "merge"), "pass");
        assert_eq!(state_of(&view, "lint"), "fail");
        Ok(())
    }
}
