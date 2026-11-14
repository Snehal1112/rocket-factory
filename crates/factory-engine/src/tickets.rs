//! The tickets step: once the last plan of a job has merged, file the follow-ups as new tickets,
//! comment on the parent tickets and close them. Every call is recorded in `tickets.json` so a re-run
//! never repeats one.

use std::path::Path;

use factory_core::{parse_issue_refs, write_atomic, Job, Stage};
use serde_json::{json, Value};

use crate::ports::short_sha;
use crate::{follow_ups, Driver, EngineError};

/// The marker file in the job folder.
pub const TICKETS_FILE: &str = "tickets.json";

/// What the step did or would do, one line per action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TicketsReport {
    pub lines: Vec<String>,
    pub failed: bool,
    pub skipped: bool,
}

impl TicketsReport {
    fn skip(mut self, why: String) -> TicketsReport {
        self.skipped = true;
        self.lines.push(format!("skipped: {why}"));
        self
    }

    fn fail(&mut self, what: String) {
        self.failed = true;
        self.lines.push(format!("failed: {what}"));
    }
}

/// The calls that already happened.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Marker {
    created: Vec<(String, u32)>,
    /// Titles whose create was started and not yet recorded.
    pending: Vec<String>,
    commented: Vec<u32>,
    closed: Vec<u32>,
}

