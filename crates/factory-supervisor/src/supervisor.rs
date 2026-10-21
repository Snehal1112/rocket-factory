//! The supervisor loop: finish what ended, then start what the scheduler allows.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::Duration;

use factory_core::outcome_lines::BUSY_EXIT_CODE;
use factory_core::{project, CoreError, Home, HomeConfig};
use factory_engine::procinfo::{process_state, ProcState};
use factory_engine::{owner_pid, project_lease_status, ProjectLeaseStatus};

use crate::lock::{stop_file, InstanceLock, LockError};
use crate::queue::{self, QueueItem};
use crate::records::{self, ChildRecord, Outcome, ResultRecord};
use crate::request::Request;
use crate::scheduler::{decide, Decision, ProjectState, SchedulerInput};
use crate::spawn::{ChildPoll, Spawner};

/// How much of the end of a log file is read.
const TAIL_BYTES: u64 = 64 * 1024;
const TAIL_LINES: usize = 20;
const LINE_CAP: usize = 500;
/// How many times a request may find its project busy before it ends as failed.
const MAX_BUSY_RETRIES: u32 = 20;
/// The most chars of a stderr tail that go into a result.
const TAIL_CHARS: usize = 4000;
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
    /// Projects whose lease is held by a live driver process that is not a child of this supervisor.
    /// Nothing is started for them until that process ends.
    pub orphans: Vec<String>,
    /// One-time lines worth printing once, such as the daily budget being reached or reset.
    pub notices: Vec<String>,
}

/// What start-up recovery found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Request ids of children that still run and are now watched again.
    pub adopted: Vec<String>,
    /// What recovery ended or found, in the shape of a tick report.
    pub report: TickReport,
}

/// Starts and watches the children of one factory home.
///
/// Exactly one `Supervisor` may run per home. Use `Supervisor::new_locked`, which holds the instance lock.
/// Without it a second supervisor can run recovery while the first is between spawning a child and writing
/// its child record, and mark that live request `Lost`. A result is never overwritten, so the real outcome
/// would be lost.
pub struct Supervisor {
    home: Home,
    spawner: Box<dyn Spawner>,
    clock: Box<dyn Fn() -> u64>,
    max_parallel: u32,
    /// The daily budget from the last valid `config.toml`. `None` means no limit.
    daily_limit: Option<u64>,
    /// Whether the daily budget was used up at the last tick. A change of this logs one line.
    daily_exhausted: bool,
    /// Requests whose child runs but could not be recorded. They are never started again by this process.
    started_ids: HashSet<String>,
    /// How often a request's child ended with "project busy" and nothing done. Past `MAX_BUSY_RETRIES`
    /// the request fails, so a lease that never frees cannot loop forever.
    busy_retries: HashMap<String, u32>,
    /// True once start-up recovery ran.
    recovered: bool,
    /// Held for the life of the supervisor when it was made by `new_locked`. It must stay the LAST field:
    /// fields drop in declaration order, and the lock must outlive every other field.
    _lock: Option<InstanceLock>,
}

/// True when the pid runs and is not a zombie.
pub fn pid_alive(pid: u32) -> bool {
    pid == std::process::id() || process_state(pid) == ProcState::Running
}

/// True when the recorded child still runs: the pid is alive and started when the record says.
fn adopted_alive(child: &ChildRecord) -> bool {
    pid_alive(child.pid) && crate::procinfo::may_be_our_child(child.pid, child.started_ms)
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
        f.seek(std::io::SeekFrom::Start(len - TAIL_BYTES))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut buf = Vec::new();
    let _ = f.take(TAIL_BYTES).read_to_end(&mut buf);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Cuts text to `max` chars on a char boundary and marks the cut with an ellipsis.
fn cap_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

/// Caps every text of an outcome, so the stored result is always smaller than the read cap.
/// The tail keeps its last chars, because the end of stderr says most.
fn cap_outcome(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::NeedsHuman { reason } => Outcome::NeedsHuman {
            reason: cap_text(&reason, LINE_CAP),
        },
        Outcome::Lost { reason } => Outcome::Lost {
            reason: cap_text(&reason, LINE_CAP),
        },
        Outcome::Rejected { reason } => Outcome::Rejected {
            reason: cap_text(&reason, LINE_CAP),
        },
        Outcome::Failed { code, stderr_tail } => {
            let count = stderr_tail.chars().count();
            let tail = if count > TAIL_CHARS {
                let kept: String = stderr_tail.chars().skip(count - TAIL_CHARS).collect();
                format!("…{kept}")
            } else {
                stderr_tail
            };
            Outcome::Failed {
                code,
                stderr_tail: tail,
            }
        }
        other => other,
    }
}

