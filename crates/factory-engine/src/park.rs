//! Parks a job and records why, with the evidence, in `park.json`.

use std::path::Path;

use factory_core::{
    classify_paths, open_blockers, park_summary, DiffClass, ParkKind, ParkPart, ParkRecord,
};

use crate::decide::risk_summary;
use crate::ports::now;
use crate::{Engine, EngineError, StageOutcome};

/// The reviewer's open `[blocker]` lines, or the whole reply when it has none.
pub(crate) fn blocker_lines(review_text: &str) -> String {
    let blockers = open_blockers(review_text);
    if blockers.is_empty() {
        review_text.to_string()
    } else {
        blockers.join("\n")
    }
}

/// The file that holds the risky paths of the first review of a job.
pub(crate) const RISKY_PATHS_FILE: &str = "risky-paths.txt";

/// Rewrites a ledger line `Ruling: review round N (source) verdict V, B blockers` as `round N (source): v, B blockers`.
/// Lines from older drivers have no source or count, and show `?` for them.
fn round_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("Ruling: ")?;
    let rest = rest
        .strip_prefix("sync review round ")
        .or_else(|| rest.strip_prefix("review round "))?;
    let (round, rest) = rest.split_once(' ')?;
    let (source, rest) = match rest.strip_prefix('(') {
        Some(r) => {
            let (src, after) = r.split_once(')')?;
            (src, after.trim_start())
        }
        None => ("?", rest),
    };
    let rest = rest.strip_prefix("verdict ")?;
    let (verdict, count) = match rest.split_once(", ") {
        Some((v, c)) => (v, c.strip_suffix(" blockers").unwrap_or("?")),
        None => (rest, "?"),
    };
    Some(format!(
        "round {round} ({source}): {}, {count} blockers",
        verdict.to_lowercase()
    ))
}

/// An evidence part read from a file in the job folder. A missing file gives no part.
pub(crate) fn part_from_file(dir: &Path, file: &str, title: &str) -> Option<ParkPart> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    Some(ParkPart::new(title, file, &text))
}

/// The evidence list, or one `Why it stopped` part with the reason when it is empty.
pub(crate) fn evidence_or_reason(parts: Vec<ParkPart>, reason: &str) -> Vec<ParkPart> {
    // No park keeps an empty evidence list.
    if parts.is_empty() {
        vec![ParkPart::new("Why it stopped", "driver", reason)]
    } else {
        parts
    }
}

/// Removes `park.json` from `dir`. A missing file is fine, any other error is logged.
fn remove_record(dir: &Path) {
    let path = dir.join(factory_core::PARK_FILE);
    if let Err(err) = std::fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(event = "park.record_error", path = %path.display(), msg = %format!("park record could not be removed: {err}"));
        }
    }
}

/// Replaces `park.json` in `dir`. The old record goes first, so a failed write leaves none behind.
pub(crate) fn write_record(dir: &Path, record: &ParkRecord) {
    remove_record(dir);
    // A job folder that cannot be written must not stop the park, so a failure is only logged.
    if let Err(err) = record.write(dir) {
        tracing::warn!(event = "park.record_error", dir = %dir.display(), msg = %format!("park record could not be written: {err}"));
    }
}

/// Runs the park writes in order: clear, board update, stage move, record, ruling.
/// The clear step removes the record of an earlier park first. It stops at the first error.
/// The record is written only after the update and the stage move succeed, and stays when the ruling fails.
pub(crate) fn in_park_order(
    clear: impl FnOnce(),
    update: impl FnOnce() -> Result<(), EngineError>,
    stage: impl FnOnce() -> Result<(), EngineError>,
    record: impl FnOnce(),
    ruling: impl FnOnce() -> Result<(), EngineError>,
) -> Result<(), EngineError> {
    clear();
    update()?;
    stage()?;
    record();
    ruling()
}

/// The Risk part for a job diff that could not be read.
fn risk_unknown_part(err: &EngineError) -> ParkPart {
    ParkPart::new(
        "Risk",
        "driver",
        &format!("unknown: the job diff could not be read ({err})"),
    )
}

