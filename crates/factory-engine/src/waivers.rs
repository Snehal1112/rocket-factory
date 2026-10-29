//! Waivers: when the user approves a job over open review blockers, the blockers are recorded in
//! `waivers.json` and, once the job has merged, filed as tickets. Every ticket number is stored, so
//! a re-run never files one twice.

use std::path::Path;

use factory_core::{open_blockers, write_atomic, Stage};
use serde::{Deserialize, Serialize};

use crate::ports::short_sha;
use crate::tickets::TicketsReport;
use crate::{Driver, EngineError};

/// The file name inside the job folder.
pub const WAIVERS_FILE: &str = "waivers.json";

/// One waiver: the blockers the user accepted, and the ticket filed for each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiver {
    pub head: String,
    pub round: u32,
    pub file: String,
    pub reason: String,
    pub at: u64,
    pub blockers: Vec<String>,
    /// One slot per blocker. `None` until its ticket is filed.
    pub tickets: Vec<Option<u32>>,
}

/// The open blockers of a review reply, or the whole trimmed reply as one entry when none is tagged.
pub fn waived_blockers(review_text: &str) -> Vec<String> {
    let found = open_blockers(review_text);
    if found.is_empty() {
        vec![review_text.trim().to_string()]
    } else {
        found
    }
}

/// The waivers of a job, oldest first. A missing or broken file reads as empty.
pub fn read_waivers(job_dir: &Path) -> Vec<Waiver> {
    std::fs::read_to_string(job_dir.join(WAIVERS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_waivers(job_dir: &Path, list: &[Waiver]) -> Result<(), EngineError> {
    let text = serde_json::to_string_pretty(list).map_err(|e| EngineError::Msg(e.to_string()))?;
    write_atomic(&job_dir.join(WAIVERS_FILE), text.as_bytes())?;
    Ok(())
}

/// Appends a waiver. The same head and review file as the last entry is a retry and adds nothing.
pub fn record_waiver(job_dir: &Path, waiver: Waiver) -> Result<(), EngineError> {
    let mut list = read_waivers(job_dir);
    if list
        .last()
        .is_some_and(|last| last.head == waiver.head && last.file == waiver.file)
    {
        return Ok(());
    }
    list.push(waiver);
    write_waivers(job_dir, &list)
}

/// Files one ticket for every waived blocker that has none yet. With `dry_run` it only lists them.
/// A tracker failure is reported, never raised, and never changes the merge.
pub fn waiver_tickets_step(
    driver: &Driver,
    id: &str,
    dry_run: bool,
) -> Result<TicketsReport, EngineError> {
    let job = driver.job(id)?;
    let mut report = TicketsReport::default();
    if job.stage != Stage::Merged {
        report.skipped = true;
        report
            .lines
            .push(format!("skipped: job {id} is not merged"));
        return Ok(report);
    }
    let dir = driver.job_dir(&job);
    let mut list = read_waivers(&dir);
    if list.is_empty() {
        report.skipped = true;
        report
            .lines
            .push("skipped: the job has no waivers".to_string());
        return Ok(report);
    }
    let base = driver.repo().base_branch().to_string();
    let tip = driver.repo().tip(&base).unwrap_or_default();
    let sha = short_sha(&tip).to_string();
    let tracker = driver.tracker();
    'all: for w in 0..list.len() {
        for b in 0..list[w].blockers.len() {
            if list[w].tickets.get(b).copied().flatten().is_some() {
                continue;
            }
            let blocker = list[w].blockers[b].clone();
            let head: String = blocker.chars().take(80).collect();
            let title = format!("Waived review finding from job {id}: {head}");
            if dry_run {
                report.lines.push(format!("would create: {title}"));
                continue;
            }
            let body = format!(
                "{blocker}\n\nWaived by the user: {}\n\nJob {id} merged at {sha}. Review file {}, round {}.",
                list[w].reason, list[w].file, list[w].round
            );
            match tracker.create(&title, &body, &[]) {
                Ok(number) => {
                    let n = list[w].blockers.len();
                    list[w].tickets.resize(n, None);
                    list[w].tickets[b] = Some(number);
                    write_waivers(&dir, &list)?;
                    report.lines.push(format!("created #{number}: {title}"));
                }
                Err(e) => {
                    report.failed = true;
                    report.lines.push(format!("failed: create `{title}`: {e}"));
                    break 'all;
                }
            }
        }
    }
    if !dry_run && !report.lines.is_empty() {
        let state = if report.failed { "failed" } else { "done" };
        let text = format!("waiver tickets {state}: {}", report.lines.join("; "));
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Err(e) = driver.ledger(&job).ruling(&one_line) {
            tracing::error!(event = "step.error", step = "waivers_ledger", msg = %format!("waiver ledger error for job {id}: {e}"));
        }
    }
    Ok(report)
}
