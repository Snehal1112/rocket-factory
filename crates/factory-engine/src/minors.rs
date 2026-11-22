//! Minor tickets: after a merge, the open `[minor]` lines of the last review are filed as one
//! checklist ticket. `minors.json` records the number, so a re-run never files a second one.

use std::path::Path;

use factory_core::{open_minors, write_atomic, Stage};

use crate::ports::short_sha;
use crate::tickets::TicketsReport;
use crate::{Driver, EngineError};

/// The file name inside the job folder.
pub const MINORS_FILE: &str = "minors.json";

fn skipped(why: String) -> TicketsReport {
    TicketsReport {
        skipped: true,
        lines: vec![format!("skipped: {why}")],
        ..TicketsReport::default()
    }
}

/// The bare names of the review files of `round` that exist, in read order: the round review, the
/// rebase review, the sync reviews by number, then `last_file` when it is none of those.
pub(crate) fn round_review_files(job_dir: &Path, round: u32, last_file: &str) -> Vec<String> {
    let sync_prefix = format!("review-code-r{round}-sync");
    let mut syncs: Vec<u32> = std::fs::read_dir(job_dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let digits = name.strip_prefix(&sync_prefix)?.strip_suffix(".md")?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()
        })
        .collect();
    syncs.sort_unstable();
    let mut names = vec![
        format!("review-code-r{round}.md"),
        format!("review-code-r{round}-rebase.md"),
    ];
    names.extend(syncs.iter().map(|k| format!("{sync_prefix}{k}.md")));
    // Only the last component is joined, so a hand-edited board cannot point outside the folder.
    if let Some(last) = Path::new(last_file).file_name() {
        let last = last.to_string_lossy().into_owned();
        if !names.contains(&last) {
            names.push(last);
        }
    }
    names.retain(|name| job_dir.join(name).is_file());
    names
}

/// Files one ticket for the open minor findings of the last review. With `dry_run` it only lists it.
/// A tracker failure is reported, never raised, and never changes the merge.
pub fn minor_tickets_step(
    driver: &Driver,
    id: &str,
    dry_run: bool,
) -> Result<TicketsReport, EngineError> {
    let job = driver.job(id)?;
    if !driver.config().minor_tickets {
        return Ok(skipped("minor_tickets is off".to_string()));
    }
    if job.stage != Stage::Merged {
        return Ok(skipped(format!("job {id} is not merged")));
    }
    if job.intake_issue.is_some() {
        return Ok(skipped(format!("job {id} is an intake job")));
    }
    let dir = driver.job_dir(&job);
    if dir.join(MINORS_FILE).exists() {
        return Ok(skipped("the minor ticket is already filed".to_string()));
    }
    let Some(last) = job.last_review.as_ref() else {
        return Ok(skipped("the job has no review".to_string()));
    };
    let mut minors: Vec<String> = Vec::new();
    let mut read: Vec<String> = Vec::new();
    for name in round_review_files(&dir, last.round, &last.file) {
        let Ok(text) = std::fs::read_to_string(dir.join(&name)) else {
            continue;
        };
        for minor in open_minors(&text) {
            if !minors.contains(&minor) {
                minors.push(minor);
            }
        }
        read.push(name);
    }
    if minors.is_empty() {
        return Ok(skipped("the review has no open minor findings".to_string()));
    }
    let mut report = TicketsReport::default();
    let title = format!("Minor review findings from job {id}: {}", job.slug);
    if dry_run {
        report.lines.push(format!("would create: {title}"));
        return Ok(report);
    }
    let base = driver.repo().base_branch().to_string();
    let tip = driver.repo().tip(&base).unwrap_or_default();
    let checklist: Vec<String> = minors.iter().map(|m| format!("- [ ] {m}")).collect();
    let body = format!(
        "{}\n\nReview files {}. Job {id} merged at {}.",
        checklist.join("\n"),
        read.join(", "),
        short_sha(&tip)
    );
    match driver.tracker_for(&job).create(&title, &body, &[]) {
        Ok(number) => {
            let text = serde_json::json!({ "issue": number }).to_string();
            match write_atomic(&dir.join(MINORS_FILE), text.as_bytes()) {
                Ok(()) => report.lines.push(format!("created #{number}: {title}")),
                Err(e) => {
                    report.failed = true;
                    report.lines.push(format!(
                        "failed: created #{number} but could not record it in {MINORS_FILE}: {e}"
                    ));
                }
            }
        }
        Err(e) => {
            report.failed = true;
            report.lines.push(format!("failed: create `{title}`: {e}"));
        }
    }
    let state = if report.failed { "failed" } else { "done" };
    let text = format!("minor tickets {state}: {}", report.lines.join("; "));
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Err(e) = driver.ledger(&job).ruling(&one_line) {
        tracing::error!(event = "step.error", step = "minors_ledger", msg = %format!("minor ledger error for job {id}: {e}"));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_review_files_keep_the_read_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in [
            "review-code-r2-sync10.md",
            "review-code-r2-sync2.md",
            "review-code-r2-rebase.md",
            "review-code-r2.md",
            "review-code-r1.md",
            "review-code-r2-syncx.md",
        ] {
            std::fs::write(dir.path().join(name), "x").expect("write");
        }
        assert_eq!(
            round_review_files(dir.path(), 2, "review-code-r2-sync10.md"),
            vec![
                "review-code-r2.md",
                "review-code-r2-rebase.md",
                "review-code-r2-sync2.md",
                "review-code-r2-sync10.md"
            ]
        );
    }
}
