use std::time::Duration;

use factory_git::Repo;

use crate::{lock_board, BoardLock, StoreError};

/// The lock file name, kept in the git dir shared by every worktree of the repository.
pub const MERGE_LOCK_FILE: &str = "factory-merge.lock";

/// How long a merge waits for another project to finish merging into the same repository.
pub const MERGE_LOCK_WAIT: Duration = Duration::from_secs(30);

/// Takes the merge lock of the repository. The OS frees it if the holder dies, so it never goes stale.
pub fn lock_repo_merge(repo: &Repo, wait: Duration) -> Result<BoardLock, StoreError> {
    lock_board(&repo.common_git_dir().join(MERGE_LOCK_FILE), wait)
}
