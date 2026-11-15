//! Test helper: registers one child, installs the handler and waits to be signalled.
//!
//! Arguments: `<registry dir> <owner>`.

use std::io::Write;
use std::process::Command;

use factory_procgroup::{install_shutdown_handler, set_registry, spawn};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(owner)) = (args.next(), args.next()) else {
        eprintln!("usage: shutdown-demo <registry dir> <owner>");
        std::process::exit(2);
    };
    set_registry(dir.into(), owner, |_| Some(7000));
    install_shutdown_handler();
    let child = match spawn(Command::new("sleep").arg("60")) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("spawn failed: {e}");
            std::process::exit(1);
        }
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", child.id());
    let _ = out.flush();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
