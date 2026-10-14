use std::path::PathBuf;
use std::time::Duration;

/// Which optional `claude` flags exist. Defaults come from the probe report in `.claude/software-factory.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub agent: bool,
    pub max_turns: bool,
    pub append_system_prompt: bool,
}

impl Default for Caps {
    fn default() -> Self {
        // Values from the 2026-10-13 probe report: all three flags are accepted by claude 2.1.296.
        Caps { agent: true, max_turns: true, append_system_prompt: true }
    }
}

/// Everything needed to run one headless agent.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub job_id: String,
    /// Short role name such as `builder` or `reviewer`. Used for the event agent id.
    pub role: String,
    pub cwd: PathBuf,
    pub prompt: String,
    /// Agent name for `--agent`.
    pub agent: Option<String>,
    /// Agent file body, used when `--agent` is not available.
    pub agent_prompt: Option<String>,
    pub model: Option<String>,
    pub resume: Option<String>,
    pub permission_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub max_turns: Option<u32>,
    /// Job budget in weighted tokens. `None` means no budget kill.
    pub budget_weighted: Option<u64>,
    /// Weighted tokens already spent by earlier runs of this job.
    pub used_before: u64,
    pub stall_after: Option<Duration>,
    /// Extra environment variables for the child process. Empty for every agent
    /// except the committer, which sets `HARNESS_GATE=0`.
    pub env: Vec<(String, String)>,
    /// Adds `--strict-mcp-config`, so the run loads no MCP servers from settings.
    pub strict_mcp: bool,
    /// Value for `--tools`, a comma separated tool list such as `Bash,Read,Skill`.
    pub tools: Option<String>,
    /// Extra directories the agent may read, one `--add-dir` flag each.
    pub add_dirs: Vec<PathBuf>,
}

impl AgentSpec {
    pub fn new(job_id: &str, role: &str, cwd: impl Into<PathBuf>, prompt: &str) -> AgentSpec {
        AgentSpec {
            job_id: job_id.to_string(),
            role: role.to_string(),
            cwd: cwd.into(),
            prompt: prompt.to_string(),
            agent: None,
            agent_prompt: None,
            model: None,
            resume: None,
            permission_mode: None,
            allowed_tools: Vec::new(),
            max_turns: None,
            budget_weighted: None,
            used_before: 0,
            stall_after: None,
            env: Vec::new(),
            strict_mcp: false,
            tools: None,
            add_dirs: Vec::new(),
        }
    }

    /// Stable id for dashboard events, for example `001-builder`.
    pub fn agent_id(&self) -> String {
        format!("{}-{}", self.job_id, self.role)
    }

    /// Builds the argument list. The prompt itself goes to stdin, see `stdin_prompt`.
    pub fn args(&self, caps: &Caps) -> Vec<String> {
        let mut a: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose"].iter().map(|s| s.to_string()).collect();
        if self.strict_mcp {
            a.push("--strict-mcp-config".to_string());
        }
        let mut push = |flag: &str, value: &str| {
            a.push(flag.to_string());
            a.push(value.to_string());
        };
        if let Some(m) = &self.model {
            push("--model", m);
        }
        if let Some(s) = &self.resume {
            push("--resume", s);
        }
        if let Some(p) = &self.permission_mode {
            push("--permission-mode", p);
        }
        if !self.allowed_tools.is_empty() {
            push("--allowedTools", &self.allowed_tools.join(","));
        }
        if let Some(t) = &self.tools {
            push("--tools", t);
        }
        for dir in &self.add_dirs {
            push("--add-dir", &dir.to_string_lossy());
        }
        match (&self.agent, &self.agent_prompt) {
            (Some(name), _) if caps.agent => push("--agent", name),
            (_, Some(text)) if caps.append_system_prompt => push("--append-system-prompt", text),
            _ => {}
        }
        if let (Some(n), true) = (self.max_turns, caps.max_turns) {
            push("--max-turns", &n.to_string());
        }
        a
    }

    /// The text written to stdin. The agent text is prepended when neither `--agent` nor
    /// `--append-system-prompt` can carry it.
    pub fn stdin_prompt(&self, caps: &Caps) -> String {
        let carried = (self.agent.is_some() && caps.agent) || caps.append_system_prompt;
        match (&self.agent_prompt, carried) {
            (Some(text), false) => format!("{text}\n\n{}", self.prompt),
            _ => self.prompt.clone(),
        }
    }
}