/// The review part, then the risk part: from `risk` when the paths were read, else the unknown part.
fn lead_parts_from(
    paths: Result<Vec<String>, EngineError>,
    review: Option<ParkPart>,
    risk: impl FnOnce(&[String]) -> Option<ParkPart>,
) -> Vec<ParkPart> {
    let mut parts: Vec<ParkPart> = review.into_iter().collect();
    match paths {
        Ok(paths) => parts.extend(risk(&paths)),
        Err(err) => parts.push(risk_unknown_part(&err)),
    }
    parts
}

/// The kind, reason and Checks note of a risky park after a sync, from the final checks.
/// Passing checks keep the risky kind. A failing run parks as checks-failed. Checks that could not run
/// keep the risky kind and give a note for the Checks part.
fn sync_park_choice(
    checks: &Result<factory_checks::ChecksReport, EngineError>,
    reason: &str,
) -> (ParkKind, String, Option<String>) {
    match checks {
        Ok(report) if report.all_passed() => (ParkKind::RiskyDiff, reason.to_string(), None),
        Ok(_) => (
            ParkKind::ChecksFailed,
            "final checks failed after sync".to_string(),
            None,
        ),
        Err(err) => (
            ParkKind::RiskyDiff,
            reason.to_string(),
            Some(format!("final checks could not run: {err}")),
        ),
    }
}

/// The reason a sync park reports. A park that was written gives its own reason, because the final
/// checks can change it. A park that failed keeps `text` and names the error.
pub(crate) fn sync_park_text(parked: Result<StageOutcome, EngineError>, text: &str) -> String {
    match parked {
        Ok(StageOutcome::NeedsHuman(reason)) => reason,
        Ok(_) => text.to_string(),
        Err(e) => format!("{text} (the park card could not be written: {e})"),
    }
}

