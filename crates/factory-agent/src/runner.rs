use std::collections::VecDeque;
use std::sync::Mutex;

use crate::run::AgentRun;
use crate::spec::AgentSpec;

/// Runs one agent to completion. Failures are returned as `StopReason::Failed`, never as a panic.
pub trait AgentRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun;
}

/// A scripted runner for engine tests. It replays queued runs in order.
#[derive(Debug, Default)]
pub struct FakeRunner {
    script: Mutex<VecDeque<AgentRun>>,
    calls: Mutex<Vec<AgentSpec>>,
}

impl FakeRunner {
    pub fn new(script: Vec<AgentRun>) -> FakeRunner {
        FakeRunner { script: Mutex::new(script.into()), calls: Mutex::new(Vec::new()) }
    }

    /// Every spec the runner was called with, in order.
    pub fn calls(&self) -> Vec<AgentSpec> {
        match self.calls.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }
}

impl AgentRunner for FakeRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        match self.calls.lock() {
            Ok(mut g) => g.push(spec.clone()),
            Err(p) => p.into_inner().push(spec.clone()),
        }
        let next = match self.script.lock() {
            Ok(mut g) => g.pop_front(),
            Err(p) => p.into_inner().pop_front(),
        };
        next.unwrap_or_else(|| AgentRun::failed("fake script exhausted"))
    }
}
