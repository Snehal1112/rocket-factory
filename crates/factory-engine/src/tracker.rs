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
    /// The number of an issue this job already filed, or None. It reads only. When several issues
    /// match the title and the footer, the lowest number wins.
    fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>;
    fn comment(&self, issue: u32, body: &str) -> Result<(), String>;
    fn close(&self, issue: u32) -> Result<(), String>;
    /// The open issues that carry `label`.
    fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String>;
    /// The login of the user who last added `label` to the issue, or None when no event is found.
    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<String>, String>;
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
    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<String>, String> {
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
        "100",
        "--json",
        "number,title,body,labels,createdAt",
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

pub(crate) fn events_args(repo: &str, issue: u32) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{repo}/issues/{issue}/events"),
        "--paginate".to_string(),
    ]
}

pub(crate) fn label_create_args(name: &str) -> Vec<String> {
    ["label", "create", name, "--force"]
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

/// The issues from `gh issue list --json number,title,body,labels,createdAt`. A missing or null
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
            })
        })
        .collect();
    Ok(issues)
}

/// The login of the last `labeled` event for `label`, from `gh api .../events --paginate`. That
/// prints one JSON array per page, so the pages are read one after another.
pub(crate) fn parse_actor(json: &str, label: &str) -> Result<Option<String>, String> {
    let mut actor = None;
    for page in serde_json::Deserializer::from_str(json).into_iter::<serde_json::Value>() {
        let page = page.map_err(|e| format!("gh printed invalid JSON: {e}"))?;
        for event in page.as_array().into_iter().flatten() {
            if event["event"].as_str() == Some("labeled")
                && event["label"]["name"].as_str() == Some(label)
            {
                if let Some(login) = event["actor"]["login"].as_str() {
                    actor = Some(login.to_string());
                }
            }
        }
    }
    Ok(actor)
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

    fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<String>, String> {
        let repo = self.repo_name()?;
        parse_actor(&self.run(&events_args(&repo, issue))?, label)
    }

    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        for label in add {
            self.run(&label_create_args(label))?;
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
    fn list_labelled_args() {
        assert_eq!(
            super::list_labelled_args("factory"),
            strings(&[
                "issue",
                "list",
                "--state",
                "open",
                "--label",
                "factory",
                "--limit",
                "100",
                "--json",
                "number,title,body,labels,createdAt"
            ])
        );
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
        assert_eq!(
            super::events_args("o/r", 7),
            strings(&["api", "repos/o/r/issues/7/events", "--paginate"])
        );
    }

    #[test]
    fn label_create_args() {
        assert_eq!(
            super::label_create_args("factory: running"),
            strings(&["label", "create", "factory: running", "--force"])
        );
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
                },
                IssueInfo {
                    number: 6,
                    title: "B".into(),
                    body: String::new(),
                    labels: vec![],
                    created_at: "2026-11-02T10:00:00Z".into(),
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

    #[test]
    fn parse_actor_returns_the_last_labeled_event_for_the_label() {
        assert_eq!(
            parse_actor(EVENTS, "factory").expect("parse"),
            Some("alice".to_string())
        );
        assert_eq!(
            parse_actor(EVENTS, "bug").expect("parse"),
            Some("bob".to_string())
        );
    }

    #[test]
    fn parse_actor_reads_paginated_pages() {
        let pages = format!(
            "{EVENTS}{}",
            r#"[{"event":"labeled","label":{"name":"factory"},"actor":{"login":"erin"}}]"#
        );
        assert_eq!(
            parse_actor(&pages, "factory").expect("parse"),
            Some("erin".to_string())
        );
    }

    #[test]
    fn parse_actor_is_none_without_a_matching_event() {
        assert_eq!(parse_actor(EVENTS, "other").expect("parse"), None);
        assert_eq!(parse_actor("[]", "factory").expect("parse"), None);
        assert_eq!(parse_actor("", "factory").expect("parse"), None);
        assert!(parse_actor("nope", "factory").is_err());
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
