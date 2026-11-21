//! Waits for the machine when the disk, the network or the API key is down, instead of parking at once.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use factory_agent::{AgentRun, MachineCause};
use factory_core::{ParkKind, Stage};

use crate::{Engine, EngineError, StageOutcome};

/// The probe times in seconds after the wait starts. After the last one, a probe runs every 15 minutes.
const PROBE_AT: [f64; 5] = [60.0, 120.0, 240.0, 480.0, 900.0];
/// The time between the later probes.
const PROBE_EVERY: f64 = 900.0;
/// The longest wait. A last probe runs at the cap, and a failed one parks the job.
const WAIT_CAP: f64 = 1500.0;
/// The longest single sleep, so a cancel or a lost lease ends the wait quickly.
const SLEEP_STEP: f64 = 5.0;
/// The most characters of a detail kept in park reasons and ledger lines.
const DETAIL_CAP: usize = 200;

/// Gives the free space in MiB on the file system that holds a path, or `None` when it is unknown.
type FreeMb = dyn Fn(&Path) -> Option<u64> + Send + Sync;

/// The free disk space, the sleep and the clock the wait uses. Tests pass fakes.
#[derive(Clone)]
pub struct MachineWait {
    /// The free space in MiB on the file system that holds the path, or `None` when it is unknown.
    pub free_mb: Arc<FreeMb>,
    pub sleep: Arc<dyn Fn(Duration) + Send + Sync>,
    /// Seconds since the Unix epoch.
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

impl Default for MachineWait {
    fn default() -> Self {
        MachineWait {
            free_mb: Arc::new(free_mb),
            sleep: Arc::new(std::thread::sleep),
            now: Arc::new(crate::clock::now_secs),
        }
    }
}

/// The free space in MiB at `path`, measured at its nearest existing folder.
fn free_mb(path: &Path) -> Option<u64> {
    let dir = path.ancestors().find(|p| p.exists())?;
    fs2::available_space(dir).ok().map(|b| b / (1024 * 1024))
}

/// What one probe found.
pub(crate) enum Probe {
    /// The machine is fine again.
    Fine,
    /// The machine is still down.
    StillDown,
    /// The probe ran the agent, and its run is the result.
    #[allow(dead_code)] // The builder rerun uses it.
    Final(AgentRun),
    /// The probe parked the job.
    #[allow(dead_code)] // The builder rerun uses it.
    Parked(StageOutcome),
}

/// How a wait ended.
pub(crate) enum WaitEnd {
    Ready,
    #[allow(dead_code)] // The builder rerun uses it.
    Final(AgentRun),
    Parked(StageOutcome),
}

/// The first `cap` characters of `text`.
fn capped(text: &str, cap: usize) -> String {
    text.chars().take(cap).collect()
}

/// The short form `machine_wait` holds: `disk: 0.4 GB free`, `network` or `auth`.
fn short_form(cause: MachineCause, detail: &str) -> String {
    match cause {
        MachineCause::Disk => {
            let rest = detail.strip_prefix("disk full: ").unwrap_or(detail);
            let free = rest.split(" on ").next().unwrap_or(rest);
            format!("disk: {free}")
        }
        other => other.as_str().to_string(),
    }
}

/// The park reason at the cap, `<detail>, waited 25 min`. The detail is cut so the whole reason fits the cap.
fn park_reason(detail: &str) -> String {
    let suffix = format!(", waited {} min", (WAIT_CAP / 60.0) as u64);
    let room = DETAIL_CAP.saturating_sub(suffix.chars().count());
    format!("{}{suffix}", capped(detail, room))
}

/// The probe times after the start, ending with one at the cap.
fn probe_times() -> Vec<f64> {
    let mut times = PROBE_AT.to_vec();
    let mut next = PROBE_AT[PROBE_AT.len() - 1] + PROBE_EVERY;
    while next < WAIT_CAP {
        times.push(next);
        next += PROBE_EVERY;
    }
    times.push(WAIT_CAP);
    times
}

impl Engine<'_> {
    /// Replaces the disk check, the sleep and the clock of the machine wait.
    pub fn with_machine_wait(mut self, machine: MachineWait) -> Self {
        self.machine = machine;
        self
    }

    /// What is down on the machine before a builder runs in `dir`, or `None` when nothing is.
    pub(crate) fn preflight(&self, dir: &Path) -> Option<(MachineCause, String)> {
        let need = self.config.min_free_disk_mb;
        if need > 0 {
            let board_dir = self.board_path.parent().unwrap_or(Path::new("."));
            for path in [dir, board_dir] {
                if let Some(free) = (self.machine.free_mb)(path).filter(|free| *free < need) {
                    let gb = free as f64 / 1024.0;
                    let detail = format!("disk full: {gb:.1} GB free on {}", path.display());
                    return Some((MachineCause::Disk, detail));
                }
            }
        }
        if self.config.agent_cli() == "claude" {
            if let Some(key) = self.config.api_key_path() {
                if let Err(e) = factory_agent::check_key_file(&key) {
                    return Some((MachineCause::Auth, format!("auth: {e}")));
                }
            }
        }
        None
    }

    /// Clears a `machine_wait` a killed run left, then waits while `preflight` finds the machine down.
    /// `Some` holds the outcome when the wait parked the job.
    pub(crate) fn machine_gate(
        &self,
        worktree: Option<&Path>,
    ) -> Result<Option<StageOutcome>, EngineError> {
        self.set_machine_wait(None);
        let dir = worktree.unwrap_or(self.repo.worktrees_dir()).to_path_buf();
        let Some((cause, detail)) = self.preflight(&dir) else {
            return Ok(None);
        };
        let mut probe = || {
            Ok(match self.preflight(&dir) {
                None => Probe::Fine,
                Some(_) => Probe::StillDown,
            })
        };
        match self.wait_for_machine(cause, &detail, &mut probe)? {
            WaitEnd::Ready | WaitEnd::Final(_) => Ok(None),
            WaitEnd::Parked(out) => Ok(Some(out)),
        }
    }

