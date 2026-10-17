//! `GET /usage`: weighted tokens per UTC day, from the `usage.jsonl` of every job.

use std::collections::BTreeMap;

use factory_core::project;
use factory_core::{weighted_tokens, Home};
use serde_json::{json, Value};

use crate::api::http::{error_response, ApiResponse};
use crate::api::ApiDeps;

const DEFAULT_DAYS: u32 = 14;
const MAX_DAYS: u32 = 90;
const SECS_PER_DAY: u64 = 86_400;

/// The civil date `YYYY-MM-DD` of a day count since 1970-01-01 (proleptic Gregorian).
fn civil(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Weighted tokens per UTC day. Lines that are not JSON or have no numeric `ts` are skipped.
pub fn per_day(text: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(ts) = v["ts"].as_u64() else { continue };
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let w = weighted_tokens(n("input"), n("cache_creation"), n("output"), n("cache_read"));
        let slot = out.entry(civil(ts / SECS_PER_DAY)).or_insert(0u64);
        *slot = slot.saturating_add(w);
    }
    out
}

fn parse_days(query: &str) -> Result<u32, String> {
    let mut days = DEFAULT_DAYS;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        if let Some(("days", value)) = pair.split_once('=') {
            days = value.parse::<u32>().ok().filter(|d| (1..=MAX_DAYS).contains(d)).ok_or_else(|| {
                format!("days must be a number from 1 to {MAX_DAYS}")
            })?;
        }
    }
    Ok(days)
}

/// The sum of every job folder's usage for one project, per day.
fn project_usage(home: &Home, name: &str, file: &project::ProjectFile) -> BTreeMap<String, u64> {
    let mut total = BTreeMap::new();
    let Ok(layout) = project::layout(home, name, file) else { return total };
    let Ok(dir) = std::fs::read_dir(&layout.jobs_dir) else { return total };
    for entry in dir.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("usage.jsonl")) else { continue };
        for (day, tokens) in per_day(&text) {
            let slot = total.entry(day).or_insert(0u64);
            *slot = slot.saturating_add(tokens);
        }
    }
    total
}

pub fn report(home: &Home, deps: &ApiDeps, query: &str) -> ApiResponse {
    let days = match parse_days(query) {
        Ok(d) => d,
        Err(m) => return error_response(400, "bad_request", &m),
    };
    let entries = match project::list(home) {
        Ok(e) => e,
        Err(e) => return error_response(500, "failed", &e.to_string()),
    };
    let today = (deps.now_ms)() / 1000 / SECS_PER_DAY;
    let window: Vec<String> = (0..u64::from(days)).rev().map(|back| civil(today.saturating_sub(back))).collect();
    let mut projects = Vec::new();
    for e in entries {
        let Ok(file) = &e.file else { continue };
        let sums = project_usage(home, &e.name, file);
        let per: Vec<Value> =
            window.iter().map(|d| json!({"day": d, "tokens": sums.get(d).copied().unwrap_or(0)})).collect();
        let total = window.iter().fold(0u64, |acc, d| acc.saturating_add(sums.get(d).copied().unwrap_or(0)));
        projects.push(json!({"name": e.name, "total": total, "perDay": per}));
    }
    ApiResponse::ok(json!({"days": days, "projects": projects}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(19_675), "2023-11-14");
        assert_eq!(civil(11_016), "2000-02-29");
    }

    #[test]
    fn per_day_sums_weighted_tokens_and_skips_bad_lines() {
        let text = "{\"input\":1,\"cache_read\":50,\"ts\":0}\nnot json\n{\"input\":2,\"ts\":86399}\n{\"input\":9}\n{\"input\":4,\"ts\":86400}\n";
        let m = per_day(text);
        assert_eq!(m.get("1970-01-01"), Some(&8));
        assert_eq!(m.get("1970-01-02"), Some(&4));
        assert_eq!(m.len(), 2);
    }
}
