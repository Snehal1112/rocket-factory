//! Checks on a spec that run before `factory new` creates any job.

use std::fmt;
use std::path::PathBuf;

use factory_core::{classify_paths, Config, DiffClass};

use crate::runloop::user_decisions;

/// How many risky paths the refusal names.
const MAX_NAMED: usize = 5;

/// Why `factory new` refused a spec. Nothing is created when it does.
#[derive(Debug)]
pub enum SpecRefusal {
    /// The spec touches risky paths and the user answered no decision.
    NoUserDecision(String),
    /// The spec reviewer asked for changes.
    ReviewChanges { file: PathBuf, findings: String },
    /// The spec reviewer gave no verdict, which is never a pass.
    ReviewMissing { file: PathBuf },
}

impl fmt::Display for SpecRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecRefusal::NoUserDecision(reason) => write!(f, "{reason}"),
            SpecRefusal::ReviewChanges { file, findings } => write!(
                f,
                "the spec review asked for changes in {}:\n{findings}",
                file.display()
            ),
            SpecRefusal::ReviewMissing { file } => write!(
                f,
                "the spec review gave no verdict for {}; a missing verdict is not a pass",
                file.display()
            ),
        }
    }
}

/// The backticked paths on the `- Create:`, `- Modify:` and `- Test:` lines under each `**Files:**` block.
/// A `:line` or `:from-to` suffix is removed.
pub fn plan_file_paths(plan: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_block = false;
    for line in plan.lines() {
        let t = line.trim();
        if t.starts_with("**Files:**") {
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        let Some(item) = t.strip_prefix("- ").map(str::trim_start) else {
            // A blank line stays in the block. Any other text ends it.
            if !t.is_empty() {
                in_block = false;
            }
            continue;
        };
        let rest = ["Create:", "Modify:", "Test:"]
            .iter()
            .find_map(|k| item.strip_prefix(k));
        let Some(rest) = rest else {
            in_block = false;
            continue;
        };
        for (i, part) in rest.split('`').enumerate() {
            if i % 2 == 1 && !part.is_empty() {
                paths.push(strip_line_suffix(part).to_string());
            }
        }
    }
    paths
}

/// Removes a trailing `:12` or `:12-40` from a path.
fn strip_line_suffix(path: &str) -> &str {
    match path.rsplit_once(':') {
        Some((head, tail))
            if !head.is_empty()
                && !tail.is_empty()
                && tail.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            head
        }
        _ => path,
    }
}

/// A refusal reason when a plan path is risky and the user answered no decision in the spec.
pub fn risky_without_user_decision(spec: &str, plans: &[String], cfg: &Config) -> Option<String> {
    if user_decisions(spec) > 0 {
        return None;
    }
    let mut risky: Vec<String> = Vec::new();
    for plan in plans {
        for path in plan_file_paths(plan) {
            if classify_paths(&[path.as_str()], cfg) == DiffClass::Risky && !risky.contains(&path) {
                risky.push(path);
            }
        }
    }
    if risky.is_empty() {
        return None;
    }
    let named: Vec<&str> = risky.iter().take(MAX_NAMED).map(String::as_str).collect();
    Some(format!(
        "the spec touches risky paths ({}) and no decision in it was answered by the user",
        named.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            risk_paths: vec!["src-tauri/**".to_string()],
            ..Config::default()
        }
    }

    fn plan(path: &str) -> String {
        format!("### Task 1\n\n**Files:**\n- Modify: `{path}:12-40`\n- Test: `tests/a.rs`\n\n- [ ] **Step 1**\n")
    }

    const SPEC_NONE: &str = "## Decisions\n\n- Use X. Decided by: assistant\n";
    const SPEC_USER: &str = "## Decisions\n\n- Use X. Decided by: user\n";

    #[test]
    fn plan_file_paths_reads_the_files_blocks() {
        let paths = plan_file_paths(&plan("src-tauri/src/lib.rs"));
        assert_eq!(paths, vec!["src-tauri/src/lib.rs", "tests/a.rs"]);
    }

    #[test]
    fn risky_spec_without_user_decision_is_refused() {
        let got = risky_without_user_decision(SPEC_NONE, &[plan("src-tauri/src/lib.rs")], &cfg());
        let msg = got.expect("refused");
        assert!(msg.contains("src-tauri/src/lib.rs"));
        assert!(msg.contains("no decision in it was answered by the user"));
    }

    #[test]
    fn risky_spec_with_a_user_decision_passes() {
        let got = risky_without_user_decision(SPEC_USER, &[plan("src-tauri/src/lib.rs")], &cfg());
        assert_eq!(got, None);
    }

    #[test]
    fn no_files_no_risky_check() {
        let got =
            risky_without_user_decision(SPEC_NONE, &["### Task 1\n\n- [ ] step\n".into()], &cfg());
        assert_eq!(got, None);
    }
}
