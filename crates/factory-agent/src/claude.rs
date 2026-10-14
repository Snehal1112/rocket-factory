use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use factory_core::{budget_state, BudgetState};

use crate::events::{AgentEvent, EventKind, EventSink};
use crate::run::{AgentRun, StopReason};
use crate::runner::AgentRunner;
use crate::spec::{AgentSpec, Caps};
use crate::stream::{StreamEvent, StreamState};

const POLL: Duration = Duration::from_millis(100);
const MAX_LINE: usize = 1 << 20;
const STDERR_TAIL: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("cannot start the agent process: {0}")]
    Spawn(std::io::Error),
    #[error("the agent process has no stdout or stderr pipe")]
    Pipe,
    #[error("waiting for the agent process failed: {0}")]
    Wait(std::io::Error),
}

/// Runs headless `claude -p` and reads its stream-json output.
#[derive(Debug)]
pub struct ClaudeRunner {
    program: OsString,
    caps: Caps,
    sink: EventSink,
}

enum Msg {
    Line(String),
    Eof,
}

impl ClaudeRunner {
    pub fn new(program: impl Into<OsString>, caps: Caps, sink: EventSink) -> ClaudeRunner {
        ClaudeRunner { program: program.into(), caps, sink }
    }

    fn event(&self, spec: &AgentSpec, kind: EventKind, tokens: u64) -> AgentEvent {
        AgentEvent {
            kind,
            agent_id: spec.agent_id(),
            agent_type: spec.agent.clone().map_or_else(|| spec.role.clone(), |a| a),
            tool_name: None,
            tool_target: None,
            cwd: Some(spec.cwd.display().to_string()),
            last_message: None,
            tokens,
        }
    }

    fn run_inner(&self, spec: &AgentSpec) -> Result<AgentRun, AgentError> {
        let mut child = Command::new(&self.program)
            .args(spec.args(&self.caps))
            .current_dir(&spec.cwd)
            .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AgentError::Spawn)?;
        let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AgentError::Pipe);
        };

        let prompt = spec.stdin_prompt(&self.caps);
        let writer = thread::spawn(move || {
            if let Some(mut s) = stdin {
                // The child may exit before reading everything, so errors are ignored.
                let _ = s.write_all(prompt.as_bytes());
            }
        });
        let err_thread = thread::spawn(move || tail_of(stderr, STDERR_TAIL));
        let (tx, rx) = mpsc::channel::<Msg>();
        let reader = thread::spawn(move || read_lines(stdout, tx));

        let mut state = StreamState::default();
        let mut last_event = Instant::now();
        let mut stop: Option<StopReason> = None;
        let mut warned = false;
        self.sink.emit(&self.event(spec, EventKind::Start, spec.used_before));

        loop {
            match rx.recv_timeout(POLL) {
                Ok(Msg::Line(line)) => {
                    last_event = Instant::now();
                    let events = state.feed(&line);
                    let used = spec.used_before.saturating_add(state.usage().weighted());
                    for ev in events {
                        if let StreamEvent::Tool { name, target } = ev {
                            let mut e = self.event(spec, EventKind::Tool, used);
                            e.tool_name = Some(name);
                            e.tool_target = Some(target);
                            self.sink.emit(&e);
                        }
                    }
                    if let Some(limit) = spec.budget_weighted {
                        match budget_state(used, limit) {
                            BudgetState::Stop => {
                                stop = Some(StopReason::BudgetStop);
                                break;
                            }
                            BudgetState::Warn if !warned => {
                                warned = true;
                                self.sink.emit(&self.event(spec, EventKind::BudgetWarn, used));
                            }
                            _ => {}
                        }
                    }
                    if let Some(max) = spec.max_turns {
                        if state.turns() > max {
                            stop = Some(StopReason::TurnLimit);
                            break;
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
            err_thread.join().map_or(String::new(), |t| t)
        };

        let usage = state.usage();
        let weighted = usage.weighted();
        let exit_code = status.code();
        let result_text = state.result_text.clone().map_or(String::new(), |t| t);
        let (stop_reason, error) = match stop {
            Some(reason) => (reason, None),
            None if !state.has_result() => {
                let msg = format!("claude exited without a result (exit {exit_code:?}): {}", stderr_tail.trim());
                (StopReason::Failed, Some(msg))
            }
            None if state.result_is_error => {
                let subtype = state.result_subtype.clone().map_or(String::new(), |s| s);
                let reason = if subtype.contains("max_turns") { StopReason::TurnLimit } else { StopReason::Failed };
                (reason, Some(format!("claude reported {subtype}")))
            }
            None => (StopReason::Done, None),
        };

        let mut stop_event = self.event(spec, EventKind::Stop, spec.used_before.saturating_add(weighted));
        stop_event.last_message = Some(if result_text.is_empty() { error.clone().map_or(String::new(), |e| e) } else { result_text.clone() });
        self.sink.emit(&stop_event);

        Ok(AgentRun {
            session_id: state.session_id.clone(),
            usage,
            weighted,
            result_text,
            stop_reason,
            turns: state.turns(),
            exit_code,
            error,
        })
    }
}

impl AgentRunner for ClaudeRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        match self.run_inner(spec) {
            Ok(run) => run,
            Err(e) => AgentRun::failed(&e.to_string()),
        }
    }
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
