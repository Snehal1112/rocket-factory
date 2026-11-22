//! The engine side of the gate record: the best-effort writer and the park-kind map.

use std::path::Path;

use factory_core::gate_record::{append, read, GateKey, GateLine, GateResult};
use factory_core::{Job, ParkKind, Stage};

use crate::ports::{now_whole_secs, short_sha};
use crate::runloop::{user_decisions, Validated};
use crate::{Driver, Engine};

/// Which fail line a park writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateTarget {
    /// The gate follows from the park kind, or from the stage for a park of kind `Other`.
    FromKind,
    /// The fail line goes under this gate.
    Key(GateKey),
    /// No fail line; only the `approve: waiting` line, if the park offers approve.
    NoFailLine,
}

/// The gate and result of the line a park writes, or `None` when the park writes no fail line.
/// `stage_before` is the job's stage before the park moved it to needs-human.
pub(crate) fn fail_key(
    kind: ParkKind,
    stage_before: Option<Stage>,
    target: GateTarget,
) -> Option<(GateKey, GateResult)> {
    let key = match target {
        GateTarget::Key(key) => return Some((key, GateResult::Fail)),
        GateTarget::NoFailLine => return None,
        GateTarget::FromKind => match kind {
            ParkKind::BuilderStopped
            | ParkKind::BuilderBlocked
            | ParkKind::BuilderCommitted
            | ParkKind::NothingChanged
            | ParkKind::CommitFailed => GateKey::Builder,
            ParkKind::BuilderTampered => GateKey::BuilderGuard,
            ParkKind::ChecksFailed => GateKey::Checks,
            ParkKind::NoChecks => GateKey::NoChecks,
            ParkKind::NoTests => GateKey::NoTests,
            ParkKind::DiffTooLarge => GateKey::DiffCap,
            ParkKind::BudgetStopped => GateKey::Budget,
            ParkKind::RiskyDiff => GateKey::Risk,
            ParkKind::MachineWaiting => GateKey::Machine,
            ParkKind::SeriesWaiting => return Some((GateKey::Series, GateResult::Waiting)),
            // The review role loop already wrote the review fail line.
            ParkKind::ReviewFailed | ParkKind::ChangesRemain => return None,
            ParkKind::Other => match stage_before? {
                Stage::Build => GateKey::Builder,
                Stage::Review => GateKey::Review,
                Stage::Approve => GateKey::Merge,
                _ => return None,
            },
        },
    };
    Some((key, GateResult::Fail))
}

/// True when one of the park's next commands is a `factory approve`.
pub(crate) fn offers_approve(next: &[String]) -> bool {
    next.iter().any(|n| n.starts_with("factory approve"))
}

/// Appends one line to the job folder, and logs a warning when that fails.
fn write_line(dir: &Path, job_id: &str, line: &GateLine) {
    if let Err(e) = append(dir, line) {
        tracing::warn!(event = "gates.record_error", job = %job_id, error = %e);
    }
}

impl Engine<'_> {
    /// Records one gate result when the project's `gate_record` setting is on. It never fails.
    pub(crate) fn gate(&self, key: GateKey, result: GateResult, detail: &str) {
        self.gate_lines(&[(key, result, detail)]);
    }

    /// Records several gate results with one board read when the project's `gate_record` setting
    /// is on. It never fails.
    pub(crate) fn gate_lines(&self, lines: &[(GateKey, GateResult, &str)]) {
        if !self.config.gate_record {
            return;
        }
        // A board that cannot be read gives round 1 and no head, as a park does.
        let job = self.job().ok();
        let round = job.as_ref().map_or(1, |j| j.round);
        let head = job
            .as_ref()
            .and_then(|j| j.head.as_deref())
            .map(|h| short_sha(h).to_string());
        let at = now_whole_secs();
        for (key, result, detail) in lines {
            let line = GateLine::new(*key, *result, round, head.clone(), at, detail);
            write_line(&self.job_dir, &self.job_id, &line);
        }
    }

    /// Records a `machine` line only when its result differs from the last `machine` line of this round.
    pub(crate) fn gate_machine(&self, result: GateResult, detail: &str) {
        if !self.config.gate_record {
            return;
        }
        let round = self.job().map_or(1, |j| j.round.max(1));
        let last = read(&self.job_dir)
            .into_iter()
            .rev()
            .find(|l| l.gate == GateKey::Machine && l.round == round);
        if last.is_some_and(|l| l.result == result) {
            return;
        }
        self.gate(GateKey::Machine, result, detail);
    }
}

