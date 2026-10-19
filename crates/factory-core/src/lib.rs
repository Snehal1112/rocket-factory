//! Board model, ledger, config, risk, verdict and budget rules for the factory driver.
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
pub mod project;
pub mod resolve;
pub mod risk;
pub mod stage;
#[cfg(test)]
mod test_support;
pub mod verdict;

/// The global `/factory` slash command that `factory install` writes.
pub const GLOBAL_COMMAND: &str = include_str!("../assets/global-command.md");

pub use board::{
    Board, Draft, Job, JobTokens, Lease, ReviewState, SeriesPos, StageEntry, DRAFT_STAGES,
};
pub use budget::{budget_state, weighted_tokens, BudgetState};
pub use commitmsg::{
    append_footers, blocked_reason, builder_rulings, builder_status, extract_message,
    fallback_subject, has_footer, issue_footer, parse_builder_status, parse_issue_refs,
    parse_issues, validate_message, BuilderStatus, IssueRef, MAX_COMMITS_PER_RUN,
    MAX_SUBJECT_CHARS, MESSAGE_BEGIN, MESSAGE_END,
};
pub use config::{CheckRule, Config};
pub use cost::{
    count_tasks, estimate_for, group_of, stats, usage_summary, CostsFile, Estimate, GroupStats,
    JobCost, SizeGroup, Spread, Thresholds, UsageSummary,
};
pub use error::CoreError;
pub use fsutil::write_atomic;
pub use gate::{code_paths_of, has_test_marker, has_test_path};
pub use home::Home;
pub use homeconfig::HomeConfig;
pub use ids::{job_dir_name, next_job_id, next_series_id, normalize_id, slugify};
pub use layout::{
    agent_files_rel_dir, claude_commands_dir, is_agent_file_name, Layout, AGENT_EXCLUDE_PATTERN,
    PROJECT_LEASE_FILE,
};
pub use ledger::{Ledger, TaskState};
pub use park::{ParkKind, ParkPart, ParkRecord, PARK_FILE, PART_CAP};
pub use resolve::{resolve, AgentFile, BaseTipFiles, Resolved};
pub use risk::{classify_paths, glob_match, DiffClass};
pub use stage::Stage;
pub use verdict::{parse_verdict, Verdict};
