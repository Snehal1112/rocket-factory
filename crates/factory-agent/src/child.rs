use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use factory_core::{budget_state, BudgetState};

use crate::claude::AgentError;
use crate::events::{AgentEvent, EventKind, EventSink};
use crate::run::{AgentRun, StopReason};
use crate::spec::AgentSpec;
use crate::stream::StreamEvent;
use crate::usage::Usage;

const POLL: Duration = Duration::from_millis(100);
const MAX_LINE: usize = 1 << 20;
const STDERR_TAIL: usize = 4096;

/// Folds the stdout lines of one agent CLI into run state.
pub trait AgentStream {
    fn feed(&mut self, line: &str) -> Vec<StreamEvent>;
    fn usage(&self) -> Usage;
    fn final_usage(&self) -> Usage;
    fn turns(&self) -> u32;
    fn has_result(&self) -> bool;
    fn result_text(&self) -> Option<String>;
    fn result_is_error(&self) -> bool;
    fn result_subtype(&self) -> Option<String>;
    fn session_id(&self) -> Option<String>;
    fn start_ctx(&self) -> u64;
    fn end_ctx(&self) -> u64;
    fn start_cache(&self) -> (u64, u64);
}

/// What differs between the runners: the command line and a few switches.
#[derive(Debug)]
pub(crate) struct ChildConfig {
    pub program: OsString,
    pub args: Vec<String>,
    pub stdin: String,
    /// Name of the CLI, used in the error texts.
    pub label: String,
    /// Variables removed from the child after the spec env is applied.
    pub remove_env: Vec<&'static str>,
    pub honor_max_turns: bool,
    pub mid_run_budget: bool,
}

enum Msg {
    Line(String),
    Eof,
}

fn event(spec: &AgentSpec, kind: EventKind, tokens: u64) -> AgentEvent {
    AgentEvent {
        kind,
        agent_id: spec.agent_id(),
        agent_type: spec.agent.clone().unwrap_or_else(|| spec.role.clone()),
        tool_name: None,
        tool_target: None,
        cwd: Some(spec.cwd.display().to_string()),
        last_message: None,
        tokens,
    }
}

