use clap::Parser;
use factory_cli::{dispatch, Cli, CliCtx};

fn main() {
    let cli = Cli::parse();
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
