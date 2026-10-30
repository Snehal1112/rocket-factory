//! `factory upgrade`: updates the command and the unedited presets, then reports shadowing overrides.

use std::ffi::OsStr;
use std::path::Path;

use factory_agent::installed_plugin_ids;
use factory_core::preset::{self, ShadowLine};
use factory_core::version::{self, FACTORY_VERSION};
use factory_core::{project, BaseTipFiles, Home};
use factory_engine::RepoTip;
use factory_git::Repo;

use crate::presets::report_lines;
use crate::{install, plugin, CliError};

/// A repo that owns its handle and reads files from the tip of its base branch.
struct OwnedTip(Repo);

impl BaseTipFiles for OwnedTip {
    fn read_file(&self, rel: &str) -> Result<Option<String>, String> {
        RepoTip(&self.0).read_file(rel)
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
        RepoTip(&self.0).list_dir(rel)
    }
}

/// Opens the base tip of a registered project.
fn tip_for(home: &Home) -> impl Fn(&str) -> Result<Box<dyn BaseTipFiles>, String> + '_ {
    move |name| {
        let file = project::get(home, name).map_err(|e| e.to_string())?;
        let base = crate::project::base_of_strict(&file)
            .ok_or_else(|| "no base branch in project.toml".to_string())?;
        let repo = crate::project::open_repo(&file.repo, &base)?;
        Ok(Box::new(OwnedTip(repo)) as Box<dyn BaseTipFiles>)
    }
}

/// Runs the upgrade. With `dry_run` nothing is written.
pub fn run(home: &Home, commands_dir: &Path, dry_run: bool) -> Result<String, CliError> {
    run_with_claude(home, commands_dir, dry_run, OsStr::new("claude"))
}

/// Like `run`, asking `claude_program` which plugins are installed.
pub fn run_with_claude(
    home: &Home,
    commands_dir: &Path,
    dry_run: bool,
    claude_program: &OsStr,
) -> Result<String, CliError> {
    version::check(home, FACTORY_VERSION)?;
    let old_command = install::command_file(commands_dir);
    // Plugin state is read only for the standard commands folder, never for a temp one.
    let standard = install::home_of_standard_commands_dir(commands_dir).is_some();
    let has_plugin = standard && plugin::plugin_installed(&installed_plugin_ids(claude_program));
    let mut lines = Vec::new();
    if has_plugin {
        lines.push(if old_command.exists() {
            plugin::duplicate_note(&old_command)
        } else {
            plugin::update_line()
        });
    } else {
        lines.push(install::upgrade_into(commands_dir, dry_run)?);
        if standard {
            lines.push(if old_command.exists() {
                plugin::switch_note(&old_command)
            } else {
                plugin::install_hint()
            });
        }
    }
    let codex_error = match install::upgrade_codex_skill(commands_dir, dry_run) {
        Ok(Some(line)) => {
            lines.push(line);
            None
        }
        Ok(None) => None,
        Err(error) => Some(error),
    };

    let report = if dry_run {
        preset::plan_refresh(&home.presets_dir())?
    } else {
        preset::refresh(&home.presets_dir(), false)?
    };
    lines.extend(report_lines(&report, dry_run));
    if !report.refused.is_empty() {
        lines.push(
            "refused files stay as they are: factory presets refresh --force overwrites them"
                .to_string(),
        );
    }

    for line in preset::shadowed(home, &report.updated, &tip_for(home)) {
        lines.push(match line {
            ShadowLine::Shadow {
                project,
                layer,
                file,
            } => format!("shadowed: {project} {} {file}", layer.label()),
            ShadowLine::Skipped { project, reason } => format!("skipped: {project} ({reason})"),
        });
    }

    if dry_run {
        let stale = match version::read_marker(home)? {
            None => true,
            Some(text) => text.trim() != FACTORY_VERSION,
        };
        lines.push(if stale {
            format!("would version marker: written {FACTORY_VERSION}")
        } else {
            format!("version marker: {FACTORY_VERSION}")
        });
    } else if version::stamp(home, FACTORY_VERSION)? {
        lines.push(format!("version marker: written {FACTORY_VERSION}"));
    } else {
        lines.push(format!("version marker: {FACTORY_VERSION}"));
    }
    let report = lines.join("\n");
    if let Some(error) = codex_error {
        let completed = if dry_run { "preview" } else { "upgrade" };
        return Err(CliError::Refused(format!(
            "{report}\nClaude {completed} completed; Codex skill not upgraded: {error}"
        )));
    }
    Ok(report)
}
