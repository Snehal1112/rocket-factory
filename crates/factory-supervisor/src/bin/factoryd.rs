//! factoryd: the supervisor daemon. It runs queued factory requests, a few at a time.

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use factory_core::{Home, HomeConfig};
use factory_supervisor::api::{ApiDeps, ApiServer};
use factory_supervisor::lock::stop_file;
use factory_supervisor::token;
use factory_supervisor::request::{Action, Request};
use factory_supervisor::{pid_alive, queue, records, InstanceLock, Outcome, ProcessFactoryCli, ProcessSpawner, QueueItem, Supervisor};

const DEFAULT_INTERVAL_MS: u64 = 2000;
const RECENT_RESULTS: usize = 10;

#[derive(Parser)]
#[command(name = "factoryd", about = "Runs queued factory requests")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the supervisor loop.
    Run {
        #[arg(long, default_value_t = DEFAULT_INTERVAL_MS)]
        interval_ms: u64,
        /// Do one tick and exit.
        #[arg(long)]
        once: bool,
    },
    /// Show the queue, the running children and the last results.
    Status,
    /// Ask a running instance to stop. Running children keep running.
    Stop,
    /// Exit 0 when a factoryd runs for this home, else exit 1. Prints nothing.
    IsRunning,
    /// Add a request to the queue.
    Enqueue {
        #[arg(long)]
        project: String,
        #[arg(long)]
        job: String,
        #[arg(long)]
        series: bool,
        #[arg(long)]
        approve: bool,
    },
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn run(home: Home, interval_ms: u64, once: bool) -> Result<(), String> {
    let cfg = HomeConfig::load(&home).map_err(|e| e.to_string())?;
    let spawner = ProcessSpawner::from_env().map_err(|e| e.to_string())?;
    // The supervisor holds the instance lock for its whole life.
    let mut sup = Supervisor::new_locked(home.clone(), Box::new(spawner), Box::new(now_ms)).map_err(|e| e.to_string())?;
    // A stop file left from an earlier run must not end this one. It is cleared only under the lock.
    let _ = std::fs::remove_file(stop_file(&home));
    sup.set_max_parallel(cfg.max_parallel);
    if once {
        let report = sup.tick().map_err(|e| e.to_string())?;
        for line in factory_supervisor::supervisor::new_lines(&mut std::collections::HashSet::new(), &report) {
            eprintln!("{line}");
        }
        for line in &report.notices {
            eprintln!("{line}");
        }
        println!("started {}, finished {}, rejected {}", report.started.len(), report.finished.len(), report.rejected.len());
        return Ok(());
    }
    // A bind failure ends factoryd here, before any job is taken.
    let listen: std::net::SocketAddr = cfg.listen.parse().map_err(|e| format!("listen {:?}: {e}", cfg.listen))?;
    token::ensure(&home).map_err(|e| e.to_string())?;
    let cli = ProcessFactoryCli::from_env().map_err(|e| e.to_string())?;
    let deps = ApiDeps { now_ms: std::sync::Arc::new(now_ms), cli: std::sync::Arc::new(cli) };
    let api = ApiServer::start(home.clone(), listen, deps).map_err(|e| e.to_string())?;
    println!("api listening on {}", api.addr);
    let stop_home = home.clone();
    sup.run_loop(factory_supervisor::supervisor::clamp_interval(Duration::from_millis(interval_ms)), &move || Supervisor::stop_requested(&stop_home));
    api.stop();
    Ok(())
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Merged => "merged".to_string(),
        Outcome::NeedsHuman { reason } => format!("needs-human: {reason}"),
        Outcome::Failed { code, stderr_tail } => {
            let last = stderr_tail.lines().last().unwrap_or("");
            format!("failed (code {}) {last}", code.map(|c| c.to_string()).unwrap_or_else(|| "none".into()))
        }
        Outcome::Lost { reason } => format!("lost: {reason}"),
        Outcome::Rejected { reason } => format!("rejected: {reason}"),
    }
}

