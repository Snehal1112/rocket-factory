//! `factory install`: writes the global `/factory` slash command.

use std::path::{Path, PathBuf};

pub use factory_core::GLOBAL_COMMAND;
use factory_core::{claude_commands_dir, write_atomic};

use crate::CliError;

const FILE_NAME: &str = "factory.md";

/// Installs the command into `dir`, else into the Claude commands folder under `$HOME`.
pub fn run(dir: Option<&Path>, force: bool) -> Result<String, CliError> {
    let folder = commands_folder(dir)?;
    install_into(&folder, force)
}

/// The folder for the command: `dir`, else the Claude commands folder under `$HOME`.
pub fn commands_folder(dir: Option<&Path>) -> Result<PathBuf, CliError> {
    match dir {
        Some(d) => Ok(d.to_path_buf()),
        None => {
            let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
            let home =
                home.ok_or_else(|| CliError::Refused("$HOME is not set, pass --dir".into()))?;
            Ok(claude_commands_dir(Path::new(&home)))
        }
    }
}

/// Installs or updates the command in `folder`, keeping the old text as `factory.md.bak`.
/// With `dry_run` it only says what it would do.
pub fn upgrade_into(folder: &Path, dry_run: bool) -> Result<String, CliError> {
    let target = folder.join(FILE_NAME);
    let bak = folder.join(format!("{FILE_NAME}.bak"));
    let prefix = if dry_run { "would " } else { "" };
    match std::fs::read(&target) {
        Ok(existing) if existing == GLOBAL_COMMAND.as_bytes() => {
            Ok(format!("already installed: {}", target.display()))
        }
        Ok(existing) => {
            if !dry_run {
                write_atomic(&bak, &existing).map_err(|e| {
                    CliError::Refused(format!("cannot save {}: {e}", bak.display()))
                })?;
                write_atomic(&target, GLOBAL_COMMAND.as_bytes())?;
            }
            Ok(format!(
                "{prefix}updated {} (old copy saved as {})",
                target.display(),
                bak.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !dry_run {
                write_atomic(&target, GLOBAL_COMMAND.as_bytes())?;
            }
            Ok(format!("{prefix}installed {}", target.display()))
        }
        Err(e) => Err(CliError::Refused(format!(
            "cannot read {}: {e}",
            target.display()
        ))),
    }
}

fn install_into(folder: &Path, force: bool) -> Result<String, CliError> {
    let target = folder.join(FILE_NAME);
    match std::fs::read_to_string(&target) {
        Ok(existing) if existing == GLOBAL_COMMAND => {
            return Ok(format!("already installed: {}", target.display()));
        }
        Ok(_) if !force => {
            return Err(CliError::Refused(format!(
                "{} exists and differs from the shipped command, use --force to replace it",
                target.display()
            )));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(CliError::Refused(format!(
                "cannot read {}: {e}",
                target.display()
            )))
        }
    }
    write_atomic(&target, GLOBAL_COMMAND.as_bytes())?;
    Ok(format!("installed {}", target.display()))
}
