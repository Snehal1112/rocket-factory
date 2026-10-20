//! `factory upgrade`: updates the command and the unedited presets, then reports shadowing overrides.

use std::path::Path;

use factory_core::preset::{self, ShadowLine};
use factory_core::version::{self, FACTORY_VERSION};
use factory_core::{project, BaseTipFiles, Home};
use factory_engine::RepoTip;
use factory_git::Repo;

use crate::presets::report_lines;
use crate::{install, CliError};

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
    version::check(home, FACTORY_VERSION)?;
    let mut lines = vec![install::upgrade_into(commands_dir, dry_run)?];

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
    Ok(lines.join("\n"))
}