/// Records one gate result for driver code when the project's `gate_record` setting is on. It never fails.
pub fn record_gate(driver: &Driver, job: &Job, key: GateKey, result: GateResult, detail: &str) {
    if !driver.config().gate_record {
        return;
    }
    let head = job.head.as_deref().map(|h| short_sha(h).to_string());
    let line = GateLine::new(key, result, job.round, head, now_whole_secs(), detail);
    write_line(&driver.job_dir(job), &job.id, &line);
}

/// Records `spec-check` and `spec-review` in the folder of each new job. `validated` is `None` when
/// the job has no plan. `spec_review_on` is the project setting, not whether a review was found.
pub(crate) fn record_spec_gates(
    driver: &Driver,
    jobs: &[Job],
    validated: Option<&Validated>,
    spec_review_on: bool,
) {
    if !driver.config().gate_record {
        return;
    }
    let (check, review) = spec_gate_lines(driver, validated, spec_review_on);
    for job in jobs {
        record_gate(driver, job, GateKey::SpecCheck, check.0, &check.1);
        if let Some((result, detail)) = &review {
            record_gate(driver, job, GateKey::SpecReview, *result, detail);
        }
    }
}

type GateText = (GateResult, String);

/// The `spec-check` line, and the `spec-review` line when one is due.
fn spec_gate_lines(
    driver: &Driver,
    validated: Option<&Validated>,
    spec_review_on: bool,
) -> (GateText, Option<GateText>) {
    let skipped = |why: &str| (GateResult::Skipped, why.to_string());
    let Some(v) = validated else {
        return (skipped("no plan"), Some(skipped("no plan")));
    };
    let Some(text) = v.spec_text.as_deref() else {
        return (skipped("no spec"), Some(skipped("no spec")));
    };
    let check = match &v.accepted {
        Some(reason) => (GateResult::Waived, reason.clone()),
        None => {
            // An intake spec gets no decision warning, so it counts none.
            let decisions = usize::from(!v.intake && user_decisions(text) == 0);
            (
                GateResult::Pass,
                format!(
                    "{} plan warnings, {decisions} decision warnings",
                    v.plan_warnings.len()
                ),
            )
        }
    };
    if !spec_review_on {
        return (check, Some(skipped("spec review is off")));
    }
    if !v.review_ran {
        return (check, Some(skipped("no review ran")));
    }
    // A review that did not finish leaves no line, so the gate shows as not reached.
    let review = driver.last_spec_review().and_then(|info| {
        let stem = info
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match info.verdict {
            "approved" => Some((GateResult::Pass, stem)),
            _ if v.accepted.is_some() => Some((GateResult::Waived, stem)),
            _ => None,
        }
    });
    (check, review)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::park::tests::with_engine_config;

    #[test]
    fn park_kinds_map_to_their_gates() {
        use GateResult::*;
        let cases = [
            (
                ParkKind::BuilderStopped,
                None,
                Some((GateKey::Builder, Fail)),
            ),
            (
                ParkKind::BuilderBlocked,
                None,
                Some((GateKey::Builder, Fail)),
            ),
            (
                ParkKind::BuilderCommitted,
                None,
                Some((GateKey::Builder, Fail)),
            ),
            (
                ParkKind::NothingChanged,
                None,
                Some((GateKey::Builder, Fail)),
            ),
            (ParkKind::CommitFailed, None, Some((GateKey::Builder, Fail))),
            (
                ParkKind::BuilderTampered,
                None,
                Some((GateKey::BuilderGuard, Fail)),
            ),
            (ParkKind::ChecksFailed, None, Some((GateKey::Checks, Fail))),
            (ParkKind::NoChecks, None, Some((GateKey::NoChecks, Fail))),
            (ParkKind::NoTests, None, Some((GateKey::NoTests, Fail))),
            (ParkKind::DiffTooLarge, None, Some((GateKey::DiffCap, Fail))),
            (ParkKind::BudgetStopped, None, Some((GateKey::Budget, Fail))),
            (ParkKind::RiskyDiff, None, Some((GateKey::Risk, Fail))),
            (
                ParkKind::SeriesWaiting,
                None,
                Some((GateKey::Series, Waiting)),
            ),
            (
                ParkKind::MachineWaiting,
                None,
                Some((GateKey::Machine, Fail)),
            ),
            (ParkKind::ReviewFailed, None, None),
            (ParkKind::ChangesRemain, None, None),
            (
                ParkKind::Other,
                Some(Stage::Build),
                Some((GateKey::Builder, Fail)),
            ),
            (
                ParkKind::Other,
                Some(Stage::Review),
                Some((GateKey::Review, Fail)),
            ),
            (
                ParkKind::Other,
                Some(Stage::Approve),
                Some((GateKey::Merge, Fail)),
            ),
            (ParkKind::Other, Some(Stage::Spec), None),
            (ParkKind::Other, None, None),
        ];
        for (kind, stage, want) in cases {
            assert_eq!(
                fail_key(kind, stage, GateTarget::FromKind),
                want,
                "{kind:?} {stage:?}"
            );
        }
        assert_eq!(
            fail_key(
                ParkKind::Other,
                Some(Stage::Review),
                GateTarget::Key(GateKey::Merge)
            ),
            Some((GateKey::Merge, Fail))
        );
        assert_eq!(
            fail_key(ParkKind::Other, Some(Stage::Review), GateTarget::NoFailLine),
            None
        );
    }

    /// Runs `body` on an engine over a board with job 001 in the given round.
    fn with_job(gate_record: bool, round: u32, body: impl FnOnce(&Engine, &Path)) {
        let config = factory_core::Config {
            gate_record,
            min_free_disk_mb: 0,
            ..factory_core::Config::default()
        };
        with_engine_config(
            config,
            |board_path| {
                let mut job = Job::new("001", "job", "s", "2026-11-22", "f", 1.0);
                job.round = round;
                crate::BoardStore::new(board_path, std::time::Duration::from_secs(1))
                    .update(|board| {
                        board.jobs.push(job);
                        Ok(())
                    })
                    .expect("save board");
            },
            |engine, job_dir, _| body(engine, job_dir),
        );
    }

    #[test]
    fn gate_lines_write_every_line_once() {
        with_job(true, 1, |engine, job_dir| {
            engine.gate_lines(&[
                (GateKey::Format, GateResult::Pass, "a"),
                (GateKey::Lint, GateResult::Pass, "b"),
            ]);
            let lines = read(job_dir);
            let gates: Vec<GateKey> = lines.iter().map(|l| l.gate).collect();
            assert_eq!(gates, [GateKey::Format, GateKey::Lint]);
        });
    }

    #[test]
    fn gate_lines_write_nothing_when_off() {
        with_job(false, 1, |engine, job_dir| {
            engine.gate_lines(&[(GateKey::Format, GateResult::Pass, "a")]);
            assert!(!job_dir.join(factory_core::gate_record::GATES_FILE).exists());
        });
    }

    #[test]
    fn record_gate_keeps_round_zero_as_one() {
        with_job(true, 0, |engine, job_dir| {
            engine.gate(GateKey::Format, GateResult::Pass, "a");
            let lines = read(job_dir);
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].round, 1);
        });
    }

    #[test]
    fn approve_is_offered_only_by_a_factory_approve_command() {
        assert!(offers_approve(&[
            "factory rework 001".to_string(),
            "factory approve 001 --without-review".to_string(),
        ]));
        assert!(!offers_approve(&["factory rework 001".to_string()]));
        assert!(!offers_approve(&[]));
    }
}
