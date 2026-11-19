use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, Subcommand};
use factory_core::{Harness, ProbeRole};
use factory_engine::Delivery;
use std::path::PathBuf;

/// A parser for a value that must be one of `names`, then read with `FromStr`.
/// An unknown value fails as an invalid value, not as a validation error.
fn one_of<T>(names: &[&'static str]) -> impl TypedValueParser<Value = T>
where
    T: std::str::FromStr<Err = String> + Clone + Send + Sync + 'static,
{
    PossibleValuesParser::new(names.to_vec()).try_map(|s| s.parse::<T>())
}

fn harness_parser() -> impl TypedValueParser<Value = Harness> {
    one_of(&["claude", "codex"])
}

fn role_parser() -> impl TypedValueParser<Value = ProbeRole> {
    one_of(&ProbeRole::ALL.map(ProbeRole::as_str))
}

fn delivery_parser() -> impl TypedValueParser<Value = Delivery> {
    one_of(&["agent", "append", "stdin"])
}

/// The usual flow, shown under the command list of `factory --help`.
const AFTER_HELP: &str = "\
Typical flow:
  factory new <slug> \"<feature>\" --plans <dir>   create a job from plans
  factory run <id> --series                     build it until it merges or needs you
  factory status                                see every job and what waits on you
  factory approve <id> | retry <id> | rework <id> \"<note>\"

Run `factory <command> --help` for the details of one command.";

#[derive(Parser, Debug)]
#[command(
    name = "factory",
    version,
    about = "Build, check, review and merge planned work with coding agents",
    arg_required_else_help = true,
    after_help = AFTER_HELP
)]
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
    /// Create a job from a plan or a folder of plans.
    ///
    /// A folder of plan-NN files becomes a series, one job per plan, unless --one-job combines
    /// them. The first job starts at stage build. Nothing runs until `factory run`.
    New {
        /// A short name for the job, used in its branch and folder, for example login-fix.
        slug: String,
        /// One line that says what the job builds.
        feature: String,
        /// A plan file or a folder of plan-NN files.
        #[arg(long)]
        plans: Option<PathBuf>,
        /// The spec file, for example one committed under docs/superpowers/specs/. Without it a plans
        /// folder must hold spec.md. It needs --plans.
        #[arg(long, requires = "plans")]
        spec: Option<PathBuf>,
        /// Run every agent of the job on this model instead of the project's role models.
        #[arg(long)]
        model: Option<String>,
        /// Combine the whole plan chain into one job. It needs --plans.
        #[arg(long, requires = "plans")]
        one_job: bool,
        /// Create the one job even when its estimate is over the budget. It needs --one-job.
        #[arg(long, requires = "one_job")]
        accept_over_budget: bool,
        /// Create the jobs even when the spec check refuses the spec. The reason is one line, at most
        /// 200 characters, and goes to the first job's ledger and warning.
        #[arg(long)]
        accept_spec: Option<String>,
    },
    /// Build, check and review a job until it merges or needs you.
    Run {
        /// The job id, for example 7 or 007.
        id: String,
        /// Continue with the next plan of the series after each merge.
        #[arg(long)]
        series: bool,
    },
    /// Show every job, its stage and what waits on you.
    ///
    /// It never changes anything, so it is safe to run at any time.
    Status,
    /// Print the full record of one job as JSON, for debugging.
    Show {
        /// The job id, for example 7 or 007.
        id: String,
    },
    /// Merge a job that waits on you.
    ///
    /// The job must be in needs-human or approve. A job whose review did not run, or asked for
    /// changes, needs --without-review or --waive.
    Approve {
        /// The job id, for example 7 or 007.
        id: String,
        /// Merge a needs-human job even though its code review has not run.
        #[arg(long)]
        without_review: bool,
        /// Merge a job whose review left open blockers, with a one-line reason.
        #[arg(long, conflicts_with = "without_review")]
        waive: Option<String>,
    },
    /// Continue a parked job at the stage that parked it.
    ///
    /// For a failed review, missing checks, a busy main, a busy merge lock or a spent sync cap. Fix
    /// the cause first. The job then goes on like `run` until it merges or parks again.
    Retry {
        /// The job id, for example 7 or 007.
        id: String,
    },
    /// Send the builder a one-line note and return the job to build.
    ///
    /// It also revives a cancelled job. It does not run the job: run `factory run <id>` next.
    Rework {
        /// The job id, for example 7 or 007.
        id: String,
        /// What the builder should change, one line of at most 200 characters.
        note: String,
        /// Send a job to the builder even though its park has a retry stage.
        #[arg(long)]
        force: bool,
    },
    /// Give up on a job, or remove a draft by its name.
    ///
    /// Nothing is deleted: the branch and worktree stay until `factory cleanup`. `rework` revives
    /// a cancelled job.
    Cancel {
        /// The job id, or the name of a draft.
        id: String,
        /// Why the job is cancelled, stored on its card.
        #[arg(long, default_value = "cancelled by user")]
        reason: String,
        /// Cancel even though the job holds a live lease, for when its process is gone.
        #[arg(long)]
        force: bool,
    },
    /// Remove the worktree and branch of a merged or cancelled job.
    ///
    /// The job folder always stays. A cancelled job also loses its board card, unless it is in a
    /// series with other jobs or holds a live lease.
    Cleanup {
        /// The job id, for example 7 or 007.
        id: String,
        /// Remove the branch even though it has commits that are not in the base branch.
        #[arg(long)]
        discard_commits: bool,
    },
    /// Show the token use of a job, or the project's cache use with --cache.
    Tokens {
        /// The job id, for example 7 or 007.
        #[arg(required_unless_present = "cache", conflicts_with = "cache")]
        id: Option<String>,
        /// Print cache use per role and the resumed runs, for every job of the project.
        #[arg(long)]
        cache: bool,
        /// Only list resumed runs of the last N days.
        #[arg(long, requires = "cache", value_parser = clap::value_parser!(u32).range(1..))]
        days: Option<u32>,
    },
    /// Show the cost of every job and of each size group, and write costs.json.
    Costs {
        /// Print the cost trend per marker instead of the job tables.
        #[arg(long)]
        trend: bool,
    },
    /// Check which context reaches each role's agent.
    ///
    /// Each role's agent is asked for a marker, and every channel gets a pass or a fail.
    Probe {
        /// The agent CLI to probe. Defaults to the project's agent_cli.
        #[arg(long, value_parser = harness_parser())]
        harness: Option<Harness>,
        /// A role to probe, repeatable. Defaults to every role.
        #[arg(long, value_parser = role_parser())]
        role: Vec<ProbeRole>,
        /// Print the report as JSON and nothing else.
        #[arg(long)]
        json: bool,
        /// How the marker reaches Claude: agent (a job-folder agents file, passed with --agents), append or stdin.
        #[arg(long, value_parser = delivery_parser(), default_value = "agent")]
        delivery: Delivery,
    },
    /// Build one labelled GitHub issue from spec to merge.
    ///
    /// It picks the issue, drafts its spec and plans, builds the job and settles it. The project
    /// needs `intake = true`.
    Intake {
        /// Run one pass and exit. A pass is always one issue, so this is the only mode.
        #[arg(long)]
        once: bool,
        /// Name the issue that would be picked and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Track a feature that is still being specified.
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// Register, inspect and remove the projects in the factory home.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Refresh the presets in the factory home.
    Presets {
        #[command(subcommand)]
        cmd: PresetsCmd,
    },
    /// Install the /factory command and the Codex skill without the plugin.
    ///
    /// A fallback for when the Claude Code plugin is not used. It prints the plugin commands too.
    Install {
        /// The folder to write factory.md into. By default installs for Claude and Codex under $HOME.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Replace an existing factory.md that differs.
        #[arg(long)]
        force: bool,
    },
    /// Regenerate or check the Claude Code plugin files.
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Update the /factory command and the unedited presets.
    ///
    /// It also reports overrides that shadow them and writes the version marker.
    Upgrade {
        /// Report what would change and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Merge the base branch into a job, then check and review it again.
    ///
    /// A branch that was rewritten is reset first.
    Sync {
        /// The job id, for example 7 or 007.
        id: String,
    },
    /// File a merged job's follow-up tickets and close its parent tickets.
    Tickets {
        /// The job id, for example 7 or 007.
        id: String,
        /// Print what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the factory log, filtered by job, level or event.
    Log {
        /// Only the lines of this job.
        #[arg(long)]
        job: Option<String>,
        /// Only lines at this level or above (error, warn, info, debug, trace).
        #[arg(long)]
        level: Option<String>,
        /// Only lines whose event is this name or starts with it plus a dot.
        #[arg(long)]
        event: Option<String>,
        /// Keep the last N lines; 0 means all.
        #[arg(long, default_value_t = 100)]
        tail: usize,
    },
    /// Refresh the knowledge base now.
    ///
    /// It ignores the stop marker.
    KbRefresh {
        /// The job whose ledger gets the rulings.
        #[arg(long)]
        job: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SpecCmd {
    /// Add a draft card at stage interviewing and print where its spec file goes.
    Start {
        /// A short name for the draft, for example login-fix.
        slug: String,
        /// One line that says what the feature does.
        feature: String,
    },
    /// Move a draft to interviewing, writing-spec or writing-plans.
    Stage {
        /// The name of the draft.
        slug: String,
        /// The new stage: interviewing, writing-spec or writing-plans.
        stage: String,
    },
    /// Print the stage and spec file of a draft.
    Resume {
        /// The name of the draft.
        slug: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum PluginCmd {
    /// Write the plugin files from the shipped command text.
    ///
    /// With --check it only verifies them and writes nothing.
    Sync {
        /// The repository root. Defaults to the current directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Fail when a plugin file differs from the generated text, and write nothing.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum PresetsCmd {
    /// Bring the seeded presets up to the built-in text.
    ///
    /// Files you edited by hand are listed and kept, unless --force.
    Refresh {
        /// Overwrite files that were edited by hand.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    /// Register a repository, or a folder inside one, as a project.
    Add {
        /// The repository top folder, or a subfolder of it for a monorepo project.
        path: PathBuf,
        /// The preset to start from, a folder name under presets/ in the factory home, for example rust.
        #[arg(long)]
        preset: String,
        /// The branch jobs start from and merge into. Defaults to the branch the repo is on.
        #[arg(long)]
        base: Option<String>,
        /// The project name. Defaults to the lowercased repo folder name.
        #[arg(long)]
        name: Option<String>,
        /// The agent CLI for the project. `codex` seeds the Codex model names.
        #[arg(long, value_parser = ["claude", "codex"])]
        agent_cli: Option<String>,
    },
    /// List the projects.
    List,
    /// Print the name of the registered project that contains a folder.
    Which {
        /// Defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Show the resolved settings of a project.
    Show {
        /// The project name.
        name: String,
    },
    /// Add the settings a project file does not list yet, as comments.
    ///
    /// Each one is written as a comment with its default value, so the file shows every setting.
    Defaults {
        /// The project. Leave it out with --all.
        name: Option<String>,
        /// Every project.
        #[arg(long, conflicts_with = "name")]
        all: bool,
    },
    /// Delete a project folder from the factory home.
    ///
    /// The repository and the presets stay.
    Remove {
        /// The project name.
        name: String,
        /// Remove it even though some jobs are not merged or cancelled, or its file is broken.
        #[arg(long)]
        force: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    /// Every command and subcommand, with its path such as `project add`.
    fn all_commands(cmd: &clap::Command, prefix: &str, out: &mut Vec<(String, clap::Command)>) {
        for sub in cmd.get_subcommands() {
            let path = format!("{prefix}{}", sub.get_name());
            out.push((path.clone(), sub.clone()));
            all_commands(sub, &format!("{path} "), out);
        }
    }

    #[test]
    fn every_command_has_a_short_summary() {
        use clap::CommandFactory;
        let mut commands = Vec::new();
        all_commands(&Cli::command(), "", &mut commands);
        assert!(commands.len() > 20, "{}", commands.len());
        let bad: Vec<String> = commands
            .into_iter()
            .filter_map(|(path, cmd)| {
                let about = cmd.get_about().map(|a| a.to_string()).unwrap_or_default();
                (about.is_empty() || about.chars().count() > 80)
                    .then(|| format!("`factory {path}`: {about:?}"))
            })
            .collect();
        assert!(
            bad.is_empty(),
            "summaries missing or over 80 characters:\n{}",
            bad.join("\n")
        );
    }

    #[test]
    fn every_positional_argument_is_described() {
        use clap::CommandFactory;
        let mut commands = Vec::new();
        all_commands(&Cli::command(), "", &mut commands);
        for (path, cmd) in commands {
            for arg in cmd.get_positionals() {
                assert!(
                    arg.get_help().is_some(),
                    "`factory {path}` argument <{}> has no help",
                    arg.get_id()
                );
            }
        }
    }

    #[test]
    fn bare_factory_prints_the_help() {
        let err = Cli::try_parse_from(["factory"]).expect_err("shows help");
        assert_eq!(
            err.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        let text = err.to_string();
        assert!(text.contains("Typical flow:"), "{text}");
        assert!(text.contains("retry"), "{text}");
    }

    #[test]
    fn version_flag_prints_the_workspace_version() {
        let err = Cli::try_parse_from(["factory", "--version"]).expect_err("prints the version");
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        let text = err.to_string();
        assert!(text.contains("factory "), "{text}");
        assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    }

    #[test]
    fn project_add_takes_agent_cli() {
        let cli = Cli::try_parse_from([
            "factory",
            "project",
            "add",
            "/r",
            "--preset",
            "rust",
            "--agent-cli",
            "codex",
        ])
        .expect("parses");
        match cli.command {
            Command::Project {
                cmd: ProjectCmd::Add { agent_cli, .. },
            } => assert_eq!(agent_cli.as_deref(), Some("codex")),
            other => panic!("unexpected {other:?}"),
        }
        let err = Cli::try_parse_from([
            "factory",
            "project",
            "add",
            "/r",
            "--preset",
            "rust",
            "--agent-cli",
            "gpt",
        ])
        .expect_err("gpt is refused");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn spec_without_plans_is_a_usage_error() {
        let err = Cli::try_parse_from(["factory", "new", "s", "f", "--spec", "x.md"])
            .expect_err("needs --plans");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn one_job_needs_plans() {
        let err = Cli::try_parse_from(["factory", "new", "s", "f", "--one-job"])
            .expect_err("needs --plans");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn accept_over_budget_needs_one_job() {
        let err = Cli::try_parse_from([
            "factory",
            "new",
            "s",
            "f",
            "--plans",
            "p",
            "--accept-over-budget",
        ])
        .expect_err("needs --one-job");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn one_job_flags_parse() {
        let cli = Cli::try_parse_from([
            "factory",
            "new",
            "s",
            "f",
            "--plans",
            "p",
            "--one-job",
            "--accept-over-budget",
        ])
        .expect("parse");
        assert!(matches!(
            cli.command,
            Command::New {
                one_job: true,
                accept_over_budget: true,
                ..
            }
        ));
    }

    #[test]
    fn new_parses_accept_spec() {
        let cli = Cli::try_parse_from(["factory", "new", "d", "f", "--accept-spec", "checked"])
            .expect("parse");
        assert!(matches!(
            cli.command,
            Command::New { accept_spec: Some(ref r), .. } if r == "checked"
        ));
    }

    #[test]
    fn spec_subcommands_parse() {
        let cli =
            Cli::try_parse_from(["factory", "spec", "start", "demo", "a feature"]).expect("start");
        assert!(
            matches!(cli.command, Command::Spec { cmd: SpecCmd::Start { ref slug, ref feature } } if slug == "demo" && feature == "a feature")
        );
        let cli = Cli::try_parse_from(["factory", "spec", "stage", "demo", "writing-spec"])
            .expect("stage");
        assert!(
            matches!(cli.command, Command::Spec { cmd: SpecCmd::Stage { ref slug, ref stage } } if slug == "demo" && stage == "writing-spec")
        );
        let cli = Cli::try_parse_from(["factory", "spec", "resume", "demo"]).expect("resume");
        assert!(
            matches!(cli.command, Command::Spec { cmd: SpecCmd::Resume { ref slug } } if slug == "demo")
        );
        assert!(Cli::try_parse_from(["factory", "spec", "start", "demo"]).is_err());
    }

    #[test]
    fn probe_parses_flags() {
        let cli = Cli::try_parse_from([
            "factory",
            "probe",
            "--harness",
            "codex",
            "--role",
            "builder",
            "--role",
            "kb",
            "--json",
        ])
        .expect("parses");
        match cli.command {
            Command::Probe {
                harness,
                role,
                json,
                delivery,
            } => {
                assert_eq!(harness, Some(Harness::Codex));
                assert_eq!(role, vec![ProbeRole::Builder, ProbeRole::Kb]);
                assert!(json);
                assert_eq!(delivery, Delivery::Agent);
            }
            other => panic!("not a probe: {other:?}"),
        }
    }

    #[test]
    fn intake_flags_parse() {
        let flags = |args: &[&str]| {
            let mut argv = vec!["factory", "intake"];
            argv.extend_from_slice(args);
            match Cli::try_parse_from(argv).expect("parses").command {
                Command::Intake { once, dry_run } => (once, dry_run),
                other => panic!("not an intake: {other:?}"),
            }
        };
        assert_eq!(flags(&["--once"]), (true, false));
        assert_eq!(flags(&["--dry-run"]), (false, true));
        assert_eq!(flags(&[]), (false, false));
    }

    #[test]
    fn probe_rejects_unknown_role() {
        let err = Cli::try_parse_from(["factory", "probe", "--role", "nope"]).expect_err("role");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        let err = Cli::try_parse_from(["factory", "probe", "--harness", "gpt"]).expect_err("cli");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        let err =
            Cli::try_parse_from(["factory", "probe", "--delivery", "pipe"]).expect_err("delivery");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn project_conflicts_with_root() {
        let err = Cli::try_parse_from(["factory", "--project", "p", "--root", "/x", "status"])
            .expect_err("conflict");
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        let cli = Cli::try_parse_from(["factory", "--project", "p", "status"]).expect("parses");
        assert_eq!(cli.project.as_deref(), Some("p"));
    }

    #[test]
    fn tokens_cache_with_an_id_is_refused() {
        let err = Cli::try_parse_from(["factory", "tokens", "1", "--cache"]).expect_err("conflict");
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn tokens_without_id_or_cache_is_refused() {
        Cli::try_parse_from(["factory", "tokens"]).expect_err("needs an id or --cache");
    }

    #[test]
    fn tokens_days_needs_cache_and_at_least_one() {
        Cli::try_parse_from(["factory", "tokens", "--days", "3"]).expect_err("days alone");
        Cli::try_parse_from(["factory", "tokens", "--cache", "--days", "0"]).expect_err("zero");
    }

    #[test]
    fn tokens_cache_with_days_parses() {
        let cli =
            Cli::try_parse_from(["factory", "tokens", "--cache", "--days", "7"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Tokens {
                id: None,
                cache: true,
                days: Some(7)
            }
        ));
    }

    #[test]
    fn spec_with_plans_parses() {
        Cli::try_parse_from(["factory", "new", "s", "f", "--plans", "p", "--spec", "x.md"])
            .expect("parses");
    }

    #[test]
    fn log_parses_flags() {
        let cli = Cli::try_parse_from([
            "factory", "log", "--job", "003", "--level", "warn", "--tail", "5",
        ])
        .expect("parses");
        assert!(matches!(
            cli.command,
            Command::Log { job: Some(ref j), level: Some(ref l), event: None, tail: 5 } if j == "003" && l == "warn"
        ));
    }

    #[test]
    fn log_parses_event_flag() {
        let cli = Cli::try_parse_from(["factory", "log", "--event", "task"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Log { event: Some(ref e), .. } if e == "task"
        ));
    }

    #[test]
    fn log_tail_defaults_to_100() {
        let cli = Cli::try_parse_from(["factory", "log"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Log {
                job: None,
                level: None,
                event: None,
                tail: 100
            }
        ));
    }

    #[test]
    fn tickets_parses_dry_run() {
        let cli = Cli::try_parse_from(["factory", "tickets", "012", "--dry-run"]).expect("parses");
        assert!(matches!(cli.command, Command::Tickets { ref id, dry_run: true } if id == "012"));
        let cli = Cli::try_parse_from(["factory", "tickets", "012"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Tickets { dry_run: false, .. }
        ));
    }

    #[test]
    fn plugin_sync_parses_check() {
        let cli = Cli::try_parse_from(["factory", "plugin", "sync", "--check"]).expect("parse");
        assert!(matches!(
            cli.command,
            Command::Plugin {
                cmd: PluginCmd::Sync {
                    repo: None,
                    check: true
                }
            }
        ));
        let cli =
            Cli::try_parse_from(["factory", "plugin", "sync", "--repo", "/r"]).expect("parse");
        assert!(matches!(
            cli.command,
            Command::Plugin {
                cmd: PluginCmd::Sync {
                    repo: Some(_),
                    check: false
                }
            }
        ));
    }

    #[test]
    fn upgrade_parses_dry_run() {
        let cli = Cli::try_parse_from(["factory", "upgrade", "--dry-run"]).expect("parse");
        assert!(matches!(cli.command, Command::Upgrade { dry_run: true }));
        let cli = Cli::try_parse_from(["factory", "upgrade"]).expect("parse");
        assert!(matches!(cli.command, Command::Upgrade { dry_run: false }));
    }

    #[test]
    fn sync_parses_one_id() {
        let cli = Cli::try_parse_from(["factory", "sync", "012"]).expect("parses");
        assert!(matches!(cli.command, Command::Sync { ref id } if id == "012"));
    }

    #[test]
    fn tickets_needs_an_id() {
        let err = Cli::try_parse_from(["factory", "tickets"]).expect_err("needs an id");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }
}
