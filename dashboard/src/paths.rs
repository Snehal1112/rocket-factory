//! The folders one project's dashboard data lives in.

use std::path::{Path, PathBuf};

/// Where one project keeps the board, the event log, the cost summary and the job folders.
#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    pub board: PathBuf,
    pub events: PathBuf,
    pub costs: PathBuf,
    pub sdd: PathBuf,
}

impl Paths {
    /// The layout of a plain checkout: `<root>/.superpowers/factory` and `<root>/.superpowers/sdd`.
    pub fn for_root(root: &Path) -> Paths {
        let superpowers = root.join(".superpowers");
        Paths::from_project(&superpowers.join("factory"), &superpowers.join("sdd"))
    }

    /// The folders the factory API reports for a project (`stateDir` and `jobsDir`).
    pub fn from_project(state_dir: &Path, jobs_dir: &Path) -> Paths {
        Paths {
            board: state_dir.join("board.json"),
            events: state_dir.join("events.jsonl"),
            costs: state_dir.join("costs.json"),
            sdd: jobs_dir.to_path_buf(),
        }
    }

    /// Paths that exist nowhere, for a project that cannot take work. Every read comes back empty.
    pub fn nowhere() -> Paths {
        let none = Path::new("/nonexistent/factory-server");
        Paths::from_project(none, none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_root_matches_the_old_hardcoded_joins() {
        let paths = Paths::for_root(Path::new("/r"));
        assert_eq!(paths.board, Path::new("/r/.superpowers/factory/board.json"));
        assert_eq!(
            paths.events,
            Path::new("/r/.superpowers/factory/events.jsonl")
        );
        assert_eq!(paths.costs, Path::new("/r/.superpowers/factory/costs.json"));
        assert_eq!(paths.sdd, Path::new("/r/.superpowers/sdd"));
    }

    #[test]
    fn from_project_uses_the_api_folders() {
        let paths = Paths::from_project(Path::new("/h/p/state"), Path::new("/h/p/state/jobs"));
        assert_eq!(paths.board, Path::new("/h/p/state/board.json"));
        assert_eq!(paths.events, Path::new("/h/p/state/events.jsonl"));
        assert_eq!(paths.costs, Path::new("/h/p/state/costs.json"));
        assert_eq!(paths.sdd, Path::new("/h/p/state/jobs"));
    }
}
