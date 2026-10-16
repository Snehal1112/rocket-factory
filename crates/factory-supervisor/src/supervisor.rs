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
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// Request ids started in this tick.
    pub started: Vec<String>,
    /// Request ids that ended in this tick.
    pub finished: Vec<(String, Outcome)>,
    /// Request ids refused in this tick, with the reason.
    pub rejected: Vec<(String, String)>,
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

/// The last bytes of a file as text. A missing file is empty.
fn read_tail(path: &Path) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > TAIL_BYTES && f.seek(std::io::SeekFrom::Start(len - TAIL_BYTES)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.take(TAIL_BYTES).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn stderr_tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(TAIL_LINES);
    lines[from..].iter().map(|l| l.chars().take(LINE_CAP).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Decides how a finished child ended. `code` is Some only for a child this process started.
pub fn classify(out: &str, err: &str, code: Option<Option<i32>>) -> Outcome {
    let last = out.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if last == "merged" {
        return Outcome::Merged;
    }
    if let Some(rest) = last.strip_prefix("needs-human") {
        let reason = rest.trim_start_matches(':').trim().to_string();
        return Outcome::NeedsHuman { reason };
    }
    match code {
        Some(code) => Outcome::Failed { code, stderr_tail: stderr_tail(err) },
        None => Outcome::Lost { reason: "the process ended and left no result line".to_string() },
    }
}

impl Supervisor {
    pub fn new(home: Home, spawner: Box<dyn Spawner>, clock: Box<dyn Fn() -> u64>) -> Supervisor {
        Supervisor { home, spawner, clock, max_parallel: HomeConfig::default().max_parallel }
    }

    pub fn set_max_parallel(&mut self, n: u32) {
        self.max_parallel = n;
    }

    fn record_result(&self, req_id: &str, project: &str, job: &str, outcome: Outcome) -> Result<(), SupervisorError> {
        let rec = ResultRecord {
            request_id: req_id.to_string(),
            project: project.to_string(),
            job: job.to_string(),
            outcome,
            finished_ms: (self.clock)(),
        };
        records::write_result(&self.home, &rec)?;
        Ok(())
    }

    /// Result first, then the queue entry, then the child record. A crash between them is repaired by the next tick.
    fn finish(&self, child: &ChildRecord, outcome: Outcome, report: &mut TickReport) -> Result<(), SupervisorError> {
        self.record_result(&child.request_id, &child.project, &child.job, outcome.clone())?;
        let _ = queue::remove(&self.home, &child.request_id);
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

    fn start(&self, req: &Request, report: &mut TickReport) -> Result<(), SupervisorError> {
        let dir = self.home.logs_dir(&req.project);
        let out = dir.join(format!("{}.out", req.id));
        let err = dir.join(format!("{}.err", req.id));
        let fail = |this: &Supervisor, text: String, report: &mut TickReport| -> Result<(), SupervisorError> {
            let outcome = Outcome::Failed { code: None, stderr_tail: text };
            this.record_result(&req.id, &req.project, &req.job, outcome.clone())?;
            let _ = queue::remove(&this.home, &req.id);
            report.finished.push((req.id.clone(), outcome));
            Ok(())
        };
        match self.spawner.spawn(&self.home, req, &out, &err) {
            Err(e) => fail(self, e.to_string(), report),
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
                    Err(e) => fail(self, format!("started pid {pid} but could not record it: {e}"), report),
                }
            }
        }
    }

    pub fn tick(&mut self) -> Result<TickReport, SupervisorError> {
        let mut report = TickReport::default();
        // Step 1: finish what ended.
        let mut running: Vec<ChildRecord> = Vec::new();
        for child in records::read_children(&self.home).into_iter().flatten() {
            let (ended, code) = match self.spawner.poll(child.pid) {
                ChildPoll::Running => (false, None),
                ChildPoll::Exited(code) => (true, Some(code)),
                ChildPoll::Unknown => (!pid_alive(child.pid), None),
            };
            if !ended {
                running.push(child);
                continue;
            }
            let outcome = classify(&read_tail(&child.out), &read_tail(&child.err), code);
            self.finish(&child, outcome, &mut report)?;
        }
        // Step 2: the snapshot.
        let child_pids: BTreeSet<u32> = running.iter().map(|c| c.pid).collect();
        let queue: Vec<Request> = queue::list(&self.home)
            .into_iter()
            .filter_map(|item| match item {
                QueueItem::Ok(r) => Some(r),
                QueueItem::Bad { .. } => None,
            })
            .collect();
        let input = SchedulerInput {
            max_parallel: self.max_parallel,
            queue,
            running_projects: running.iter().map(|c| c.project.clone()).collect(),
            project_states: self.project_states(&child_pids)?,
        };
        // Step 3: act.
        for decision in decide(&input) {
            match decision {
                Decision::Reject { request, reason } => {
                    self.record_result(&request.id, &request.project, &request.job, Outcome::Rejected { reason: reason.clone() })?;
                    let _ = queue::remove(&self.home, &request.id);
                    report.rejected.push((request.id, reason));
                }
                Decision::Start(request) => self.start(&request, &mut report)?,
            }
        }
        Ok(report)
    }

    /// Ticks until `stop` says so. It never touches a running child.
    pub fn run_loop(&mut self, interval: Duration, stop: &dyn Fn() -> bool) {
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
    fn stderr_tail_is_capped() {
        let text: String = (0..30).map(|i| format!("{i}{}\n", "x".repeat(600))).collect();
        let tail = stderr_tail(&text);
        assert_eq!(tail.lines().count(), 20);
        assert!(tail.lines().all(|l| l.chars().count() <= 500));
        assert!(tail.starts_with("10x"));
    }
}
