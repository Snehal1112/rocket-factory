//! Sorts a failed agent run into a machine cause or something else.

use crate::run::{AgentRun, StopReason};

/// Why the machine, not the agent, stopped a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineCause {
    Disk,
    Network,
    Auth,
}

impl MachineCause {
    pub fn as_str(self) -> &'static str {
        match self {
            MachineCause::Disk => "disk",
            MachineCause::Network => "network",
            MachineCause::Auth => "auth",
        }
    }
}

/// The class of a failed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    Machine(MachineCause),
    Other,
}

const DISK: &[&str] = &["no space left on device", "enospc"];
const AUTH: &[&str] = &[
    "invalid api key",
    "authentication_error",
    "api error: 401",
    "api_key_file",
];
const NETWORK: &[&str] = &[
    "can't reach the api server",
    "connection error",
    "econnreset",
    "enotfound",
    "eai_again",
    "api error: 529",
    "overloaded_error",
];

/// Classifies a failed run by its error text. Returns `None` for a run that did not fail.
pub fn classify_failure(run: &AgentRun) -> Option<FailureClass> {
    if run.stop_reason != StopReason::Failed {
        return None;
    }
    let text = run.error.as_deref().unwrap_or_default().to_lowercase();
    let has = |patterns: &[&str]| patterns.iter().any(|p| text.contains(p));
    Some(if has(DISK) {
        FailureClass::Machine(MachineCause::Disk)
    } else if has(AUTH) {
        FailureClass::Machine(MachineCause::Auth)
    } else if has(NETWORK) {
        FailureClass::Machine(MachineCause::Network)
    } else {
        FailureClass::Other
    })
}

/// True when a run ended almost at once, so a retry costs next to nothing.
pub fn did_no_work(run: &AgentRun) -> bool {
    run.turns <= 1 && run.usage.output_tokens < 2000 && run.weighted < 20000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(error: Option<&str>) -> AgentRun {
        let mut run = AgentRun::done("");
        run.stop_reason = StopReason::Failed;
        run.error = error.map(str::to_string);
        run
    }

    fn class(error: &str) -> Option<FailureClass> {
        classify_failure(&failed(Some(error)))
    }

    #[test]
    fn auth_patterns() {
        for e in [
            "claude reported success: Invalid API key",
            "authentication_error: bad",
            "claude reported success: API Error: 401 nope",
            "cannot read api_key_file",
        ] {
            assert_eq!(
                class(e),
                Some(FailureClass::Machine(MachineCause::Auth)),
                "{e}"
            );
        }
    }

    #[test]
    fn network_patterns() {
        for e in [
            "Can't reach the API server",
            "Connection error.",
            "read ECONNRESET",
            "getaddrinfo ENOTFOUND api.anthropic.com",
            "getaddrinfo EAI_AGAIN",
            "API Error: 529 busy",
            "overloaded_error",
        ] {
            assert_eq!(
                class(e),
                Some(FailureClass::Machine(MachineCause::Network)),
                "{e}"
            );
        }
        assert_eq!(
            class("exited (exit 1): saw 401 and 529 in a log"),
            Some(FailureClass::Other)
        );
    }

    #[test]
    fn disk_patterns() {
        for e in ["write: No space left on device", "ENOSPC: failed"] {
            assert_eq!(
                class(e),
                Some(FailureClass::Machine(MachineCause::Disk)),
                "{e}"
            );
        }
    }

    #[test]
    fn other_and_negative() {
        assert_eq!(
            class("claude reported success: it broke"),
            Some(FailureClass::Other)
        );
        let done = AgentRun::done("test timed out after 401 retries");
        assert_eq!(classify_failure(&done), None);
        assert_eq!(classify_failure(&failed(None)), Some(FailureClass::Other));
    }

    #[test]
    fn cause_names() {
        assert_eq!(MachineCause::Disk.as_str(), "disk");
        assert_eq!(MachineCause::Network.as_str(), "network");
        assert_eq!(MachineCause::Auth.as_str(), "auth");
    }

    #[test]
    fn did_no_work_limits() {
        let mut run = AgentRun::done("");
        run.turns = 1;
        run.usage.output_tokens = 1999;
        run.weighted = 19999;
        assert!(did_no_work(&run));
        run.turns = 2;
        assert!(!did_no_work(&run));
        run.turns = 1;
        run.usage.output_tokens = 2000;
        assert!(!did_no_work(&run));
    }
}
