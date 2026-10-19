//! Structured logging for the factory driver and factoryd.

pub mod init;
pub mod layer;
pub mod read;
pub mod redact;
pub mod rotate;
pub mod time;

pub use init::{capture, init, resolve_filter, Guard, LogTarget};
pub use layer::{JsonLayer, LineSink, Scope, StderrLayer};
pub use read::{read_log, LogFilter};

pub use redact::{cap_message, redact_command, redact_field};
pub use rotate::{RotatingFile, LOG_CAP_BYTES};
pub use time::rfc3339_utc;

/// Errors from installing or reading telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    AlreadyInstalled,
    Io(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TelemetryError::AlreadyInstalled => write!(f, "telemetry is already installed"),
            TelemetryError::Io(m) => write!(f, "telemetry io error: {m}"),
        }
    }
}

impl std::error::Error for TelemetryError {}
