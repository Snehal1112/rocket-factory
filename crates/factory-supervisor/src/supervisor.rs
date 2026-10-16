//! The supervisor loop: finish what ended, then start what the scheduler allows.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::Duration;

use factory_core::{project, CoreError, Home, HomeConfig};
use factory_engine::{owner_pid, process_gone, project_lease_status, ProjectLeaseStatus};

use crate::lock::stop_file;
use crate::queue::{self, QueueItem};
use crate::records::{self, ChildRecord, Outcome, ResultRecord};
use crate::request::Request;
use crate::scheduler::{decide, Decision, ProjectState, SchedulerInput};
use crate::spawn::{ChildPoll, Spawner};

/// How much of the end of a log file is read.
const TAIL_BYTES: u64 = 64 * 1024;
const TAIL_LINES: usize = 20;
const LINE_CAP: usize = 500;
const SLICE: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Record(#[from] records::RecordError),
    #[error(transparent)]
    Queue(#[from] queue::QueueError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// Request ids started in this tick.
    pub started: Vec<String>,
    /// Request ids that ended in this tick.
    pub finished: Vec<(String, Outcome)>,
    /// Request ids refused in this tick, with the reason.
    pub rejected: Vec<(String, String)>,
    /// Child records that could not be read. Their project counts as busy.
    pub bad_children: Vec<String>,
    /// Problems that did not stop the tick, such as a queue entry that could not be removed.
    pub errors: Vec<String>,
}

pub struct Supervisor {
    home: Home,
    spawner: Box<dyn Spawner>,
    clock: Box<dyn Fn() -> u64>,
    max_parallel: u32,
}

/// True when the pid runs and is not a zombie.
pub fn pid_alive(pid: u32) -> bool {
    if process_gone(pid) {
        return false;
    }
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next().map(|s| s != "Z")).unwrap_or(true),
        Err(_) => true,
    }
}

/// Splits a tail read into the text and the note.
fn split_read(r: Result<String, String>) -> (String, Option<String>) {
    match r {
        Ok(t) => (t, None),
        Err(n) => (String::new(), Some(n)),
    }
}

