//! Reads the driver's `park.json` for the dashboard. The server never writes it.

use std::path::{Path, PathBuf};

/// The file the driver writes into a job folder when it parks the job.
pub const PARK_FILE: &str = "park.json";

/// `<root>/.superpowers/sdd/<dir>/park.json`, or `None` when `dir` could point outside the sdd folder.
pub fn park_path(root: &Path, dir: &str) -> Option<PathBuf> {
    let bad = dir.is_empty() || dir.contains(['/', '\\', '\0']) || dir.contains("..");
    if bad {
        return None;
    }
    Some(root.join(".superpowers").join("sdd").join(dir).join(PARK_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn park_path_joins_the_job_folder_under_the_sdd_root() {
        let p = park_path(Path::new("/r"), "2026-10-13-001-x");
        assert_eq!(p, Some(PathBuf::from("/r/.superpowers/sdd/2026-10-13-001-x/park.json")));
    }

    #[test]
    fn park_path_refuses_anything_that_could_leave_the_folder() {
        for bad in ["", "..", "../x", "a/b", "a\\b", "a/../b", "x\0y", "/abs"] {
            assert_eq!(park_path(Path::new("/r"), bad), None, "{bad:?}");
        }
    }
}
