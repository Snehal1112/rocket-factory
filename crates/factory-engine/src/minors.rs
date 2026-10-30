//! Minor tickets: after a merge, the open `[minor]` lines of the last review are filed as one
//! checklist ticket. `minors.json` records the number, so a re-run never files a second one.

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
    let dir = driver.job_dir(&job);
    if dir.join(MINORS_FILE).exists() {
        return Ok(skipped("the minor ticket is already filed".to_string()));
    }
    let Some(last) = job.last_review.as_ref() else {
        return Ok(skipped("the job has no review".to_string()));
    };
    let text = std::fs::read_to_string(dir.join(&last.file)).unwrap_or_default();
    let minors = open_minors(&text);
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
        "{}\n\nReview file {}. Job {id} merged at {}.",
        checklist.join("\n"),
        last.file,
        short_sha(&tip)
    );
    match driver.tracker().create(&title, &body, &[]) {
        Ok(number) => {
            let text = serde_json::json!({ "issue": number }).to_string();
            write_atomic(&dir.join(MINORS_FILE), text.as_bytes())?;
            report.lines.push(format!("created #{number}: {title}"));
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
