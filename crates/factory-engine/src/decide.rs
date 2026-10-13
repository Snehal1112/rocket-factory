use factory_core::{classify_paths, BudgetState, Config, DiffClass, Verdict};

/// What to do with a job after the reviewer ran.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    AutoMerge,
    FixRound,
    NeedsHuman(String),
}

pub struct DecideInput<'a> {
    pub checks_ok: bool,
    pub verdict: &'a Verdict,
    pub class: DiffClass,
    pub risky_summary: String,
    pub diff_lines: usize,
    pub diff_cap: usize,
    pub budget: BudgetState,
    pub round: u32,
    pub fix_rounds: u32,
}

/// Implements spec decisions 6 and 7. The order of the checks matters.
pub fn decide(i: &DecideInput) -> Decision {
    // Decision 6: a risky diff always stops for the human.
    if i.class == DiffClass::Risky {
        return Decision::NeedsHuman(format!("risky diff: {}", i.risky_summary));
    }
    if i.budget == BudgetState::Stop {
        return Decision::NeedsHuman("budget is stopped".to_string());
    }
    if i.diff_lines > i.diff_cap {
        return Decision::NeedsHuman(format!("diff has {} changed lines, the cap is {}", i.diff_lines, i.diff_cap));
    }
    let used = i.round.saturating_sub(1);
    let rounds_left = used < i.fix_rounds;
    if !i.checks_ok {
        return if rounds_left {
            Decision::FixRound
        } else {
            Decision::NeedsHuman(format!("checks failed after {used} fix rounds"))
        };
    }
    match i.verdict {
        Verdict::Approve => Decision::AutoMerge,
        // A missing verdict is never a pass.
        Verdict::Changes | Verdict::Missing => {
            if rounds_left {
                Decision::FixRound
            } else {
                Decision::NeedsHuman(format!("changes remain after {used} fix rounds"))
            }
        }
    }
}

/// Names the risky paths for the human, at most five, with the count of the rest.
pub fn risk_summary(paths: &[String], cfg: &Config) -> String {
    let risky: Vec<&String> = paths
        .iter()
        .filter(|p| classify_paths(std::slice::from_ref(*p), cfg) == DiffClass::Risky)
        .collect();
    let shown: Vec<&str> = risky.iter().take(5).map(|s| s.as_str()).collect();
    let more = risky.len().saturating_sub(shown.len());
    let tail = if more > 0 { format!(" (+{more} more)") } else { String::new() };
    format!("risky paths ({}): {}{}", risky.len(), shown.join(", "), tail)
}