impl Engine<'_> {
    /// Parks a job after a sync that needs a human. It runs the final checks again, so the card carries
    /// Review, Risk, Rounds, Changed files and Checks. A failing check parks as checks-failed with no fix round.
    pub(crate) fn park_after_sync(&self, reason: &str) -> Result<StageOutcome, EngineError> {
        let checks = self.final_checks();
        let (kind, reason, note) = sync_park_choice(&checks, reason);
        let mut parts = self.lead_parts_for_card();
        parts.extend(self.rounds_part());
        parts.extend(self.stat_part());
        match note {
            Some(note) => parts.push(ParkPart::new("Checks", "driver", &note)),
            None => parts.extend(part_from_file(&self.job_dir, "checks.md", "Checks")),
        }
        self.park_with(kind, reason, parts)
    }

    /// Records why the job parks, then parks it. Writing `park.json` is best effort.
    pub(crate) fn park_with(
        &self,
        kind: ParkKind,
        reason: String,
        parts: Vec<ParkPart>,
    ) -> Result<StageOutcome, EngineError> {
        // A park must still happen when the board cannot be read, so a missing job falls back to round 1.
        let job = self.job().ok();
        let round = job.as_ref().map_or(1, |j| j.round);
        let review_missing = job.as_ref().is_some_and(|j| j.review_missing);
        let at = now().max(0.0) as u64;
        let parts = evidence_or_reason(parts, &reason);
        let mut record = ParkRecord::new(kind, &reason, &self.job_id, round, at, parts);
        // A park that offers approve says so when the review has not run, because a plain approve is refused.
        record.verdict = job
            .as_ref()
            .and_then(|j| j.last_review.as_ref())
            .map(|r| r.verdict.clone());
        record.next = kind.next_for(&self.job_id, review_missing, record.verdict.as_deref());
        if self.is_risky() {
            record.risky = Some(true);
        }
        let summary = park_summary(&reason, &record.parts);
        in_park_order(
            || remove_record(&self.job_dir),
            || {
                self.update(|j| {
                    j.park_kind = Some(kind.as_str().to_string());
                    j.park_at = Some(at);
                    j.park_summary = Some(summary);
                    Ok(())
                })
            },
            || self.move_to_needs_human(&reason),
            || write_record(&self.job_dir, &record),
            || self.ruling(&format!("needs-human, {reason}")),
        )?;
        Ok(StageOutcome::NeedsHuman(reason))
    }

    /// Evidence for a park the decision step chose. Anything that cannot be read is left out.
    pub(crate) fn decision_parts(&self, kind: ParkKind, reason: &str) -> Vec<ParkPart> {
        // The changed paths are read only by the kinds that lead with Review and Risk.
        let lead = || self.lead_parts_for_card();
        match kind {
            ParkKind::ChecksFailed => part_from_file(&self.job_dir, "checks.md", "Checks")
                .into_iter()
                .collect(),
            ParkKind::ChangesRemain => {
                let mut parts = lead();
                parts.extend(self.rounds_part());
                parts.extend(self.stat_part());
                parts
            }
            ParkKind::RiskyDiff => {
                let mut parts = lead();
                if parts.is_empty() {
                    parts.push(ParkPart::new("Why it stopped", "driver", reason));
                }
                parts.extend(self.rounds_part());
                parts.extend(self.stat_part());
                parts
            }
            ParkKind::DiffTooLarge => {
                let mut parts = vec![ParkPart::new("Why it stopped", "driver", reason)];
                parts.extend(self.stat_part());
                parts
            }
            ParkKind::NoChecks | ParkKind::NoTests => {
                let mut parts = lead();
                parts.extend(part_from_file(&self.job_dir, "checks.md", "Checks"));
                parts.push(ParkPart::new("Why it stopped", "driver", reason));
                parts
            }
            ParkKind::BudgetStopped => {
                let text = format!(
                    "{} weighted tokens used against a budget of {}",
                    self.job_weighted_tokens(),
                    self.config.budget_weighted
                );
                vec![ParkPart::new("Budget", "board", &text)]
            }
            _ => Vec::new(),
        }
    }

    /// True when the whole job diff touches a risky path. A diff that cannot be read counts as not risky.
    pub(crate) fn is_risky(&self) -> bool {
        self.job_changed_paths()
            .is_ok_and(|p| classify_paths(&p, &self.config) == DiffClass::Risky)
    }

    /// The Review part, then the Risk part when the diff is risky.
    pub(crate) fn lead_parts(&self, paths: &[String]) -> Vec<ParkPart> {
        let mut parts: Vec<ParkPart> = self.review_part().into_iter().collect();
        parts.extend(self.risk_part(paths));
        parts
    }

    /// The lead parts of a card. A diff that cannot be read gives a `Risk` part that says so.
    pub(crate) fn lead_parts_for_card(&self) -> Vec<ParkPart> {
        lead_parts_from(self.job_changed_paths(), self.review_part(), |paths| {
            self.risk_part(paths)
        })
    }

    /// The last review: its verdict line first, then the open blockers.
    pub(crate) fn review_part(&self) -> Option<ParkPart> {
        let last = self.job().ok()?.last_review?;
        let head = format!("verdict {}, round {}", last.verdict, last.round);
        let reply = last
            .path_in(&self.job_dir)
            .and_then(|path| std::fs::read_to_string(path).ok());
        let text = match reply {
            Some(reply) if last.verdict == "approve" => {
                let open = open_blockers(&reply);
                if open.is_empty() {
                    head
                } else {
                    format!("{head}\n{}", open.join("\n"))
                }
            }
            Some(reply) => format!("{head}\n{}", blocker_lines(&reply)),
            None => format!("verdict {}, findings file missing", last.verdict),
        };
        Some(ParkPart::new("Review", &last.file, &text))
    }

    /// The risky paths, and the ones fix rounds added after the first review. `None` when the diff is not risky.
    pub(crate) fn risk_part(&self, paths: &[String]) -> Option<ParkPart> {
        if classify_paths(paths, &self.config) != DiffClass::Risky {
            return None;
        }
        let mut text = risk_summary(paths, &self.config);
        if let Ok(first) = std::fs::read_to_string(self.job_dir.join(RISKY_PATHS_FILE)) {
            let added: Vec<&str> = paths
                .iter()
                .filter(|p| {
                    classify_paths(std::slice::from_ref(*p), &self.config) == DiffClass::Risky
                })
                .map(String::as_str)
                .filter(|p| !first.lines().any(|l| l == *p))
                .collect();
            if !added.is_empty() {
                text.push_str(&format!("\nadded by fix rounds: {}", added.join(", ")));
            }
        }
        Some(ParkPart::new("Risk", "driver", &text))
    }

    /// One line per review round in the ledger.
    pub(crate) fn rounds_part(&self) -> Option<ParkPart> {
        let ledger = std::fs::read_to_string(self.job_dir.join("progress.md")).ok()?;
        let lines: Vec<String> = ledger.lines().filter_map(round_line).collect();
        if lines.is_empty() {
            return None;
        }
        Some(ParkPart::new("Rounds", "progress.md", &lines.join("\n")))
    }

    /// The `Changed files` part, when the diff stat can be made.
    fn stat_part(&self) -> Option<ParkPart> {
        let stat = self.diff_stat_text()?;
        Some(ParkPart::new("Changed files", "diff stat", &stat))
    }

    /// The `--stat` text of the whole job diff, or `None` when the diff cannot be made.
    fn diff_stat_text(&self) -> Option<String> {
        let job = self.job().ok()?;
        let (base, head) = (job.base?, job.head?);
        let base = self.range_start(&base, &head).ok()?;
        let files = self.repo.diff_to_files(&base, &head, &self.job_dir).ok()?;
        std::fs::read_to_string(files.stat).ok()
    }
}

