//! Process start times, to tell a recorded child from a stranger that reused its pid.
//!
//! The start time comes from `factory_engine::procinfo`. When it is unknown, nothing can be compared
//! and a recorded pid is trusted.

pub use factory_engine::procinfo::process_start_ms;

/// How far the process start may be from the record's `started_ms`. The record is written right after
/// the spawn, so a real child starts within a moment of it. The margin covers clock steps and slow disks.
pub const START_TOLERANCE_MS: u64 = 60_000;

/// True when a process that started at `process_ms` can be the child recorded at `record_ms`.
pub fn start_plausible(process_ms: u64, record_ms: u64) -> bool {
    process_ms.abs_diff(record_ms) <= START_TOLERANCE_MS
}

/// False only when this machine shows the pid started far from `record_ms`, so it is a stranger.
/// When nothing can be read the pid is trusted.
pub fn may_be_our_child(pid: u32, record_ms: u64) -> bool {
    process_start_ms(pid).is_none_or(|p| start_plausible(p, record_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let pid = std::process::id();
        assert!(
            process_start_ms(pid).is_some(),
            "this machine must say when a pid started"
        );
        assert!(may_be_our_child(pid, now));
        assert!(!may_be_our_child(pid, now + 3_600_000));
        assert!(!may_be_our_child(pid, now.saturating_sub(3_600_000)));
    }
}
