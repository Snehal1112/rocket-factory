/// Errors from the git layer.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("{} is not the top folder of a git repository", .0.display())]
    NotTopFolder(std::path::PathBuf),
}

pub type Result<T> = std::result::Result<T, GitError>;
