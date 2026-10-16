use clap::Parser;
use factory_cli::{dispatch, project, Cli, CliCtx, Command};

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
    let ctx = match CliCtx::from_env(cli.root.clone()) {
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
