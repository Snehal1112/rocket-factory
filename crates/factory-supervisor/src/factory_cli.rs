//! Running `factory --project <p> <cmd>` as a child process, for the commands only factory-cli implements.
//!
//! Arguments go to `Command` one by one, never through a shell.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use factory_core::Home;

use crate::spawn::ProcessSpawner;

const HOME_ENV: &str = "FACTORY_HOME";
const TIMEOUT: Duration = Duration::from_secs(60);
/// Output kept per stream. A command prints a few lines, so this is generous.
const OUTPUT_CAP: u64 = 64 * 1024;

/// What a finished command printed. `code` is -1 when a signal ended it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CliSpawnError {
    #[error("cannot find the factory binary: {0}")]
    NoBin(String),
    #[error("cannot run {path}: {reason}")]
    Start { path: PathBuf, reason: String },
    #[error("the factory command took longer than {0} seconds")]
    Timeout(u64),
}

pub trait FactoryCli: Send + Sync {
    /// Runs `factory --project <project> <args...>` and waits for it.
    fn run(&self, project: &str, args: &[String]) -> Result<CliOutput, CliSpawnError>;
}

/// Runs the real `factory` binary.
#[derive(Debug)]
pub struct ProcessFactoryCli {
    bin: PathBuf,
    home: PathBuf,
}

impl ProcessFactoryCli {
    pub fn new(bin: PathBuf, home: PathBuf) -> ProcessFactoryCli {
        ProcessFactoryCli { bin, home }
    }

    /// The binary is found like the spawner finds it: `FACTORY_BIN`, else next to this executable.
    pub fn from_env() -> Result<ProcessFactoryCli, CliSpawnError> {
        let spawner =
            ProcessSpawner::from_env().map_err(|e| CliSpawnError::NoBin(e.to_string()))?;
        let home = Home::from_env().map_err(|e| CliSpawnError::NoBin(e.to_string()))?;
        Ok(ProcessFactoryCli::new(spawner.bin, home.dir))
    }
}

/// Reads at most `OUTPUT_CAP` bytes and drops the rest, so a chatty child never blocks on a full pipe.
fn read_capped<R: Read>(stream: Option<R>) -> String {
    let mut bytes = Vec::new();
    if let Some(mut s) = stream {
        let _ = (&mut s).take(OUTPUT_CAP).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut s, &mut std::io::sink());
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What `drain` returns: a running reader thread, or the stream when no thread could be started.
enum Drain<R> {
    Thread(std::thread::JoinHandle<String>),
    Unstarted(Option<R>),
}

impl<R: Read> Drain<R> {
    fn finish(self) -> String {
        match self {
            Drain::Thread(t) => t.join().unwrap_or_default(),
            // Without a thread the pipe is read here, after the child has ended.
            Drain::Unstarted(s) => read_capped(s),
        }
    }
}

fn drain<R: Read + Send + 'static>(stream: Option<R>, name: &str) -> Drain<R> {
    // The stream moves into the closure, so a refused thread needs the shared cell to get it back.
    let cell = std::sync::Arc::new(std::sync::Mutex::new(stream));
    let theirs = cell.clone();
    let spawned = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let taken = theirs.lock().ok().and_then(|mut s| s.take());
            read_capped(taken)
        });
    match spawned {
        Ok(t) => Drain::Thread(t),
        Err(_) => Drain::Unstarted(cell.lock().ok().and_then(|mut s| s.take())),
    }
}

impl FactoryCli for ProcessFactoryCli {
    fn run(&self, project: &str, args: &[String]) -> Result<CliOutput, CliSpawnError> {
        let start = |reason: String| CliSpawnError::Start {
            path: self.bin.clone(),
            reason,
        };
        let mut child = Command::new(&self.bin)
            .arg("--project")
            .arg(project)
            .args(args)
            .env(HOME_ENV, &self.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| start(e.to_string()))?;
        let out = drain(child.stdout.take(), "factoryd-cli-stdout");
        let err = drain(child.stderr.take(), "factoryd-cli-stderr");
        let began = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if began.elapsed() >= TIMEOUT => {
                    // This child was started here, so stopping it is ours to do.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CliSpawnError::Timeout(TIMEOUT.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(start(e.to_string()));
                }
            }
        };
        Ok(CliOutput {
            code: status.code().unwrap_or(-1),
            stdout: out.finish(),
            stderr: err.finish(),
        })
    }
}