/// Runs the child to the end and folds its output through `stream`.
pub(crate) fn drive<S: AgentStream>(
    cfg: &ChildConfig,
    spec: &AgentSpec,
    sink: &EventSink,
    mut state: S,
) -> Result<AgentRun, AgentError> {
    let mut command = Command::new(&cfg.program);
    command
        .args(&cfg.args)
        .current_dir(&spec.cwd)
        .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Removed after the spec env, so nothing can bring the variables back by accident.
    for var in &cfg.remove_env {
        command.env_remove(var);
    }
    let mut child = command
        .spawn()
        .map_err(|e| AgentError::Spawn(cfg.program.to_string_lossy().into_owned(), e))?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(AgentError::Pipe);
    };

    let prompt = cfg.stdin.clone();
    let writer = thread::spawn(move || {
        if let Some(mut s) = stdin {
            // The child may exit before reading everything, so errors are ignored.
            let _ = s.write_all(prompt.as_bytes());
        }
    });
    let err_thread = thread::spawn(move || tail_of(stderr, STDERR_TAIL));
    let (tx, rx) = mpsc::channel::<Msg>();
    let reader = thread::spawn(move || read_lines(stdout, tx));

    let mut last_event = Instant::now();
    let mut stop: Option<StopReason> = None;
    let mut warned = false;
    let mut session_seen = false;
    sink.emit(&event(spec, EventKind::Start, spec.used_before));

    loop {
        match rx.recv_timeout(POLL) {
            Ok(Msg::Line(line)) => {
                last_event = Instant::now();
                let events = state.feed(&line);
                let used = spec.used_before.saturating_add(state.usage().weighted());
                for ev in events {
                    match ev {
                        StreamEvent::Tool { name, target } => {
                            let mut e = event(spec, EventKind::Tool, used);
                            e.tool_name = Some(name);
                            e.tool_target = Some(target);
                            sink.emit(&e);
                        }
                        StreamEvent::Init { session_id } if !session_seen => {
                            session_seen = true;
                            if let Some(hook) = &spec.on_session {
                                hook.call(&session_id);
                                let mut e = event(spec, EventKind::Session, used);
                                e.last_message = Some(session_id);
                                sink.emit(&e);
                            }
                        }
                        _ => {}
                    }
                }
                if cfg.mid_run_budget {
                    if let Some(limit) = spec.budget_weighted {
                        match budget_state(used, limit) {
                            BudgetState::Stop => {
                                stop = Some(StopReason::BudgetStop);
                                break;
                            }
                            BudgetState::Warn if !warned => {
                                warned = true;
                                sink.emit(&event(spec, EventKind::BudgetWarn, used));
                            }
                            _ => {}
                        }
                    }
                }
                if cfg.honor_max_turns {
                    if let Some(max) = spec.max_turns {
                        if state.turns() > max {
                            stop = Some(StopReason::TurnLimit);
                            break;
                        }
                    }
                }
            }
            Ok(Msg::Eof) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(limit) = spec.stall_after {
                    if last_event.elapsed() >= limit {
                        stop = Some(StopReason::Stalled);
                        break;
                    }
                }
            }
        }
    }

    let killed = stop.is_some();
    if killed {
        let _ = child.kill();
    }
    let status = child.wait().map_err(AgentError::Wait)?;
    let stderr_tail = if killed {
        // A grandchild may still hold the pipes, so the helper threads are left to end on their own.
        String::new()
    } else {
        let _ = writer.join();
        let _ = reader.join();
        err_thread.join().unwrap_or(String::new())
    };

    let usage = state.final_usage();
    let weighted = usage.weighted();
    let exit_code = status.code();
    let result_text = state.result_text().unwrap_or_default();
    let label = &cfg.label;
    let (stop_reason, error) = match stop {
        Some(reason) => (reason, None),
        None if !state.has_result() => {
            let msg = format!(
                "{label} exited without a result (exit {exit_code:?}): {}",
                stderr_tail.trim()
            );
            (StopReason::Failed, Some(msg))
        }
        None if state.result_is_error() => {
            let subtype = state.result_subtype().unwrap_or_default();
            let reason = if cfg.honor_max_turns && subtype.contains("max_turns") {
                StopReason::TurnLimit
            } else {
                StopReason::Failed
            };
            let detail = if label == "claude" || result_text.is_empty() {
                String::new()
            } else {
                format!(": {}", result_text.trim())
            };
            (reason, Some(format!("{label} reported {subtype}{detail}")))
        }
        None => (StopReason::Done, None),
    };

    let mut stop_event = event(
        spec,
        EventKind::Stop,
        spec.used_before.saturating_add(weighted),
    );
    stop_event.last_message = Some(if result_text.is_empty() {
        error.clone().unwrap_or(String::new())
    } else {
        result_text.clone()
    });
    sink.emit(&stop_event);

    let (start_cc, start_cr) = state.start_cache();
    Ok(AgentRun {
        session_id: state.session_id(),
        usage,
        weighted,
        result_text,
        stop_reason,
        turns: state.turns(),
        start_ctx: state.start_ctx(),
        end_ctx: state.end_ctx(),
        start_cc,
        start_cr,
        exit_code,
        error,
    })
}

/// Forwards stdout lines. It always drains to EOF, so the child never blocks on a full pipe.
fn read_lines(stdout: impl Read, tx: mpsc::Sender<Msg>) {
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                buf.truncate(MAX_LINE);
                let line = String::from_utf8_lossy(&buf).into_owned();
                if tx.send(Msg::Line(line)).is_err() {
                    // The runner stopped listening. Keep draining so the child is not blocked.
                    let _ = std::io::copy(&mut reader, &mut std::io::sink());
                    return;
                }
            }
            Err(_) => {
                let _ = std::io::copy(&mut reader, &mut std::io::sink());
                break;
            }
        }
    }
    let _ = tx.send(Msg::Eof);
}

/// Reads a stream to the end and keeps only the last `keep` bytes.
fn tail_of(mut r: impl Read, keep: usize) -> String {
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > keep {
                    let cut = tail.len() - keep;
                    tail.drain(..cut);
                }
            }
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}