/// The last bytes of a file as text. Only a regular file is opened: a FIFO, a symlink or a device
/// is never read, because opening it could block forever. The Err text says why nothing was read.
fn read_tail(path: &Path) -> Result<String, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let len = meta.len();
    if len > TAIL_BYTES {
        f.seek(std::io::SeekFrom::Start(len - TAIL_BYTES)).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut buf = Vec::new();
    let _ = f.take(TAIL_BYTES).read_to_end(&mut buf);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn stderr_tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(TAIL_LINES);
    lines[from..].iter().map(|l| l.chars().take(LINE_CAP).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Decides how a finished child ended. `code` is Some only for a child this process started.
/// The last non-empty stdout line is what `factory run` and `factory approve` print:
/// `merged`, `merged <sha>`, `needs-human: <reason>` or `parked: <reason>`.
pub fn classify(out: &str, err: &str, code: Option<Option<i32>>) -> Outcome {
    let last = out.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if last == "merged" || last.starts_with("merged ") {
        return Outcome::Merged;
    }
    if let Some(rest) = last.strip_prefix("needs-human") {
        if rest.is_empty() || rest.starts_with(':') || rest.starts_with(' ') {
            return Outcome::NeedsHuman { reason: rest.trim().trim_start_matches(':').trim().to_string() };
        }
    }
    if let Some(rest) = last.strip_prefix("parked:") {
        return Outcome::NeedsHuman { reason: rest.trim().to_string() };
    }
    match code {
        Some(code) => Outcome::Failed { code, stderr_tail: stderr_tail(err) },
        None => Outcome::Lost { reason: "the process ended and left no result line".to_string() },
    }
}

/// The shortest pause between two ticks.
pub const MIN_INTERVAL: Duration = Duration::from_millis(50);

/// Raises an interval below the minimum.
pub fn clamp_interval(interval: Duration) -> Duration {
    interval.max(MIN_INTERVAL)
}

impl Supervisor {
    pub fn new(home: Home, spawner: Box<dyn Spawner>, clock: Box<dyn Fn() -> u64>) -> Supervisor {
        Supervisor { home, spawner, clock, max_parallel: HomeConfig::default().max_parallel }
    }

    pub fn set_max_parallel(&mut self, n: u32) {
        self.max_parallel = n;
    }

    fn result_path(&self, id: &str) -> std::path::PathBuf {
        self.home.results_dir().join(format!("{id}.json"))
    }

    /// Writes the result unless one exists for the id. An existing result is never overwritten, and
    /// the outcome that is stored is the one returned.
    fn record_result(&self, req_id: &str, project: &str, job: &str, outcome: Outcome) -> Result<Outcome, SupervisorError> {
        if self.result_path(req_id).exists() {
            let stored = records::read_results(&self.home).into_iter().flatten().find(|r| r.request_id == req_id);
            return Ok(stored.map(|r| r.outcome).unwrap_or(outcome));
        }
        let rec = ResultRecord {
            request_id: req_id.to_string(),
            project: project.to_string(),
            job: job.to_string(),
            outcome: outcome.clone(),
            finished_ms: (self.clock)(),
        };
        records::write_result(&self.home, &rec)?;
        Ok(outcome)
    }

    /// Removes a queue entry. A missing entry is fine, any other error is returned.
    fn drop_queued(&self, id: &str) -> Result<(), SupervisorError> {
        match queue::remove(&self.home, id) {
            Ok(()) | Err(queue::QueueError::Unknown(_)) => Ok(()),
            Err(e) => Err(SupervisorError::Queue(e)),
        }
    }

    /// Result first, then the queue entry, then the child record. An error before the last step keeps
    /// the child record, so the project stays busy and the next tick retries.
    fn finish(&self, child: &ChildRecord, outcome: Outcome, report: &mut TickReport) -> Result<(), SupervisorError> {
        let outcome = self.record_result(&child.request_id, &child.project, &child.job, outcome)?;
        self.drop_queued(&child.request_id)?;
        records::remove_child(&self.home, &child.project)?;
        report.finished.push((child.request_id.clone(), outcome));
        Ok(())
    }

    fn project_states(&self, child_pids: &BTreeSet<u32>) -> Result<BTreeMap<String, ProjectState>, SupervisorError> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let mut states = BTreeMap::new();
        for entry in project::list(&self.home)? {
            let state = match &entry.file {
                Err(reason) => ProjectState::Disabled { reason: reason.clone() },
                Ok(file) if !file.repo.is_dir() => {
                    ProjectState::Disabled { reason: format!("repo path {} is missing", file.repo.display()) }
                }
                Ok(file) => match project::layout(&self.home, &entry.name, file) {
                    Err(e) => ProjectState::Disabled { reason: e.to_string() },
                    Ok(layout) => match project_lease_status(&layout.state_dir, now) {
                        ProjectLeaseStatus::Held { owner, .. } if !owner_pid(&owner).is_some_and(|p| child_pids.contains(&p)) => {
                            ProjectState::LeaseHeld { owner }
                        }
                        _ => ProjectState::Ready,
                    },
                },
            };
            states.insert(entry.name, state);
        }
        Ok(states)
    }

    /// Ends a request that did not start: the queue entry goes first, so the request can never run later.
    fn fail_start(&self, req: &Request, text: String, report: &mut TickReport) -> Result<(), SupervisorError> {
        self.drop_queued(&req.id)?;
        let outcome = self.record_result(&req.id, &req.project, &req.job, Outcome::Failed { code: None, stderr_tail: text })?;
        report.finished.push((req.id.clone(), outcome));
        Ok(())
    }

    fn start(&self, req: &Request, report: &mut TickReport) -> Result<(), SupervisorError> {
        let dir = self.home.logs_dir(&req.project);
        let out = dir.join(format!("{}.out", req.id));
        let err = dir.join(format!("{}.err", req.id));
        match self.spawner.spawn(&self.home, req, &out, &err) {
            Err(e) => self.fail_start(req, e.to_string(), report),
            Ok(pid) => {
                let rec = ChildRecord {
                    pid,
                    project: req.project.clone(),
                    job: req.job.clone(),
                    request_id: req.id.clone(),
                    started_ms: (self.clock)(),
                    out,
                    err,
                };
                match records::write_child(&self.home, &rec) {
                    Ok(()) => {
                        report.started.push(req.id.clone());
                        Ok(())
                    }
                    // The child runs and holds the project lease, but it cannot be tracked. Never start it twice.
                    Err(e) => self.fail_start(req, format!("started pid {pid} but could not record it: {e}"), report),
                }
            }
        }
    }

    /// The project a bad child record file belongs to, from its file name.
    fn bad_child_project(reason: &str) -> Option<String> {
        let name = reason.split(": ").next()?.strip_suffix(".json")?;
        project::validate_name(name).ok()?;
        Some(name.to_string())
    }

    pub fn tick(&mut self) -> Result<TickReport, SupervisorError> {
        let mut report = TickReport::default();
        // Step 1: finish what ended. A child that cannot be finished yet keeps its project busy.
        let mut running: Vec<ChildRecord> = Vec::new();
        let mut blocked: Vec<String> = Vec::new();
        for child in records::read_children(&self.home) {
            let child = match child {
                Ok(c) => c,
                Err(reason) => {
                    if let Some(p) = Self::bad_child_project(&reason) {
                        blocked.push(p);
                    }
                    report.bad_children.push(reason);
                    continue;
                }
            };
            let (ended, code) = match self.spawner.poll(child.pid) {
                ChildPoll::Running => (false, None),
                ChildPoll::Exited(code) => (true, Some(code)),
                ChildPoll::Unknown => (!pid_alive(child.pid), None),
            };
            if !ended {
                running.push(child);
                continue;
            }
            let (out, out_note) = split_read(read_tail(&child.out));
            let (err, err_note) = split_read(read_tail(&child.err));
            let mut outcome = classify(&out, &err, code);
            let notes: Vec<String> = [out_note, err_note].into_iter().flatten().collect();
            if !notes.is_empty() {
                let note = format!("(log not read: {})", notes.join("; "));
                match &mut outcome {
                    Outcome::Failed { stderr_tail, .. } => stderr_tail.push_str(&format!("\n{note}")),
                    Outcome::Lost { reason } => reason.push_str(&format!(" {note}")),
                    _ => {}
                }
            }
            if let Err(e) = self.finish(&child, outcome, &mut report) {
                report.errors.push(format!("finish {}: {e}", child.request_id));
                running.push(child);
            }
        }
        // Step 2: the snapshot. A request that already has a result is dropped and never started.
        let mut queue: Vec<Request> = Vec::new();
        for item in queue::list(&self.home) {
            if let QueueItem::Ok(r) = item {
                if self.result_path(&r.id).exists() {
                    if let Err(e) = self.drop_queued(&r.id) {
                        report.errors.push(format!("drop {}: {e}", r.id));
                    }
                } else {
                    queue.push(r);
                }
            }
        }
        let child_pids: BTreeSet<u32> = running.iter().map(|c| c.pid).collect();
        let mut running_projects: Vec<String> = running.iter().map(|c| c.project.clone()).collect();
        running_projects.extend(blocked);
        let input = SchedulerInput {
            max_parallel: self.max_parallel,
            queue,
            running_projects,
            project_states: self.project_states(&child_pids)?,
        };
        // Step 3: act.
        for decision in decide(&input) {
            match decision {
                Decision::Reject { request, reason } => {
                    self.record_result(&request.id, &request.project, &request.job, Outcome::Rejected { reason: reason.clone() })?;
                    // A failure here is repaired by the next tick, which drops a request that has a result.
                    if let Err(e) = self.drop_queued(&request.id) {
                        report.errors.push(format!("drop {}: {e}", request.id));
                    }
                    report.rejected.push((request.id, reason));
                }
                Decision::Start(request) => self.start(&request, &mut report)?,
            }
        }
        Ok(report)
    }

    /// Ticks until `stop` says so. It never touches a running child.
    pub fn run_loop(&mut self, interval: Duration, stop: &dyn Fn() -> bool) {
        let interval = clamp_interval(interval);
        loop {
            if let Err(e) = self.tick() {
                eprintln!("factoryd: tick failed: {e}");
            }
            if stop() {
                return;
            }
            let mut waited = Duration::ZERO;
            while waited < interval {
                if stop() {
                    return;
                }
                let step = SLICE.min(interval - waited);
                std::thread::sleep(step);
                waited += step;
            }
        }
    }

    pub fn home(&self) -> &Home {
        &self.home
    }

    /// True when the stop file exists.
    pub fn stop_requested(home: &Home) -> bool {
        stop_file(home).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_the_last_stdout_line() {
        assert_eq!(classify("a\nmerged\n\n", "", Some(Some(0))), Outcome::Merged);
        assert_eq!(classify("needs-human: risky\n", "", None), Outcome::NeedsHuman { reason: "risky".into() });
        assert_eq!(
            classify("x\n", "e1\n\ne2\n", Some(Some(1))),
            Outcome::Failed { code: Some(1), stderr_tail: "e1\ne2".into() }
        );
        assert!(matches!(classify("", "", None), Outcome::Lost { .. }));
    }

    #[test]
    fn classifies_the_approve_shapes() {
        assert_eq!(classify("merged abc1234\n", "", Some(Some(0))), Outcome::Merged);
        assert_eq!(classify("parked: x\n", "", Some(Some(0))), Outcome::NeedsHuman { reason: "x".into() });
        assert_eq!(classify("needs-human\n", "", None), Outcome::NeedsHuman { reason: String::new() });
        assert!(matches!(classify("needs-humanfoo\n", "", None), Outcome::Lost { .. }));
        assert!(matches!(classify("mergedx\n", "", Some(Some(0))), Outcome::Failed { .. }));
    }

    #[test]
    fn the_interval_has_a_floor() {
        assert_eq!(clamp_interval(Duration::ZERO), MIN_INTERVAL);
        assert_eq!(clamp_interval(Duration::from_secs(3)), Duration::from_secs(3));
    }

    #[test]
    fn stderr_tail_is_capped() {
        let text: String = (0..30).map(|i| format!("{i}{}\n", "x".repeat(600))).collect();
        let tail = stderr_tail(&text);
        assert_eq!(tail.lines().count(), 20);
        assert!(tail.lines().all(|l| l.chars().count() <= 500));
        assert!(tail.starts_with("10x"));
    }
}
