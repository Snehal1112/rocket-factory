//! Where the driver keeps its files. Everything is passed in, nothing is a literal elsewhere.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The git exclude line that covers every agent file the driver writes into a job worktree.
pub const AGENT_EXCLUDE_PATTERN: &str = "/.claude/agents/factory-*.md";

/// The folder, relative to a worktree root, where Claude Code looks for agent files.
pub fn agent_files_rel_dir() -> PathBuf {
    Path::new(".claude").join("agents")
}

/// The folder where Claude Code looks for user-wide slash commands: `<user home>/.claude/commands`.
pub fn claude_commands_dir(user_home: &Path) -> PathBuf {
    user_home.join(".claude").join("commands")
}

/// The files of the Claude Code plugin, relative to the repo root.
pub fn plugin_rel_files() -> [&'static str; 3] {
    [
        "plugins/factory/.claude-plugin/plugin.json",
        "plugins/factory/commands/factory.md",
        "plugins/factory/README.md",
    ]
}

/// The marketplace file that marks the repository root, relative to the repo root.
pub const MARKETPLACE_REL_FILE: &str = ".claude-plugin/marketplace.json";

/// The folder where Codex looks for user-wide skills: `<user home>/.agents/skills`.
pub fn codex_skills_dir(user_home: &Path) -> PathBuf {
    user_home.join(".agents").join("skills")
}

/// The folder where Codex looks for skills in its home: `<codex home>/skills`.
pub fn codex_home_skills_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("skills")
}

/// The Codex home: the variable when set and non-empty, else `<user home>/.codex`.
pub fn codex_home_from_env(user_home: &Path, codex_home_var: Option<&OsStr>) -> PathBuf {
    match codex_home_var {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => user_home.join(".codex"),
    }
}

/// The folder where Codex looks for skills in a worktree: `<worktree>/.agents/skills`.
pub fn codex_worktree_skills_dir(worktree: &Path) -> PathBuf {
    codex_skills_dir(worktree)
}

/// The user-wide folders Codex reads skills from, in order.
pub fn codex_user_skill_roots(user_home: &Path, codex_home_var: Option<&OsStr>) -> Vec<PathBuf> {
    vec![
        codex_skills_dir(user_home),
        codex_home_skills_dir(&codex_home_from_env(user_home, codex_home_var)),
    ]
}

/// Like `codex_user_skill_roots` for raw environment values. Without a user home there are no roots.
pub fn codex_user_skill_roots_from_env(
    user_home: Option<&OsStr>,
    codex_home_var: Option<&OsStr>,
) -> Vec<PathBuf> {
    match user_home {
        Some(home) => codex_user_skill_roots(Path::new(home), codex_home_var),
        None => Vec::new(),
    }
}

