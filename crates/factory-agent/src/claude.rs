use std::ffi::OsString;

use crate::child::{drive, ChildConfig};
use crate::events::EventSink;
use crate::run::AgentRun;
use crate::runner::AgentRunner;
use crate::spec::{AgentSpec, Caps};
use crate::stream::StreamState;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("cannot start {0}: {1}")]
    Spawn(String, std::io::Error),
    #[error("the agent process has no stdout or stderr pipe")]
    Pipe,
    #[error("waiting for the agent process failed: {0}")]
    Wait(std::io::Error),
}

/// Variables that make `claude` bill an API account instead of the subscription login.
const API_BILLING_VARS: [&str; 2] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// Set to `1` on purpose to let the agents use the variables above. Off by default.
pub const ALLOW_API_KEY_VAR: &str = "FACTORY_ALLOW_API_KEY";

fn api_key_allowed_by_env() -> bool {
    std::env::var(ALLOW_API_KEY_VAR).is_ok_and(|v| v == "1")
}

/// Runs headless `claude -p` and reads its stream-json output.
#[derive(Debug)]
pub struct ClaudeRunner {
    program: OsString,
    caps: Caps,
    sink: EventSink,
    allow_api_key: bool,
}

impl ClaudeRunner {
    pub fn new(program: impl Into<OsString>, caps: Caps, sink: EventSink) -> ClaudeRunner {
        ClaudeRunner {
            program: program.into(),
            caps,
            sink,
            allow_api_key: api_key_allowed_by_env(),
        }
    }

    /// Overrides the `FACTORY_ALLOW_API_KEY` setting. Meant for tests.
    #[must_use]
    pub fn with_api_key_allowed(mut self, allowed: bool) -> ClaudeRunner {
        self.allow_api_key = allowed;
        self
    }

    fn run_inner(&self, spec: &AgentSpec) -> Result<AgentRun, AgentError> {
        let cfg = ChildConfig {
            program: self.program.clone(),
            args: spec.args(&self.caps),
            stdin: spec.stdin_prompt(&self.caps),
            label: "claude".to_string(),
            remove_env: if self.allow_api_key {
                Vec::new()
            } else {
                API_BILLING_VARS.to_vec()
            },
            honor_max_turns: true,
            fail_on_exit_after_result: false,
            mid_run_budget: true,
        };
        drive(&cfg, spec, &self.sink, StreamState::default())
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
