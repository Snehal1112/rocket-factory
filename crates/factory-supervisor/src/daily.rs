//! The daily budget: weighted tokens used today (UTC) by every registered project.

use factory_core::{project, Home};

use crate::api::usage::{civil, project_usage, SECS_PER_DAY};

const MS_PER_DAY: u64 = SECS_PER_DAY * 1000;

/// Today's use against the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyBudget {
    pub limit: u64,
    pub used: u64,
    /// The next UTC midnight, in ms since the epoch.
    pub resets_at_ms: u64,
}

impl DailyBudget {
    /// True when today's use has reached the limit.
    pub fn exhausted(&self) -> bool {
        self.used >= self.limit
    }
}

/// Today's use of all projects. `None` when no limit is set. An unreadable project or a bad line counts as zero.
pub fn status(home: &Home, limit: Option<u64>, now_ms: u64) -> Option<DailyBudget> {
    let limit = limit?;
    let day = now_ms / MS_PER_DAY;
    let today = civil(day);
    let mut used = 0u64;
    for entry in project::list(home).unwrap_or_default() {
        let Ok(file) = &entry.file else { continue };
        let sums = project_usage(home, &entry.name, file);
        used = used.saturating_add(sums.get(&today).copied().unwrap_or(0));
    }
    Some(DailyBudget {
        limit,
        used,
        resets_at_ms: day.saturating_add(1).saturating_mul(MS_PER_DAY),
    })
}

/// An RFC 3339 UTC time like `2026-10-18T00:00:00Z`.
pub fn rfc3339_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let rem = secs % SECS_PER_DAY;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        civil(secs / SECS_PER_DAY),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The line `factoryd status` prints for a budget.
pub fn status_line(b: &DailyBudget) -> String {
    let pct = if b.limit == 0 {
        100
    } else {
        u128::from(b.used) * 100 / u128::from(b.limit)
    };
    let mut line = format!(
        "daily budget: {} of {} weighted tokens today ({pct}%), resets at {}",
        b.used,
        b.limit,
        rfc3339_utc(b.resets_at_ms)
    );
    if b.exhausted() {
        line.push_str(", reached: new runs are waiting");
    } else if u128::from(b.used) * 100 >= u128::from(b.limit) * 80 {
        line.push_str(", warning");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2023-11-14 is day 19_675.
    const DAY: u64 = 19_675;
    const NOW_MS: u64 = DAY * MS_PER_DAY + 12 * 3_600_000;

    fn home_with(names: &[&str]) -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().join("home"));
        for n in names {
            let repo = t.path().join("repos").join(n);
            std::fs::create_dir_all(&repo).expect("repo");
            project::add(&home, n, &repo, "rust", None, &|_| Ok(()), None).expect("add");
        }
        (t, home)
    }

    fn write_usage(home: &Home, name: &str, job: &str, text: &str) {
        let file = project::get(home, name).expect("project");
        let dir = project::layout(home, name, &file)
            .expect("layout")
            .jobs_dir
            .join(job);
        std::fs::create_dir_all(&dir).expect("job dir");
        std::fs::write(dir.join("usage.jsonl"), text).expect("usage");
    }

    fn line(input: u64, cache_read: u64, day: u64) -> String {
        format!(
            "{{\"input\":{input},\"cache_read\":{cache_read},\"ts\":{}}}\n",
            day * SECS_PER_DAY + 100
        )
    }

    #[test]
    fn no_limit_gives_none() {
        let (_t, home) = home_with(&["a"]);
        assert_eq!(status(&home, None, NOW_MS), None);
    }

    #[test]
    fn sums_today_of_every_project_and_job_and_ignores_yesterday() {
        let (_t, home) = home_with(&["a", "b"]);
        write_usage(
            &home,
            "a",
            "001",
            &(line(10, 0, DAY) + &line(999, 0, DAY - 1)),
        );
        write_usage(&home, "a", "002", &line(5, 0, DAY));
        write_usage(&home, "b", "001", &line(7, 0, DAY));
        let b = status(&home, Some(100), NOW_MS).expect("some");
        assert_eq!((b.limit, b.used), (100, 22));
    }

    #[test]
    fn cache_reads_weigh_one_tenth() {
        let (_t, home) = home_with(&["a"]);
        write_usage(&home, "a", "001", &line(1, 50, DAY));
        assert_eq!(status(&home, Some(100), NOW_MS).expect("some").used, 6);
    }

    #[test]
    fn bad_lines_and_missing_folders_count_as_zero() {
        let (t, home) = home_with(&["a", "b"]);
        write_usage(&home, "a", "001", &format!("not json\n{}", line(3, 0, DAY)));
        let repo = t.path().join("repos").join("b");
        std::fs::remove_dir_all(repo).expect("remove repo");
        assert_eq!(status(&home, Some(100), NOW_MS).expect("some").used, 3);
    }

    #[test]
    fn usage_at_the_limit_reports_used_equal_to_limit() {
        let (_t, home) = home_with(&["a"]);
        write_usage(&home, "a", "001", &line(100, 0, DAY));
        let b = status(&home, Some(100), NOW_MS).expect("some");
        assert_eq!(b.used, b.limit);
        assert!(b.exhausted());
    }

    #[test]
    fn resets_at_the_next_utc_midnight() {
        let (_t, home) = home_with(&[]);
        let at = |ms: u64| status(&home, Some(1), ms).expect("some").resets_at_ms;
        assert_eq!(at(NOW_MS), (DAY + 1) * MS_PER_DAY);
        assert_eq!(at(DAY * MS_PER_DAY), (DAY + 1) * MS_PER_DAY);
        assert_eq!(at((DAY + 1) * MS_PER_DAY - 1), (DAY + 1) * MS_PER_DAY);
        // 2023-12-31 to 2024-01-01 crosses a month and a year.
        let dec31 = 19_722u64;
        assert_eq!(
            rfc3339_utc(at(dec31 * MS_PER_DAY + 5)),
            "2024-01-01T00:00:00Z"
        );
        // 2000-02-29 is a leap day.
        assert_eq!(
            rfc3339_utc(at(11_016 * MS_PER_DAY + 5)),
            "2000-03-01T00:00:00Z"
        );
        // 2023-02-28 is followed by March 1.
        assert_eq!(rfc3339_utc(at(19_416 * MS_PER_DAY)), "2023-03-01T00:00:00Z");
    }

    #[test]
    fn rfc3339_has_the_expected_shape() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(NOW_MS), "2023-11-14T12:00:00Z");
    }

    #[test]
    fn the_status_line_marks_warning_and_reached() {
        let b = |used| DailyBudget {
            limit: 100,
            used,
            resets_at_ms: 0,
        };
        assert!(!status_line(&b(10)).contains("warning"));
        assert!(status_line(&b(10)).starts_with(
            "daily budget: 10 of 100 weighted tokens today (10%), resets at 1970-01-01T00:00:00Z"
        ));
        assert!(status_line(&b(80)).ends_with(", warning"));
        assert!(status_line(&b(100)).ends_with("reached: new runs are waiting"));
        assert!(status_line(&b(150)).contains("(150%)"));
    }
}
