//! Where the driver keeps its files. Everything is passed in, nothing is a literal elsewhere.

use std::path::{Path, PathBuf};

/// The folders and files of one project. `in_repo` gives the layout Rocket uses today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The main checkout of the project.
    pub repo: PathBuf,
    /// Holds `board.json`, `events.jsonl`, `costs.json` and `kb-stopped`.
    pub state_dir: PathBuf,
    /// One folder per job, named by the job's `dir`.
    pub jobs_dir: PathBuf,
    /// Holds the job worktrees, one folder per job.
    pub worktrees_dir: PathBuf,
    /// The factory config file.
    pub config_file: PathBuf,
    /// The context file the committer prompt reads.
    pub commit_context_file: PathBuf,
}

impl Layout {
    /// Everything inside the repository, the way Rocket keeps it.
    pub fn in_repo(repo: &Path) -> Layout {
        Layout {
            repo: repo.to_path_buf(),
            state_dir: repo.join(".superpowers/factory"),
            jobs_dir: repo.join(".superpowers/sdd"),
            worktrees_dir: repo.join(".claude/worktrees"),
            config_file: repo.join(".claude/factory/config.toml"),
            commit_context_file: repo.join(".claude/factory/commit-context.md"),
        }
    }

    pub fn board_file(&self) -> PathBuf {
        self.state_dir.join("board.json")
    }

    pub fn events_file(&self) -> PathBuf {
        self.state_dir.join("events.jsonl")
    }

    pub fn costs_file(&self) -> PathBuf {
        self.state_dir.join("costs.json")
    }

    /// The marker that stops automatic KB refreshes.
    pub fn kb_stop_file(&self) -> PathBuf {
        self.state_dir.join("kb-stopped")
    }

    pub fn job_dir(&self, dir: &str) -> PathBuf {
        self.jobs_dir.join(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_repo_reproduces_the_rocket_paths() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(l.repo, PathBuf::from("/r"));
        assert_eq!(l.board_file(), PathBuf::from("/r/.superpowers/factory/board.json"));
        assert_eq!(l.events_file(), PathBuf::from("/r/.superpowers/factory/events.jsonl"));
        assert_eq!(l.costs_file(), PathBuf::from("/r/.superpowers/factory/costs.json"));
        assert_eq!(l.kb_stop_file(), PathBuf::from("/r/.superpowers/factory/kb-stopped"));
        assert_eq!(l.job_dir("2026-10-16-001-x"), PathBuf::from("/r/.superpowers/sdd/2026-10-16-001-x"));
        assert_eq!(l.worktrees_dir, PathBuf::from("/r/.claude/worktrees"));
        assert_eq!(l.config_file, PathBuf::from("/r/.claude/factory/config.toml"));
        assert_eq!(l.commit_context_file, PathBuf::from("/r/.claude/factory/commit-context.md"));
    }

    #[test]
    fn fields_can_point_outside_the_repo() {
        let mut l = Layout::in_repo(Path::new("/r"));
        l.state_dir = PathBuf::from("/home/u/.factory/projects/p/state");
        assert_eq!(l.board_file(), PathBuf::from("/home/u/.factory/projects/p/state/board.json"));
        assert_eq!(l.kb_stop_file(), PathBuf::from("/home/u/.factory/projects/p/state/kb-stopped"));
    }
}
