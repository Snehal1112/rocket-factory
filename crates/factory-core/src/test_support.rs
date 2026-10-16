//! Helpers for tests that touch process-global state.

use std::sync::{Mutex, MutexGuard};

/// One lock for every test that sets or reads an environment variable.
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Sets a variable and restores the previous value when dropped, also on panic.
/// It holds `ENV_LOCK` for its whole life.
pub(crate) struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &str) -> EnvVarGuard {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        EnvVarGuard { key, old, _lock: lock }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
