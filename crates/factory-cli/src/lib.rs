//! The `factory` command line: argument parsing and the commands over `factory-engine`.

mod cli;
mod commands;
mod costs;
pub mod install;
mod log;
pub mod presets;
mod probe;
pub mod project;
mod status;
pub mod tokens;
pub mod upgrade;

pub use cli::{Cli, Command, PresetsCmd, ProjectCmd, SpecCmd};
pub use commands::{
    check_home, dispatch, env_lock_wait, env_owner, open_home, validate_rework_note, CliCtx,
    CliError,
};
pub use costs::{estimate_lines, format_estimate, render_costs};
pub use log::run_log;
pub use probe::render_probe;
pub use status::{render_park_details, render_parked, render_status};
