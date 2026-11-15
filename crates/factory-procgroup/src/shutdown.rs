//! The shutdown handler: a signal ends the children, then the process.

/// Waits for SIGTERM, SIGINT and SIGHUP on one thread, kills every group,
/// and exits with `128 + signal`. Calling it again does nothing.
#[cfg(unix)]
pub fn install_shutdown_handler() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let Ok(mut signals) = Signals::new([SIGTERM, SIGINT, SIGHUP]) else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("procgroup-shutdown".into())
            .spawn(move || {
                if let Some(signal) = signals.forever().next() {
                    crate::kill_all();
                    std::process::exit(128 + signal);
                }
            });
        // Without the thread the signals would be swallowed, so say so.
        if let Err(e) = spawned {
            eprintln!("shutdown handler not started: {e}");
        }
    });
}

#[cfg(not(unix))]
pub fn install_shutdown_handler() {}
