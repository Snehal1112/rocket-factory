//! `factory intake`: one pass of the issue queue, printed as one line.

use factory_engine::IntakeOutcome;

use crate::commands::{CliCtx, CliError};

/// Runs one intake pass. A dry run starts no agent and changes no label. A failed issue is
/// reported in the line and is not an error: the issue is labelled and the queue moves on.
pub fn run_intake_command(ctx: &CliCtx, dry_run: bool) -> Result<String, CliError> {
    if !dry_run {
        ctx.runnable()?;
    }
    let d = ctx.driver()?;
    let outcome = factory_engine::run_intake(&d, d.tracker(), dry_run)?;
    Ok(render_outcome(&outcome))
}

fn render_outcome(outcome: &IntakeOutcome) -> String {
    match outcome {
        IntakeOutcome::Off => "intake is off".to_string(),
        IntakeOutcome::NoIssue => "no issue".to_string(),
        IntakeOutcome::DryRun { issue, reason } => format!("would pick #{issue}: {reason}"),
        IntakeOutcome::Merged { issue, jobs } => {
            format!("merged #{issue} as job {}", jobs.join(", "))
        }
        IntakeOutcome::Failed { issue, reason } => format!("failed #{issue}: {reason}"),
    }
}
