mod events;
mod model;
mod plan;
mod server;
mod state;

use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match server::parse_args(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("factory-server: {message}");
            eprintln!("usage: factory-server [--root PATH] [--port N]");
            std::process::exit(2);
        }
    };
    let http = match tiny_http::Server::http(("127.0.0.1", config.port)) {
        Ok(http) => http,
        Err(error) => {
            eprintln!(
                "factory-server: cannot listen on port {}: {error}",
                config.port
            );
            std::process::exit(1);
        }
    };
    let app = Arc::new(server::App {
        root: config.root.clone(),
        registry: Mutex::new(events::Registry::new()),
    });
    if let Some(warning) = server::missing_factory_dir_warning(&config.root) {
        eprintln!("{warning}");
    }
    server::spawn_tailer(Arc::clone(&app));
    println!(
        "factory-server: http://127.0.0.1:{}/ (root {})",
        config.port,
        config.root.display()
    );
    server::serve(http, app, config.port);
}
