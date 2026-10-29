use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use factory_core::write_atomic;

use crate::plan::CheckPlan;
use crate::CheckError;

const TAIL_LINES: usize = 60;
const SUMMARY_CHARS: usize = 100;

/// The outcome of one check command.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckResult {
    pub command: String,
    pub passed: bool,
    /// A short one-line result. For a failure it is the exit status or timeout note.
    pub summary: String,
    /// The last 60 lines of output. Empty for a passing command.
    pub tail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChecksReport {
    pub results: Vec<CheckResult>,
}

impl ChecksReport {
    /// An empty report passes.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }

    pub fn failures(&self) -> Vec<&CheckResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    /// The text handed to the builder: each failing command and its trimmed output.
    pub fn failure_text(&self) -> String {
        let mut out = String::new();
        for r in self.failures() {
            out.push_str(&format!("## {} ({})\n{}\n\n", r.command, r.summary, r.tail));
        }
        out
    }
}

/// Keeps the last `max_lines` lines and notes how many were dropped.
pub fn trim_tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    if lines.len() <= max_lines {
        return lines.join("\n");
    }
    let dropped = lines.len() - max_lines;
    format!(
        "... {dropped} earlier lines omitted\n{}",
        lines[dropped..].join("\n")
    )
}

fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(SUMMARY_CHARS).collect()
}

/// The counts of one `test result:` line, as passed, failed and ignored.
fn test_counts(line: &str) -> Option<(u64, u64, u64)> {
    let rest = line.split_once("test result:")?.1;
    let count = |word: &str| -> Option<u64> {
        let part = rest.split(';').find(|p| p.trim_end().ends_with(word))?;
        part.split_whitespace().rev().nth(1)?.parse().ok()
    };
    Some((count("passed")?, count("failed")?, count("ignored")?))
}

/// Sums every `test result:` line, one per test binary. A single line is returned as it is.
fn summed_test_result(lines: &[&str]) -> Option<String> {
    let results: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains("test result:"))
        .collect();
    match results.as_slice() {
        [] => None,
        [only] => Some((**only).to_string()),
        many => {
            let counts: Option<Vec<(u64, u64, u64)>> =
                many.iter().map(|l| test_counts(l)).collect();
            let Some(counts) = counts else {
                return many.last().map(|l| (**l).to_string());
            };
            let sum = |f: fn(&(u64, u64, u64)) -> u64| counts.iter().map(f).sum::<u64>();
            let status = if many.iter().all(|l| l.contains("test result: ok.")) {
                "ok."
            } else {
                "FAILED."
            };
            Some(format!(
                "test result: {status} {} passed; {} failed; {} ignored across {} binaries",
                sum(|c| c.0),
                sum(|c| c.1),
                sum(|c| c.2),
                many.len()
            ))
        }
    }
}

/// Picks the line that best says what a passing command did.
fn pass_summary(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let summed = summed_test_result(&lines);
    let pick = summed.as_deref().or_else(|| lines.last().copied());
    match pick {
        Some(l) => one_line(l),
        None => "ok".to_string(),
    }
}

fn read_all<R: Read>(reader: Option<R>) -> String {
    let mut buf = Vec::new();
    if let Some(mut r) = reader {
        let _ = r.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn failed(command: &str, summary: String, tail: String) -> CheckResult {
    CheckResult {
        command: command.to_string(),
        passed: false,
        summary,
        tail,
    }
}

fn run_one(worktree: &Path, command: &str, timeout: Duration) -> CheckResult {
    let mut tokens = command.split_whitespace();
    let Some(program) = tokens.next() else {
        return failed(
            command,
            "could not start: empty command".into(),
            String::new(),
        );
    };
    let mut child = match Command::new(program)
        .args(tokens)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return failed(command, format!("could not start: {e}"), String::new()),
    };
    let out_reader = child.stdout.take();
    let err_reader = child.stderr.take();
    let out_thread = thread::spawn(move || read_all(out_reader));
    let err_thread = thread::spawn(move || read_all(err_reader));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                return failed(command, format!("could not wait: {e}"), String::new());
            }
        }
    };
    let stdout: String = out_thread.join().unwrap_or_default();
    let stderr: String = err_thread.join().unwrap_or_default();
    // Stderr goes last because compilers print their errors there.
    let text = format!("{stdout}{stderr}");
    match status {
        None => failed(
            command,
            format!("timed out after {}s", timeout.as_secs()),
            trim_tail(&text, TAIL_LINES),
        ),
        Some(s) if s.success() => CheckResult {
            command: command.to_string(),
            passed: true,
            summary: pass_summary(&text),
            tail: String::new(),
        },
        Some(s) => {
            let code = s
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            failed(
                command,
                format!("exit {code}"),
                trim_tail(&text, TAIL_LINES),
            )
        }
    }
}

/// Runs each command with the worktree as the working directory.
pub fn run_checks(worktree: &Path, plan: &CheckPlan) -> ChecksReport {
    ChecksReport {
        results: plan
            .commands
            .iter()
            .map(|c| run_one(worktree, c, plan.timeout))
            .collect(),
    }
}

/// Writes `checks.md`. This is the evidence format that `can-auto-merge` reads.
pub fn write_checks_md(path: &Path, report: &ChecksReport) -> Result<(), CheckError> {
    let mut body = String::from("# Checks\n\n");
    if report.results.is_empty() {
        body.push_str("PASS none (no check is mapped to the changed paths)\n");
    }
    for r in &report.results {
        let word = if r.passed { "PASS" } else { "FAIL" };
        body.push_str(&format!(
            "{word} {} ({})\n",
            r.command,
            one_line(&r.summary)
        ));
    }
    write_atomic(path, body.as_bytes()).map_err(|e| CheckError::Write(e.to_string()))
}

