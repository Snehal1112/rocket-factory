use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "factory", about = "Software factory driver")]
pub struct Cli {
    /// Main checkout root. Defaults to the repository containing the current directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    /// Run against a registered project instead of the current repository.
    #[arg(long, global = true, conflicts_with = "root")]
    pub project: Option<String>,
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
        /// Combine the whole plan chain into one job. It needs --plans.
        #[arg(long, requires = "plans")]
        one_job: bool,
        /// Create the one job even when its estimate is over the budget. It needs --one-job.
        #[arg(long, requires = "one_job")]
        accept_over_budget: bool,
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
        /// Merge a needs-human job even though its code review has not run.
        #[arg(long)]
        without_review: bool,
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
    /// Remove the worktree and branch of a merged or cancelled job. A cancelled job also loses its board card, unless it is in a series with other jobs or holds a live lease. The job folder stays.
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
    /// Track a feature that is still being specified, before it has a job.
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// Manage the registered projects in the factory home.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Manage the seeded presets in the factory home.
    Presets {
        #[command(subcommand)]
        cmd: PresetsCmd,
    },
    /// Install the global /factory slash command for the current user.
    Install {
        /// The folder to write factory.md into. Defaults to the Claude commands folder under $HOME.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Replace an existing factory.md that differs.
        #[arg(long)]
        force: bool,
    },
    /// File the follow-up tickets of a merged job and close its parent tickets.
    Tickets {
        id: String,
        /// Print what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Refresh the knowledge base now, ignoring the stop marker.
    KbRefresh {
        /// The job whose ledger gets the rulings.
        #[arg(long)]
        job: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SpecCmd {
    /// Add a draft card at stage interviewing and print where its spec file goes.
    Start { slug: String, feature: String },
    /// Move a draft to interviewing, writing-spec or writing-plans.
    Stage { slug: String, stage: String },
    /// Print the stage and spec file of a draft.
    Resume { slug: String },
}

#[derive(Subcommand, Debug)]
pub enum PresetsCmd {
    /// Bring the seeded presets up to the built-in text. Hand-edited files are listed and kept.
    Refresh {
        /// Overwrite files that were edited by hand.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    /// Register a repository as a project.
    Add {
        path: PathBuf,
        #[arg(long)]
        preset: String,
        /// The branch jobs start from and merge into. Defaults to the branch the repo is on.
        #[arg(long)]
        base: Option<String>,
        /// The project name. Defaults to the lowercased repo folder name.
        #[arg(long)]
        name: Option<String>,
    },
    /// List the projects.
    List,
    /// Print the name of the registered project that contains a folder.
    Which {
        /// Defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Show the resolved settings of a project.
    Show { name: String },
    /// Delete a project folder. The repository and the presets stay.
    Remove {
        name: String,
        #[arg(long)]
        force: bool,
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
    fn one_job_needs_plans() {
        let err = Cli::try_parse_from(["factory", "new", "s", "f", "--one-job"]).expect_err("needs --plans");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn accept_over_budget_needs_one_job() {
        let err = Cli::try_parse_from(["factory", "new", "s", "f", "--plans", "p", "--accept-over-budget"])
            .expect_err("needs --one-job");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn one_job_flags_parse() {
        let cli = Cli::try_parse_from(["factory", "new", "s", "f", "--plans", "p", "--one-job", "--accept-over-budget"])
            .expect("parse");
        assert!(matches!(cli.command, Command::New { one_job: true, accept_over_budget: true, .. }));
    }

    #[test]
    fn spec_subcommands_parse() {
        let cli = Cli::try_parse_from(["factory", "spec", "start", "demo", "a feature"]).expect("start");
        assert!(matches!(cli.command, Command::Spec { cmd: SpecCmd::Start { ref slug, ref feature } } if slug == "demo" && feature == "a feature"));
        let cli = Cli::try_parse_from(["factory", "spec", "stage", "demo", "writing-spec"]).expect("stage");
        assert!(matches!(cli.command, Command::Spec { cmd: SpecCmd::Stage { ref slug, ref stage } } if slug == "demo" && stage == "writing-spec"));
        let cli = Cli::try_parse_from(["factory", "spec", "resume", "demo"]).expect("resume");
        assert!(matches!(cli.command, Command::Spec { cmd: SpecCmd::Resume { ref slug } } if slug == "demo"));
        assert!(Cli::try_parse_from(["factory", "spec", "start", "demo"]).is_err());
    }

    #[test]
    fn project_conflicts_with_root() {
        let err = Cli::try_parse_from(["factory", "--project", "p", "--root", "/x", "status"]).expect_err("conflict");
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        let cli = Cli::try_parse_from(["factory", "--project", "p", "status"]).expect("parses");
        assert_eq!(cli.project.as_deref(), Some("p"));
    }

    #[test]
    fn spec_with_plans_parses() {
        Cli::try_parse_from(["factory", "new", "s", "f", "--plans", "p", "--spec", "x.md"]).expect("parses");
    }

    #[test]
    fn tickets_parses_dry_run() {
        let cli = Cli::try_parse_from(["factory", "tickets", "012", "--dry-run"]).expect("parses");
        assert!(matches!(cli.command, Command::Tickets { ref id, dry_run: true } if id == "012"));
        let cli = Cli::try_parse_from(["factory", "tickets", "012"]).expect("parses");
        assert!(matches!(cli.command, Command::Tickets { dry_run: false, .. }));
    }

    #[test]
    fn tickets_needs_an_id() {
        let err = Cli::try_parse_from(["factory", "tickets"]).expect_err("needs an id");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }
}