/// Evidence for a builder that stopped: the stop reason, the error if any, and its last message.
pub(crate) fn builder_stop_parts(run: &factory_agent::AgentRun) -> Vec<ParkPart> {
    let mut text = format!("Stop reason: {}", run.stop_reason.as_str());
    if let Some(error) = &run.error {
        text.push_str(&format!("\nError: {error}"));
    }
    if !run.result_text.trim().is_empty() {
        text.push_str(&format!(
            "\n\nLast message from the builder:\n{}",
            run.result_text
        ));
    }
    vec![ParkPart::new("Builder stopped", "builder", &text)]
}

/// Evidence for a builder that reported BLOCKED or NEEDS_CONTEXT: the status, its text and the full reply.
pub(crate) fn builder_blocked_parts(
    status: &factory_core::BuilderStatus,
    run: &factory_agent::AgentRun,
) -> Vec<ParkPart> {
    let (label, why) = match status {
        factory_core::BuilderStatus::NeedsContext(why) => ("NEEDS_CONTEXT", why),
        factory_core::BuilderStatus::Blocked(why) => ("BLOCKED", why),
        _ => return Vec::new(),
    };
    let mut text = format!("Status: {label}\nReason: {why}");
    if !run.result_text.trim().is_empty() {
        text.push_str(&format!(
            "\n\nLast message from the builder:\n{}",
            run.result_text
        ));
    }
    vec![ParkPart::new("Builder blocked", "builder", &text)]
}

#[cfg(test)]
mod tests {
    use super::*;

    type Order = std::cell::RefCell<Vec<&'static str>>;

    fn step(order: &Order, name: &'static str) -> Result<(), EngineError> {
        order.borrow_mut().push(name);
        Ok(())
    }

    #[test]
    fn park_steps_run_clear_then_update_then_stage_then_record_then_ruling() {
        let order = Order::default();
        in_park_order(
            || order.borrow_mut().push("clear"),
            || step(&order, "update"),
            || step(&order, "stage"),
            || order.borrow_mut().push("record"),
            || step(&order, "ruling"),
        )
        .expect("parked");
        assert_eq!(
            *order.borrow(),
            vec!["clear", "update", "stage", "record", "ruling"]
        );
    }

    #[test]
    fn a_failed_board_update_leaves_no_park_record() {
        let order = Order::default();
        let err = in_park_order(
            || {},
            || Err(EngineError::Msg("board locked".into())),
            || step(&order, "stage"),
            || order.borrow_mut().push("record"),
            || step(&order, "ruling"),
        )
        .expect_err("update failed");
        assert_eq!(err.to_string(), "board locked");
        assert!(order.borrow().is_empty());
    }

    #[test]
    fn a_failed_stage_move_leaves_no_park_record() {
        let order = Order::default();
        let err = in_park_order(
            || {},
            || step(&order, "update"),
            || Err(EngineError::Msg("stage failed".into())),
            || order.borrow_mut().push("record"),
            || step(&order, "ruling"),
        )
        .expect_err("stage failed");
        assert_eq!(err.to_string(), "stage failed");
        assert_eq!(*order.borrow(), vec!["update"]);
    }

    #[test]
    fn a_failed_ruling_leaves_the_park_record() {
        let order = Order::default();
        let err = in_park_order(
            || {},
            || step(&order, "update"),
            || step(&order, "stage"),
            || order.borrow_mut().push("record"),
            || Err(EngineError::Msg("ledger locked".into())),
        )
        .expect_err("ruling failed");
        assert_eq!(err.to_string(), "ledger locked");
        assert_eq!(*order.borrow(), vec!["update", "stage", "record"]);
    }

