use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

use fs2::FileExt;

use crate::StoreError;

/// An exclusive advisory lock that is released when it is dropped.
pub struct BoardLock {
    file: File,
}

impl Drop for BoardLock {
    fn drop(&mut self) {
        // Closing the file also releases the lock, so an unlock error is harmless.
        let _ = FileExt::unlock(&self.file);
    }
}

/// Takes an exclusive advisory lock, polling until `wait` has passed.
pub fn lock_board(path: &Path, wait: Duration) -> Result<BoardLock, StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(BoardLock { file }),
            Err(_) if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(StoreError::Locked),
        }
    }
}
