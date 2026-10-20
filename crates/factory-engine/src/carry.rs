//! The decision whether a task builder resumes the previous task's session.

use factory_core::BuilderLast;

/// The outcome of the carry decision.
#[derive(Debug, PartialEq)]
pub(crate) enum Carry {
    Yes { id: String },
    No { reason: String },
}

/// The plain values the decision needs.
pub(crate) struct CarryInput<'a> {
    pub enabled: bool,
    pub max_ctx: u64,
    pub idle_limit: u64,
    pub last: Option<&'a BuilderLast>,
    pub task_n: u32,
    pub now: u64,
    pub rework_note: bool,
    pub saved: bool,
}

/// The idle limit in seconds, which follows the cache TTL.
pub(crate) fn carry_idle_limit(cache_ttl: Option<&str>) -> u64 {
    match cache_ttl {
        Some("1h") => 3300,
        _ => 240,
    }
}

fn no(reason: &str) -> Carry {
    Carry::No {
        reason: reason.to_string(),
    }
}

/// Decides the carry. The first matching rule wins.
pub(crate) fn carry_decision(i: &CarryInput) -> Carry {
    if !i.enabled {
        return no("off");
    }
    if i.saved {
        return no("crash resume");
    }
    let last = match i.last {
        Some(l) if i.task_n > 1 => l,
        _ => return no("first task"),
    };
    if i.rework_note {
        return no("rework note");
    }
    if last.task + 1 != i.task_n {
        return no("previous session is not task N-1");
    }
    let gap = i.now.saturating_sub(last.ended_at);
    if gap > i.idle_limit {
        return no(&format!("idle {gap}s over {}s", i.idle_limit));
    }
    if last.end_ctx > i.max_ctx {
        return no(&format!("context {} over {}", last.end_ctx, i.max_ctx));
    }
    Carry::Yes {
        id: last.id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(last: Option<&'a BuilderLast>) -> CarryInput<'a> {
        CarryInput {
            enabled: true,
            max_ctx: 100_000,
            idle_limit: 240,
            last,
            task_n: 2,
            now: 1_000,
            rework_note: false,
            saved: false,
        }
    }
    fn last(task: u32, ended_at: u64, end_ctx: u64) -> BuilderLast {
        BuilderLast {
            task,
            id: "sess-1".into(),
            ended_at,
            end_ctx,
        }
    }
    fn refused(reason: &str) -> Carry {
        Carry::No {
            reason: reason.into(),
        }
    }

    #[test]
    fn a_recent_small_previous_task_is_carried() {
        let l = last(1, 900, 30_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            Carry::Yes {
                id: "sess-1".into()
            }
        );
    }

    #[test]
    fn off_never_carries() {
        let l = last(1, 900, 30_000);
        let mut i = input(Some(&l));
        i.enabled = false;
        assert_eq!(carry_decision(&i), refused("off"));
    }

    #[test]
    fn a_crash_session_comes_before_the_other_rules() {
        let l = last(1, 900, 30_000);
        let mut i = input(Some(&l));
        i.saved = true;
        assert_eq!(carry_decision(&i), refused("crash resume"));
    }

    #[test]
    fn task_one_and_a_missing_last_are_first_task() {
        let l = last(0, 900, 30_000);
        let mut i = input(Some(&l));
        i.task_n = 1;
        assert_eq!(carry_decision(&i), refused("first task"));
        assert_eq!(carry_decision(&input(None)), refused("first task"));
    }

    #[test]
    fn a_rework_note_blocks_the_carry() {
        let l = last(1, 900, 30_000);
        let mut i = input(Some(&l));
        i.rework_note = true;
        assert_eq!(carry_decision(&i), refused("rework note"));
    }

    #[test]
    fn a_non_adjacent_previous_task_is_refused() {
        let l = last(3, 900, 30_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            refused("previous session is not task N-1")
        );
        let l = last(1, 900, 30_000);
        let mut i = input(Some(&l));
        i.task_n = 3;
        assert_eq!(
            carry_decision(&i),
            refused("previous session is not task N-1")
        );
    }

    #[test]
    fn an_idle_gap_over_the_limit_is_refused() {
        let l = last(1, 600, 30_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            refused("idle 400s over 240s")
        );
    }

    #[test]
    fn a_gap_equal_to_the_limit_is_still_carried() {
        let l = last(1, 760, 30_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            Carry::Yes {
                id: "sess-1".into()
            }
        );
    }

    #[test]
    fn a_clock_before_ended_at_counts_as_no_gap() {
        let l = last(1, 2_000, 30_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            Carry::Yes {
                id: "sess-1".into()
            }
        );
    }

    #[test]
    fn an_oversized_context_is_refused() {
        let l = last(1, 900, 130_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            refused("context 130000 over 100000")
        );
    }

    #[test]
    fn a_context_equal_to_the_cap_is_carried() {
        let l = last(1, 900, 100_000);
        assert_eq!(
            carry_decision(&input(Some(&l))),
            Carry::Yes {
                id: "sess-1".into()
            }
        );
    }

    #[test]
    fn carry_idle_limit_follows_the_cache_ttl() {
        assert_eq!(carry_idle_limit(Some("1h")), 3300);
        assert_eq!(carry_idle_limit(Some("5m")), 240);
        assert_eq!(carry_idle_limit(None), 240);
    }
}