fn status(home: &Home) -> String {
    let mut out = String::new();
    let limit = HomeConfig::load(home).ok().and_then(|c| c.daily_budget_weighted);
    if let Some(b) = factory_supervisor::daily::status(home, limit, now_ms()) {
        out.push_str(&factory_supervisor::daily::status_line(&b));
        out.push('\n');
    }
    out.push_str("Queue:\n");
    let items = queue::list(home);
    if items.is_empty() {
        out.push_str("  (empty)\n");
    }
    for item in items {
        match item {
            QueueItem::Ok(r) => out.push_str(&format!("  {} {} job {}\n", r.id, r.project, r.job)),
            QueueItem::Bad { file, reason } => out.push_str(&format!("  bad {file}: {reason}\n")),
        }
    }
    out.push_str("Running:\n");
    let now = now_ms();
    let mut any = false;
    for child in records::read_children(home) {
        any = true;
        match child {
            Ok(c) => {
                let age = now.saturating_sub(c.started_ms) / 1000;
                let state = if pid_alive(c.pid) { "" } else { " (ended, waiting for the supervisor)" };
                out.push_str(&format!("  {} job {} pid {} age {age}s{state}\n", c.project, c.job, c.pid));
            }
            Err(reason) => out.push_str(&format!("  bad record: {reason}\n")),
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out.push_str("Results:\n");
    let mut results: Vec<_> = records::read_results(home).into_iter().flatten().collect();
    results.sort_by(|a, b| b.finished_ms.cmp(&a.finished_ms).then_with(|| b.request_id.cmp(&a.request_id)));
    if results.is_empty() {
        out.push_str("  (none)\n");
    }
    for r in results.into_iter().take(RECENT_RESULTS) {
        out.push_str(&format!("  {} {} job {}: {}\n", r.request_id, r.project, r.job, describe(&r.outcome)));
    }
    out
}

/// True while some factoryd holds the instance lock of this home.
fn is_running(home: &Home) -> bool {
    InstanceLock::running(home).is_some()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Cmd::IsRunning = cli.cmd {
        return match Home::from_env() {
            Ok(home) if is_running(&home) => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        };
    }
    let result = Home::from_env().map_err(|e| e.to_string()).and_then(|home| match cli.cmd {
        Cmd::Run { interval_ms, once } => run(home, interval_ms, once),
        Cmd::Status => {
            print!("{}", status(&home));
            Ok(())
        }
        Cmd::Stop => {
            let dir = home.run_dir();
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            std::fs::write(stop_file(&home), b"").map_err(|e| e.to_string())?;
            match InstanceLock::running(&home) {
                Some(pid) => println!("stop requested, factoryd pid {pid} will end after its current tick; running jobs keep running"),
                None => println!("no factoryd is running; the stop file is removed when the next one starts"),
            }
            Ok(())
        }
        Cmd::IsRunning => Ok(()),
        Cmd::Enqueue { project, job, series, approve } => {
            let action = if approve { Action::Approve } else { Action::Run };
            let req = Request::new(&project, action, &job, series, now_ms()).map_err(|e| e.to_string())?;
            queue::enqueue(&home, &req).map_err(|e| e.to_string())?;
            println!("{}", req.id);
            Ok(())
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("factoryd: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_shows_the_daily_budget_only_with_a_limit() {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().to_path_buf());
        assert!(!status(&home).contains("daily budget"));
        std::fs::write(home.config_file(), "daily_budget_weighted = 1000\n").expect("config");
        let text = status(&home);
        assert!(text.starts_with("daily budget: 0 of 1000 weighted tokens today (0%), resets at "), "{text}");
    }

    #[test]
    fn is_running_follows_the_instance_lock() {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().to_path_buf());
        assert!(!is_running(&home));
        let lock = InstanceLock::acquire(&home).expect("lock");
        assert!(is_running(&home));
        drop(lock);
        assert!(!is_running(&home));
    }
}
