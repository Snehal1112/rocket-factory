//! Local ignore rules in the common git dir, shared by the main checkout and its linked worktrees.

use crate::error::{GitError, Result};
use crate::repo::Repo;
use std::path::PathBuf;

impl Repo {
    fn exclude_file(&self) -> PathBuf {
        self.inner.path().join("info").join("exclude")
    }

    /// Appends the patterns that are missing to `info/exclude`. Creates the file and folder.
    /// Calling it again with the same patterns changes nothing.
    pub fn exclude_paths(&self, patterns: &[&str]) -> Result<()> {
        let file = self.exclude_file();
        let io = |e: std::io::Error| GitError::Refused(format!("{}: {e}", file.display()));
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let old = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io(e)),
        };
        let mut add = String::new();
        for p in patterns {
            if !old.lines().any(|l| l == *p) && !add.lines().any(|l| l == *p) {
                add.push_str(p);
                add.push('\n');
            }
        }
        if add.is_empty() {
            return Ok(());
        }
        let mut text = old;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&add);
        std::fs::write(&file, text).map_err(io)
    }
}