/// Appends plan notes to `checks.md`. Only a `FAIL` line fails the checks, so a note never changes the verdict.
pub fn append_notes(path: &Path, notes: &[String]) -> Result<(), CheckError> {
    if notes.is_empty() {
        return Ok(());
    }
    let mut body = std::fs::read_to_string(path).map_err(|e| CheckError::Write(e.to_string()))?;
    if !body.ends_with('\n') {
        body.push('\n');
    }
    for note in notes {
        body.push_str(&one_line(note));
        body.push('\n');
    }
    write_atomic(path, body.as_bytes()).map_err(|e| CheckError::Write(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::CheckPlan;
    use std::time::Duration;

    fn plan(cmds: &[&str], secs: u64) -> CheckPlan {
        CheckPlan {
            commands: cmds.iter().map(|s| s.to_string()).collect(),
            timeout: Duration::from_secs(secs),
            notes: Vec::new(),
        }
    }

    #[test]
    fn passing_and_failing_commands() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = run_checks(
            dir.path(),
            &plan(&["true", "false", "echo hello world"], 10),
        );
        assert_eq!(r.results.len(), 3);
        assert!(r.results[0].passed);
        assert!(!r.results[1].passed);
        assert!(r.results[2].passed);
        assert_eq!(r.results[2].summary, "hello world");
        assert!(!r.all_passed());
        assert_eq!(r.failures().len(), 1);
    }

    #[test]
    fn a_cargo_test_run_sums_every_test_result_line() {
        let out = "running 2 tests\ntest result: ok. 100 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.10s\n\nrunning 3 tests\ntest result: ok. 51 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s\n\n   Doc-tests foo\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        assert_eq!(
            pass_summary(out),
            "test result: ok. 151 passed; 0 failed; 1 ignored across 3 binaries"
        );
    }

    #[test]
    fn a_single_test_result_line_is_kept_as_it_is() {
        let out = "running 1 test\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
        assert!(
            pass_summary(out).starts_with("test result: ok. 5 passed; 0 failed"),
            "{}",
            pass_summary(out)
        );
        assert_eq!(pass_summary("a\nb"), "b");
    }

    #[test]
    fn missing_program_is_a_failure_not_a_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = run_checks(
            dir.path(),
            &plan(&["definitely-not-a-real-program-xyz"], 10),
        );
        assert!(!r.results[0].passed);
        assert!(r.results[0].summary.contains("could not start"));
    }

    #[test]
    fn timeout_is_a_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = run_checks(dir.path(), &plan(&["sleep 5"], 1));
        assert!(!r.results[0].passed);
        assert!(r.results[0].summary.contains("timed out"));
    }

    #[test]
    fn runs_in_the_worktree() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("marker.txt"), "x").expect("write");
        let r = run_checks(dir.path(), &plan(&["ls marker.txt"], 10));
        assert!(r.results[0].passed);
    }

    #[test]
    fn trim_keeps_the_last_lines() {
        let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let out = trim_tail(&text, 60);
        assert_eq!(out.lines().count(), 61);
        assert!(out.starts_with("... 40 earlier lines omitted"));
        assert!(out.ends_with("line 100"));
        assert_eq!(trim_tail("a\nb\n", 60), "a\nb");
    }

    #[test]
    fn failure_text_names_each_failing_command() {
        let report = ChecksReport {
            results: vec![
                CheckResult {
                    command: "cargo check -j4".into(),
                    passed: false,
                    summary: "exit 101".into(),
                    tail: "error[E0432]".into(),
                },
                CheckResult {
                    command: "yarn check".into(),
                    passed: true,
                    summary: "ok".into(),
                    tail: String::new(),
                },
            ],
        };
        let text = report.failure_text();
        assert!(text.contains("cargo check -j4 (exit 101)"));
        assert!(text.contains("error[E0432]"));
        assert!(!text.contains("yarn check"));
    }

    #[test]
    fn checks_md_has_one_evidence_line_per_command() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("checks.md");
        let report = ChecksReport {
            results: vec![
                CheckResult {
                    command: "cargo check -j4".into(),
                    passed: true,
                    summary: "ok".into(),
                    tail: String::new(),
                },
                CheckResult {
                    command: "yarn tsc --noEmit".into(),
                    passed: false,
                    summary: "exit 2\nsecond line".into(),
                    tail: "TS2304".into(),
                },
            ],
        };
        write_checks_md(&path, &report).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.starts_with("# Checks\n\n"));
        assert!(text.contains("\nPASS cargo check -j4 (ok)\n"));
        assert!(text.contains("\nFAIL yarn tsc --noEmit (exit 2 second line)\n"));
        assert!(!text.contains("TS2304"));
    }

    #[test]
    fn notes_are_appended_without_a_fail_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("checks.md");
        write_checks_md(&path, &ChecksReport { results: vec![] }).expect("write");
        append_notes(&path, &["SKIP dependents over the cap: a, b".to_string()]).expect("append");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.ends_with("\nSKIP dependents over the cap: a, b\n"),
            "{text}"
        );
        assert!(!text.contains("FAIL"));
    }

    #[test]
    fn empty_report_still_records_a_pass_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("checks.md");
        write_checks_md(&path, &ChecksReport { results: vec![] }).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("PASS none (no check is mapped to the changed paths)"));
    }
}
