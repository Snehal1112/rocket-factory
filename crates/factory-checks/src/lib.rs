//! Path to check mapping and check execution for the factory.

mod plan;
mod run;
mod workspace;

pub use plan::{
    plan_checks, plan_checks_in, validate_command, validate_config, CheckPlan, DEFAULT_TIMEOUT,
};
pub use run::{append_notes, run_checks, trim_tail, write_checks_md, CheckResult, ChecksReport};

pub use workspace::{Package, Workspace};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("empty check command")]
    Empty,
    #[error("cargo command must pass -j4: {0}")]
    MissingJobs(String),
    #[error("cargo command must not use {flag}: {cmd}")]
    Forbidden { flag: &'static str, cmd: String },
    #[error("could not write checks file: {0}")]
    Write(String),
    #[error("workspace map: {0}")]
    Workspace(String),
}
