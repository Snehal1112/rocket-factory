//! Checks on a spec that run before `factory new` creates any job.

use std::fmt;
use std::path::{Path, PathBuf};

use factory_core::{classify_paths, Config, DiffClass, AGENT_EXCLUDE_PATTERN};
use sha2::{Digest, Sha256};

use crate::runloop::user_decisions;
use crate::specs::{SPEC_REVIEWER_AGENT, SPEC_REVIEW_JOB};
use crate::Driver;

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

/// How the user gets past a refusal.
const OVERRIDE_HINT: &str =
    "Record the user's answer as a \"Decided by: user\" decision, or pass --accept-spec \"<reason>\".";

impl fmt::Display for SpecRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecRefusal::NoUserDecision(reason) => write!(f, "{reason}. {OVERRIDE_HINT}"),
            SpecRefusal::ReviewChanges { file, findings } => write!(
                f,
                "the spec review asked for changes:\n{findings}\nThe review is in {}. {OVERRIDE_HINT}",
                file.display()
            ),
            SpecRefusal::ReviewMissing { file } => write!(
                f,
                "the spec review gave no verdict; a missing verdict is not a pass. The review is in {}. {OVERRIDE_HINT}",
                file.display()
            ),
        }
    }
}

/// The hex SHA-256 of the spec text followed by each plan text, in order.
fn review_key(spec: &str, plans: &[String]) -> String {
    let mut hash = Sha256::new();
    hash.update(spec.as_bytes());
    for plan in plans {
        hash.update(plan.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The last non-empty line of a review.
fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// Puts the spec reviewer's agent file in the main checkout when the repo does not track its own.
/// A failure is ignored: the run then gives no verdict, and a missing verdict refuses.
fn write_reviewer_file(driver: &Driver) {
    let name = format!("{SPEC_REVIEWER_AGENT}.md");
    let Some(file) = driver.agent_files().iter().find(|f| f.name == name) else {
        return;
    };
    let rel = factory_core::agent_files_rel_dir().join(&name);
    let tracked = driver
        .repo()
        .is_tracked_at_branch(driver.repo().base_branch(), &rel.to_string_lossy())
        .unwrap_or(true);
    if tracked
        || driver
            .repo()
            .exclude_paths(&[AGENT_EXCLUDE_PATTERN])
            .is_err()
    {
        return;
    }
    let path = driver.root().join(&rel);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, &file.content);
}

/// Runs the spec reviewer on the spec and the plans, unless an APPROVE review of the same text is on file.
/// The review is saved under the spec reviews folder, and its path is in every refusal.
pub fn review_spec(driver: &Driver, spec: &Path, plans: &[PathBuf]) -> Result<(), SpecRefusal> {
    if !driver.config().spec_review {
        return Ok(());
    }
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let plan_texts: Vec<String> = plans.iter().map(|p| read(p)).collect();
    let key = review_key(&read(spec), &plan_texts);
    let file = driver.layout().spec_reviews_dir().join(format!("{key}.md"));
    if last_line(&read(&file)) == "VERDICT: APPROVE" {
        return Ok(());
    }
    write_reviewer_file(driver);
    let agent = driver.spec_reviewer_spec(driver.root(), spec, plans);
    let run = driver.run_agent_for(SPEC_REVIEW_JOB, &agent);
    let mut text = run.result_text.trim_end().to_string();
    text.push('\n');
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // An unsaved review still refuses with its path, so the write result is not checked.
    let _ = std::fs::write(&file, &text);
    match last_line(&text) {
        "VERDICT: APPROVE" => Ok(()),
        "VERDICT: CHANGES" => Err(SpecRefusal::ReviewChanges {
            file,
            findings: text.trim_end().to_string(),
        }),
        _ => Err(SpecRefusal::ReviewMissing { file }),
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
    fn refusals_end_with_the_override_hint() {
        let hint = "Record the user's answer as a \"Decided by: user\" decision, or pass --accept-spec \"<reason>\".";
        let file = PathBuf::from("/s/spec-reviews/k.md");
        for refusal in [
            SpecRefusal::NoUserDecision("the spec touches risky paths (a)".into()),
            SpecRefusal::ReviewChanges {
                file: file.clone(),
                findings: "1. [blocker] x".into(),
            },
            SpecRefusal::ReviewMissing { file },
        ] {
            assert!(refusal.to_string().ends_with(hint), "{refusal}");
        }
    }

    #[test]
    fn the_review_key_changes_with_a_plan() {
        let a = review_key("spec", &["p1".into()]);
        assert_eq!(a.len(), 64);
        assert_eq!(a, review_key("spec", &["p1".into()]));
        assert_ne!(a, review_key("spec", &["p2".into()]));
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
