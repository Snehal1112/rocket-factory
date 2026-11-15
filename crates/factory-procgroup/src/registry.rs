//! The per-process registry file: `children-<owner>.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One registered child. `start_ms` is whole seconds times 1000.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub pid: u32,
    pub start_ms: Option<u64>,
    pub owner: String,
}

/// Reads the entries of a registry file.
pub(crate) fn read(file: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Rewrites the file atomically, or deletes it when no entry is left.
/// Failures are ignored: the registry is a safety net, not a duty.
pub(crate) fn write(file: &Path, entries: &[Entry]) {
    if entries.is_empty() {
        let _ = std::fs::remove_file(file);
        return;
    }
    let Ok(text) = serde_json::to_string(entries) else {
        return;
    };
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

const PREFIX: &str = "children-";
const SUFFIX: &str = ".json";

/// The registry file name of an owner.
pub(crate) fn file_name(owner: &str) -> String {
    format!("{PREFIX}{owner}{SUFFIX}")
}

/// The owner in a registry file name, or `None` for any other name.
fn owner_of(name: &str) -> Option<&str> {
    let owner = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    valid_owner(owner).then_some(owner)
}

/// True when the pid leads its own process group.
#[cfg(unix)]
fn leads_group(pid: i32) -> bool {
    // SAFETY: getpgid only reads process facts.
    unsafe { libc::getpgid(pid) == pid }
}

#[cfg(not(unix))]
fn leads_group(_pid: i32) -> bool {
    false
}

#[cfg(unix)]
fn kill_group(pid: i32) -> std::io::Result<()> {
    crate::killpg(pid)
}

#[cfg(not(unix))]
fn kill_group(_pid: i32) -> std::io::Result<()> {
    Ok(())
}

/// Handles one entry of a dead owner and says what happened.
fn reap_entry(
    entry: &Entry,
    owner: &str,
    running: &dyn Fn(u32) -> bool,
    start_of: crate::StartFn,
) -> String {
    let pid = entry.pid;
    let left = |why: &str| format!("left alone: pid {pid} of {owner}: {why}");
    // Pid 0 and values above `i32::MAX` would name our own or every group.
    let Some(signed) = i32::try_from(pid).ok().filter(|p| *p > 0) else {
        return left("invalid pid");
    };
    if !running(pid) {
        return left("not running");
    }
    if !leads_group(signed) {
        return left("not a group leader");
    }
    match (start_of(pid), entry.start_ms) {
        (Some(now), Some(then)) if now == then => {}
        (Some(_), Some(_)) => return left("start time differs"),
        _ => return left("start time unknown"),
    }
    match kill_group(signed) {
        Ok(()) => format!("killed: pid {pid} of {owner}"),
        Err(e) => left(&format!("kill failed: {e}")),
    }
}

/// Kills the groups that dead owners left in `dir`, then deletes their files.
///
/// A group dies only when the pid runs, leads its group and has the recorded
/// start time. Returns one line per entry; an unreadable file is kept.
pub fn reap_leftovers(
    dir: &Path,
    is_dead: &dyn Fn(&str) -> bool,
    running: &dyn Fn(u32) -> bool,
    start_of: crate::StartFn,
) -> Vec<String> {
    let names = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) => return vec![format!("left alone: registry {}: {e}", dir.display())],
    };
    let mut files: Vec<(String, std::path::PathBuf)> = names
        .filter_map(Result::ok)
        .filter_map(|f| {
            let name = f.file_name().into_string().ok()?;
            let owner = owner_of(&name)?.to_string();
            Some((owner, f.path()))
        })
        .filter(|(owner, _)| is_dead(owner))
        .collect();
    files.sort();
    let mut lines = Vec::new();
    for (owner, path) in files {
        match read(&path) {
            Ok(entries) => {
                lines.extend(
                    entries
                        .iter()
                        .map(|e| reap_entry(e, &owner, running, start_of)),
                );
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => lines.push(format!("left alone: registry {}: {e}", path.display())),
        }
    }
    lines
}

/// True when the owner is safe to use in a file name.
pub(crate) fn valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
