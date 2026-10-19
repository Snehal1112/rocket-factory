//! `factory install`: writes the global `/factory` slash command.

use std::path::{Path, PathBuf};

pub use factory_core::GLOBAL_COMMAND;
use factory_core::{claude_commands_dir, write_atomic};

use crate::CliError;

const FILE_NAME: &str = "factory.md";

/// Installs the command into `dir`, else into the Claude commands folder under `$HOME`.
pub fn run(dir: Option<&Path>, force: bool) -> Result<String, CliError> {
    let folder: PathBuf = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
            let home =
                home.ok_or_else(|| CliError::Refused("$HOME is not set, pass --dir".into()))?;
            claude_commands_dir(Path::new(&home))
        }
    };
    install_into(&folder, force)
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
