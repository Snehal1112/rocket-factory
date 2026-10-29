use factory_checks::{plan_checks, run_checks, write_checks_md, ChecksReport};
use factory_core::{
    classify_paths, code_paths_of, has_test_marker, has_test_path, Config, DiffClass, ParkKind,
    ParkPart, Stage,
};

use crate::decide::fix_rounds_used;
use crate::park::part_from_file;

const NO_TESTS_NOTE: &str = "no test file or test marker in a code diff";

/// None unless a code diff ran no check. A risky diff is left to `decide()`.
pub(crate) fn no_checks_reason(
    paths: &[String],
    report_is_empty: bool,
    config: &Config,
) -> Option<String> {
    if !report_is_empty || classify_paths(paths, config) == DiffClass::Risky {
        return None;
    }
    let code = code_paths_of(paths, config);
    let n = code.len();
    if n == 0 {
        return None;
    }
    Some(format!("no check ran for {}", code_path_list(&code)))
}

/// `N changed code path(s): a, b, ...` with at most five names and a count of the rest.
fn code_path_list(code: &[String]) -> String {
    let n = code.len();
    let noun = if n == 1 { "path" } else { "paths" };
    let mut text = format!(
        "{n} changed code {noun}: {}",
        code.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
    );
    if n > 5 {
        text.push_str(&format!(" (+{} more)", n - 5));
    }
    text
}

/// True when the diff has a code path and neither a test path nor an added test marker.
/// `diff_text` is None when the diff file could not be read, which counts as no marker.
pub(crate) fn no_tests_added(paths: &[String], diff_text: Option<&str>, config: &Config) -> bool {
    if code_paths_of(paths, config).is_empty() || has_test_path(paths, config) {
        return false;
    }
    !diff_text.is_some_and(|t| has_test_marker(t, config))
}
use crate::ports::{git_err, io_err, now};
use crate::{Engine, EngineError, StageOutcome};

/// Logs one `check.run` event per command. The command goes through `redact_command`, the output tail is never logged.
fn log_check_results(report: &ChecksReport) {
    for r in &report.results {
        // A passing summary is taken from the command output, so only a failure's exit or timeout note is logged.
        let summary = if r.passed { "" } else { r.summary.as_str() };
        tracing::info!(
            event = "check.run",
            command = %factory_telemetry::redact_command(&r.command),
            passed = r.passed,
            summary = summary
        );
    }
}