    #[test]
    fn an_unreadable_diff_gets_a_risk_unknown_part() {
        let review = Some(ParkPart::new(
            "Review",
            "review-code-r1.md",
            "verdict approve, round 1",
        ));
        let err = EngineError::Msg("job has no head".into());
        let parts = lead_parts_from(Err(err), review, |_| None);
        let titles: Vec<&str> = parts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["Review", "Risk"]);
        assert_eq!(parts[1].source, "driver");
        assert_eq!(
            parts[1].text,
            "unknown: the job diff could not be read (job has no head)"
        );
    }

    #[test]
    fn sync_park_text_uses_the_park_reason() {
        let parked = Ok(StageOutcome::NeedsHuman(
            "final checks failed after sync".into(),
        ));
        assert_eq!(
            sync_park_text(parked, "after sync: risky diff"),
            "final checks failed after sync"
        );
    }

    #[test]
    fn sync_park_text_keeps_the_text_for_other_outcomes() {
        let parked = Ok(StageOutcome::Advanced(factory_core::Stage::Review));
        assert_eq!(
            sync_park_text(parked, "after sync: risky diff"),
            "after sync: risky diff"
        );
    }

    #[test]
    fn a_failed_sync_park_keeps_the_risky_text_and_names_the_error() {
        let parked = Err(EngineError::Msg("board locked".into()));
        assert_eq!(
            sync_park_text(parked, "after sync: risky diff"),
            "after sync: risky diff (the park card could not be written: board locked)"
        );
    }

    fn report(passed: bool) -> factory_checks::ChecksReport {
        factory_checks::ChecksReport {
            results: vec![factory_checks::CheckResult {
                command: "cargo check".into(),
                passed,
                summary: if passed { "ok".into() } else { "exit 1".into() },
                tail: String::new(),
            }],
        }
    }

    #[test]
    fn passing_final_checks_keep_the_risky_kind() {
        let (kind, reason, note) =
            sync_park_choice(&Ok(report(true)), "after sync: risky diff: a.rs");
        assert_eq!(kind, ParkKind::RiskyDiff);
        assert_eq!(reason, "after sync: risky diff: a.rs");
        assert_eq!(note, None);
    }

    #[test]
    fn failing_final_checks_park_as_checks_failed() {
        let (kind, reason, note) =
            sync_park_choice(&Ok(report(false)), "after sync: risky diff: a.rs");
        assert_eq!(kind, ParkKind::ChecksFailed);
        assert_eq!(reason, "final checks failed after sync");
        assert_eq!(note, None);
    }

    #[test]
    fn final_checks_that_cannot_run_keep_the_risky_kind_and_say_why() {
        let err: Result<factory_checks::ChecksReport, EngineError> =
            Err(EngineError::Check("no worktree".into()));
        let (kind, reason, note) = sync_park_choice(&err, "after sync: risky diff: a.rs");
        assert_eq!(kind, ParkKind::RiskyDiff);
        assert_eq!(reason, "after sync: risky diff: a.rs");
        assert_eq!(
            note.as_deref(),
            Some("final checks could not run: checks: no worktree")
        );
    }

    #[test]
    fn a_readable_diff_keeps_the_lead_parts() {
        let review = Some(ParkPart::new("Review", "r.md", "verdict approve, round 1"));
        let parts = lead_parts_from(Ok(vec!["a.rs".to_string()]), review, |paths| {
            assert_eq!(paths, ["a.rs"]);
            Some(ParkPart::new("Risk", "driver", "risky: a.rs"))
        });
        let titles: Vec<&str> = parts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["Review", "Risk"]);
        assert_eq!(parts[1].text, "risky: a.rs");
    }

    /// Runs `body` with an engine whose board file does not exist, so every board read and write fails.
    fn with_unreadable_board(body: impl FnOnce(&Engine, &Path)) {
        let dir = tempfile::tempdir().expect("temp dir");
        git2::Repository::init(dir.path()).expect("init repo");
        let repo = factory_git::Repo::open(dir.path()).expect("open repo");
        let runner = factory_agent::FakeRunner::new(Vec::new());
        let events = factory_agent::EventSink::new(dir.path().join("events.jsonl"), "001");
        let job_dir = dir.path().join("job");
        std::fs::create_dir(&job_dir).expect("job dir");
        let engine = Engine::new(
            dir.path().join("board.json"),
            job_dir.clone(),
            "001".to_string(),
            job_dir.join("plan.md"),
            &repo,
            &runner,
            factory_core::Config::default(),
            &events,
        )
        .expect("engine");
        body(&engine, &job_dir);
    }

    #[test]
    fn a_failed_stage_move_removes_the_record_of_an_earlier_park() {
        with_unreadable_board(|engine, job_dir| {
            let first = ParkRecord::new(ParkKind::ChecksFailed, "first", "001", 1, 7, Vec::new());
            write_record(job_dir, &first);
            assert!(job_dir.join(factory_core::PARK_FILE).exists());
            engine
                .park_with(ParkKind::Other, "second".to_string(), Vec::new())
                .expect_err("the park fails");
            assert!(!job_dir.join(factory_core::PARK_FILE).exists());
        });
    }

    #[test]
    fn round_line_renders_round_sync_and_old_lines() {
        assert_eq!(
            round_line("Ruling: sync review round 1 (sync) verdict Approve, 0 blockers"),
            Some("round 1 (sync): approve, 0 blockers".to_string())
        );
        assert_eq!(
            round_line("Ruling: review round 2 (rework) verdict Changes, 3 blockers"),
            Some("round 2 (rework): changes, 3 blockers".to_string())
        );
        assert_eq!(
            round_line("Ruling: review round 2 verdict Changes"),
            Some("round 2 (?): changes, ? blockers".to_string())
        );
        assert_eq!(round_line("Task 1: complete"), None);
    }

    #[test]
    fn builder_stop_parts_carry_the_stop_reason_and_the_last_message() {
        let mut run = factory_agent::AgentRun::failed("boom");
        run.result_text = "I could not find the file.".to_string();
        run.stop_reason = factory_agent::StopReason::TurnLimit;
        let parts = builder_stop_parts(&run);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].text.contains("turn-limit"), "{}", parts[0].text);
        assert!(
            parts[0].text.contains("I could not find the file."),
            "{}",
            parts[0].text
        );
    }

    #[test]
    fn builder_stop_parts_without_a_message_still_name_the_reason() {
        let run = factory_agent::AgentRun::failed("boom");
        let parts = builder_stop_parts(&run);
        assert!(parts[0].text.contains("failed"), "{}", parts[0].text);
        assert!(parts[0].text.contains("boom"), "{}", parts[0].text);
    }

    #[test]
    fn blocker_lines_keeps_only_the_blockers() {
        let text = "Summary line\n[blocker] src/a.rs:1 wrong\n[minor] nit\n[blocker] src/b.rs:2 bad\nVERDICT: CHANGES";
        assert_eq!(
            blocker_lines(text),
            "[blocker] src/a.rs:1 wrong\n[blocker] src/b.rs:2 bad"
        );
    }

    #[test]
    fn blocker_lines_falls_back_to_the_whole_reply() {
        let text = "Please rework this\nVERDICT: CHANGES";
        assert_eq!(blocker_lines(text), text);
    }

    #[test]
    fn part_from_file_reads_the_file_and_skips_a_missing_one(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("checks.md"), "FAIL cargo check")?;
        let p = part_from_file(dir.path(), "checks.md", "Checks");
        assert_eq!(p.map(|p| p.text), Some("FAIL cargo check".to_string()));
        assert!(part_from_file(dir.path(), "missing.md", "Nope").is_none());
        Ok(())
    }

    #[test]
    fn a_failed_write_never_leaves_the_old_record_behind() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let old = ParkRecord::new(ParkKind::Other, "old park", "job", 1, 1, Vec::new());
        old.write(dir.path())?;
        assert!(dir.path().join(factory_core::PARK_FILE).exists());
        // A directory in the temp file's place makes the new write fail.
        let tmp = dir.path().join(format!(
            ".{}.{}.tmp",
            factory_core::PARK_FILE,
            std::process::id()
        ));
        std::fs::create_dir(&tmp)?;
        let new = ParkRecord::new(ParkKind::ChecksFailed, "new park", "job", 2, 2, Vec::new());
        write_record(dir.path(), &new);
        assert!(!dir.path().join(factory_core::PARK_FILE).exists());
        assert!(ParkRecord::read(dir.path()).is_none());
        Ok(())
    }
}
