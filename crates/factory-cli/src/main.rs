use clap::Parser;
use factory_cli::{dispatch, env_lock_wait, env_owner, project, Cli, CliCtx, Command};

fn main() {
    let cli = Cli::parse();
    if let Command::Project { cmd } = cli.command {
        let result = factory_core::Home::from_env()
            .map_err(factory_cli::CliError::from)
            .and_then(|home| project::run(&home, cmd));
        match result {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("factory: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let built = match &cli.project {
        Some(name) => factory_core::Home::from_env().map_err(factory_cli::CliError::from).and_then(|home| {
            let ctx = CliCtx::for_project(&home, name, None, &env_owner(), env_lock_wait())?;
            for n in &ctx.notes {
                eprintln!("note: {n}");
            }
            Ok(ctx)
        }),
        None => CliCtx::from_env(cli.root.clone()),
    };
    let ctx = match built {
        Ok(c) => c,
        Err(e) => {
            eprintln!("factory: {e}");
            std::process::exit(2);
        }
    };
    match dispatch(cli, &ctx) {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("factory: {e}");
            std::process::exit(1);
        }
    }
}
