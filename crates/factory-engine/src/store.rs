use std::path::{Path, PathBuf};

use factory_core::{Board, CoreError, Job};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("board.json is not valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Core(#[from] CoreError),
    #[error("the board is locked by another factory process, try again")]
    Locked,
    #[error("{0}")]
    Other(String),
}

/// The only place the engine and CLI write `board.json` through.
pub struct BoardStore {
    board_path: PathBuf,
}

impl BoardStore {
    pub fn new(root: &Path) -> BoardStore {
        BoardStore { board_path: root.join(".superpowers/factory/board.json") }
    }

    pub fn board_path(&self) -> &Path {
        &self.board_path
    }

    pub fn load(&self) -> Result<Board, StoreError> {
        match std::fs::read_to_string(&self.board_path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Board::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Board) -> Result<R, StoreError>) -> Result<R, StoreError> {
        let mut board = self.load()?;
        let out = f(&mut board)?;
        let mut text = serde_json::to_string_pretty(&board)?;
        text.push('\n');
        if let Some(parent) = self.board_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        factory_core::write_atomic(&self.board_path, text.as_bytes())?;
        Ok(out)
    }

    pub fn update_job<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Job) -> Result<R, CoreError>,
    ) -> Result<R, StoreError> {
        self.update(|board| Ok(f(board.find_mut(id)?)?))
    }
}

/// The next free job id, looking at the board only. `create_job` also passes folder and branch names.
pub fn next_id(board: &Board) -> String {
    let ids: Vec<String> = board.jobs.iter().map(|j| j.id.clone()).collect();
    factory_core::next_job_id(&ids, &[], &[])
}
