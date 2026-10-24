use crate::TelemetryError;
use std::path::{Path, PathBuf};

/// Which lines `read_log` keeps. `tail` 0 means all.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub job: Option<String>,
    pub min_level: Option<String>,
    /// An event name or a dotted prefix, such as `task` or `agent.end`.
    pub event: Option<String>,
    pub tail: usize,
}

fn rank(level: &str) -> Option<u8> {
    match level.to_ascii_lowercase().as_str() {
        "error" => Some(4),
        "warn" => Some(3),
        "info" => Some(2),
        "debug" => Some(1),
        "trace" => Some(0),
        _ => None,
    }
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Read `.3`, `.2`, `.1`, then the file, oldest first.
pub fn read_log(path: &Path, filter: &LogFilter) -> Result<Vec<String>, TelemetryError> {
    let min = filter.min_level.as_deref().and_then(rank);
    let mut out = Vec::new();
    for suffix in [".3", ".2", ".1", ""] {
        let Ok(text) = std::fs::read_to_string(suffixed(path, suffix)) else {
            continue;
        };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if !v.is_object() {
                continue;
            }
            if let Some(job) = &filter.job {
                if v["job_id"].as_str() != Some(job.as_str()) {
                    continue;
                }
            }
            if let Some(want) = &filter.event {
                match v["event"].as_str() {
                    Some(e)
                        if e == want
                            || e.strip_prefix(want.as_str())
                                .is_some_and(|rest| rest.starts_with('.')) => {}
                    _ => continue,
                }
            }
            if let Some(min) = min {
                match v["level"].as_str().and_then(rank) {
                    Some(r) if r >= min => {}
                    _ => continue,
                }
            }
            out.push(line.to_string());
        }
    }
    if filter.tail > 0 && out.len() > filter.tail {
        out.drain(..out.len() - filter.tail);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: &str, job: &str, n: u32) -> String {
        format!(r#"{{"level":"{level}","job_id":"{job}","n":{n}}}"#)
    }

    fn write(dir: &Path, name: &str, lines: &[String]) {
        std::fs::write(dir.join(name), lines.join("\n") + "\n").expect("write");
    }

    #[test]
    fn read_log_filters_by_job_and_level() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "log.jsonl",
            &[
                line("info", "001", 1),
                line("error", "001", 2),
                line("error", "002", 3),
            ],
        );
        let f = LogFilter {
            job: Some("001".into()),
            min_level: Some("warn".into()),
            event: None,
            tail: 0,
        };
        let got = read_log(&dir.path().join("log.jsonl"), &f).expect("read");
        assert_eq!(got, vec![line("error", "001", 2)]);
    }

    #[test]
    fn read_log_tail_and_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("log.jsonl");
        assert_eq!(
            read_log(&p, &LogFilter::default()).expect("read"),
            Vec::<String>::new()
        );
        write(
            dir.path(),
            "log.jsonl",
            &[
                line("info", "a", 1),
                line("info", "a", 2),
                line("info", "a", 3),
            ],
        );
        let f = LogFilter {
            tail: 2,
            ..Default::default()
        };
        assert_eq!(
            read_log(&p, &f).expect("read"),
            vec![line("info", "a", 2), line("info", "a", 3)]
        );
    }

    fn event_line(event: &str, job: &str, level: &str) -> String {
        format!(r#"{{"event":"{event}","job_id":"{job}","level":"{level}"}}"#)
    }

    fn event_filter(event: &str) -> LogFilter {
        LogFilter {
            event: Some(event.into()),
            ..Default::default()
        }
    }

    #[test]
    fn event_filter_matches_exact_and_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("log.jsonl");
        let lines = [
            event_line("task.start", "a", "info"),
            event_line("task.end", "a", "info"),
            event_line("agent.end", "a", "info"),
            event_line("agent.endless", "a", "info"),
            event_line("agent.start", "a", "info"),
        ];
        write(dir.path(), "log.jsonl", &lines);
        assert_eq!(
            read_log(&p, &event_filter("task")).expect("read"),
            lines[..2].to_vec()
        );
        assert_eq!(
            read_log(&p, &event_filter("agent.end")).expect("read"),
            vec![lines[2].clone()]
        );
    }

    #[test]
    fn event_filter_ignores_a_partial_word() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("log.jsonl");
        write(
            dir.path(),
            "log.jsonl",
            &[event_line("task.start", "a", "info")],
        );
        assert!(read_log(&p, &event_filter("tas")).expect("read").is_empty());
    }

    #[test]
    fn event_filter_combines_with_job_and_level() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("log.jsonl");
        let keep = event_line("task.end", "a", "warn");
        write(
            dir.path(),
            "log.jsonl",
            &[
                event_line("task.end", "b", "warn"),
                event_line("task.end", "a", "info"),
                event_line("agent.end", "a", "warn"),
                keep.clone(),
            ],
        );
        let f = LogFilter {
            job: Some("a".into()),
            min_level: Some("warn".into()),
            event: Some("task".into()),
            tail: 0,
        };
        assert_eq!(read_log(&p, &f).expect("read"), vec![keep]);
    }

    #[test]
    fn event_filter_then_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("log.jsonl");
        let last = event_line("task.end", "a", "info");
        write(
            dir.path(),
            "log.jsonl",
            &[
                event_line("task.start", "a", "info"),
                last.clone(),
                event_line("agent.end", "a", "info"),
            ],
        );
        let f = LogFilter {
            event: Some("task".into()),
            tail: 1,
            ..Default::default()
        };
        assert_eq!(read_log(&p, &f).expect("read"), vec![last]);
    }

    #[test]
    fn read_log_skips_corrupt_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "log.jsonl",
            &[line("info", "a", 1), "{broken".into(), line("info", "a", 2)],
        );
        let got = read_log(&dir.path().join("log.jsonl"), &LogFilter::default()).expect("read");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn read_log_reads_rotated_files_oldest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "log.jsonl.3", &[line("info", "a", 1)]);
        write(dir.path(), "log.jsonl.1", &[line("info", "a", 2)]);
        write(dir.path(), "log.jsonl", &[line("info", "a", 3)]);
        let got = read_log(&dir.path().join("log.jsonl"), &LogFilter::default()).expect("read");
        assert_eq!(
            got,
            vec![
                line("info", "a", 1),
                line("info", "a", 2),
                line("info", "a", 3)
            ]
        );
    }
}
