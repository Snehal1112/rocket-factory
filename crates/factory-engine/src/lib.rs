//! The factory run loop stages: build, checks, review and decide.

mod agents_file;
mod build;
mod cargo_risk;
mod carry;
mod checks_stage;
mod clock;
mod codex_prep;
mod codex_skills;
mod combine;
mod compat;
mod costs;
mod decide;
mod drafts;
mod escalation;
mod followups;
mod gate_record;
mod intake;
mod jobmsg;
mod kb;
mod lease;
mod lock;
mod merge;
mod message;
mod minors;
mod notes;
mod one_job;
mod park;
mod permissions;
mod planlint;
mod ports;
mod probe;
pub mod procinfo;
mod project_lease;
mod recover;
mod repo_lock;
mod retry;
mod review;
mod rework;
mod runloop;
mod scope;
mod series;
mod series_guard;
mod spec_check;
mod specs;
mod store;
mod sync;
mod tasks;
mod tick;
mod tickets;
mod tipfiles;
mod tracker;
mod waivers;

use std::path::PathBuf;
use std::time::Duration;

use factory_agent::{AgentRunner, EventSink};
use factory_checks::validate_config;
use factory_core::{AgentFile, Config, Stage};
use factory_git::Repo;
use thiserror::Error;

pub use build::build_stage;
pub use checks_stage::checks_stage;
pub use checks_stage::CHECKS_FAILURES_FILE;
pub use clock::{date_from_days, now_secs, today, utc_stamp};
pub use combine::{combine_plans, CombinedPlan};
pub use compat::{
    Driver, JobGuard, LostFlag, LostReason, ProjectGuard, RunnerFactory, HEARTBEAT_SECS,
    LEASE_TTL_SECS,
};
pub use costs::{costs_path, estimate_for_plan, gather_costs, refresh_costs};
pub use decide::{decide, risk_summary, DecideInput, Decision};
pub use drafts::{find_draft, remove_draft, set_draft_stage, start_draft};
pub use followups::{follow_ups, FollowUp};
pub use gate_record::{record_gate, GateTarget};
pub use intake::{
    pick_next, run_intake, IntakeOutcome, STATE_DONE, STATE_NEEDS_HUMAN, STATE_RUNNING,
};
pub use kb::{kb_prompt, kb_spec, parse_kb_reply, KbOutcome, KB_AGENT};
pub use lease::{acquire_lease, lease_is_live, release_lease, Lease, LeaseError};
pub use lock::{lock_board, BoardLock};
pub use merge::{
    merge_stage, park, park_with_parts, park_with_parts_resume, park_with_parts_target,
    MergeResult, MAX_SYNCS,
};
pub use message::{driver_footers, fixed_message, MessageKind, MessageRequest};
pub use minors::{minor_tickets_step, MINORS_FILE};
pub use notes::{read_notes, Notes, NOTES_DIR};
pub use one_job::{
    create_one_job, create_one_job_accepting, one_job_estimate, OneJob, ONE_JOB_OVERHEAD_WEIGHTED,
    ONE_JOB_PER_TASK_WEIGHTED,
};
pub use permissions::{builder_allowed_tools, builder_denied_tools, restore_file};
pub use planlint::{lint_plan, PlanLint};
pub use ports::{resume_logged, set_stage_logged};
pub use probe::{run_probe, unique_stamp, Delivery};
pub use project_lease::{
    acquire_project_lease, force_release_project_lease, lease_held, owner_pid, owner_state,
    process_start_secs, project_lease_status, release_project_lease, LeaseTake, OwnerState,
    ProjectLease, ProjectLeaseError, ProjectLeaseStatus, START_SLACK_SECS,
};
pub use recover::MachineWait;
pub use repo_lock::{lock_repo_merge, MERGE_LOCK_FILE, MERGE_LOCK_WAIT};
pub use retry::retry_job;
pub use review::{apply_decision, final_checks, review_stage, ReviewOutcome, ReviewRun};
pub use rework::ReworkKind;
pub use runloop::{
    create_job, create_job_with_spec, create_job_with_spec_accepting, create_replacement, job_span,
    mark_cancelled, record_cancel, run_job, run_series, Replacement, RunEnd, SpecMode,
};
pub use scope::scope_violation;
pub use series::{parse_series, parse_series_with, Series, SeriesError, SeriesPlan, MAX_PLANS};
pub use spec_check::{
    plan_file_paths, review_spec, risky_without_user_decision, SpecRefusal, SpecReviewInfo,
    SPEC_OVERRIDE_HINT,
};
pub use specs::{
    ReviewerExtra, BUILDER_MAX_TURNS, BUILDER_TOOLS, COMMIT_NOTE_LINE, REVIEWER_MAX_TURNS,
    RULES_VERSION,
};
pub use store::{next_id, BoardStore, StoreError, DEFAULT_LOCK_WAIT};
pub use sync::{sync_requested, sync_requested_by, sync_stage, SyncCaller, SyncKind, SyncOutcome};
pub use tasks::{
    parse_checkbox_tasks, parse_tasks, plan_outline, plan_title, review_focus, spec_goal, PlanTask,
};
pub use tick::tick_plan;
pub use tickets::{file_follow_ups, tickets_step, FiledFollowUps, TicketsReport, TICKETS_FILE};
pub use tipfiles::RepoTip;
pub use tracker::{GhTracker, IssueInfo, LabelEvent, Tracker};
pub use waivers::{
    read_waivers, record_waiver, supersede_waivers, waived_blockers, waiver_tickets_step, Waiver,
    WAIVERS_FILE,
};