impl Marker {
    fn load(path: &Path) -> Result<Marker, EngineError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Marker::default()),
            Err(e) => return Err(e.into()),
        };
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            EngineError::Msg(format!(
                "{} is not valid JSON, fix or remove it: {e}",
                path.display()
            ))
        })?;
        let numbers = |key: &str| -> Vec<u32> {
            v[key]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let created = v["created"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|c| {
                        Some((
                            c["title"].as_str()?.to_string(),
                            u32::try_from(c["number"].as_u64()?).ok()?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let pending = v["pending"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Marker {
            created,
            pending,
            commented: numbers("commented"),
            closed: numbers("closed"),
        })
    }

    fn save(&self, path: &Path) -> Result<(), EngineError> {
        let created: Vec<Value> = self
            .created
            .iter()
            .map(|(t, n)| json!({ "title": t, "number": n }))
            .collect();
        let v = json!({
            "created": created,
            "pending": self.pending,
            "commented": self.commented,
            "closed": self.closed,
        });
        let text = serde_json::to_string_pretty(&v).map_err(|e| EngineError::Msg(e.to_string()))?;
        write_atomic(path, text.as_bytes())?;
        Ok(())
    }
}

/// The most chars GitHub takes in an issue body, with some room to spare.
const MAX_BODY_CHARS: usize = 60_000;

const CUT_LINE: &str = "\n\nThis text was cut to fit GitHub's limit.";

/// Joins the body and the footer text, cutting the body on a char boundary when the total is too long.
pub(crate) fn fit_body(item_body: &str, footer_text: &str) -> String {
    let footer_chars = footer_text.chars().count();
    if item_body.chars().count() + footer_chars <= MAX_BODY_CHARS {
        return format!("{item_body}{footer_text}");
    }
    let keep = MAX_BODY_CHARS.saturating_sub(footer_chars + CUT_LINE.chars().count());
    let cut: String = item_body.chars().take(keep).collect();
    format!("{cut}{CUT_LINE}{footer_text}")
}

fn list(numbers: &[u32]) -> String {
    numbers
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Files the follow-ups of a merged job, comments on its parent tickets and closes them.
/// With `dry_run` it makes only read calls and writes nothing. A `gh` failure is reported, never raised.
pub fn tickets_step(
    driver: &Driver,
    id: &str,
    dry_run: bool,
) -> Result<TicketsReport, EngineError> {
    let job = driver.job(id)?;
    let mut report = TicketsReport::default();
    if job.stage != Stage::Merged {
        return Ok(report.skip(format!("job {id} is not merged")));
    }
    if let Some(pos) = &job.series {
        if pos.index < pos.total {
            return Ok(report.skip(format!(
                "plan {} of {} is not the last plan of the series",
                pos.index, pos.total
            )));
        }
    }
    if job.intake_issue.is_some() {
        return Ok(report.skip(format!("job {id} is an intake job")));
    }
    let refs = parse_issue_refs(&job.feature);
    if refs.is_empty() {
        return Ok(report.skip("the feature text has no issue references".to_string()));
    }
    let tracker = driver.tracker_for(&job);
    let repo = match tracker.repo_name() {
        Ok(name) => name,
        Err(e) => {
            report.fail(format!("repository name: {e}"));
            return Ok(finish(driver, &job, dry_run, report));
        }
    };
    for r in &refs {
        if let Some(other) = &r.repo {
            if !other.eq_ignore_ascii_case(&repo) {
                return Ok(report.skip(format!(
                    "issue #{} belongs to {other}, this repository is {repo}",
                    r.number
                )));
            }
        }
    }
    let mut parents: Vec<u32> = refs.iter().map(|r| r.number).collect();
    parents.sort_unstable();
    parents.dedup();

    let dir = driver.job_dir(&job);
    let marker_path = dir.join(TICKETS_FILE);
    let mut marker = Marker::load(&marker_path)?;
    let spec = std::fs::read_to_string(dir.join("spec.md")).unwrap_or_default();
    let items = follow_ups(&spec);

    let mut labels: Vec<String> = Vec::new();
    for parent in &parents {
        match tracker.labels_of(*parent) {
            Ok(found) => {
                for label in found {
                    if label != "tracking" && !labels.contains(&label) {
                        labels.push(label);
                    }
                }
            }
            Err(e) => {
                report.fail(format!("labels of #{parent}: {e}"));
                return Ok(finish(driver, &job, dry_run, report));
            }
        }
    }

    let footer = format!("Filed by factory job {} ({})", job.id, job.slug);
    let footer_text = format!("\n\n{footer} from the follow-ups of {}.", list(&parents));
    for item in &items {
        if marker.created.iter().any(|(title, _)| *title == item.title) {
            continue;
        }
        let was_pending = marker.pending.contains(&item.title);
        if dry_run {
            if was_pending {
                report.lines.push(format!(
                    "would look for an existing issue, then create: {}",
                    item.title
                ));
            } else {
                report.lines.push(format!(
                    "would create: {} [{}]",
                    item.title,
                    labels.join(", ")
                ));
            }
            continue;
        }
        if was_pending {
            match tracker.find_created(&item.title, &footer) {
                Ok(Some(number)) => {
                    marker.pending.retain(|t| *t != item.title);
                    marker.created.push((item.title.clone(), number));
                    if let Err(e) = marker.save(&marker_path) {
                        report.fail(format!(
                            "adopted #{number} `{}` but could not record it: {e}",
                            item.title
                        ));
                        return Ok(finish(driver, &job, dry_run, report));
                    }
                    report
                        .lines
                        .push(format!("adopted #{number}: {}", item.title));
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    report.fail(format!("find `{}`: {e}", item.title));
                    return Ok(finish(driver, &job, dry_run, report));
                }
            }
        } else {
            marker.pending.push(item.title.clone());
            if let Err(e) = marker.save(&marker_path) {
                report.fail(format!("record pending `{}`: {e}", item.title));
                return Ok(finish(driver, &job, dry_run, report));
            }
        }
        let body = fit_body(&item.body, &footer_text);
        match tracker.create(&item.title, &body, &labels) {
            Ok(number) => {
                marker.pending.retain(|t| *t != item.title);
                marker.created.push((item.title.clone(), number));
                if let Err(e) = marker.save(&marker_path) {
                    report.fail(format!(
                        "created #{number} `{}` but could not record it: {e}",
                        item.title
                    ));
                    return Ok(finish(driver, &job, dry_run, report));
                }
                report
                    .lines
                    .push(format!("created #{number}: {}", item.title));
            }
            Err(e) => {
                report.fail(format!("create `{}`: {e}", item.title));
                return Ok(finish(driver, &job, dry_run, report));
            }
        }
    }

    let filed: Vec<u32> = items
        .iter()
        .filter_map(|item| {
            marker
                .created
                .iter()
                .find(|(title, _)| *title == item.title)
                .map(|(_, n)| *n)
        })
        .collect();
    let base = driver.repo().base_branch().to_string();
    let main_tip = driver.repo().tip(&base).unwrap_or_default();
    let short = short_sha(&main_tip).to_string();
    let pushed = driver
        .repo()
        .is_ancestor(&main_tip, &format!("origin/{base}"))
        .unwrap_or(false);
    let note = if pushed {
        ""
    } else {
        " (not yet pushed to origin)"
    };
    let remaining = if items.is_empty() {
        "No remaining work was listed in the spec.".to_string()
    } else if filed.len() == items.len() {
        format!("Remaining work is filed as {}.", list(&filed))
    } else {
        format!(
            "Remaining work: {}.",
            items
                .iter()
                .map(|i| i.title.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )
    };
    let comment = format!(
        "Job {} ({}) is merged, {base} is at {short}{note}.\n\n{remaining}",
        job.id, job.slug
    );

    for parent in &parents {
        if !marker.commented.contains(parent) {
            if dry_run {
                report.lines.push(format!("would comment on #{parent}"));
            } else {
                match tracker.comment(*parent, &comment) {
                    Ok(()) => {
                        marker.commented.push(*parent);
                        marker.save(&marker_path)?;
                        report.lines.push(format!("commented on #{parent}"));
                    }
                    Err(e) => {
                        report.fail(format!("comment on #{parent}: {e}"));
                        return Ok(finish(driver, &job, dry_run, report));
                    }
                }
            }
        }
        if marker.closed.contains(parent) {
            continue;
        }
        match tracker.is_closed(*parent) {
            Ok(true) => {
                if dry_run {
                    report.lines.push(format!("#{parent} is already closed"));
                } else {
                    marker.closed.push(*parent);
                    marker.save(&marker_path)?;
                    report.lines.push(format!("#{parent} was already closed"));
                }
            }
            Ok(false) if dry_run => report.lines.push(format!("would close #{parent}")),
            Ok(false) => match tracker.close(*parent) {
                Ok(()) => {
                    marker.closed.push(*parent);
                    marker.save(&marker_path)?;
                    report.lines.push(format!("closed #{parent}"));
                }
                Err(e) => {
                    report.fail(format!("close #{parent}: {e}"));
                    return Ok(finish(driver, &job, dry_run, report));
                }
            },
            Err(e) => {
                report.fail(format!("state of #{parent}: {e}"));
                return Ok(finish(driver, &job, dry_run, report));
            }
        }
    }
    Ok(finish(driver, &job, dry_run, report))
}

/// Writes one ledger line for a real run and hands the report back.
fn finish(driver: &Driver, job: &Job, dry_run: bool, report: TicketsReport) -> TicketsReport {
    if !dry_run {
        let state = if report.failed { "failed" } else { "done" };
        let text = format!("tickets {state}: {}", report.lines.join("; "));
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Err(e) = driver.ledger(job).ruling(&one_line) {
            tracing::error!(event = "step.error", step = "tickets_ledger", msg = %format!("tickets ledger error for job {}: {e}", job.id));
        }
    }
    report
}

/// Writes a failed or errored soft step to the job's events as a driver note. Never fails the caller.
pub(crate) fn notify_failure(driver: &Driver, id: &str, step: &str, text: &str) {
    let sink = factory_agent::EventSink::new(driver.events_path(), id);
    sink.emit(&factory_agent::AgentEvent {
        kind: factory_agent::EventKind::Notice,
        agent_id: format!("driver-{id}"),
        agent_type: "driver".to_string(),
        tool_name: None,
        tool_target: None,
        cwd: None,
        last_message: Some(format!("{step} step: {text}")),
        tokens: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::fit_body;

    const FOOT: &str = "\n\nFiled by factory job 001 (s) from the follow-ups of #1.";

    #[test]
    fn fit_body_keeps_a_body_at_the_limit() {
        let body = "a".repeat(60_000 - FOOT.chars().count());
        let out = fit_body(&body, FOOT);
        assert_eq!(out.chars().count(), 60_000);
        assert!(!out.contains("cut to fit"));
    }

    #[test]
    fn fit_body_cuts_one_char_over_the_limit() {
        let body = "a".repeat(60_000 - FOOT.chars().count() + 1);
        let out = fit_body(&body, FOOT);
        assert!(out.chars().count() <= 60_000, "{}", out.chars().count());
        assert!(out.contains("This text was cut to fit GitHub's limit."));
        assert!(out.ends_with(FOOT));
    }

    #[test]
    fn fit_body_cuts_on_a_char_boundary() {
        let body = "é".repeat(70_000);
        let out = fit_body(&body, FOOT);
        assert!(out.chars().count() <= 60_000);
        assert!(out.ends_with(FOOT));
    }
}
