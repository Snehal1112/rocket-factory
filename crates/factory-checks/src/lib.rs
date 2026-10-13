//! Path to check mapping and check execution for the factory.

mod plan;
mod run;

pub use plan::{plan_checks, validate_command, validate_config, CheckPlan, DEFAULT_TIMEOUT};
pub use run::{run_checks, trim_tail, write_checks_md, CheckResult, ChecksReport};

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
}
