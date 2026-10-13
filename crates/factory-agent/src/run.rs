use crate::usage::Usage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Done,
    BudgetStop,
    Stalled,
    TurnLimit,
    Failed,
}

/// The outcome of one agent run.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRun {
    pub session_id: Option<String>,
    pub usage: Usage,
    pub weighted: u64,
    pub result_text: String,
    pub stop_reason: StopReason,
    pub turns: u32,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

impl AgentRun {
    pub fn done(text: &str) -> AgentRun {
        AgentRun {
            session_id: None,
            usage: Usage::default(),
            weighted: 0,
            result_text: text.to_string(),
            stop_reason: StopReason::Done,
            turns: 0,
            exit_code: Some(0),
            error: None,
        }
    }

    pub fn failed(msg: &str) -> AgentRun {
        AgentRun {
            stop_reason: StopReason::Failed,
            error: Some(msg.to_string()),
            exit_code: None,
            ..AgentRun::done("")
        }
    }
}