/// True for `factory-<a-z, 0-9, ->.md`, the only names the exclude line covers and the engine writes.
pub fn is_agent_file_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("factory-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".md") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The file name of the project lease, inside the state folder.
pub const PROJECT_LEASE_FILE: &str = "project.lease";

/// The slice of a repo a subfolder project owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The project name.
    pub project: String,
    /// The project folder, relative to the repo root.
    pub subpath: PathBuf,
}

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
    /// Set for a project that owns one subfolder of a shared repo, else `None`.
    pub scope: Option<Scope>,
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
            scope: None,
        }
    }

    pub fn board_file(&self) -> PathBuf {
        self.state_dir.join("board.json")
    }

    /// The folder of saved spec reviews, one file per hash of the spec and its plans.
    pub fn spec_reviews_dir(&self) -> PathBuf {
        self.state_dir.join("spec-reviews")
    }

    /// The folder of saved probe reports.
    pub fn probes_dir(&self) -> PathBuf {
        self.state_dir.join("probes")
    }

    /// The usage log of the spec reviews. No job owns those runs, so their tokens are logged here.
    pub fn spec_review_usage_file(&self) -> PathBuf {
        self.spec_reviews_dir().join("usage.jsonl")
    }

    pub fn events_file(&self) -> PathBuf {
        self.state_dir.join("events.jsonl")
    }

    /// The structured driver log.
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("log.jsonl")
    }

    pub fn costs_file(&self) -> PathBuf {
        self.state_dir.join("costs.json")
    }

    /// The marker that stops automatic KB refreshes.
    pub fn kb_stop_file(&self) -> PathBuf {
        self.state_dir.join("kb-stopped")
    }

    /// The project lease: at most one active job per project.
    pub fn project_lease_file(&self) -> PathBuf {
        self.state_dir.join(PROJECT_LEASE_FILE)
    }

    pub fn job_dir(&self, dir: &str) -> PathBuf {
        self.jobs_dir.join(dir)
    }

    /// The spec file of a draft card, inside the repository.
    pub fn draft_spec_file(&self, date: &str, slug: &str) -> PathBuf {
        self.repo
            .join("docs/superpowers/specs")
            .join(format!("{date}-{slug}-design.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn codex_home_defaults_to_dot_codex() {
        let home = Path::new("/h");
        assert_eq!(codex_home_from_env(home, None), PathBuf::from("/h/.codex"));
        assert_eq!(
            codex_home_from_env(home, Some(OsStr::new(""))),
            PathBuf::from("/h/.codex")
        );
    }

    #[test]
    fn codex_home_var_wins() {
        assert_eq!(
            codex_home_from_env(Path::new("/h"), Some(OsStr::new("/x"))),
            PathBuf::from("/x")
        );
    }

    #[test]
    fn user_skill_roots_lists_agents_then_codex_home() {
        assert_eq!(
            codex_user_skill_roots(Path::new("/h"), None),
            vec![
                PathBuf::from("/h/.agents/skills"),
                PathBuf::from("/h/.codex/skills")
            ]
        );
    }

    #[test]
    fn env_skill_roots_are_empty_without_a_home_and_use_codex_home() {
        assert!(codex_user_skill_roots_from_env(None, Some(OsStr::new("/x"))).is_empty());
        assert_eq!(
            codex_user_skill_roots_from_env(Some(OsStr::new("/h")), Some(OsStr::new("/x"))),
            vec![
                PathBuf::from("/h/.agents/skills"),
                PathBuf::from("/x/skills")
            ]
        );
    }

    #[test]
    fn worktree_skills_dir_is_under_dot_agents() {
        assert_eq!(
            codex_worktree_skills_dir(Path::new("/w")),
            PathBuf::from("/w/.agents/skills")
        );
    }

    #[test]
    fn log_file_is_in_the_state_dir() {
        assert_eq!(
            Layout::in_repo(Path::new("/r")).log_file(),
            PathBuf::from("/r/.superpowers/factory/log.jsonl")
        );
    }

    #[test]
    fn draft_spec_file_is_under_the_repo_specs_folder() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(
            l.draft_spec_file("2026-10-18", "cards"),
            PathBuf::from("/r/docs/superpowers/specs/2026-10-18-cards-design.md")
        );
    }

    #[test]
    fn in_repo_reproduces_the_rocket_paths() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(l.repo, PathBuf::from("/r"));
        assert_eq!(
            l.board_file(),
            PathBuf::from("/r/.superpowers/factory/board.json")
        );
        assert_eq!(
            l.events_file(),
            PathBuf::from("/r/.superpowers/factory/events.jsonl")
        );
        assert_eq!(
            l.costs_file(),
            PathBuf::from("/r/.superpowers/factory/costs.json")
        );
        assert_eq!(
            l.kb_stop_file(),
            PathBuf::from("/r/.superpowers/factory/kb-stopped")
        );
        assert_eq!(
            l.project_lease_file(),
            PathBuf::from("/r/.superpowers/factory/project.lease")
        );
        assert_eq!(
            l.job_dir("2026-10-16-001-x"),
            PathBuf::from("/r/.superpowers/sdd/2026-10-16-001-x")
        );
        assert_eq!(l.worktrees_dir, PathBuf::from("/r/.claude/worktrees"));
        assert_eq!(
            l.config_file,
            PathBuf::from("/r/.claude/factory/config.toml")
        );
        assert_eq!(
            l.commit_context_file,
            PathBuf::from("/r/.claude/factory/commit-context.md")
        );
    }

    #[test]
    fn the_commands_folder_is_under_the_user_home() {
        let d = claude_commands_dir(Path::new("/h"));
        assert!(d.starts_with("/h"));
        assert!(d.ends_with(Path::new(".claude").join("commands")));
    }

    #[test]
    fn agent_file_names_are_limited() {
        for ok in ["factory-builder.md", "factory-a-1.md"] {
            assert!(is_agent_file_name(ok), "{ok}");
        }
        for bad in [
            "factory-.md",
            "builder.md",
            "factory-x.txt",
            "factory-X.md",
            "factory-../x.md",
            "factory-a/b.md",
            "factory-a.md.md",
            "factory-a b.md",
        ] {
            assert!(!is_agent_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn spec_reviews_live_in_the_state_dir() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(
            l.spec_reviews_dir(),
            PathBuf::from("/r/.superpowers/factory/spec-reviews")
        );
    }

    #[test]
    fn probes_dir_is_under_the_state_dir() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(
            l.probes_dir(),
            PathBuf::from("/r/.superpowers/factory/probes")
        );
    }

    #[test]
    fn fields_can_point_outside_the_repo() {
        let mut l = Layout::in_repo(Path::new("/r"));
        l.state_dir = PathBuf::from("/home/u/.factory/projects/p/state");
        assert_eq!(
            l.board_file(),
            PathBuf::from("/home/u/.factory/projects/p/state/board.json")
        );
        assert_eq!(
            l.kb_stop_file(),
            PathBuf::from("/home/u/.factory/projects/p/state/kb-stopped")
        );
    }
}
