//! `factory install`: writes the global `/factory` slash command and Codex skill.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use factory_agent::installed_plugin_ids;
pub use factory_core::GLOBAL_COMMAND;
use factory_core::{
    claude_commands_dir, codex_skills_dir, write_atomic, TOOL_MAP_CLAUDE, TOOL_MAP_CODEX,
};

use crate::{plugin, CliError};

const FILE_NAME: &str = "factory.md";
const SKILL_DIR_NAME: &str = "factory";
const SKILL_FILE_NAME: &str = "SKILL.md";
const SKILL_DESCRIPTION: &str = "Run the software factory workflow for a project: brainstorm, plan, then build, check, review, and merge through the factory driver.";

/// The text of the Claude `/factory` command, shared by `factory install` and the plugin.
pub fn claude_command_text() -> String {
    GLOBAL_COMMAND.to_string()
}

/// Installs into `dir` when given, else installs the Claude command and Codex skill under `$HOME`.
pub fn run(dir: Option<&Path>, force: bool) -> Result<String, CliError> {
    run_with_claude(dir, force, OsStr::new("claude"))
}

/// Like `run`, asking `claude_program` which plugins are installed.
pub fn run_with_claude(
    dir: Option<&Path>,
    force: bool,
    claude_program: &OsStr,
) -> Result<String, CliError> {
    let command = claude_command_text();
    if let Some(folder) = dir {
        return install_file(folder, FILE_NAME, &command, force);
    }
    let home = user_home()?;
    let commands = claude_commands_dir(&home);
    let old_command = commands.join(FILE_NAME);
    let has_plugin = plugin::plugin_installed(&installed_plugin_ids(claude_program));
    let claude = if has_plugin && !force {
        let mut line = "skipped Claude command: the factory plugin is installed".to_string();
        if old_command.exists() {
            line = format!("{line}\n{}", plugin::duplicate_note(&old_command));
        }
        line
    } else {
        let line = install_file(&commands, FILE_NAME, &command, force)?;
        if has_plugin {
            format!("{line}\n{}", plugin::duplicate_note(&old_command))
        } else {
            format!("{line}\n{}", plugin::install_hint())
        }
    };
    let codex_folder = codex_skills_dir(&home).join(SKILL_DIR_NAME);
    let codex_target = codex_folder.join(SKILL_FILE_NAME);
    let skill = codex_skill().map_err(|error| {
        CliError::Refused(format!("{claude}\nCodex skill install failed: {error}"))
    })?;
    if !force {
        match std::fs::read_to_string(&codex_target) {
            Ok(existing) if existing != skill => {
                return Ok(format!(
                    "{claude}\nnote: Codex skill not installed: {} exists and differs; use --force to replace it",
                    codex_target.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::Refused(format!(
                    "{claude}\nCodex skill install failed: cannot read {}: {error}",
                    codex_target.display()
                )));
            }
        }
    }
    let codex = install_file(&codex_folder, SKILL_FILE_NAME, &skill, force).map_err(|error| {
        CliError::Refused(format!("{claude}\nCodex skill install failed: {error}"))
    })?;
    Ok(format!("{claude}\n{codex}"))
}

/// The Claude commands folder, used by `factory upgrade`.
pub fn commands_folder(dir: Option<&Path>) -> Result<PathBuf, CliError> {
    match dir {
        Some(d) => Ok(d.to_path_buf()),
        None => Ok(claude_commands_dir(&user_home()?)),
    }
}

/// The home folder when `commands_dir` is the standard `<home>/.claude/commands` path.
pub fn home_of_standard_commands_dir(commands_dir: &Path) -> Option<&Path> {
    let home = commands_dir.parent().and_then(Path::parent)?;
    (claude_commands_dir(home).as_path() == commands_dir).then_some(home)
}

/// The path of the Claude command file inside a commands folder.
pub fn command_file(folder: &Path) -> PathBuf {
    folder.join(FILE_NAME)
}

/// Updates the Codex skill when the command folder is the standard `~/.claude/commands` path.
pub fn upgrade_codex_skill(commands_dir: &Path, dry_run: bool) -> Result<Option<String>, CliError> {
    let Some(home) = home_of_standard_commands_dir(commands_dir) else {
        return Ok(None);
    };
    let folder = codex_skills_dir(home).join(SKILL_DIR_NAME);
    match std::fs::metadata(folder.join(SKILL_FILE_NAME)) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(CliError::Refused(format!(
                "cannot read {}: {e}",
                folder.join(SKILL_FILE_NAME).display()
            )))
        }
    }
    Ok(Some(upgrade_file(
        &folder,
        SKILL_FILE_NAME,
        &codex_skill()?,
        dry_run,
    )?))
}

