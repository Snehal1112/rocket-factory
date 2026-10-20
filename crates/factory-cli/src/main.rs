use clap::Parser;
use factory_cli::{dispatch, env_lock_wait, env_owner, project, Cli, CliCtx, Command};

fn main() {
    let cli = Cli::parse();
    if let Command::Project { cmd } = cli.command {
        let result = factory_cli::open_home().and_then(|home| project::run(&home, cmd));
        match result {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Command::Presets { cmd } = cli.command {
        let result =
            factory_cli::open_home().and_then(|home| factory_cli::presets::run(&home, cmd));
        match result {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Command::Upgrade { dry_run } = &cli.command {
        // Both lookups come before `run`, so an unset $HOME changes nothing.
        let result = factory_core::Home::from_env()
            .map_err(factory_cli::CliError::from)
            .and_then(|home| {
                let commands = factory_cli::install::commands_folder(None)?;
                factory_cli::upgrade::run(&home, &commands, *dry_run)
            });
        match result {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Command::Install { dir, force } = &cli.command {
        match factory_cli::install::run(dir.as_deref(), *force) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let built = match &cli.project {
        Some(name) => {
            let home = match factory_core::Home::from_env() {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("factory: {e}");
                    std::process::exit(2);
                }
            };
            if let Err(e) = factory_cli::check_home(&home) {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
            CliCtx::for_project(&home, name, None, &env_owner(), env_lock_wait()).map(|ctx| {
                for n in &ctx.notes {
                    eprintln!("note: {n}");
                }
                ctx
            })
        }
        None => CliCtx::from_env(cli.root.clone()),
    };
    let ctx = match built {
        Ok(c) => c,
        Err(e) => {
            eprintln!("factory: {e}");
            std::process::exit(2);
        }
    };
    // The log is installed once the context exists. Reading the log never writes to it.
    let _log_guard = if matches!(cli.command, Command::Log { .. }) {
        None
    } else {
        let env = std::env::var("FACTORY_LOG").ok();
        match factory_telemetry::init(
            ctx.log_target(cli.project.as_deref()),
            &ctx.log_level(),
            env.as_deref(),
        ) {
            Ok(guard) => {
                if let Some(note) = guard.note() {
                    eprintln!("note: FACTORY_LOG ignored: {note}");
                }
                Some(guard)
            }
            Err(e) => {
                eprintln!("note: log disabled: {e}");
                None
            }
        }
    };
    match dispatch(cli, &ctx) {
        Ok(text) => println!("{text}"),
        Err(e) => {
            // The consent refusal is printed bare so its line starts with `needs consent:`.
            if matches!(
                e,
                factory_cli::CliError::Engine(factory_engine::EngineError::NeedsConsent { .. })
            ) {
                eprintln!("{e}");
            } else {
                eprintln!("factory: {e}");
            }
            let busy = matches!(
                e,
                factory_cli::CliError::Engine(factory_engine::EngineError::ProjectBusy(_))
            );
            std::process::exit(if busy {
                factory_core::outcome_lines::BUSY_EXIT_CODE
            } else {
                1
            });
        }
    }
}
