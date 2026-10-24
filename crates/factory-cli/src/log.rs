//! `factory log`: read the structured log back, filtered.

use factory_telemetry::{read_log, LogFilter};

use crate::commands::{CliCtx, CliError};

const LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// The log lines of this project, oldest first, joined with newlines.
/// `no log yet` when neither the file nor a rotated file exists.
pub fn run_log(
    ctx: &CliCtx,
    job: Option<&str>,
    level: Option<&str>,
    event: Option<&str>,
    tail: usize,
) -> Result<String, CliError> {
    if event == Some("") {
        return Err(CliError::Refused(
            "empty event, use a name such as task or agent.end".to_string(),
        ));
    }
    if let Some(l) = level {
        if !LEVELS.contains(&l.to_ascii_lowercase().as_str()) {
            return Err(CliError::Refused(format!(
                "unknown level {l:?}, use one of error, warn, info, debug, trace"
            )));
        }
    }
    let path = ctx.layout.log_file();
    let any = ["", ".1", ".2", ".3"].iter().any(|s| {
        let mut p = path.as_os_str().to_os_string();
        p.push(s);
        std::path::PathBuf::from(p).is_file()
    });
    if !any {
        return Ok("no log yet".to_string());
    }
    let filter = LogFilter {
        job: job.map(str::to_string),
        min_level: level.map(str::to_string),
        event: event.map(str::to_string),
        tail,
    };
    let lines = read_log(&path, &filter)
        .map_err(|e| CliError::Refused(format!("cannot read the log: {e}")))?;
    Ok(lines.join("\n"))
}
