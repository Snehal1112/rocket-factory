//! The `factory presets` commands. They work on the factory home and need no repository.

use factory_core::{preset, Home};

use crate::cli::PresetsCmd;
use crate::CliError;

pub fn run(home: &Home, cmd: PresetsCmd) -> Result<String, CliError> {
    match cmd {
        PresetsCmd::Refresh { force } => refresh(home, force),
    }
}

fn refresh(home: &Home, force: bool) -> Result<String, CliError> {
    let report = preset::refresh(&home.presets_dir(), force)?;
    let mut lines = Vec::new();
    for (label, paths) in [
        ("written", &report.written),
        ("updated", &report.updated),
        ("unchanged", &report.skipped),
        ("refused", &report.refused),
    ] {
        for path in paths {
            lines.push(format!("{label}: {}", path.display()));
        }
    }
    lines.push(format!(
        "{} written, {} updated, {} unchanged, {} refused",
        report.written.len(),
        report.updated.len(),
        report.skipped.len(),
        report.refused.len()
    ));
    if !report.refused.is_empty() {
        lines.push("refused files differ from every known text, use --force to overwrite them".to_string());
    }
    Ok(lines.join("\n"))
}
