//! What this machine says about a pid: whether it exists, whether it is a zombie, and when it started.
//!
//! One library (`sysinfo`) answers on Linux and macOS. Each call refreshes only the pid it asks about
//! (and this process, as a check), never the whole process table. When the lookup cannot work (an
//! unsupported system, or a Linux without `/proc` mounted, where even this process is not found),
//! every pid is trusted: it exists, is no zombie, and has no known start time.

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

/// The state of a pid on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    /// The pid runs (or sleeps, or is stopped).
    Running,
    /// The pid ended and waits for its parent to reap it. Only Linux tells this apart: on macOS
    /// `sysinfo` cannot read a zombie, so it reads as Gone. Either way it is not alive.
    Zombie,
    /// No process has this pid.
    Gone,
}

/// What one lookup found: the state, and the start in milliseconds since the epoch when known.
fn lookup(pid: u32) -> (ProcState, Option<u64>) {
    const TRUSTED: (ProcState, Option<u64>) = (ProcState::Running, None);
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        return TRUSTED;
    }
    let (pid, me) = (Pid::from_u32(pid), Pid::from_u32(std::process::id()));
    let mut sys = System::new();
    // Each pid only once: sysinfo drops a pid that is listed twice.
    let pids = if pid == me { vec![pid] } else { vec![pid, me] };
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing());
    // A lookup that cannot even see this process is broken, so it says nothing about the pid.
    if sys.process(me).is_none() {
        return TRUSTED;
    }
    match sys.process(pid) {
        None => (ProcState::Gone, None),
        Some(p) => {
            let state = if p.status() == ProcessStatus::Zombie { ProcState::Zombie } else { ProcState::Running };
            // sysinfo reports whole seconds. 0 means it does not know.
            let start = p.start_time();
            (state, (start > 0).then(|| start.saturating_mul(1000)))
        }
    }
}

/// The state of the pid.
pub fn process_state(pid: u32) -> ProcState {
    lookup(pid).0
}

/// True unless this machine says no process has the pid. A zombie exists on Linux, not on macOS.
pub fn pid_exists(pid: u32) -> bool {
    process_state(pid) != ProcState::Gone
}

/// True when the pid ended and waits for its parent to reap it. A zombie does not count as alive.
pub fn pid_is_zombie(pid: u32) -> bool {
    process_state(pid) == ProcState::Zombie
}

/// When the pid started, in milliseconds since the epoch, to the second. None when this machine does not say.
pub fn process_start_ms(pid: u32) -> Option<u64> {
    lookup(pid).1
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn sleeper() -> Child {
        Command::new("sleep").arg("60").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("sleep")
    }

    fn now_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    /// Waits until the pid no longer runs, and fails after a while.
    fn wait_ended(pid: u32) -> ProcState {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let s = process_state(pid);
            if s != ProcState::Running {
                return s;
            }
            assert!(Instant::now() < end, "pid {pid} kept running");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_running_sleep_exists_and_is_no_zombie() {
        let mut c = sleeper();
        assert_eq!(process_state(c.id()), ProcState::Running);
        assert!(pid_exists(c.id()));
        assert!(!pid_is_zombie(c.id()));
        c.kill().expect("kill");
        c.wait().expect("wait");
    }

    #[test]
    fn this_process_exists_and_started_in_the_past() {
        let me = std::process::id();
        assert!(pid_exists(me));
        assert!(!pid_is_zombie(me));
        let start = process_start_ms(me).expect("start time");
        assert!(start > 0 && start <= now_ms() + 1_000, "{start}");
    }

    #[test]
    fn a_killed_and_reaped_sleep_is_gone() {
        let mut c = sleeper();
        let pid = c.id();
        c.kill().expect("kill");
        c.wait().expect("wait");
        assert_eq!(process_state(pid), ProcState::Gone);
        assert!(!pid_exists(pid));
        assert_eq!(process_start_ms(pid), None);
    }

    #[test]
    fn a_killed_but_unreaped_sleep_is_a_zombie_and_not_alive() {
        let mut c = sleeper();
        let pid = c.id();
        c.kill().expect("kill");
        let state = wait_ended(pid);
        // The child stays unreaped until `wait` below, so nothing else can take its pid.
        #[cfg(target_os = "linux")]
        {
            assert_eq!(state, ProcState::Zombie);
            assert!(pid_exists(pid));
            assert!(pid_is_zombie(pid));
        }
        #[cfg(not(target_os = "linux"))]
        assert_eq!(state, ProcState::Gone, "macOS cannot read a zombie");
        assert_ne!(process_state(pid), ProcState::Running);
        c.wait().expect("wait");
    }

    #[test]
    fn a_fresh_child_started_just_now() {
        let before = now_ms();
        let mut c = sleeper();
        let start = process_start_ms(c.id()).expect("start time");
        let after = now_ms();
        c.kill().expect("kill");
        c.wait().expect("wait");
        // Whole seconds, so the start may read up to a second before `before`.
        assert!(start + 2_000 >= before && start <= after + 2_000, "start {start}, window {before}..{after}");
    }
}