/// The start of the `NeedsConsent` text. factoryd matches it to answer a conflict.
pub const NEEDS_CONSENT: &str = "needs consent:";

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("core: {0}")]
    Core(String),
    #[error("git: {0}")]
    Git(String),
    #[error("agent: {0}")]
    Agent(String),
    #[error("checks: {0}")]
    Check(String),
    #[error("io: {0}")]
    Io(String),
    #[error("{0}")]
    Msg(String),
    /// A requested sync was refused before it changed anything.
    #[error("{0}")]
    Refused(String),
    /// A drafted spec or plan failed a check before any job existed.
    #[error("{0}")]
    Invalid(String),
    /// The project lease was held when a run or approve started, so nothing was done.
    #[error("{0}")]
    ProjectBusy(String),
    #[error("needs consent: this job is estimated at {estimate} weighted tokens and the budget is {budget}; raise budget_weighted in project.toml or pass --accept-over-budget")]
    NeedsConsent { estimate: u64, budget: u64 },
    /// `factory new` refused the spec, so no job exists.
    #[error("{0}")]
    SpecRefused(SpecRefusal),
    /// The job was cancelled, so the stage stops and the run ends as needs-human.
    #[error("job is cancelled")]
    Cancelled,
}

/// What a stage did to the job. The caller chains stages by the new board stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    Advanced(Stage),
    NeedsHuman(String),
}

pub struct Engine<'a> {
    pub board_path: PathBuf,
    pub job_dir: PathBuf,
    pub job_id: String,
    pub plan_file: PathBuf,
    pub repo: &'a Repo,
    pub runner: &'a dyn AgentRunner,
    pub config: Config,
    pub events: &'a EventSink,
    pub lock_wait: Duration,
    /// Agent files written into each new job worktree. Empty by default.
    pub agent_files: Vec<AgentFile>,
    /// The user-wide folders Codex lists skills from. Empty by default.
    pub codex_skill_roots: Vec<PathBuf>,
    /// Set once the job's provider matches the config, so later calls skip the board read.
    /// An engine lives for one driver call and its config never changes, so it cannot go stale.
    pub provider_synced: std::cell::Cell<bool>,
    /// The driver's lost flag. While it is set, no agent starts. None for an engine with no driver.
    pub lost: Option<std::sync::Arc<LostFlag>>,
    /// The disk check, sleep and clock the machine wait uses.
    pub machine: MachineWait,
    /// The last Cargo check verdict of this engine with its store key, so one run writes one ledger line.
    pub(crate) cargo_memo: std::cell::RefCell<Option<(String, cargo_risk::Risk)>>,
}

impl<'a> Engine<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board_path: PathBuf,
        job_dir: PathBuf,
        job_id: String,
        plan_file: PathBuf,
        repo: &'a Repo,
        runner: &'a dyn AgentRunner,
        config: Config,
        events: &'a EventSink,
    ) -> Result<Self, EngineError> {
        validate_config(&config).map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(Self {
            board_path,
            job_dir,
            job_id,
            plan_file,
            repo,
            runner,
            config,
            events,
            lock_wait: DEFAULT_LOCK_WAIT,
            agent_files: Vec::new(),
            codex_skill_roots: Vec::new(),
            provider_synced: std::cell::Cell::new(false),
            lost: None,
            machine: MachineWait::default(),
            cargo_memo: std::cell::RefCell::new(None),
        })
    }

    /// Shares the driver's lost flag with this engine.
    pub fn with_lost(mut self, flag: std::sync::Arc<LostFlag>) -> Self {
        self.lost = Some(flag);
        self
    }

    /// True when the lease of the run was lost.
    pub fn lost(&self) -> bool {
        self.lost.as_ref().is_some_and(|flag| flag.is_set())
    }

    /// Changes how long board writes wait for the lock.
    pub fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    /// Sets the agent files written into each new job worktree.
    pub fn with_agent_files(mut self, files: Vec<AgentFile>) -> Self {
        self.agent_files = files;
        self
    }

    /// Sets the user-wide skill folders a Codex run lists skills from.
    pub fn with_codex_skill_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.codex_skill_roots = roots;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_consent_text_starts_with_the_const() {
        let text = EngineError::NeedsConsent {
            estimate: 2,
            budget: 1,
        }
        .to_string();
        assert!(text.starts_with(NEEDS_CONSENT), "{text}");
    }
}
