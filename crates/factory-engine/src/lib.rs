//! The factory run loop stages: build, checks, review and decide.

mod build;
mod checks_stage;
mod ports;
mod specs;

use std::path::PathBuf;

use factory_agent::{AgentRunner, EventSink};
use factory_checks::validate_config;
use factory_core::{Config, Stage};
use factory_git::Repo;
use thiserror::Error;

pub use build::build_stage;
pub use checks_stage::checks_stage;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("core: {0}")]
    Core(String),
    #[error("git: {0}")]
    Git(String),
    #[error("agent: {0}")]
    Agent(String),
    #[error("checks: {0}")]
    Check(String),
    #[error("io: {0}")]
    Io(String),
    #[error("{0}")]
    Msg(String),
}

/// What a stage did to the job. The caller chains stages by the new board stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    Advanced(Stage),
    NeedsHuman(String),
}

pub struct Engine<'a> {
    pub board_path: PathBuf,
    pub job_dir: PathBuf,
    pub job_id: String,
    pub plan_file: PathBuf,
    pub repo: &'a Repo,
    pub runner: &'a dyn AgentRunner,
    pub config: Config,
    pub events: &'a EventSink,
}

impl<'a> Engine<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board_path: PathBuf,
        job_dir: PathBuf,
        job_id: String,
        plan_file: PathBuf,
        repo: &'a Repo,
        runner: &'a dyn AgentRunner,
        config: Config,
        events: &'a EventSink,
    ) -> Result<Self, EngineError> {
        validate_config(&config).map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(Self { board_path, job_dir, job_id, plan_file, repo, runner, config, events })
    }
}
