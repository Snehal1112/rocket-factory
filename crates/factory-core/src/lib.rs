//! Board model, ledger, config, risk, verdict and budget rules for the factory driver.
pub mod agent_drift;
pub mod board;
pub mod budget;
pub mod commitmsg;
pub mod config;
pub mod cost;
pub mod error;
pub mod fsutil;
mod gate;
pub mod home;
pub mod homeconfig;
pub mod ids;
pub mod layout;
pub mod ledger;
pub mod outcome_lines;
pub mod park;
pub mod preset;
pub mod probe;
pub mod project;
pub mod resolve;
pub mod risk;
pub mod settings_block;
pub mod stage;
#[cfg(test)]
mod test_support;
pub mod verdict;
pub mod version;

/// The global `/factory` slash command that `factory install` writes.
pub const GLOBAL_COMMAND: &str = include_str!("../assets/global-command.md");

pub use agent_drift::{agent_drift, rules_marker};
pub use board::{
    Board, BuilderLast, Draft, Job, JobTokens, LastReview, Lease, ReviewState, SeriesPos,
    StageEntry, TaskSession, DRAFT_STAGES,
};
pub use budget::{budget_state, weighted_tokens, BudgetState};
pub use commitmsg::{
    append_footers, blocked_reason, builder_rulings, builder_status, extract_message,
    fallback_subject, has_footer, issue_footer, parse_builder_status, parse_issue_refs,
    parse_issues, validate_message, BuilderStatus, IssueRef, MAX_COMMITS_PER_RUN,
    MAX_SUBJECT_CHARS, MESSAGE_BEGIN, MESSAGE_END,
};
pub use config::{
    builder_program_ok, is_env_assignment, CheckRule, Config, NEVER_ALLOWED_PROGRAMS, READ_ONLY_GIT,
};
pub use cost::{
    count_tasks, estimate_for, group_of, stats, usage_summary, CostsFile, Estimate, GroupStats,
    JobCost, SizeGroup, Spread, Thresholds, UsageSummary,
};
pub use error::CoreError;
pub use fsutil::write_atomic;
pub use gate::{code_paths_of, has_test_marker, has_test_path, uncovered_code_paths};
pub use home::Home;
pub use homeconfig::HomeConfig;
pub use ids::{job_dir_name, next_job_id, next_series_id, normalize_id, slugify};
pub use layout::{
    agent_files_rel_dir, claude_commands_dir, codex_home_from_env, codex_home_skills_dir,
    codex_skills_dir, codex_user_skill_roots, codex_user_skill_roots_from_env,
    codex_worktree_skills_dir, is_agent_file_name, Layout, Scope, AGENT_EXCLUDE_PATTERN,
    PROJECT_LEASE_FILE,
};
pub use ledger::{Ledger, TaskState};
pub use park::{park_summary, ParkKind, ParkPart, ParkRecord, PARK_FILE, PART_CAP};
pub use probe::{
    allowed_skill_entries, evaluate, expected_channels, parse_probe_answer, probe_prompt, Channel,
    Harness, ProbeAnswer, ProbeReport, ProbeRole, RoleProbe, KNOWN_BUILTIN_SKILLS,
};
pub use resolve::{resolve, AgentFile, BaseTipFiles, Resolved};
pub use risk::{classify_paths, glob_match, DiffClass};
pub use stage::Stage;
pub use verdict::{open_blockers, open_minors, parse_verdict, Verdict};