fn stderr_tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(TAIL_LINES);
    lines[from..]
        .iter()
        .map(|l| l.chars().take(LINE_CAP).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decides how a finished child ended. `code` is Some only for a child this process started.
/// The last non-empty stdout line is what `factory run` and `factory approve` print:
/// `merged`, `merged <sha>`, `needs-human: <reason>` or `parked: <reason>`.
pub fn classify(out: &str, err: &str, code: Option<Option<i32>>) -> Outcome {
    let last = out
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    // An adopted child has no exit code, so its last line decides. A series and approve print merged only at the very end, so that line is a finished result.
    // A merge ends with exit code 0. A `merged` line next to another code is not a result.
    let clean_exit = matches!(code, None | Some(Some(0)));
    if clean_exit && (last == "merged" || last.starts_with("merged ")) {
        return Outcome::Merged;
    }
    if let Some(rest) = last.strip_prefix("needs-human") {
        if rest.is_empty() || rest.starts_with(':') || rest.starts_with(' ') {
            return Outcome::NeedsHuman {
                reason: rest.trim().trim_start_matches(':').trim().to_string(),
            };
        }
    }
    if let Some(rest) = last.strip_prefix("parked:") {
        return Outcome::NeedsHuman {
            reason: rest.trim().to_string(),
        };
    }
    match code {
        Some(code) => Outcome::Failed {
            code,
            stderr_tail: stderr_tail(err),
        },
        None => Outcome::Lost {
            reason: "the process ended without a result".to_string(),
        },
    }
}

/// The lines worth printing for a report: its errors and bad child records. A line that was printed
/// before is not returned again, and `prev` forgets lines that are gone, so a returning problem shows again.
pub fn new_lines(prev: &mut HashSet<String>, report: &TickReport) -> Vec<String> {
    let mut now: Vec<String> = report
        .errors
        .iter()
        .map(|e| format!("factoryd: {e}"))
        .collect();
    now.extend(
        report
            .bad_children
            .iter()
            .map(|b| format!("factoryd: bad child record {b}")),
    );
    now.extend(report.orphans.iter().map(|p| format!("factoryd: project {p} is busy, its lease belongs to a live process that is not a child of factoryd")));
    let fresh: Vec<String> = now.iter().filter(|l| !prev.contains(*l)).cloned().collect();
    *prev = now.into_iter().collect();
    let mut seen = HashSet::new();
    fresh
        .into_iter()
        .filter(|l| seen.insert(l.clone()))
        .collect()
}

/// Logs a tick report. Start, finish and reject are always logged. Errors, bad child records and
/// orphans are logged once per change, by the same rules as `new_lines`. Notices are logged each time.
pub fn log_report(report: &TickReport, printed: &mut HashSet<String>) {
    for id in &report.started {
        tracing::info!(event = "sup.start", request = id.as_str());
    }
    for (id, outcome) in &report.finished {
        let outcome = format!("{outcome:?}");
        tracing::info!(
            event = "sup.finish",
            request = id.as_str(),
            outcome = outcome.as_str()
        );
    }
    for (id, reason) in &report.rejected {
        tracing::info!(
            event = "sup.reject",
            request = id.as_str(),
            reason = reason.as_str()
        );
    }
    let mut fresh: HashSet<String> = new_lines(printed, report).into_iter().collect();
    for e in &report.errors {
        let line = format!("factoryd: {e}");
        if fresh.remove(&line) {
            tracing::error!(event = "sup.error", msg = line.as_str());
        }
    }
    for b in &report.bad_children {
        let line = format!("factoryd: bad child record {b}");
        if fresh.remove(&line) {
            tracing::warn!(event = "sup.warn", msg = line.as_str());
        }
    }
    for p in &report.orphans {
        let line = format!("factoryd: project {p} is busy, its lease belongs to a live process that is not a child of factoryd");
        if fresh.remove(&line) {
            tracing::warn!(event = "sup.warn", msg = line.as_str());
        }
    }
    for n in &report.notices {
        tracing::info!(event = "sup.notice", msg = n.as_str());
    }
}

/// The shortest pause between two ticks.
pub const MIN_INTERVAL: Duration = Duration::from_millis(50);

/// Raises an interval below the minimum.
pub fn clamp_interval(interval: Duration) -> Duration {
    interval.max(MIN_INTERVAL)
}

impl Supervisor {
    /// Makes a supervisor without the instance lock. It requires that no other supervisor runs on this
    /// home, see the type docs. Use `new_locked` in production.
    pub fn new(home: Home, spawner: Box<dyn Spawner>, clock: Box<dyn Fn() -> u64>) -> Supervisor {
        Supervisor {
            home,
            spawner,
            clock,
            max_parallel: HomeConfig::default().max_parallel,
            daily_limit: None,
            daily_exhausted: false,
            started_ids: HashSet::new(),
            busy_retries: HashMap::new(),
            recovered: false,
            _lock: None,
        }
    }

    /// Takes the instance lock of the home and keeps it as long as the supervisor lives. Fails with the
    /// pid of the supervisor that already runs.
    pub fn new_locked(
        home: Home,
        spawner: Box<dyn Spawner>,
        clock: Box<dyn Fn() -> u64>,
    ) -> Result<Supervisor, LockError> {
        let lock = InstanceLock::acquire(&home)?;
        let mut s = Supervisor::new(home, spawner, clock);
        s._lock = Some(lock);
        Ok(s)
    }

    pub fn set_max_parallel(&mut self, n: u32) {
        self.max_parallel = n;
    }

    /// Re-reads the daily limit from `config.toml`. An invalid file keeps the last valid value and
    /// adds one error line to the report (the loop prints a repeated line once).
    fn reload_daily_limit(&mut self, report: &mut TickReport) {
        match HomeConfig::load(&self.home) {
            Ok(cfg) => self.daily_limit = cfg.daily_budget_weighted,
            Err(e) => report.errors.push(format!(
                "config.toml is invalid, keeping the last daily budget: {e}"
            )),
        }
    }

    /// True when new runs must wait. Logs one notice when the state changes.
    fn daily_exhausted_now(&mut self, report: &mut TickReport) -> bool {
        let now = (self.clock)();
        let status = crate::daily::status(&self.home, self.daily_limit, now);
        let exhausted = status.is_some_and(|b| b.exhausted());
        if exhausted != self.daily_exhausted {
            self.daily_exhausted = exhausted;
            report.notices.push(match status {
                Some(b) if exhausted => format!(
                    "factoryd: daily budget reached ({} of {} weighted tokens), new runs wait until {}",
                    b.used,
                    b.limit,
                    crate::daily::rfc3339_utc(b.resets_at_ms)
                ),
                _ => "factoryd: daily budget no longer reached, waiting runs may start".to_string(),
            });
        }
        exhausted
    }

    fn result_path(&self, id: &str) -> std::path::PathBuf {
        self.home.results_dir().join(format!("{id}.json"))
    }

    /// Writes the result unless one exists for the id. An existing result is never overwritten, and
    /// the outcome that is stored is the one returned.
    fn record_result(
        &self,
        req_id: &str,
        project: &str,
        job: &str,
        outcome: Outcome,
    ) -> Result<Outcome, SupervisorError> {
        if self.result_path(req_id).exists() {
            let stored = records::read_results(&self.home)
                .into_iter()
                .flatten()
                .find(|r| r.request_id == req_id);
            return Ok(stored.map(|r| r.outcome).unwrap_or(outcome));
        }
        let outcome = cap_outcome(outcome);
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
    fn finish(
        &self,
        child: &ChildRecord,
        outcome: Outcome,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        let outcome = self.record_result(&child.request_id, &child.project, &child.job, outcome)?;
        self.drop_queued(&child.request_id)?;
        records::remove_child(&self.home, &child.project)?;
        report.finished.push((child.request_id.clone(), outcome));
        Ok(())
    }

    fn project_states(
        &self,
        child_pids: &BTreeSet<u32>,
    ) -> Result<BTreeMap<String, ProjectState>, SupervisorError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let mut states = BTreeMap::new();
        for entry in project::list(&self.home)? {
            let state = match &entry.file {
                Err(reason) => ProjectState::Disabled {
                    reason: reason.clone(),
                },
                Ok(file) if !file.repo.is_dir() => ProjectState::Disabled {
                    reason: format!("repo path {} is missing", file.repo.display()),
                },
                Ok(file) if !file.folder().is_dir() => ProjectState::Disabled {
                    reason: format!("project folder {} is missing", file.folder().display()),
                },
                Ok(file) => match project::layout(&self.home, &entry.name, file) {
                    Err(e) => ProjectState::Disabled {
                        reason: e.to_string(),
                    },
                    Ok(layout) => match project_lease_status(&layout.state_dir, now) {
                        ProjectLeaseStatus::Held { owner, .. }
                            if !owner_pid(&owner).is_some_and(|p| child_pids.contains(&p)) =>
                        {
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

    /// Ends a request without a usable child. The result goes first, because a request that has a
    /// result is never started. The queue drop is best effort: the next tick repeats it.
    fn end_request(
        &self,
        req: &Request,
        text: String,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        let outcome = self.record_result(
            &req.id,
            &req.project,
            &req.job,
            Outcome::Failed {
                code: None,
                stderr_tail: text,
            },
        )?;
        report.finished.push((req.id.clone(), outcome));
        if let Err(e) = self.drop_queued(&req.id) {
            report.errors.push(format!("drop {}: {e}", req.id));
        }
        Ok(())
    }

    fn start(&mut self, req: &Request, report: &mut TickReport) -> Result<(), SupervisorError> {
        let dir = self.home.logs_dir(&req.project);
        let out = dir.join(format!("{}.out", req.id));
        let err = dir.join(format!("{}.err", req.id));
        match self.spawner.spawn(&self.home, req, &out, &err) {
            // No process exists. When the result cannot be written the request stays queued and may retry.
            Err(e) => self.end_request(req, e.to_string(), report),
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
                    Err(e) => {
                        // The child runs and holds the project lease, but it cannot be tracked. Whatever else
                        // fails now, this process never starts the request again.
                        self.started_ids.insert(req.id.clone());
                        let text = format!("started pid {pid} but could not record it: {e}");
                        report.errors.push(format!("request {} {text}", req.id));
                        if let Err(e2) = self.end_request(req, text, report) {
                            report.errors.push(format!(
                                "request {} has no result and is blocked in memory only: {e2}",
                                req.id
                            ));
                        }
                        Ok(())
                    }
                }
            }
        }
    }

    fn reject(
        &self,
        req: &Request,
        reason: String,
        report: &mut TickReport,
    ) -> Result<(), SupervisorError> {
        self.record_result(
            &req.id,
            &req.project,
            &req.job,
            Outcome::Rejected {
                reason: reason.clone(),
            },
        )?;
        // A failure here is repaired by the next tick, which drops a request that has a result.
        if let Err(e) = self.drop_queued(&req.id) {
            report.errors.push(format!("drop {}: {e}", req.id));
        }
        report.rejected.push((req.id.clone(), reason));
        Ok(())
    }

    /// The project a bad child record file belongs to, from its file name.
    fn bad_child_project(reason: &str) -> Option<String> {
        let name = reason.split(": ").next()?.strip_suffix(".json")?;
        project::validate_name(name).ok()?;
        Some(name.to_string())
    }

    /// Finishes every child that ended. Returns the children still running and the projects that
    /// are blocked by a child record that cannot be read or finished.
    fn settle_children(&mut self, report: &mut TickReport) -> (Vec<ChildRecord>, Vec<String>) {
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
                // An adopted child is judged by the process table. A pid that started far from the record belongs to a stranger.
                ChildPoll::Unknown => (!adopted_alive(&child), None),
            };
            if !ended {
                running.push(child);
                continue;
            }
            if code == Some(Some(BUSY_EXIT_CODE)) && self.requeue_busy(&child, report) {
                continue;
            }
            let (out, out_note) = split_read(read_tail(&child.out));
            let (err, err_note) = split_read(read_tail(&child.err));
            let mut outcome = classify(&out, &err, code);
            let notes: Vec<String> = [out_note, err_note].into_iter().flatten().collect();
            if !notes.is_empty() {
                let note = format!("(log not read: {})", notes.join("; "));
                match &mut outcome {
                    Outcome::Failed { stderr_tail, .. } => {
                        stderr_tail.push_str(&format!("\n{note}"))
                    }
                    Outcome::Lost { reason } => reason.push_str(&format!(" {note}")),
                    _ => {}
                }
            }
            if let Err(e) = self.finish(&child, outcome, report) {
                report
                    .errors
                    .push(format!("finish {}: {e}", child.request_id));
                running.push(child);
            }
        }
        (running, blocked)
    }

    /// A child that found its project busy did nothing. Its logs and record go, so the request stays
    /// queued and starts again once the lease is free. Returns false when the request must end instead:
    /// the retries are used up, or the record could not be cleared.
    fn requeue_busy(&mut self, child: &ChildRecord, report: &mut TickReport) -> bool {
        let tries = self
            .busy_retries
            .entry(child.request_id.clone())
            .or_insert(0);
        *tries += 1;
        if *tries > MAX_BUSY_RETRIES {
            return false;
        }
        for log in [&child.out, &child.err] {
            if let Err(e) = std::fs::remove_file(log) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    report.errors.push(format!("remove {}: {e}", log.display()));
                    return false;
                }
            }
        }
        match records::remove_child(&self.home, &child.project) {
            Ok(()) => true,
            Err(e) => {
                report
                    .errors
                    .push(format!("remove child record of {}: {e}", child.request_id));
                false
            }
        }
    }

    /// Projects held by a live driver process that has no child record.
    fn find_orphans(
        states: &BTreeMap<String, ProjectState>,
        running_projects: &[String],
    ) -> Vec<String> {
        states
            .iter()
            .filter(|(name, state)| {
                matches!(state, ProjectState::LeaseHeld { owner } if owner_pid(owner).is_some())
                    && !running_projects.iter().any(|p| p == *name)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Start-up recovery. A child record whose process still runs is adopted and stays in place. One whose
    /// process is gone ends from its log files, as `Lost` when they hold no result line. A queued request
    /// with no result and no child record but with log files was started before a crash: it ends as `Lost`
    /// and is never run again. A job is never restarted. The first tick calls this once, and calling it
    /// first by hand is fine. Requires that no other supervisor runs on this home: a second one would
    /// mark a request that is just being started as `Lost`, and that result is never overwritten.
    pub fn recover(&mut self) -> Result<RecoveryReport, SupervisorError> {
        self.recover_inner().map(|(report, _, _)| report)
    }

    /// Recovery plus the children still running and the blocked projects, so a tick need not look again.
    fn recover_inner(
        &mut self,
    ) -> Result<(RecoveryReport, Vec<ChildRecord>, Vec<String>), SupervisorError> {
        let mut report = TickReport::default();
        let (running, blocked) = self.settle_children(&mut report);
        for item in queue::list(&self.home) {
            let QueueItem::Ok(r) = item else { continue };
            let tracked =
                running.iter().any(|c| c.request_id == r.id) || blocked.contains(&r.project);
            if tracked || self.result_path(&r.id).exists() {
                continue;
            }
            let logs = self.home.logs_dir(&r.project);
            let evidence = ["out", "err"]
                .iter()
                .map(|ext| format!("{}.{ext}", r.id))
                .find(|name| std::fs::symlink_metadata(logs.join(name)).is_ok());
            let Some(name) = evidence else { continue };
            let reason = format!("the request was started but no child record exists (found log file {name}), it is not run again");
            report.errors.push(format!("request {}: {reason}", r.id));
            // Without a result the request would start again, so this stops recovery.
            let outcome =
                self.record_result(&r.id, &r.project, &r.job, Outcome::Lost { reason })?;
            report.finished.push((r.id.clone(), outcome));
            if let Err(e) = self.drop_queued(&r.id) {
                report.errors.push(format!("drop {}: {e}", r.id));
            }
        }
        let child_pids: BTreeSet<u32> = running.iter().map(|c| c.pid).collect();
        let mut running_projects: Vec<String> = running.iter().map(|c| c.project.clone()).collect();
        running_projects.extend(blocked.iter().cloned());
        report.orphans = Self::find_orphans(&self.project_states(&child_pids)?, &running_projects);
        self.recovered = true;
        let adopted = running.iter().map(|c| c.request_id.clone()).collect();
        Ok((RecoveryReport { adopted, report }, running, blocked))
    }

    /// One pass: finish what ended, then start what the scheduler allows. Like `recover`, it requires
    /// that this is the only supervisor on the home.
    pub fn tick(&mut self) -> Result<TickReport, SupervisorError> {
        // Step 1: finish what ended. A child that cannot be finished yet keeps its project busy.
        // The first tick does this inside recovery.
        let mut report = TickReport::default();
        let (running, blocked) = if self.recovered {
            self.settle_children(&mut report)
        } else {
            let (rec, running, blocked) = self.recover_inner()?;
            report = rec.report;
            (running, blocked)
        };
        // Step 2: the snapshot. A request that already has a result is dropped and never started.
        let mut queue: Vec<Request> = Vec::new();
        for item in queue::list(&self.home) {
            if let QueueItem::Ok(r) = item {
                // The result check comes first, so a failed queue drop is retried here even for a started id.
                if self.result_path(&r.id).exists() {
                    if let Err(e) = self.drop_queued(&r.id) {
                        report.errors.push(format!("drop {}: {e}", r.id));
                    }
                } else if self.started_ids.contains(&r.id) {
                    report.errors.push(format!(
                        "request {} was started but is untracked, not starting it again",
                        r.id
                    ));
                } else {
                    queue.push(r);
                }
            }
        }
        let child_pids: BTreeSet<u32> = running.iter().map(|c| c.pid).collect();
        let mut running_projects: Vec<String> = running.iter().map(|c| c.project.clone()).collect();
        running_projects.extend(blocked);
        let project_states = self.project_states(&child_pids)?;
        // Recovery reported the orphans it found already, so this adds only the ones it did not.
        for p in Self::find_orphans(&project_states, &running_projects) {
            if !report.orphans.contains(&p) {
                report.orphans.push(p);
            }
        }
        self.reload_daily_limit(&mut report);
        let daily_exhausted = self.daily_exhausted_now(&mut report);
        let input = SchedulerInput {
            max_parallel: self.max_parallel,
            queue,
            running_projects,
            project_states,
            daily_exhausted,
        };
        // Step 3: act.
        for decision in decide(&input) {
            // One failing decision never stops the others.
            let (id, outcome) = match decision {
                Decision::Reject { request, reason } => {
                    let r = self.reject(&request, reason, &mut report);
                    (request.id, r)
                }
                Decision::Start(request) => {
                    let r = self.start(&request, &mut report);
                    (request.id, r)
                }
            };
            if let Err(e) = outcome {
                report.errors.push(format!("request {id}: {e}"));
            }
        }
        Ok(report)
    }

    /// Ticks until `stop` says so. It never touches a running child.
    pub fn run_loop(&mut self, interval: Duration, stop: &dyn Fn() -> bool) {
        let interval = clamp_interval(interval);
        let mut printed: HashSet<String> = HashSet::new();
        loop {
            match self.tick() {
                Ok(report) => log_report(&report, &mut printed),
                Err(e) => {
                    // The same failure is logged once, through the same filter as the other lines.
                    let failed = TickReport {
                        errors: vec![format!("tick failed: {e}")],
                        ..TickReport::default()
                    };
                    log_report(&failed, &mut printed);
                }
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
        use factory_core::outcome_lines::{merged_line, needs_human_line, parked_line};
        assert_eq!(
            classify(&format!("a\n{}\n\n", merged_line(None)), "", Some(Some(0))),
            Outcome::Merged
        );
        assert_eq!(
            classify(&format!("{}\n", needs_human_line("risky")), "", None),
            Outcome::NeedsHuman {
                reason: "risky".into()
            }
        );
        assert_eq!(
            classify(
                &format!("{}\n", merged_line(Some("abc1234"))),
                "",
                Some(Some(0))
            ),
            Outcome::Merged
        );
        assert_eq!(
            classify(&format!("{}\n", parked_line("x")), "", Some(Some(0))),
            Outcome::NeedsHuman { reason: "x".into() }
        );
        assert!(matches!(
            classify("merged\n", "", Some(Some(1))),
            Outcome::Failed { code: Some(1), .. }
        ));
        assert!(matches!(
            classify("merged\n", "", Some(None)),
            Outcome::Failed { code: None, .. }
        ));
        assert_eq!(
            classify("x\n", "e1\n\ne2\n", Some(Some(1))),
            Outcome::Failed {
                code: Some(1),
                stderr_tail: "e1\ne2".into()
            }
        );
        assert!(matches!(classify("", "", None), Outcome::Lost { .. }));
    }

    #[test]
    fn an_adopted_child_with_a_merged_line_is_merged() {
        use factory_core::outcome_lines::merged_line;
        assert_eq!(
            classify(&format!("{}\n", merged_line(None)), "", None),
            Outcome::Merged
        );
        assert_eq!(
            classify(&format!("{}\n", merged_line(Some("abc1234"))), "", None),
            Outcome::Merged
        );
        assert_eq!(
            classify("plan 1 of 2\nmerged\n\n", "", None),
            Outcome::Merged
        );
        assert!(matches!(
            classify("merged\nplan 2 of 2\n", "", None),
            Outcome::Lost { .. }
        ));
    }

    #[test]
    fn classifies_the_approve_shapes() {
        assert_eq!(
            classify("merged abc1234\n", "", Some(Some(0))),
            Outcome::Merged
        );
        assert_eq!(
            classify("parked: x\n", "", Some(Some(0))),
            Outcome::NeedsHuman { reason: "x".into() }
        );
        assert_eq!(
            classify("needs-human\n", "", None),
            Outcome::NeedsHuman {
                reason: String::new()
            }
        );
        assert!(matches!(
            classify("needs-humanfoo\n", "", None),
            Outcome::Lost { .. }
        ));
        assert!(matches!(
            classify("mergedx\n", "", Some(Some(0))),
            Outcome::Failed { .. }
        ));
    }

    #[test]
    fn new_lines_print_once_per_change() {
        let mut prev = HashSet::new();
        let mut r = TickReport {
            errors: vec!["a".into()],
            bad_children: vec!["x.json: bad".into()],
            ..TickReport::default()
        };
        assert_eq!(new_lines(&mut prev, &r).len(), 2);
        assert!(new_lines(&mut prev, &r).is_empty());
        r.errors.push("b".into());
        assert_eq!(new_lines(&mut prev, &r), vec!["factoryd: b".to_string()]);
        r.errors.clear();
        assert!(new_lines(&mut prev, &r).is_empty());
        r.errors.push("a".into());
        assert_eq!(new_lines(&mut prev, &r), vec!["factoryd: a".to_string()]);
    }

    #[test]
    fn the_interval_has_a_floor() {
        assert_eq!(clamp_interval(Duration::ZERO), MIN_INTERVAL);
        assert_eq!(
            clamp_interval(Duration::from_secs(3)),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn stderr_tail_is_capped() {
        let text: String = (0..30)
            .map(|i| format!("{i}{}\n", "x".repeat(600)))
            .collect();
        let tail = stderr_tail(&text);
        assert_eq!(tail.lines().count(), 20);
        assert!(tail.lines().all(|l| l.chars().count() <= 500));
        assert!(tail.starts_with("10x"));
    }

    struct DropProbe {
        home: Home,
        lock_held_at_drop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Spawner for DropProbe {
        fn spawn(
            &self,
            _: &Home,
            _: &Request,
            _: &Path,
            _: &Path,
        ) -> Result<u32, crate::spawn::SpawnError> {
            Err(crate::spawn::SpawnError::Unsupported)
        }
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            let held = InstanceLock::acquire(&self.home).is_err();
            self.lock_held_at_drop
                .store(held, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn the_instance_lock_outlives_the_other_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = Home::at(tmp.path().join("home"));
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = DropProbe {
            home: home.clone(),
            lock_held_at_drop: flag.clone(),
        };
        let sup = Supervisor::new_locked(home, Box::new(probe), Box::new(|| 1)).expect("locked");
        drop(sup);
        assert!(
            flag.load(std::sync::atomic::Ordering::SeqCst),
            "the lock was already released when the spawner dropped"
        );
    }

    fn result_roundtrip(outcome: Outcome) -> Outcome {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = Home::at(tmp.path().join("home"));
        let id = crate::request::make_id(1, "p", "1");
        let sup = Supervisor::new(
            home.clone(),
            Box::new(DropProbe {
                home: home.clone(),
                lock_held_at_drop: Default::default(),
            }),
            Box::new(|| 1),
        );
        sup.record_result(&id, "p", "1", outcome).expect("record");
        let read = records::read_results(&home);
        assert_eq!(read.len(), 1);
        read.into_iter()
            .next()
            .expect("one")
            .expect("readable")
            .outcome
    }

    #[test]
    fn a_huge_stdout_line_still_gives_a_readable_result() {
        let huge = "x".repeat(64 * 1024);
        for line in [format!("needs-human: {huge}"), format!("parked: {huge}")] {
            let outcome = classify(&format!("{line}\n"), "", None);
            match result_roundtrip(outcome) {
                Outcome::NeedsHuman { reason } => {
                    assert!(reason.chars().count() <= LINE_CAP + 1, "{}", reason.len())
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn an_escape_heavy_stderr_tail_stays_readable() {
        // Control chars cost six bytes each in JSON, and quotes and backslashes two.
        let err: String = (0..40)
            .map(|_| format!("{}\n", "\u{1}\"\\".repeat(400)))
            .collect();
        let outcome = classify("", &err, Some(Some(1)));
        match result_roundtrip(outcome) {
            Outcome::Failed { stderr_tail, .. } => {
                assert!(stderr_tail.chars().count() <= TAIL_CHARS + 1)
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn write_result_refuses_a_record_beyond_the_read_cap() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = Home::at(tmp.path().join("home"));
        let rec = ResultRecord {
            request_id: crate::request::make_id(1, "p", "1"),
            project: "p".into(),
            job: "1".into(),
            outcome: Outcome::Lost {
                reason: "x".repeat(70 * 1024),
            },
            finished_ms: 1,
        };
        assert!(records::write_result(&home, &rec).is_err());
        assert!(records::read_results(&home).is_empty());
    }

    #[test]
    fn cap_text_cuts_on_a_char_boundary() {
        assert_eq!(cap_text("héllo", 2), "hé…");
        assert_eq!(cap_text("abc", 3), "abc");
    }
}