impl Engine<'_> {
    /// Runs the mapped checks over the whole job diff and writes `checks.md`.
    pub(crate) fn run_checks_now(&self) -> Result<ChecksReport, EngineError> {
        let (base, head) = self.job_range()?;
        let raw = self.repo.changed_paths(&base, &head).map_err(git_err)?;
        let plan = plan_checks(&self.repo.scoped_paths(&raw), &self.config);
        let report = run_checks(&self.repo.workdir_in(&self.worktree()?), &plan);
        log_check_results(&report);
        write_checks_md(&self.job_dir.join("checks.md"), &report)
            .map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(report)
    }

    /// The paths changed by the whole job diff, relative to the project folder.
    pub(crate) fn job_changed_paths(&self) -> Result<Vec<String>, EngineError> {
        Ok(self.repo.scoped_paths(&self.job_changed_paths_raw()?))
    }

    /// The paths changed by the whole job diff, relative to the repo root.
    pub(crate) fn job_changed_paths_raw(&self) -> Result<Vec<String>, EngineError> {
        let (base, head) = self.job_range()?;
        self.repo.changed_paths(&base, &head).map_err(git_err)
    }

    /// The commits that hold the whole job diff. A subfolder project starts at the merge base with
    /// the base branch, because a sync moves the head onto main and leaves the recorded base behind.
    /// Other projects keep the recorded base.
    pub(crate) fn job_range(&self) -> Result<(String, String), EngineError> {
        let job = self.job()?;
        let base = job
            .base
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        let head = job
            .head
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no head".into()))?;
        let from = self.range_start(&base, &head)?;
        Ok((from, head))
    }

    /// The start of the whole job diff for the given base and head, see `job_range`.
    pub(crate) fn range_start(&self, base: &str, head: &str) -> Result<String, EngineError> {
        if self.repo.subpath().is_none() {
            return Ok(base.to_string());
        }
        self.repo
            .merge_base(self.repo.base_branch(), head)
            .map_err(git_err)
    }

    /// The no-checks gate: a park kind and reason when a code diff ran no check.
    pub(crate) fn gate_checks(
        &self,
        paths: &[String],
        report: &ChecksReport,
    ) -> Option<(ParkKind, String)> {
        no_checks_reason(paths, report.results.is_empty(), &self.config)
            .map(|r| (ParkKind::NoChecks, r))
    }

    /// Parks a job that a gate stopped, with the checks and the reason as evidence.
    pub(crate) fn park_gate(
        &self,
        kind: ParkKind,
        reason: String,
    ) -> Result<StageOutcome, EngineError> {
        self.ruling(&format!("parked, {reason}"))?;
        let mut parts: Vec<ParkPart> = part_from_file(&self.job_dir, "checks.md", "Checks")
            .into_iter()
            .collect();
        parts.push(ParkPart::new("Why it stopped", "driver", &reason));
        self.park_with(kind, reason, parts)
    }

    /// Runs the mapped checks over the diff from `from` to `to` and writes `out` in the job dir. It returns
    /// `None`, and writes nothing, when no check is mapped to the changed paths.
    pub(crate) fn run_checks_between(
        &self,
        from: &str,
        to: &str,
        out: &str,
    ) -> Result<Option<ChecksReport>, EngineError> {
        let raw = self.repo.changed_paths(from, to).map_err(git_err)?;
        let plan = plan_checks(&self.repo.scoped_paths(&raw), &self.config);
        if plan.commands.is_empty() {
            return Ok(None);
        }
        let report = run_checks(&self.repo.workdir_in(&self.worktree()?), &plan);
        log_check_results(&report);
        write_checks_md(&self.job_dir.join(out), &report)
            .map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(Some(report))
    }

    /// Starts a fix round with `findings`, or parks the job when the rounds are used up.
    pub(crate) fn fix_or_park(
        &self,
        findings: &str,
        reason: &str,
        kind: ParkKind,
        source: &str,
    ) -> Result<StageOutcome, EngineError> {
        let job = self.job()?;
        let used = fix_rounds_used(job.round);
        if used >= self.config.fix_rounds {
            let title = if kind == ParkKind::ChecksFailed {
                "Failing checks"
            } else {
                "Reviewer findings"
            };
            let part = ParkPart::new(title, source, findings);
            return self.park_with(
                kind,
                format!("{reason} after {used} fix rounds"),
                vec![part],
            );
        }
        let next = job.round + 1;
        std::fs::write(self.job_dir.join(format!("changes-r{next}.md")), findings)
            .map_err(io_err)?;
        // A fix round moves review back to build. That is a backward move, so it is forced and logged as a ruling.
        // The source tells the builder step what to do when the fix round changes nothing.
        let fix_source = if kind == ParkKind::ChecksFailed {
            "checks"
        } else {
            "review"
        };
        let escalate = crate::escalation::escalation_for(
            used,
            self.config.escalate_after_fix_rounds,
            self.config.escalation_model.as_deref(),
            self.job_model().as_deref(),
            job.escalated_model.as_deref(),
        );
        let newly = escalate
            .clone()
            .filter(|m| Some(m) != job.escalated_model.as_ref());
        self.update(|j| {
            j.round = next;
            j.fix_source = Some(fix_source.to_string());
            if escalate.is_some() {
                j.escalated_model = escalate;
            }
            j.reset_reviews();
            crate::ports::set_stage_logged(j, Stage::Build, None, true, now())
        })?;
        self.ruling(&format!("fix round {next}, {reason}"))?;
        if let Some(model) = newly {
            self.ruling(&format!(
                "escalated the builder to {model} (fix round {next}, {reason})"
            ))?;
        }
        Ok(StageOutcome::Advanced(Stage::Build))
    }

    /// Runs the checks. On a failure the job goes back to `build` with the trimmed output.
    pub fn checks_stage(&self) -> Result<StageOutcome, EngineError> {
        // Files outside the project folder park the job before any check runs.
        let job = self.job()?;
        if let Some(head) = job.head.as_deref() {
            if let Some(reason) = crate::scope::scope_violation(self.repo, head)? {
                return self.park_gate(ParkKind::Other, reason);
            }
        }
        let report = self.run_checks_now()?;
        let round = self.job()?.round;
        if report.all_passed() {
            self.ruling(&format!("checks round {round} pass"))?;
            let paths = self.job_changed_paths()?;
            if let Some((kind, reason)) = self.gate_checks(&paths, &report) {
                return self.park_gate(kind, reason);
            }
            {
                let (base, head) = self.job_range()?;
                let whole = self
                    .repo
                    .diff_to_files(&base, &head, &self.job_dir)
                    .map_err(git_err)?;
                let text = std::fs::read_to_string(&whole.diff).ok();
                if no_tests_added(&paths, text.as_deref(), &self.config) {
                    if self.config.require_tests {
                        let code = code_paths_of(&paths, &self.config);
                        let reason = format!("no test was added in {}", code_path_list(&code));
                        return self.park_gate(ParkKind::NoTests, reason);
                    }
                    self.ruling(NO_TESTS_NOTE)?;
                }
            }
            return Ok(StageOutcome::Advanced(Stage::Review));
        }
        self.ruling(&format!("checks round {round} fail"))?;
        let findings = format!(
            "The checks failed. Fix these failures only.\n\n{}",
            report.failure_text()
        );
        self.fix_or_park(&findings, "checks failed", ParkKind::ChecksFailed, "checks")
    }
}