    /// Sets or clears `machine_wait` on the board. A failed write is logged and skipped.
    fn set_machine_wait(&self, value: Option<String>) {
        if let Err(e) = self.update(|j| {
            j.machine_wait = value;
            Ok(())
        }) {
            tracing::warn!(event = "builder.wait", msg = %format!("cannot write machine_wait: {e}"));
        }
    }

    /// Appends a ledger ruling. A failed write is logged and skipped.
    fn ruling_best_effort(&self, text: &str) {
        if let Err(e) = self.ruling(text) {
            tracing::warn!(event = "builder.wait", msg = %format!("cannot write the ledger: {e}"));
        }
    }

    /// Sleeps and probes on the fixed schedule until the machine is fine, the probe ends the wait, or the
    /// cap is reached, which parks the job. A cancel or a lost lease ends it within one sleep step.
    pub(crate) fn wait_for_machine(
        &self,
        cause: MachineCause,
        detail: &str,
        probe: &mut dyn FnMut() -> Result<Probe, EngineError>,
    ) -> Result<WaitEnd, EngineError> {
        let detail = capped(&crate::ports::one_line(detail), DETAIL_CAP);
        self.set_machine_wait(Some(short_form(cause, &detail)));
        let start = (self.machine.now)();
        let end = self.wait_loop(cause, &detail, start, probe);
        self.set_machine_wait(None);
        let waited = ((self.machine.now)() - start).max(0.0) as u64;
        let outcome = match &end {
            Ok(WaitEnd::Ready | WaitEnd::Final(_)) => "ready",
            Ok(WaitEnd::Parked(_)) => "parked",
            Err(_) => "stopped",
        };
        tracing::info!(
            event = "builder.wait_end",
            cause = cause.as_str(),
            detail = %detail,
            outcome,
            waited_secs = waited,
            msg = %format!("machine wait ended: {outcome} after {waited}s")
        );
        if let Ok(WaitEnd::Ready | WaitEnd::Final(_)) = &end {
            self.ruling_best_effort(&format!("machine ok after {} min", waited / 60));
        }
        end
    }

    fn wait_loop(
        &self,
        cause: MachineCause,
        detail: &str,
        start: f64,
        probe: &mut dyn FnMut() -> Result<Probe, EngineError>,
    ) -> Result<WaitEnd, EngineError> {
        self.ruling_best_effort(&format!("waiting for the machine: {detail}"));
        tracing::info!(
            event = "builder.wait",
            cause = cause.as_str(),
            detail = %detail,
            msg = %format!("waiting for the machine: {detail}")
        );
        for at in probe_times() {
            loop {
                let elapsed = (self.machine.now)() - start;
                if elapsed >= at {
                    break;
                }
                let step = (at - elapsed).min(SLEEP_STEP);
                (self.machine.sleep)(Duration::from_secs_f64(step));
                self.check_still_wanted()?;
            }
            let found = probe()?;
            let state = match &found {
                Probe::Fine => "fine",
                Probe::StillDown => "down",
                Probe::Final(_) => "final",
                Probe::Parked(_) => "parked",
            };
            tracing::info!(
                event = "builder.probe",
                cause = cause.as_str(),
                detail = %detail,
                at_secs = at as u64,
                state,
                msg = %format!("machine probe at {}s: {state}", at as u64)
            );
            match found {
                Probe::Fine => return Ok(WaitEnd::Ready),
                Probe::StillDown => {}
                Probe::Final(run) => return Ok(WaitEnd::Final(run)),
                Probe::Parked(out) => return Ok(WaitEnd::Parked(out)),
            }
        }
        self.park_with(ParkKind::MachineWaiting, park_reason(detail), Vec::new())
            .map(WaitEnd::Parked)
    }

    /// Ends the wait when the job was cancelled, left Build, or the run lost its lease.
    fn check_still_wanted(&self) -> Result<(), EngineError> {
        if self.lost() {
            return Err(self
                .lost
                .as_ref()
                .map(|flag| flag.error())
                .unwrap_or(EngineError::Cancelled));
        }
        let job = self.job()?;
        if job.cancelled || job.stage != Stage::Build {
            return Err(EngineError::Cancelled);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_run_on_the_schedule_and_end_at_the_cap() {
        assert_eq!(
            probe_times(),
            vec![60.0, 120.0, 240.0, 480.0, 900.0, 1500.0]
        );
    }

    #[test]
    fn short_form_drops_the_path() {
        let short = short_form(MachineCause::Disk, "disk full: 0.4 GB free on /x/y");
        assert_eq!(short, "disk: 0.4 GB free");
        assert_eq!(
            short_form(MachineCause::Network, "network: down"),
            "network"
        );
        assert_eq!(short_form(MachineCause::Auth, "auth: bad key"), "auth");
    }

    #[test]
    fn capped_keeps_the_first_characters() {
        assert_eq!(capped("abcdef", 3), "abc");
        assert_eq!(capped("ab", 3), "ab");
    }

    #[test]
    fn park_reason_fits_the_cap() {
        let detail = format!("auth: {}", "x".repeat(300));
        let reason = park_reason(&detail);
        assert!(reason.chars().count() <= 200, "{}", reason.len());
        assert!(reason.ends_with(", waited 25 min"), "{reason}");
        assert!(reason.starts_with("auth: x"), "{reason}");
        assert_eq!(park_reason("network: down"), "network: down, waited 25 min");
    }
}
