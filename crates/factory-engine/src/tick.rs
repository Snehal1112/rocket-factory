use std::collections::BTreeSet;
use std::path::Path;

use factory_core::{Ledger, TaskState};

use crate::EngineError;

/// Ticks `- [ ] Task N:` lines and records `Task N: complete (merge <sha>)` once per task.
pub fn tick_plan(plan_path: &Path, ledger: &Ledger, merge_sha: &str) -> Result<Vec<u32>, EngineError> {
    let text = std::fs::read_to_string(plan_path)?;
    let already: BTreeSet<u32> = ledger
        .tasks()?
        .into_iter()
        .filter(|(_, state)| *state == TaskState::Complete)
        .map(|(n, _)| n)
        .collect();
    let numbers: BTreeSet<u32> = crate::tasks::parse_checkbox_tasks(&text).into_iter().map(|t| t.n).collect();
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        match line.strip_prefix("- [ ] Task ") {
            Some(rest) => out.push_str(&format!("- [x] Task {rest}")),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    if out != text {
        factory_core::write_atomic(plan_path, out.as_bytes())?;
    }
    let mut recorded = Vec::new();
    for n in numbers {
        if !already.contains(&n) {
            ledger.task(n, TaskState::Complete, Some(&format!("merge {merge_sha}")))?;
            recorded.push(n);
        }
    }
    Ok(recorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::Ledger;

    #[test]
    fn ticks_checkboxes_and_writes_ledger_lines_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = dir.path().join("plan.md");
        std::fs::write(&plan, "# P\n- [ ] Task 1: a\n- [ ] Task 2: b\n### Task 3: c\n").expect("write plan");
        let ledger = Ledger::new(dir.path().join("progress.md"));
        let done = tick_plan(&plan, &ledger, "abc1234").expect("tick");
        assert_eq!(done, vec![1, 2, 3]);
        let text = std::fs::read_to_string(&plan).expect("read");
        assert!(text.contains("- [x] Task 1: a") && text.contains("- [x] Task 2: b"));
        let led = std::fs::read_to_string(dir.path().join("progress.md")).expect("ledger");
        assert!(led.contains("Task 2: complete (merge abc1234)"));
        assert!(tick_plan(&plan, &ledger, "abc1234").expect("again").is_empty());
    }
}
