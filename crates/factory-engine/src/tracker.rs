//! The tracker port: how the driver reaches GitHub issues. The real one runs `gh`, tests use a fake.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long one `gh` call may run.
const GH_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a start that fails with "text file busy" is tried again.
const SPAWN_RETRIES: u32 = 10;

/// The issue operations the tickets step needs. Every failure is a short message.
pub trait Tracker {
    /// The `owner/repo` of the repository the main checkout points at.
    fn repo_name(&self) -> Result<String, String>;
    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String>;
    fn is_closed(&self, issue: u32) -> Result<bool, String>;
    /// Creates an issue and returns its number.
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String>;
    /// The number of an issue this job already filed, or None. It reads only. When several issues
    /// match the title and the footer, the lowest number wins.
    fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>;
    fn comment(&self, issue: u32, body: &str) -> Result<(), String>;
    fn close(&self, issue: u32) -> Result<(), String>;
    /// The open issues that carry `label`.
    fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String>;
    /// Who last added `label` to the issue and when, or None when no event is found or the last
    /// event has no login.
    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<LabelEvent>, String>;
    /// Adds and removes labels. A label to add that does not exist yet is created first.
    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String>;
    /// The login `gh` runs as.
    fn current_user(&self) -> Result<String, String>;
}

/// One issue as the intake queue reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueInfo {
    pub number: u32,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A `labeled` event: the login that added the label and the time it did.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelEvent {
    pub actor: String,
    pub at: String,
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
    fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String> {
        (**self).find_created(title, footer)
    }
    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        (**self).comment(issue, body)
    }
    fn close(&self, issue: u32) -> Result<(), String> {
        (**self).close(issue)
    }
    fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String> {
        (**self).list_labelled(label)
    }
    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<LabelEvent>, String> {
        (**self).labelled_by(issue, label)
    }
    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        (**self).edit_labels(issue, add, remove)
    }
    fn current_user(&self) -> Result<String, String> {
        (**self).current_user()
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

pub(crate) fn find_args() -> Vec<String> {
    [
        "issue",
        "list",
        "--state",
        "all",
        "--limit",
        "100",
        "--json",
        "number,title,body",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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

pub(crate) fn list_labelled_args(label: &str) -> Vec<String> {
    [
        "issue",
        "list",
        "--state",
        "open",
        "--label",
        label,
        "--limit",
        "500",
        "--search",
        "sort:created-asc",
        "--json",
        "number,title,body,labels,createdAt,updatedAt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub(crate) fn edit_labels_args(issue: u32, add: &[String], remove: &[String]) -> Vec<String> {
    let mut args = vec!["issue".to_string(), "edit".to_string(), issue.to_string()];
    for label in add {
        args.push("--add-label".to_string());
        args.push(label.clone());
    }
    for label in remove {
        args.push("--remove-label".to_string());
        args.push(label.clone());
    }
    args
}

/// Prints only the `labeled` events, one compact JSON object per line.
const EVENTS_JQ: &str = r#".[] | select(.event == "labeled") | {event, label: {name: .label.name}, actor: {login: .actor.login}, created_at}"#;

pub(crate) fn events_args(repo: &str, issue: u32) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{repo}/issues/{issue}/events"),
        "--paginate".to_string(),
        "--jq".to_string(),
        EVENTS_JQ.to_string(),
    ]
}

pub(crate) fn label_create_args(name: &str) -> Vec<String> {
    [
        "label",
        "create",
        name,
        "--color",
        "ededed",
        "--description",
        "factory intake state",
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

/// The lowest issue number whose title equals `title` and whose body contains `footer`, from
/// `gh issue list --json number,title,body`. A missing or null body counts as empty.
pub(crate) fn parse_found(json: &str, title: &str, footer: &str) -> Result<Option<u32>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("gh printed invalid JSON: {e}"))?;
    let found = value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|issue| issue["title"].as_str() == Some(title))
        .filter(|issue| issue["body"].as_str().unwrap_or("").contains(footer))
        .filter_map(|issue| issue["number"].as_u64().and_then(|n| u32::try_from(n).ok()))
        .min();
    Ok(found)
}

/// The issues from `gh issue list --json number,title,body,labels,createdAt,updatedAt`. A missing or null
/// body counts as empty. An entry without a number is skipped.
pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<IssueInfo>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("gh printed invalid JSON: {e}"))?;
    let issues = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            let number = u32::try_from(issue["number"].as_u64()?).ok()?;
            let labels = issue["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["name"].as_str().map(str::to_string))
                .collect();
            Some(IssueInfo {
                number,
                title: issue["title"].as_str().unwrap_or("").to_string(),
                body: issue["body"].as_str().unwrap_or("").to_string(),
                labels,
                created_at: issue["createdAt"].as_str().unwrap_or("").to_string(),
                updated_at: issue["updatedAt"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    Ok(issues)
}

/// The actor and time of the last `labeled` event for `label`. The input is either the compact
/// event lines the events call prints or full event arrays, one per page. The last matching event
/// wins, so a null actor on it gives None even when an earlier event had a login.
pub(crate) fn parse_actor(json: &str, label: &str) -> Result<Option<LabelEvent>, String> {
    let mut last = None;
    for value in serde_json::Deserializer::from_str(json).into_iter::<serde_json::Value>() {
        let value = value.map_err(|e| format!("gh printed invalid JSON: {e}"))?;
        let events = match value.as_array() {
            Some(page) => page.as_slice(),
            None => std::slice::from_ref(&value),
        };
        for event in events {
            if event["event"].as_str() == Some("labeled")
                && event["label"]["name"].as_str() == Some(label)
            {
                last = Some(event["actor"]["login"].as_str().map(|login| LabelEvent {
                    actor: login.to_string(),
                    at: event["created_at"].as_str().unwrap_or("").to_string(),
                }));
            }
        }
    }
    Ok(last.flatten())
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
        let spawn = || {
            Command::new(&self.program)
                .args(args)
                .current_dir(&self.dir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };
        // A program that was just written can be busy for a moment while a forked child still holds it.
        let mut spawned = spawn();
        for _ in 0..SPAWN_RETRIES {
            if !matches!(&spawned, Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
            spawned = spawn();
        }
        let mut child = spawned.map_err(|e| format!("{} could not start: {e}", self.program))?;
        let stdout = child.stdout.take().map(read_all);
        let stderr = child.stderr.take().map(read_all);
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() > GH_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} {name} timed out", self.program));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(format!("{} {name} failed: {e}", self.program)),
            }
        };
        let stdout = join_output(stdout);
        let stderr = join_output(stderr);
        if !status.success() {
            let err = String::from_utf8_lossy(&stderr);
            let first = err.lines().next().unwrap_or("no message");
            return Err(format!("{} {name} failed: {first}", self.program));
        }
        Ok(String::from_utf8_lossy(&stdout).trim().to_string())
    }
}

/// Reads a pipe to its end on its own thread, so a full pipe never blocks the child.
fn read_all<R: Read + Send + 'static>(mut pipe: R) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

fn join_output(handle: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle.and_then(|h| h.join().ok()).unwrap_or_default()
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

    fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String> {
        parse_found(&self.run(&find_args())?, title, footer)
    }

    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        self.run(&comment_args(issue, body)).map(|_| ())
    }

    fn close(&self, issue: u32) -> Result<(), String> {
        self.run(&close_args(issue)).map(|_| ())
    }

    fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String> {
        parse_issue_list(&self.run(&list_labelled_args(label))?)
    }

    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<LabelEvent>, String> {
        let repo = self.repo_name()?;
        parse_actor(&self.run(&events_args(&repo, issue))?, label)
    }

    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        if add.is_empty() && remove.is_empty() {
            return Ok(());
        }
        for label in add {
            match self.run(&label_create_args(label)) {
                Err(e) if !e.contains("already exists") => return Err(e),
                _ => {}
            }
        }
        self.run(&edit_labels_args(issue, add, remove)).map(|_| ())
    }

    fn current_user(&self) -> Result<String, String> {
        let args: Vec<String> = ["api", "user", "--jq", ".login"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let login = self.run(&args)?;
        if login.is_empty() {
            return Err("gh api user printed no login".to_string());
        }
        Ok(login)
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
    fn find_args_list_all_states() {
        let expected: Vec<String> = [
            "issue",
            "list",
            "--state",
            "all",
            "--limit",
            "100",
            "--json",
            "number,title,body",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(find_args(), expected);
    }

    const FOOTER: &str = "Filed by factory job 007 (slug)";

    #[test]
    fn parse_found_picks_the_lowest_number_with_title_and_footer() {
        let json = r#"[
          {"number":9,"title":"Fix x","body":"b\n\nFiled by factory job 007 (slug) from the follow-ups of #1."},
          {"number":4,"title":"Fix x","body":"Filed by factory job 007 (slug) from the follow-ups of #1."},
          {"number":2,"title":"Other","body":"Filed by factory job 007 (slug)"}
        ]"#;
        assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), Some(4));
    }

    #[test]
    fn parse_found_ignores_a_title_without_the_footer() {
        let json = r#"[{"number":3,"title":"Fix x","body":"unrelated"}]"#;
        assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), None);
    }

    #[test]
    fn parse_found_treats_a_null_body_as_empty() {
        let json = r#"[{"number":3,"title":"Fix x","body":null}]"#;
        assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), None);
    }

    #[test]
    fn parse_found_rejects_invalid_json() {
        let err = parse_found("nope", "Fix x", FOOTER).expect_err("invalid");
        assert!(err.contains("invalid JSON"), "{err}");
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn edit_labels_args() {
        assert_eq!(
            super::edit_labels_args(7, &strings(&["a", "b"]), &strings(&["c"])),
            strings(&[
                "issue",
                "edit",
                "7",
                "--add-label",
                "a",
                "--add-label",
                "b",
                "--remove-label",
                "c"
            ])
        );
    }

    #[test]
    fn events_args() {
        let args = super::events_args("o/r", 7);
        assert_eq!(
            args[..3],
            strings(&["api", "repos/o/r/issues/7/events", "--paginate"])[..]
        );
        assert_eq!(args[3], "--jq");
        assert!(args[4].contains("labeled"));
    }

    #[test]
    fn parse_issue_list_reads_labels_and_dates() {
        let json = r#"[
          {"number":5,"title":"A","body":"text","labels":[{"name":"factory"},{"name":"priority: P1"}],"createdAt":"2026-11-01T10:00:00Z"},
          {"number":6,"title":"B","labels":[],"createdAt":"2026-11-02T10:00:00Z"}
        ]"#;
        let issues = parse_issue_list(json).expect("parse");
        assert_eq!(
            issues,
            vec![
                IssueInfo {
                    number: 5,
                    title: "A".into(),
                    body: "text".into(),
                    labels: strings(&["factory", "priority: P1"]),
                    created_at: "2026-11-01T10:00:00Z".into(),
                    updated_at: String::new(),
                },
                IssueInfo {
                    number: 6,
                    title: "B".into(),
                    body: String::new(),
                    labels: vec![],
                    created_at: "2026-11-02T10:00:00Z".into(),
                    updated_at: String::new(),
                },
            ]
        );
        assert!(parse_issue_list("nope")
            .expect_err("invalid")
            .contains("invalid JSON"));
    }

    const EVENTS: &str = r#"[
      {"event":"labeled","label":{"name":"factory"},"actor":{"login":"mallory"}},
      {"event":"labeled","label":{"name":"bug"},"actor":{"login":"bob"}},
      {"event":"commented","actor":{"login":"carol"}},
      {"event":"unlabeled","label":{"name":"factory"},"actor":{"login":"dave"}},
      {"event":"labeled","label":{"name":"factory"},"actor":{"login":"alice"}}
    ]"#;

    fn login_of(json: &str, label: &str) -> Option<String> {
        parse_actor(json, label).expect("parse").map(|e| e.actor)
    }

    #[test]
    fn parse_actor_returns_the_last_labeled_event_for_the_label() {
        assert_eq!(login_of(EVENTS, "factory"), Some("alice".to_string()));
        assert_eq!(login_of(EVENTS, "bug"), Some("bob".to_string()));
    }

    #[test]
    fn parse_actor_reads_paginated_pages() {
        let pages = format!(
            "{EVENTS}{}",
            r#"[{"event":"labeled","label":{"name":"factory"},"actor":{"login":"erin"}}]"#
        );
        assert_eq!(login_of(&pages, "factory"), Some("erin".to_string()));
    }

    #[test]
    fn parse_actor_is_none_without_a_matching_event() {
        assert_eq!(parse_actor(EVENTS, "other").expect("parse"), None);
        assert_eq!(parse_actor("[]", "factory").expect("parse"), None);
        assert_eq!(parse_actor("", "factory").expect("parse"), None);
        assert!(parse_actor("nope", "factory").is_err());
    }

    const LABELED: &str = r#"{"event":"labeled","label":{"name":"factory"},"actor":{"login":"alice"},"created_at":"2026-11-01T10:00:00Z"}"#;

    #[test]
    fn parse_actor_gives_none_when_the_last_event_has_no_login() {
        let json = format!(
            "[{LABELED},{}]",
            r#"{"event":"labeled","label":{"name":"factory"},"actor":null,"created_at":"2026-11-02T10:00:00Z"}"#
        );
        assert_eq!(parse_actor(&json, "factory").expect("parse"), None);
    }

    #[test]
    fn parse_actor_keeps_the_time_of_the_last_matching_event() {
        let json = format!(
            "[{LABELED},{},{}]",
            r#"{"event":"labeled","label":{"name":"factory"},"actor":{"login":"bob"},"created_at":"2026-11-02T10:00:00Z"}"#,
            r#"{"event":"unlabeled","label":{"name":"factory"},"actor":{"login":"carol"},"created_at":"2026-11-03T10:00:00Z"}"#
        );
        assert_eq!(
            parse_actor(&json, "factory").expect("parse"),
            Some(LabelEvent {
                actor: "bob".to_string(),
                at: "2026-11-02T10:00:00Z".to_string()
            })
        );
    }

    #[test]
    fn parse_actor_reads_compact_json_lines() {
        let lines = format!("{LABELED}\n{LABELED}\n");
        assert_eq!(
            parse_actor(&lines, "factory").expect("parse"),
            Some(LabelEvent {
                actor: "alice".to_string(),
                at: "2026-11-01T10:00:00Z".to_string()
            })
        );
    }

    /// Writes an executable fake `gh` script into `dir` and returns its path.
    fn fake_gh(dir: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("gh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn edit_labels_with_nothing_to_change_makes_no_call() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = dir.path().join("argv");
        let gh = fake_gh(
            dir.path(),
            &format!("echo \"$@\" >> {}", record.to_string_lossy()),
        );
        let tracker = GhTracker::with_program(&gh, dir.path());
        tracker.edit_labels(7, &[], &[]).expect("no-op");
        assert!(!record.exists());
    }

    #[test]
    fn edit_labels_creates_a_missing_label_and_ignores_already_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = dir.path().join("argv");
        let gh = fake_gh(
            dir.path(),
            &format!(
                "echo \"$@\" >> {}\nif [ \"$1\" = label ]; then echo 'label already exists' >&2; exit 1; fi",
                record.to_string_lossy()
            ),
        );
        let tracker = GhTracker::with_program(&gh, dir.path());
        tracker
            .edit_labels(7, &strings(&["factory: running"]), &[])
            .expect("edit");
        let calls = std::fs::read_to_string(&record).expect("record");
        assert_eq!(calls.lines().count(), 2, "{calls}");
        assert!(calls.contains("issue edit 7 --add-label factory: running"));
    }

    #[test]
    fn edit_labels_reports_another_label_create_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gh = fake_gh(dir.path(), "echo 'bad credentials' >&2; exit 1");
        let tracker = GhTracker::with_program(&gh, dir.path());
        let err = tracker
            .edit_labels(7, &strings(&["x"]), &[])
            .expect_err("create fails");
        assert!(err.contains("bad credentials"), "{err}");
    }

    #[test]
    fn label_create_args_set_a_fixed_colour_and_description() {
        assert_eq!(
            super::label_create_args("factory: running"),
            strings(&[
                "label",
                "create",
                "factory: running",
                "--color",
                "ededed",
                "--description",
                "factory intake state"
            ])
        );
    }

    #[test]
    fn list_labelled_args_ask_oldest_first_with_updated_at() {
        let args = super::list_labelled_args("factory");
        let at = |flag: &str| args.iter().position(|a| a == flag).expect("flag");
        assert_eq!(args[at("--limit") + 1], "500");
        assert_eq!(args[at("--search") + 1], "sort:created-asc");
        assert!(args[at("--json") + 1].ends_with("createdAt,updatedAt"));
    }

    #[test]
    fn parse_issue_list_reads_updated_at() {
        let json = r#"[
          {"number":5,"title":"A","labels":[],"createdAt":"c","updatedAt":"2026-11-03T10:00:00Z"},
          {"number":6,"title":"B","labels":[],"createdAt":"c"}
        ]"#;
        let issues = parse_issue_list(json).expect("parse");
        assert_eq!(issues[0].updated_at, "2026-11-03T10:00:00Z");
        assert_eq!(issues[1].updated_at, "");
    }

    #[test]
    fn a_gh_call_that_prints_more_than_the_pipe_buffer_still_returns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gh = fake_gh(dir.path(), "head -c 204800 /dev/zero | tr '\\0' 'a'");
        let tracker = GhTracker::with_program(&gh, dir.path());
        let start = Instant::now();
        let out = tracker.run(&strings(&["x"])).expect("run");
        assert_eq!(out.len(), 204800);
        assert!(start.elapsed() < Duration::from_secs(20));
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
