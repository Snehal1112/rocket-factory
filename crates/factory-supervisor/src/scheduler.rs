//! The pure scheduler. It takes a snapshot and returns what to start or reject. No I/O.

use std::collections::{BTreeMap, BTreeSet};

use crate::request::Request;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectState {
    Ready,
    LeaseHeld { owner: String },
    Disabled { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerInput {
    pub max_parallel: u32,
    pub queue: Vec<Request>,
    pub running_projects: Vec<String>,
    pub project_states: BTreeMap<String, ProjectState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Start(Request),
    Reject { request: Request, reason: String },
}

/// One ordered pass over the queue (by `ts_ms`, then id).
/// A request waits, with no decision, when its project is busy, its lease is held or there is
/// no free slot. A rejection never uses a slot and never blocks a later request.
pub fn decide(input: &SchedulerInput) -> Vec<Decision> {
    let mut queue: Vec<&Request> = input.queue.iter().collect();
    queue.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms).then_with(|| a.id.cmp(&b.id)));
    let mut free = (input.max_parallel as usize).saturating_sub(input.running_projects.len());
    let mut busy: BTreeSet<&str> = input.running_projects.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for req in queue {
        match input.project_states.get(&req.project) {
            None => out.push(Decision::Reject { request: req.clone(), reason: "unknown project".to_string() }),
            Some(ProjectState::Disabled { reason }) => {
                out.push(Decision::Reject { request: req.clone(), reason: reason.clone() })
            }
            Some(ProjectState::LeaseHeld { .. }) => {}
            Some(ProjectState::Ready) => {
                if free > 0 && !busy.contains(req.project.as_str()) {
                    free -= 1;
                    busy.insert(req.project.as_str());
                    out.push(Decision::Start(req.clone()));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Action;

    fn req(p: &str, ts: u64) -> Request {
        Request::new(p, Action::Run, "1", false, ts).expect("request")
    }

    fn input(max: u32, queue: Vec<Request>, running: &[&str], states: &[(&str, ProjectState)]) -> SchedulerInput {
        SchedulerInput {
            max_parallel: max,
            queue,
            running_projects: running.iter().map(|s| s.to_string()).collect(),
            project_states: states.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        }
    }

    fn ready(names: &[&str]) -> Vec<(&'static str, ProjectState)> {
        let all = ["a", "b", "c"];
        names.iter().filter_map(|n| all.iter().find(|a| *a == n).map(|a| (*a, ProjectState::Ready))).collect()
    }

    fn started(d: &[Decision]) -> Vec<String> {
        d.iter()
            .filter_map(|x| match x {
                Decision::Start(r) => Some(r.project.clone()),
                Decision::Reject { .. } => None,
            })
            .collect()
    }

    fn rejected(d: &[Decision]) -> Vec<(String, String)> {
        d.iter()
            .filter_map(|x| match x {
                Decision::Reject { request, reason } => Some((request.project.clone(), reason.clone())),
                Decision::Start(_) => None,
            })
            .collect()
    }

    #[test]
    fn capacity_limits_the_starts_in_order() {
        let d = decide(&input(2, vec![req("a", 1), req("b", 2), req("c", 3)], &[], &ready(&["a", "b", "c"])));
        assert_eq!(started(&d), ["a", "b"]);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn one_start_per_project() {
        let d = decide(&input(3, vec![req("a", 1), req("a", 2)], &[], &ready(&["a"])));
        assert_eq!(started(&d), ["a"]);
        assert_eq!(d.len(), 1);
        match &d[0] {
            Decision::Start(r) => assert_eq!(r.ts_ms, 1),
            Decision::Reject { .. } => panic!("must start"),
        }
    }

    #[test]
    fn a_running_project_waits_and_others_start() {
        let d = decide(&input(3, vec![req("a", 1), req("b", 2)], &["a"], &ready(&["a", "b"])));
        assert_eq!(started(&d), ["b"]);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn a_held_lease_waits_and_uses_no_capacity() {
        let mut states = ready(&["b"]);
        states.push(("a", ProjectState::LeaseHeld { owner: "x".into() }));
        let d = decide(&input(1, vec![req("a", 1), req("b", 2)], &[], &states));
        assert_eq!(started(&d), ["b"]);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn a_disabled_project_is_rejected_without_using_capacity() {
        let mut states = ready(&["b"]);
        states.push(("a", ProjectState::Disabled { reason: "broken project.toml".into() }));
        let d = decide(&input(1, vec![req("a", 1), req("b", 2)], &[], &states));
        assert_eq!(rejected(&d), [("a".to_string(), "broken project.toml".to_string())]);
        assert_eq!(started(&d), ["b"]);
    }

    #[test]
    fn an_unknown_project_is_rejected() {
        let d = decide(&input(2, vec![req("c", 1)], &[], &ready(&["a"])));
        assert_eq!(rejected(&d), [("c".to_string(), "unknown project".to_string())]);
    }

    #[test]
    fn capacity_never_goes_negative() {
        let d = decide(&input(1, vec![req("c", 1)], &["a", "b"], &ready(&["a", "b", "c"])));
        assert!(d.is_empty());
        let d = decide(&input(2, vec![req("c", 1)], &["a", "b"], &ready(&["c"])));
        assert!(d.is_empty());
        let d = decide(&input(3, vec![req("c", 1)], &["a", "b"], &ready(&["c"])));
        assert_eq!(started(&d), ["c"]);
    }

    #[test]
    fn order_is_fifo_even_when_shuffled_and_rejects_do_not_block() {
        let states = ready(&["a", "b"]);
        let q = vec![req("b", 3), req("c", 2), req("a", 1)];
        let d = decide(&input(2, q, &[], &states));
        assert_eq!(started(&d), ["a", "b"]);
        assert_eq!(rejected(&d).len(), 1);
        // The reject for c sits between a and b in queue order and does not use a slot.
        assert!(matches!(&d[1], Decision::Reject { .. }));
    }
}
