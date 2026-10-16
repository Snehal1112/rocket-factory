//! Process start times from `/proc`, to tell a recorded child from a stranger that reused its pid.
//!
//! Linux only. Elsewhere `/proc` is missing, nothing can be compared, and a recorded pid is trusted.

/// Clock ticks per second in `/proc/<pid>/stat`. It is 100 on every mainstream Linux build.
/// Reading the real value needs libc or a spawned `getconf`, which this crate avoids.
pub const CLK_TICKS_PER_SEC: u64 = 100;

/// How far the process start may be from the record's `started_ms`. The record is written right after
/// the spawn, so a real child starts within a moment of it. The margin covers clock steps and slow disks.
pub const START_TOLERANCE_MS: u64 = 60_000;

/// Field 22 (start time in clock ticks since boot) of a `/proc/<pid>/stat` line.
pub fn parse_start_ticks(stat: &str) -> Option<u64> {
    // The command name is in parentheses and may hold spaces, so count from the last closing one.
    let (_, rest) = stat.rsplit_once(')')?;
    // `rest` starts at field 3, so field 22 is the 20th item.
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// The `btime` line (boot time in seconds since the epoch) of `/proc/stat`.
pub fn parse_btime(proc_stat: &str) -> Option<u64> {
    proc_stat.lines().find_map(|l| l.strip_prefix("btime ")).and_then(|v| v.trim().parse().ok())
}

/// The start of a process in milliseconds since the epoch.
pub fn start_ms(btime_secs: u64, start_ticks: u64) -> u64 {
    btime_secs.saturating_mul(1000).saturating_add(start_ticks.saturating_mul(1000) / CLK_TICKS_PER_SEC)
}

/// True when a process that started at `process_ms` can be the child recorded at `record_ms`.
pub fn start_plausible(process_ms: u64, record_ms: u64) -> bool {
    process_ms.abs_diff(record_ms) <= START_TOLERANCE_MS
}

/// When the pid started, or None when `/proc` does not say.
pub fn process_start_ms(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let boot = std::fs::read_to_string("/proc/stat").ok()?;
    Some(start_ms(parse_btime(&boot)?, parse_start_ticks(&stat)?))
}

/// False only when `/proc` shows the pid started far from `record_ms`, so it is a stranger.
/// When nothing can be read the pid is trusted.
pub fn may_be_our_child(pid: u32, record_ms: u64) -> bool {
    process_start_ms(pid).is_none_or(|p| start_plausible(p, record_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_start_field_after_a_command_with_spaces() {
        let stat = "123 (my (odd) cmd) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 4242 20";
        assert_eq!(parse_start_ticks(stat), Some(4242));
        assert_eq!(parse_start_ticks("123 (x) S 1 2"), None);
        assert_eq!(parse_start_ticks("garbage"), None);
    }

    #[test]
    fn parses_btime() {
        assert_eq!(parse_btime("cpu 1 2\nbtime 1700000000\nprocs 3\n"), Some(1_700_000_000));
        assert_eq!(parse_btime("cpu 1 2\n"), None);
    }

    #[test]
    fn converts_ticks_to_milliseconds() {
        assert_eq!(start_ms(1000, 250), 1_002_500);
    }

    #[test]
    fn compares_within_the_tolerance() {
        let record = 10_000_000;
        assert!(start_plausible(record, record));
        assert!(start_plausible(record - START_TOLERANCE_MS, record));
        assert!(start_plausible(record + START_TOLERANCE_MS, record));
        assert!(!start_plausible(record - START_TOLERANCE_MS - 1, record));
        assert!(!start_plausible(record + START_TOLERANCE_MS + 1, record));
    }

    #[test]
    fn this_process_started_before_a_record_an_hour_later() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let pid = std::process::id();
        if process_start_ms(pid).is_none() {
            return;
        }
        assert!(may_be_our_child(pid, now));
        assert!(!may_be_our_child(pid, now + 3_600_000));
        assert!(!may_be_our_child(pid, now.saturating_sub(3_600_000)));
    }
}
