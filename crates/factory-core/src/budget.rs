#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    Ok,
    Warn,
    Stop,
}

/// Input plus cache creation plus output plus one tenth of cache reads. Saturates instead of overflowing.
pub fn weighted_tokens(input: u64, cache_creation: u64, output: u64, cache_read: u64) -> u64 {
    input
        .saturating_add(cache_creation)
        .saturating_add(output)
        .saturating_add(cache_read / 10)
}

/// Warn at 80 percent of the budget, stop at 100 percent. A zero budget is always stop.
pub fn budget_state(used: u64, budget: u64) -> BudgetState {
    if used >= budget {
        return BudgetState::Stop;
    }
    if u128::from(used) * 100 >= u128::from(budget) * 80 {
        return BudgetState::Warn;
    }
    BudgetState::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_formula() {
        assert_eq!(weighted_tokens(100, 200, 300, 1000), 100 + 200 + 300 + 100);
        assert_eq!(weighted_tokens(0, 0, 0, 9), 0);
        assert_eq!(weighted_tokens(0, 0, 0, 10), 1);
    }

    #[test]
    fn weighted_never_overflows() {
        assert_eq!(
            weighted_tokens(u64::MAX, u64::MAX, u64::MAX, u64::MAX),
            u64::MAX
        );
    }

    #[test]
    fn thresholds() {
        assert_eq!(budget_state(0, 1000), BudgetState::Ok);
        assert_eq!(budget_state(799, 1000), BudgetState::Ok);
        assert_eq!(budget_state(800, 1000), BudgetState::Warn);
        assert_eq!(budget_state(999, 1000), BudgetState::Warn);
        assert_eq!(budget_state(1000, 1000), BudgetState::Stop);
        assert_eq!(budget_state(5000, 1000), BudgetState::Stop);
    }

    #[test]
    fn zero_budget_and_huge_values() {
        assert_eq!(budget_state(0, 0), BudgetState::Stop);
        assert_eq!(budget_state(u64::MAX - 1, u64::MAX), BudgetState::Warn);
        assert_eq!(budget_state(u64::MAX, u64::MAX), BudgetState::Stop);
    }
}
