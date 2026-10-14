use crate::error::Result;
use crate::repo::Repo;
use git2::StatusOptions;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::UNIX_EPOCH;

/// Files above this size hash their length and mtime instead of their content.
const MAX_HASHED_BYTES: u64 = 4 * 1024 * 1024;

impl Repo {
    /// A hash of the checkout state: the tip of the checked-out branch, every non-ignored
    /// status entry, and the content of each such file. It is only comparable inside one
    /// process, because `DefaultHasher` is not stable across releases.
    pub fn checkout_fingerprint(&self) -> Result<String> {
        let mut hasher = DefaultHasher::new();
        let tip = match self.inner.head() {
            Ok(head) => head.target().map(|oid| oid.to_string()).unwrap_or_default(),
            Err(_) => String::new(),
        };
        tip.hash(&mut hasher);

        let mut opts = StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true).include_ignored(false);
        let statuses = self.inner.statuses(Some(&mut opts))?;
        let mut entries: Vec<(String, u32)> = statuses
            .iter()
            .map(|e| (String::from_utf8_lossy(e.path_bytes()).into_owned(), e.status().bits()))
            .collect();
        entries.sort();

        for (path, flags) in &entries {
            path.hash(&mut hasher);
            flags.hash(&mut hasher);
            let full = self.root.join(path);
            let Ok(meta) = std::fs::symlink_metadata(&full) else { continue };
            if meta.file_type().is_symlink() {
                // Hash the link target, never the file it points at.
                if let Ok(target) = std::fs::read_link(&full) {
                    target.hash(&mut hasher);
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if meta.len() <= MAX_HASHED_BYTES {
                if let Ok(bytes) = std::fs::read(&full) {
                    bytes.hash(&mut hasher);
                }
            } else {
                meta.len().hash(&mut hasher);
                let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                mtime.hash(&mut hasher);
            }
        }
        Ok(format!("{:016x}", hasher.finish()))
    }
}
