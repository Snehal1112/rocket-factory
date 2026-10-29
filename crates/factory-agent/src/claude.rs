use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::child::{drive, ChildConfig, Secret};
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
    /// The text never holds the key itself.
    #[error("{0}")]
    ApiKey(String),
}

/// Variables that make `claude` bill an API account instead of the subscription login.
const API_BILLING_VARS: [&str; 2] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// Set to `1` on purpose to let the agents use the variables above. Off by default.
pub const ALLOW_API_KEY_VAR: &str = "FACTORY_ALLOW_API_KEY";

fn api_key_allowed_by_env() -> bool {
    std::env::var(ALLOW_API_KEY_VAR).is_ok_and(|v| v == "1")
}

/// Where a project's Claude agents get their API credentials from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ApiKeySource {
    /// The subscription login, unless `FACTORY_ALLOW_API_KEY=1` is set in the environment.
    #[default]
    Default,
    /// Always the subscription login. The key variables are removed.
    Deny,
    /// The key from the environment. The run fails when there is none.
    Env,
    /// The key in this file, for this project's agents only.
    File(PathBuf),
}

/// The variables to remove from the child and the ones to set.
type KeyEnv = (Vec<&'static str>, Vec<(&'static str, Secret)>);

/// Checks that `path` holds a key the agents can use, as a job start would. The error names the file, never the key.
pub fn check_key_file(path: &Path) -> Result<(), AgentError> {
    read_key_file(path).map(|_| ())
}

/// Reads one key from `path`. The errors name the file, never the key.
fn read_key_file(path: &Path) -> Result<Secret, AgentError> {
    let shown = path.display();
    let meta = std::fs::metadata(path)
        .map_err(|e| AgentError::ApiKey(format!("cannot read api_key_file {shown}: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(AgentError::ApiKey(format!(
                "api_key_file {shown} is readable by group or others, run chmod 600 on it"
            )));
        }
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| AgentError::ApiKey(format!("cannot read api_key_file {shown}: {e}")))?;
    let key = text.trim();
    if key.is_empty() {
        return Err(AgentError::ApiKey(format!("api_key_file {shown} is empty")));
    }
    if key.contains(char::is_whitespace) {
        return Err(AgentError::ApiKey(format!(
            "api_key_file {shown} must hold a single line"
        )));
    }
    Ok(Secret(key.to_string()))
}

/// Runs headless `claude -p` and reads its stream-json output.
#[derive(Debug)]
pub struct ClaudeRunner {
    program: OsString,
    caps: Caps,
    sink: EventSink,
    allow_api_key: bool,
    key_source: ApiKeySource,
    /// Installed plugin ids, read once from `claude plugin list --json`.
    installed: OnceLock<Vec<String>>,
}

/// The installed plugin ids, each once. An empty list when the command fails.
fn list_plugins(program: &OsString) -> Vec<String> {
    let out = Command::new(program)
        .args(["plugin", "list", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|p| p["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

impl ClaudeRunner {
    pub fn new(program: impl Into<OsString>, caps: Caps, sink: EventSink) -> ClaudeRunner {
        ClaudeRunner {
            program: program.into(),
            caps,
            sink,
            allow_api_key: api_key_allowed_by_env(),
            key_source: ApiKeySource::Default,
            installed: OnceLock::new(),
        }
    }

    /// Sets where this runner's agents get their credentials. The default follows the environment.
    #[must_use]
    pub fn with_api_key_source(mut self, source: ApiKeySource) -> ClaudeRunner {
        self.key_source = source;
        self
    }

    /// The variables to remove from the child and the ones to set, for this run.
    fn key_env(&self, spec: &AgentSpec) -> Result<KeyEnv, AgentError> {
        match &self.key_source {
            ApiKeySource::Default if self.allow_api_key => Ok((Vec::new(), Vec::new())),
            ApiKeySource::Default | ApiKeySource::Deny => {
                Ok((API_BILLING_VARS.to_vec(), Vec::new()))
            }
            ApiKeySource::Env => {
                let has_credential = API_BILLING_VARS.iter().any(|var| {
                    let in_spec = spec.env.iter().any(|(k, v)| k == var && !v.is_empty());
                    in_spec || std::env::var(var).is_ok_and(|v| !v.is_empty())
                });
                if !has_credential {
                    return Err(AgentError::ApiKey(
                        "allow_api_key is on but ANTHROPIC_API_KEY is not set in the environment"
                            .to_string(),
                    ));
                }
                Ok((Vec::new(), Vec::new()))
            }
            ApiKeySource::File(path) => {
                let key = read_key_file(path)?;
                Ok((
                    vec!["ANTHROPIC_AUTH_TOKEN"],
                    vec![("ANTHROPIC_API_KEY", key)],
                ))
            }
        }
    }

    /// Overrides the `FACTORY_ALLOW_API_KEY` setting. Meant for tests.
    #[must_use]
    pub fn with_api_key_allowed(mut self, allowed: bool) -> ClaudeRunner {
        self.allow_api_key = allowed;
        self
    }

    fn run_inner(&self, spec: &AgentSpec) -> Result<AgentRun, AgentError> {
        let (remove_env, set_env) = self.key_env(spec)?;
        let mut args = spec.args(&self.caps);
        if spec.plugins.is_some() {
            let installed = self.installed.get_or_init(|| list_plugins(&self.program));
            if let Some(settings) = spec.plugin_settings(installed) {
                args.push("--settings".to_string());
                args.push(settings);
            }
        }
        let cfg = ChildConfig {
            program: self.program.clone(),
            args,
            stdin: spec.stdin_prompt(&self.caps),
            label: "claude".to_string(),
            remove_env,
            set_env,
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