/// Updates one command file in an explicit folder. Kept for callers that manage one integration.
pub fn upgrade_into(folder: &Path, dry_run: bool) -> Result<String, CliError> {
    upgrade_file(folder, FILE_NAME, &claude_command_text(), dry_run)
}

fn user_home() -> Result<PathBuf, CliError> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| CliError::Refused("$HOME is not set, pass --dir".into()))?;
    Ok(PathBuf::from(home))
}

fn codex_skill() -> Result<String, CliError> {
    build_codex_skill(&claude_command_text())
}

fn build_codex_skill(command: &str) -> Result<String, CliError> {
    let body = command
        .split_once("\n---\n")
        .map(|(_, body)| body.trim_start())
        .unwrap_or(command);
    let body = body
        .strip_prefix("# Factory\n\nArguments: $ARGUMENTS\n")
        .unwrap_or(body);
    let shared = body.strip_suffix(TOOL_MAP_CLAUDE).ok_or_else(|| {
        CliError::Refused("global command must end with the Claude tool map".into())
    })?;
    let shared = shared.trim_end().replace("/factory", "$factory");
    Ok(format!(
        "---\nname: factory\ndescription: {SKILL_DESCRIPTION}\n---\n\n# Factory\n\nUse the user's accompanying request as the factory arguments.\n\n{shared}\n\n{TOOL_MAP_CODEX}"
    ))
}

fn install_file(
    folder: &Path,
    file_name: &str,
    content: &str,
    force: bool,
) -> Result<String, CliError> {
    let target = folder.join(file_name);
    match std::fs::read_to_string(&target) {
        Ok(existing) if existing == content => {
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
    write_atomic(&target, content.as_bytes())?;
    Ok(format!("installed {}", target.display()))
}

/// First of `<file>.bak`, `.bak.1`, `.bak.2`, ... that is not an existing file, so the first backup survives.
/// A non-file at a candidate path is returned as is and fails on write.
fn free_backup_path(folder: &Path, file_name: &str) -> PathBuf {
    let mut backup = folder.join(format!("{file_name}.bak"));
    let mut n = 1u32;
    while backup.is_file() {
        backup = folder.join(format!("{file_name}.bak.{n}"));
        n += 1;
    }
    backup
}

fn upgrade_file(
    folder: &Path,
    file_name: &str,
    content: &str,
    dry_run: bool,
) -> Result<String, CliError> {
    let target = folder.join(file_name);
    let backup = free_backup_path(folder, file_name);
    let prefix = if dry_run { "would " } else { "" };
    match std::fs::read(&target) {
        Ok(existing) if existing == content.as_bytes() => {
            Ok(format!("already installed: {}", target.display()))
        }
        Ok(existing) => {
            if !dry_run {
                write_atomic(&backup, &existing).map_err(|e| {
                    CliError::Refused(format!("cannot save {}: {e}", backup.display()))
                })?;
                write_atomic(&target, content.as_bytes())?;
            }
            Ok(format!(
                "{prefix}updated {} (old copy saved as {})",
                target.display(),
                backup.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !dry_run {
                write_atomic(&target, content.as_bytes())?;
            }
            Ok(format!("{prefix}installed {}", target.display()))
        }
        Err(e) => Err(CliError::Refused(format!(
            "cannot read {}: {e}",
            target.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_command_builds_a_codex_skill() {
        let skill = codex_skill().expect("skill");
        assert!(skill.contains("name: factory"));
    }

    #[test]
    fn codex_skill_reports_a_missing_claude_map() {
        let error = build_codex_skill("# Factory\n\nno map here\n")
            .expect_err("a command without the Claude map must be an error");
        assert!(error.to_string().contains("Claude tool map"), "{error}");
    }
}
