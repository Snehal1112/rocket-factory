//! `factory plugin sync`: generates the Claude Code plugin files from the shipped command text.

use std::path::Path;

use factory_core::version::FACTORY_VERSION;
use factory_core::{plugin_rel_files, write_atomic, MARKETPLACE_REL_FILE};

use crate::cli::PluginCmd;
use crate::{install, CliError};

const PLUGIN_DESCRIPTION: &str = "Run the software factory on the current project: brainstorm, plan, then the factory driver builds, checks, reviews and merges.";
const AUTHOR_NAME: &str = "Snehal Dangroshiya";

/// The `plugin.json` text for one version: pretty JSON ending in a newline.
pub fn plugin_manifest_text(version: &str) -> String {
    let manifest = serde_json::json!({
        "name": "factory",
        "description": PLUGIN_DESCRIPTION,
        "version": version,
        "author": { "name": AUTHOR_NAME },
    });
    format!("{manifest:#}\n")
}

/// The plugin id as `claude plugin list --json` reports it.
pub const PLUGIN_ID: &str = "factory@rocket-factory";
/// The marketplace the plugin is installed from.
pub const MARKETPLACE_SOURCE: &str = "Snehal1112/rocket-factory";

/// True when the factory plugin is among the installed plugin ids.
pub fn plugin_installed(ids: &[String]) -> bool {
    ids.iter().any(|id| id == PLUGIN_ID)
}

fn install_commands() -> String {
    format!(
        "  claude plugin marketplace add {MARKETPLACE_SOURCE}\n  claude plugin install {PLUGIN_ID}"
    )
}

/// The hint printed when the plugin is not installed.
pub fn install_hint() -> String {
    format!(
        "note: the Claude Code plugin is the preferred install:\n{}",
        install_commands()
    )
}

/// The note printed when the plugin is not installed but the old command file exists.
pub fn switch_note(old_command: &Path) -> String {
    format!(
        "note: to switch to the plugin, run the two commands below, then remove {}:\n{}",
        old_command.display(),
        install_commands()
    )
}

/// The note printed when the plugin and the old command file both exist.
pub fn duplicate_note(old_command: &Path) -> String {
    format!(
        "note: the factory plugin is installed and {0} also exists, so /factory is listed twice; remove it with: rm {0}",
        old_command.display()
    )
}

/// The line printed when the plugin is installed and no old command file exists.
pub fn update_line() -> String {
    format!("plugin: {PLUGIN_ID} is installed; update it with: claude plugin update {PLUGIN_ID}")
}

/// The generated files as (relative path, text) pairs.
fn generated() -> [(&'static str, String); 2] {
    let files = plugin_rel_files();
    [
        (files[0], plugin_manifest_text(FACTORY_VERSION)),
        (files[1], install::claude_command_text()),
    ]
}

/// Writes the generated plugin files under `repo` and reports `wrote` or `unchanged` per file.
pub fn sync(repo: &Path) -> Result<Vec<String>, CliError> {
    let mut lines = Vec::new();
    for (rel, text) in generated() {
        let target = repo.join(rel);
        if read_existing(&target)?.as_deref() == Some(text.as_str()) {
            lines.push(format!("unchanged {rel}"));
        } else {
            write_atomic(&target, text.as_bytes())?;
            lines.push(format!("wrote {rel}"));
        }
    }
    Ok(lines)
}

/// Fails when a generated plugin file under `repo` is missing or differs.
pub fn check(repo: &Path) -> Result<(), CliError> {
    for (rel, text) in generated() {
        if read_existing(&repo.join(rel))?.as_deref() != Some(text.as_str()) {
            return Err(CliError::Refused(format!(
                "plugin files out of date: {rel}; run factory plugin sync"
            )));
        }
    }
    Ok(())
}

/// Runs `factory plugin ...` and returns the text to print.
pub fn run(cmd: &PluginCmd) -> Result<String, CliError> {
    match cmd {
        PluginCmd::Sync { repo, check: only } => {
            let repo = match repo {
                Some(path) => path.clone(),
                None => std::env::current_dir()?,
            };
            if !repo.join(MARKETPLACE_REL_FILE).is_file() {
                return Err(CliError::Refused(format!(
                    "{} is not the repository root: {MARKETPLACE_REL_FILE} is missing; use --repo",
                    repo.display()
                )));
            }
            if *only {
                check(&repo)?;
                Ok("plugin files are current".to_string())
            } else {
                Ok(sync(&repo)?.join("\n"))
            }
        }
    }
}

fn read_existing(path: &Path) -> Result<Option<String>, CliError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CliError::Refused(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_the_claude_fields_and_a_newline() {
        let text = plugin_manifest_text("1.2.3");
        assert!(text.ends_with("}\n"));
        let json: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(json["version"], "1.2.3");
        assert_eq!(json["author"]["name"], AUTHOR_NAME);
    }

    #[test]
    fn run_refuses_a_folder_without_the_marketplace_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        for only in [false, true] {
            let cmd = PluginCmd::Sync {
                repo: Some(dir.path().to_path_buf()),
                check: only,
            };
            let error = run(&cmd).expect_err("wrong folder must fail");
            assert!(error.to_string().contains("--repo"), "{error}");
        }
        assert!(!dir.path().join("plugins").exists());
    }

    #[test]
    fn check_fails_on_an_empty_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = check(dir.path()).expect_err("missing files must fail");
        assert!(error.to_string().contains("factory plugin sync"), "{error}");
    }
}