pub fn checks_stage(e: &Engine) -> Result<StageOutcome, EngineError> {
    e.checks_stage()
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_checks_reason_names_five_paths_and_a_count() {
        let all = paths(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs"]);
        let reason = no_checks_reason(&all, true, &Config::default()).expect("a reason");
        assert_eq!(
            reason,
            "no check ran for 7 changed code paths: a.rs, b.rs, c.rs, d.rs, e.rs (+2 more)"
        );
    }

    #[test]
    fn one_path_reads_singular() {
        let reason =
            no_checks_reason(&paths(&["a.rs"]), true, &Config::default()).expect("a reason");
        assert_eq!(reason, "no check ran for 1 changed code path: a.rs");
    }

    #[test]
    fn a_report_with_results_is_never_gated() {
        assert!(no_checks_reason(&paths(&["a.rs"]), false, &Config::default()).is_none());
    }

    #[test]
    fn docs_only_and_empty_path_lists_are_not_gated() {
        assert!(no_checks_reason(
            &paths(&["README.md", "docs/a.md"]),
            true,
            &Config::default()
        )
        .is_none());
        assert!(no_checks_reason(&[], true, &Config::default()).is_none());
    }

    #[test]
    fn no_tests_added_is_true_for_a_code_diff_with_neither_path_nor_marker() {
        assert!(no_tests_added(
            &paths(&["src/a.rs"]),
            Some("+fn a() {}\n"),
            &Config::default()
        ));
    }

    #[test]
    fn a_test_path_or_a_marker_or_docs_only_is_not_flagged() {
        let c = Config::default();
        assert!(!no_tests_added(
            &paths(&["src/a.rs", "tests/a.rs"]),
            Some("+x\n"),
            &c
        ));
        assert!(!no_tests_added(
            &paths(&["src/a.rs"]),
            Some("+#[cfg(test)]\n"),
            &c
        ));
        assert!(!no_tests_added(&paths(&["docs/a.md"]), Some("+x\n"), &c));
        assert!(!no_tests_added(&[], Some("+x\n"), &c));
    }

    #[test]
    fn an_unreadable_diff_counts_as_no_marker() {
        assert!(no_tests_added(
            &paths(&["src/a.rs"]),
            None,
            &Config::default()
        ));
        assert!(!no_tests_added(
            &paths(&["tests/a.rs", "src/a.rs"]),
            None,
            &Config::default()
        ));
    }

    #[test]
    fn a_risky_diff_is_left_to_decide() {
        let all = paths(&["src-tauri/src/lib.rs"]);
        assert!(no_checks_reason(&all, true, &Config::default()).is_none());
    }
}
