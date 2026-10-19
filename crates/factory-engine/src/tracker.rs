//! The tracker port: how the driver reaches GitHub issues. The real one runs `gh`, tests use a fake.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long one `gh` call may run.
const GH_TIMEOUT: Duration = Duration::from_secs(60);

/// The issue operations the tickets step needs. Every failure is a short message.
pub trait Tracker {
    /// The `owner/repo` of the repository the main checkout points at.
    fn repo_name(&self) -> Result<String, String>;
    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String>;
    fn is_closed(&self, issue: u32) -> Result<bool, String>;
    /// Creates an issue and returns its number.
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String>;
    fn comment(&self, issue: u32, body: &str) -> Result<(), String>;
    fn close(&self, issue: u32) -> Result<(), String>;
}

impl<T: Tracker + ?Sized> Tracker for Arc<T> {
    fn repo_name(&self) -> Result<String, String> {
        (**self).repo_name()
    }
    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String> {
        (**self).labels_of(issue)
    }
    fn is_closed(&self, issue: u32) -> Result<bool, String> {
        (**self).is_closed(issue)
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String> {
        (**self).create(title, body, labels)
    }
    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        (**self).comment(issue, body)
    }
    fn close(&self, issue: u32) -> Result<(), String> {
        (**self).close(issue)
    }
}

pub(crate) fn view_args(issue: u32) -> Vec<String> {
    [
        "issue",
        "view",
        &issue.to_string(),
        "--json",
        "labels,state",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub(crate) fn create_args(title: &str, body: &str, labels: &[String]) -> Vec<String> {
    let mut args: Vec<String> = ["issue", "create", "--title", title, "--body", body]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for label in labels {
        args.push("--label".to_string());
        args.push(label.clone());
    }
    args
}

pub(crate) fn comment_args(issue: u32, body: &str) -> Vec<String> {
    ["issue", "comment", &issue.to_string(), "--body", body]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub(crate) fn close_args(issue: u32) -> Vec<String> {
    [
        "issue",
        "close",
        &issue.to_string(),
        "--reason",
        "completed",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The issue number at the end of the URL that `gh issue create` prints.
pub(crate) fn parse_created(output: &str) -> Option<u32> {
    output.trim().rsplit('/').next()?.parse().ok()
}

/// The label names and whether the issue is closed, from `gh issue view --json labels,state`.
pub(crate) fn parse_view(json: &str) -> Result<(Vec<String>, bool), String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("gh printed invalid JSON: {e}"))?;
    let labels = value["labels"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|l| l["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let closed = value["state"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("closed"));
    Ok((labels, closed))
}

/// Runs `gh` in the main checkout. It never uses a shell.
pub struct GhTracker {
    program: String,
    dir: PathBuf,
}

impl GhTracker {
    pub fn new(dir: &Path) -> GhTracker {
        GhTracker::with_program("gh", dir)
    }

    pub fn with_program(program: &str, dir: &Path) -> GhTracker {
        GhTracker {
            program: program.to_string(),
            dir: dir.to_path_buf(),
        }
    }

    /// Runs one call and returns its trimmed standard output. A slow call is killed after the timeout.
    fn run(&self, args: &[String]) -> Result<String, String> {
        let name = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
        let mut child = Command::new(&self.program)
            .args(args)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{} could not start: {e}", self.program))?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() > GH_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} {name} timed out", self.program));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(format!("{} {name} failed: {e}", self.program)),
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("{} {name} failed: {e}", self.program))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.lines().next().unwrap_or("no message");
            return Err(format!("{} {name} failed: {first}", self.program));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

impl Tracker for GhTracker {
    fn repo_name(&self) -> Result<String, String> {
        let args: Vec<String> = [
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "-q",
            ".nameWithOwner",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let name = self.run(&args)?;
        if name.is_empty() {
            return Err("gh repo view printed no repository name".to_string());
        }
        Ok(name)
    }

    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String> {
        Ok(parse_view(&self.run(&view_args(issue))?)?.0)
    }

    fn is_closed(&self, issue: u32) -> Result<bool, String> {
        Ok(parse_view(&self.run(&view_args(issue))?)?.1)
    }

    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String> {
        let out = self.run(&create_args(title, body, labels))?;
        parse_created(&out).ok_or_else(|| format!("gh issue create printed no issue URL: {out}"))
    }

    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        self.run(&comment_args(issue, body)).map(|_| ())
    }

    fn close(&self, issue: u32) -> Result<(), String> {
        self.run(&close_args(issue)).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_keep_title_and_body_as_single_arguments() {
        let title = "Fix \"quotes\" and $(rm -rf x) here";
        let body = "line one\nline two; echo hi";
        let args = create_args(title, body, &["dedup".to_string(), "size-S".to_string()]);
        assert_eq!(
            args,
            vec![
                "issue", "create", "--title", title, "--body", body, "--label", "dedup", "--label",
                "size-S"
            ]
        );
    }

    #[test]
    fn created_number_is_read_from_the_url() {
        assert_eq!(
            parse_created("https://github.com/o/r/issues/109\n"),
            Some(109)
        );
        assert_eq!(parse_created("nothing useful"), None);
        assert_eq!(parse_created(""), None);
    }

    #[test]
    fn view_json_gives_labels_and_state() {
        let json = r#"{"labels":[{"name":"dedup"},{"name":"tracking"}],"state":"OPEN"}"#;
        assert_eq!(
            parse_view(json),
            Ok((vec!["dedup".to_string(), "tracking".to_string()], false))
        );
        let closed = r#"{"labels":[],"state":"CLOSED"}"#;
        assert_eq!(parse_view(closed), Ok((vec![], true)));
        assert!(parse_view("not json").is_err());
    }

    #[test]
    fn comment_and_close_args() {
        assert_eq!(
            comment_args(76, "hi"),
            vec!["issue", "comment", "76", "--body", "hi"]
        );
        assert_eq!(
            close_args(76),
            vec!["issue", "close", "76", "--reason", "completed"]
        );
        assert_eq!(
            view_args(76),
            vec!["issue", "view", "76", "--json", "labels,state"]
        );
    }

    #[test]
    fn a_missing_gh_binary_is_an_error_not_a_panic() {
        let tracker = GhTracker::with_program(
            "definitely-not-a-real-program-xyz",
            std::path::Path::new("."),
        );
        assert!(tracker.repo_name().is_err());
    }
}
