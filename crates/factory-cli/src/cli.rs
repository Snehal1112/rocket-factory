use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "factory", about = "Software factory driver")]
pub struct Cli {
    /// Main checkout root. Defaults to the repository containing the current directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a job at stage build.
    New {
        slug: String,
        feature: String,
        /// A plan file or a folder of plan-NN files.
        #[arg(long)]
        plans: Option<PathBuf>,
        /// The spec file, for example one committed under docs/superpowers/specs/. Without it a plans
        /// folder must hold spec.md. It needs --plans.
        #[arg(long, requires = "plans")]
        spec: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Drive a job to merged or needs-human.
    Run {
        id: String,
        /// Continue with the next plan of the series after each merge.
        #[arg(long)]
        series: bool,
    },
    Status,
    Show {
        id: String,
    },
    /// Merge a job in needs-human or approve.
    Approve {
        id: String,
    },
    /// Send a one-line note to the builder and reset the round counter.
    Rework {
        id: String,
        note: String,
    },
    Cancel {
        id: String,
        #[arg(long, default_value = "cancelled by user")]
        reason: String,
        #[arg(long)]
        force: bool,
    },
    /// Remove the worktree and branch of a merged or cancelled job.
    Cleanup {
        id: String,
        #[arg(long)]
        discard_commits: bool,
    },
    Tokens {
        id: String,
    },
    /// Print the cost of every job and the figures per size group, and write costs.json.
    Costs,
    /// Refresh the knowledge base now, ignoring the stop marker.
    KbRefresh {
        /// The job whose ledger gets the rulings.
        #[arg(long)]
        job: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn spec_without_plans_is_a_usage_error() {
        let err = Cli::try_parse_from(["factory", "new", "s", "f", "--spec", "x.md"]).expect_err("needs --plans");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn spec_with_plans_parses() {
        Cli::try_parse_from(["factory", "new", "s", "f", "--plans", "p", "--spec", "x.md"]).expect("parses");
    }
}
