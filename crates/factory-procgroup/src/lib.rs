//! Children in their own process group, killed with the group on drop.
//!
//! The crate reads no process facts: the caller passes `start_of`.

mod registry;
mod shutdown;

pub use registry::reap_leftovers;
pub use shutdown::install_shutdown_handler;

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{LazyLock, Mutex, MutexGuard};

use registry::Entry;

/// Returns the start time of a pid in whole seconds times 1000.
pub type StartFn = fn(u32) -> Option<u64>;

struct Registry {
    dir: PathBuf,
    owner: String,
    start_of: StartFn,
}

impl Registry {
    fn file(&self) -> PathBuf {
        self.dir.join(registry::file_name(&self.owner))
    }
}

static GROUPS: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(Mutex::default);
static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

/// Locks a mutex even when another thread panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Keeps a registry file `<dir>/children-<owner>.json` for the children of
/// this process. An owner outside `[A-Za-z0-9_-]+` keeps no registry.
pub fn set_registry(dir: PathBuf, owner: String, start_of: StartFn) {
    let valid = registry::valid_owner(&owner);
    *lock(&REGISTRY) = valid.then_some(Registry {
        dir,
        owner,
        start_of,
    });
}

/// Adds or removes one pid in the registry file, under the registry mutex.
fn update_registry(pid: u32, add: bool) {
    let guard = lock(&REGISTRY);
    let Some(reg) = guard.as_ref() else {
        return;
    };
    let file = reg.file();
    // A missing or corrupt file counts as empty.
    let mut entries = registry::read(&file).unwrap_or_default();
    entries.retain(|e| e.pid != pid);
    if add {
        entries.push(Entry {
            pid,
            start_ms: (reg.start_of)(pid),
            owner: reg.owner.clone(),
        });
    }
    registry::write(&file, &entries);
}

/// A child that leads its own process group. Dropping it kills the group.
pub struct GroupChild {
    child: Child,
    pgid: i32,
    released: bool,
}

impl GroupChild {
    /// The process group id, equal to the pid of the child.
    pub fn pgid(&self) -> i32 {
        self.pgid
    }

    /// Sends SIGKILL to the whole group. A group that is gone is not an error.
    pub fn kill(&mut self) -> std::io::Result<()> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        lock(&GROUPS).remove(&self.pgid);
        update_registry(self.child.id(), false);
        #[cfg(unix)]
        return killpg(self.pgid);
        #[cfg(not(unix))]
        self.child.kill()
    }
}

/// SIGKILL to a whole group. A group that is gone (`ESRCH`) is not an error.
#[cfg(unix)]
pub(crate) fn killpg(pgid: i32) -> std::io::Result<()> {
    // SAFETY: killpg takes plain integers and has no memory effects.
    if unsafe { libc::killpg(pgid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        _ => Err(err),
    }
}

impl Deref for GroupChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for GroupChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for GroupChild {
    fn drop(&mut self) {
        let _ = self.kill();
        // The leader was just killed, so this returns at once and leaves no zombie.
        let _ = self.child.wait();
    }
}

/// Spawns the command as the leader of a new process group.
pub fn spawn(cmd: &mut Command) -> std::io::Result<GroupChild> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let child = cmd.spawn()?;
    let pgid = child.id() as i32;
    lock(&GROUPS).insert(pgid);
    update_registry(child.id(), true);
    Ok(GroupChild {
        child,
        pgid,
        released: false,
    })
}

/// Kills every group this process still holds, and clears the registry file.
pub fn kill_all() {
    let groups: Vec<i32> = lock(&GROUPS).drain().collect();
    #[cfg(unix)]
    for pgid in groups {
        let _ = killpg(pgid);
    }
    #[cfg(not(unix))]
    drop(groups);
    if let Some(reg) = lock(&REGISTRY).as_ref() {
        registry::write(&reg.file(), &[]);
    }
}
