use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, the unit of `Job::stage_since`.
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The UTC date `YYYY-MM-DD` for a count of days since 1970-01-01.
pub fn date_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    date_from_days((now_secs() / 86_400.0) as i64)
}

/// The UTC time `YYYYMMDD-HHMMSS` for a count of seconds since the epoch, used in file names.
pub fn stamp_from_secs(secs: i64) -> String {
    let date = date_from_days(secs.div_euclid(86_400)).replace('-', "");
    let rest = secs.rem_euclid(86_400);
    format!(
        "{date}-{:02}{:02}{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The current UTC time as `YYYYMMDD-HHMMSS`.
pub fn utc_stamp() -> String {
    stamp_from_secs(now_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_from_secs() {
        assert_eq!(stamp_from_secs(0), "19700101-000000");
        assert_eq!(stamp_from_secs(951_782_400 + 3_661), "20000229-010101");
    }

    #[test]
    fn dates_from_days() {
        assert_eq!(date_from_days(0), "1970-01-01");
        assert_eq!(date_from_days(11_016), "2000-02-29");
        assert_eq!(date_from_days(18_993), "2022-01-01");
    }
}
