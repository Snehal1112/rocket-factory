use std::collections::HashSet;

use factory_supervisor::supervisor::{log_report, new_lines, TickReport};
use factory_supervisor::Outcome;

fn of(lines: &[serde_json::Value], event: &str) -> Vec<serde_json::Value> {
    lines
        .iter()
        .filter(|l| l["event"] == event)
        .cloned()
        .collect()
}

#[test]
fn supervisor_logs_start_and_finish_without_printing_them() {
    let report = TickReport {
        started: vec!["r1".into()],
        finished: vec![("r1".into(), Outcome::Merged)],
        ..TickReport::default()
    };
    assert!(new_lines(&mut HashSet::new(), &report).is_empty());
    let ((), lines) =
        factory_telemetry::capture("info", || log_report(&report, &mut HashSet::new()));
    assert_eq!(of(&lines, "sup.start")[0]["request"], "r1");
    let fin = of(&lines, "sup.finish");
    assert_eq!(fin[0]["request"], "r1");
    assert_eq!(fin[0]["outcome"], "Merged");
}

#[test]
fn reject_logs_reason() {
    let report = TickReport {
        rejected: vec![("r2".into(), "bad".into())],
        ..TickReport::default()
    };
    let ((), lines) =
        factory_telemetry::capture("info", || log_report(&report, &mut HashSet::new()));
    let rej = of(&lines, "sup.reject");
    assert_eq!(rej[0]["request"], "r2");
    assert_eq!(rej[0]["reason"], "bad");
}

#[test]
fn repeated_error_is_logged_once() {
    let report = TickReport {
        errors: vec!["boom".into()],
        ..TickReport::default()
    };
    let mut printed = HashSet::new();
    let ((), lines) = factory_telemetry::capture("info", || {
        log_report(&report, &mut printed);
        log_report(&report, &mut printed);
    });
    let errs = of(&lines, "sup.error");
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0]["level"], "error");
    assert_eq!(errs[0]["msg"], "factoryd: boom");
}

#[test]
fn returning_error_is_logged_again() {
    let report = TickReport {
        errors: vec!["boom".into()],
        ..TickReport::default()
    };
    let mut printed = HashSet::new();
    let ((), lines) = factory_telemetry::capture("info", || {
        log_report(&report, &mut printed);
        log_report(&TickReport::default(), &mut printed);
        log_report(&report, &mut printed);
    });
    assert_eq!(of(&lines, "sup.error").len(), 2);
}

#[test]
fn bad_child_and_orphan_log_warn() {
    let report = TickReport {
        bad_children: vec!["c1".into()],
        orphans: vec!["proj".into()],
        ..TickReport::default()
    };
    let ((), lines) =
        factory_telemetry::capture("info", || log_report(&report, &mut HashSet::new()));
    let warns = of(&lines, "sup.warn");
    assert_eq!(warns.len(), 2);
    assert!(warns.iter().all(|w| w["level"] == "warn"));
}

#[test]
fn notice_logs_info_sup_notice() {
    let report = TickReport {
        notices: vec!["budget reached".into()],
        ..TickReport::default()
    };
    let ((), lines) =
        factory_telemetry::capture("info", || log_report(&report, &mut HashSet::new()));
    let n = of(&lines, "sup.notice");
    assert_eq!(n[0]["level"], "info");
    assert_eq!(n[0]["msg"], "budget reached");
}
