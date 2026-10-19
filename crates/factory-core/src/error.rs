use crate::stage::Stage;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("job id must be digits, got {0:?}")]
    BadId(String),
    #[error("no job with id {0}")]
    NoJob(String),
    #[error("illegal stage move {from} -> {to}")]
    StageMove { from: Stage, to: Stage },
    #[error("job already has base {have}, refusing to change it to {want}")]
    BaseChange { have: String, want: String },
    #[error("ledger text must be one non-empty line")]
    LedgerLine,
    #[error("config error: {0}")]
    Config(String),
}

impl CoreError {
    pub fn io(path: &std::path::Path, source: std::io::Error) -> CoreError {
        CoreError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}
