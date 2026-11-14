//! Checks on a spec that run before `factory new` creates any job.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use factory_agent::{AgentRun, AgentSpec, StopReason};
use factory_core::{classify_paths, Config, DiffClass};
use sha2::{Digest, Sha256};

use crate::compat::ReviewRecord;
use crate::runloop::{is_list_item, section_body, user_decisions};
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
    /// The resolved preset has no spec reviewer agent file, so the run would have no rules.
    ReviewerMissing,
    /// The review run did not finish, so it gave no verdict to trust.
    ReviewFailed { reason: String },
    /// An intake spec has a decision without a real source. The text is the finding for the drafter.
    UnsourcedDecision(String),
}

/// What `factory new` tells the user about the review it used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecReviewInfo {
    /// Where the review is, or would have been saved.
    pub file: PathBuf,
    /// `approved`, `asked for changes` or `gave no verdict`.
    pub verdict: &'static str,
    /// False when the review file could not be written.
    pub saved: bool,
}

/// How the user gets past a refusal.
pub const SPEC_OVERRIDE_HINT: &str =
    "Record the user's answer as a \"Decided by: user\" decision, or pass --accept-spec \"<reason>\".";

impl fmt::Display for SpecRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecRefusal::NoUserDecision(reason) => write!(f, "{reason}. {SPEC_OVERRIDE_HINT}"),
            SpecRefusal::ReviewChanges { file, findings } => write!(
                f,
                "the spec review asked for changes:\n{findings}\nThe review is in {}. {SPEC_OVERRIDE_HINT}",
                file.display()
            ),
            SpecRefusal::ReviewMissing { file } => write!(
                f,
                "the spec review gave no verdict; a missing verdict is not a pass. The review is in {}. {SPEC_OVERRIDE_HINT}",
                file.display()
            ),
            SpecRefusal::ReviewFailed { reason } => write!(
                f,
                "the spec review did not finish ({reason}). {SPEC_OVERRIDE_HINT}"
            ),
            SpecRefusal::UnsourcedDecision(message) => write!(f, "{message}"),
            SpecRefusal::ReviewerMissing => write!(
                f,
                "the spec reviewer agent is missing; run factory upgrade or factory presets refresh, or set spec_review = false"
            ),
        }
    }
}

/// The hex SHA-256 over the spec, each plan, the reviewer's agent file and the intake flag. Every text
/// part has an 8 byte length prefix, so moving text from one part to the next changes the key. The
/// flag keeps a review made for an intake spec apart from one made with a person in the loop.
fn review_key(spec: &str, plans: &[String], agent: &str, intake: bool) -> String {
    let mut hash = Sha256::new();
    hash.update([u8::from(intake)]);
    for part in std::iter::once(spec)
        .chain(plans.iter().map(String::as_str))
        .chain(std::iter::once(agent))
    {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
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

/// The spec reviewer's agent file text from the resolved agent files, empty when there is none.
fn reviewer_file_text(driver: &Driver) -> &str {
    let name = format!("{SPEC_REVIEWER_AGENT}.md");
    driver
        .agent_files()
        .iter()
        .find(|f| f.name == name)
        .map_or("", |f| f.content.as_str())
}

/// Writes the agents file for the spec reviewer in the spec reviews folder and returns its path.
/// The base branch's tracked text wins, then an untracked file without a rules marker in the main
/// checkout (the user's own). Nothing is written into the repo. A tracked file that cannot be read, or
/// a write error, is an error that names the file, so the reviewer never runs without `--agents`.
fn reviewer_agents_file(driver: &Driver) -> Result<PathBuf, String> {
    let mut files = driver.effective_agent_files().map_err(|e| e.to_string())?;
    // Every name becomes a key in the file.
    if let Some(bad) = files
        .iter()
        .find(|f| !factory_core::is_agent_file_name(&f.name))
    {
        return Err(format!(
            "agent file name {:?} is not allowed, use factory-<name>.md",
            bad.name
        ));
    }
    let name = format!("{SPEC_REVIEWER_AGENT}.md");
    let rel = factory_core::agent_files_rel_dir().join(&name);
    let tracked = driver
        .repo()
        .is_tracked_at_branch(
            driver.repo().base_branch(),
            &rel.to_string_lossy().replace('\\', "/"),
        )
        .map_err(|e| format!("cannot read tracked agent file {}: {e}", rel.display()))?;
    if !tracked {
        // A file without a marker is the user's own. A read error counts as no file.
        let own = std::fs::read_to_string(driver.root().join(&rel))
            .ok()
            .filter(|text| factory_core::rules_marker(text).is_none());
        if let (Some(text), Some(file)) = (own, files.iter_mut().find(|f| f.name == name)) {
            file.content = text;
        }
    }
    let path = factory_core::agents_file(&driver.layout().spec_reviews_dir());
    crate::agents_file::write_agents_json_at(&path, &files)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// Appends one usage line for the review run to `usage.jsonl` in the spec reviews folder.
/// No job owns the run, so this file is the only record of its tokens. A write error is ignored.
fn log_review_usage(driver: &Driver, agent: &AgentSpec, run: &AgentRun, secs: u64) {
    log_unowned_usage(
        driver,
        &driver.layout().spec_review_usage_file(),
        SPEC_REVIEW_JOB,
        agent,
        run,
        secs,
    );
}

/// Appends one usage line for a run that no job owns to `path`. A write error is ignored.
pub(crate) fn log_unowned_usage(
    driver: &Driver,
    path: &std::path::Path,
    job: &str,
    agent: &AgentSpec,
    run: &AgentRun,
    secs: u64,
) {
    use std::io::Write;
    let line = crate::ports::usage_line(
        job,
        agent,
        run,
        1,
        0,
        crate::clock::now_secs().max(0.0) as u64,
        secs,
        driver.config().agent_cli(),
    );
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{line}");
    }
}

/// Why a review run did not finish, or `None` when it did. A run that stopped at the cap is never a
/// pass, even when its partial text ends in an APPROVE line.
fn incomplete_reason(run: &AgentRun, cap: u64) -> Option<String> {
    match run.stop_reason {
        StopReason::Done => None,
        StopReason::Failed => Some(format!(
            "failed: {}",
            run.error.as_deref().unwrap_or("no error text")
        )),
        StopReason::Stalled => Some("stalled".to_string()),
        StopReason::TurnLimit => Some("stopped at the turn limit".to_string()),
        StopReason::BudgetStop => Some(format!(
            "stopped at the cap of {cap} weighted tokens; raise spec_review_budget_weighted"
        )),
    }
}

/// The stderr warning for a review file that could not be written.
fn unsaved_warning(file: &Path, err: &std::io::Error) -> String {
    format!(
        "warning: could not save the spec review to {}: {err}",
        file.display()
    )
}

/// The verdict words for the note, from the last line of a review.
fn verdict_name(text: &str) -> &'static str {
    match last_line(text) {
        "VERDICT: APPROVE" => "approved",
        "VERDICT: CHANGES" => "asked for changes",
        _ => "gave no verdict",
    }
}

/// The review `factory new` used last in this process, or `None` after none or a failed run.
pub(crate) fn last_review_info(driver: &Driver) -> Option<SpecReviewInfo> {
    let kept = driver.last_review.lock().unwrap_or_else(|e| e.into_inner());
    let rec = kept.as_ref().filter(|r| r.failure.is_none())?;
    Some(SpecReviewInfo {
        file: rec.file.clone(),
        verdict: verdict_name(&rec.text),
        saved: rec.saved,
    })
}

/// Keeps the review of this process, for a second validation and the note.
fn keep(driver: &Driver, record: ReviewRecord) {
    *driver.last_review.lock().unwrap_or_else(|e| e.into_inner()) = Some(record);
}

/// Maps the last line of a review to a result.
fn verdict_of(text: &str, file: PathBuf) -> Result<(), SpecRefusal> {
    match last_line(text) {
        "VERDICT: APPROVE" => Ok(()),
        "VERDICT: CHANGES" => Err(SpecRefusal::ReviewChanges {
            file,
            findings: text.trim_end().to_string(),
        }),
        _ => Err(SpecRefusal::ReviewMissing { file }),
    }
}

/// Runs the spec reviewer on the spec and the plans, unless an APPROVE review of the same text is on file.
/// The review is saved under the spec reviews folder, and its path is in every refusal.
pub fn review_spec(driver: &Driver, spec: &Path, plans: &[PathBuf]) -> Result<(), SpecRefusal> {
    review_spec_with(driver, spec, plans, false, None)
}

/// The line the reviewer prompt gets in intake mode.
const INTAKE_PROMPT_LINE: &str =
    "This spec was drafted unattended from an issue. Every decision cites a source file that exists";

/// The reviewer prompt, with the intake line added when the spec was drafted unattended. The line
/// names the exported tree that the sources live in.
fn reviewer_prompt(base: &str, intake_tree: Option<&Path>) -> String {
    match intake_tree {
        Some(tree) => format!("{base}\n{INTAKE_PROMPT_LINE} in {}.", tree.display()),
        None => base.to_string(),
    }
}

/// Checks that every list item under `## Decisions` of an intake spec cites a real file of the exported
/// tree: `- <decision>. Source: <ref>[, <ref>]. Decided by: call`. Only the first line of an item is read.
/// The error names the first decision that fails.
pub(crate) fn check_intake_decisions(spec: &str, tree: &Path) -> Result<(), String> {
    let body = section_body(spec, "Decisions").unwrap_or_default();
    // A nested bullet is detail of its decision, not a decision, so only top level items count.
    let items: Vec<&str> = body
        .lines()
        .filter(|l| !l.starts_with([' ', '\t']) && is_list_item(l))
        .collect();
    if items.is_empty() {
        return Err("the spec has no decisions".to_string());
    }
    let root = tree
        .canonicalize()
        .map_err(|e| format!("the exported tree cannot be read: {e}"))?;
    let mut line_counts = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        check_decision(item, &root, &mut line_counts).map_err(|why| {
            let text = item
                .trim_start()
                .split_once(' ')
                .map_or("", |(_, rest)| rest);
            let head: String = text.chars().take(80).collect();
            format!("decision {} (\"{head}\") is not sourced: {why}", index + 1)
        })?;
    }
    Ok(())
}

/// Checks one decision line against the canonical tree.
fn check_decision(
    item: &str,
    root: &Path,
    line_counts: &mut HashMap<PathBuf, usize>,
) -> Result<(), String> {
    if item.contains("Decided by: user") {
        return Err("no user answered it, so it cannot say `Decided by: user`".to_string());
    }
    let start = item
        .rfind("Source:")
        .ok_or("it has no `Source:` clause on its line")?;
    let after = &item[start + "Source:".len()..];
    let end = after
        .rfind(". Decided by: call")
        .ok_or("the `Source:` clause must end with `. Decided by: call` on the same line")?;
    for reference in after[..end].split(", ") {
        check_ref(reference.trim(), root, line_counts)?;
    }
    Ok(())
}

/// Checks one `path`, `path:line` or `path:first-last` reference.
fn check_ref(
    reference: &str,
    root: &Path,
    line_counts: &mut HashMap<PathBuf, usize>,
) -> Result<(), String> {
    let (path, range) = match reference.rsplit_once(':') {
        Some((path, tail)) if tail.starts_with(|c: char| c.is_ascii_digit()) => (path, Some(tail)),
        _ => (reference, None),
    };
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part == "..")
    {
        return Err(format!("`{reference}` is not a path inside the tree"));
    }
    let real = root
        .join(path)
        .canonicalize()
        .map_err(|_| format!("`{path}` does not exist in the tree"))?;
    if !real.starts_with(root) || !real.is_file() {
        return Err(format!("`{path}` is not a file inside the tree"));
    }
    let Some(range) = range else {
        return Ok(());
    };
    let (first, last) = match range.split_once('-') {
        Some((first, last)) => (first.parse::<usize>(), last.parse::<usize>()),
        None => (range.parse::<usize>(), range.parse::<usize>()),
    };
    let (Ok(first), Ok(last)) = (first, last) else {
        return Err(format!(
            "`{reference}` has a line range that is not a number"
        ));
    };
    let lines = match line_counts.get(&real) {
        Some(lines) => *lines,
        None => {
            let bytes =
                std::fs::read(&real).map_err(|e| format!("`{path}` cannot be read: {e}"))?;
            let text =
                String::from_utf8(bytes).map_err(|_| format!("`{path}` is not UTF-8 text"))?;
            let lines = text.lines().count();
            line_counts.insert(real, lines);
            lines
        }
    };
    if first == 0 || first > last || last > lines {
        return Err(format!(
            "`{reference}` is outside the file, which has {lines} lines"
        ));
    }
    Ok(())
}

/// Like `review_spec`. With `intake_tree`, the reviewer is told the spec was drafted unattended from it.
/// With `reuse_saved`, a review on file for the same text gives its verdict whatever it is, so
/// the user who overrides a refusal does not pay for a second run or lose the findings they read.
pub(crate) fn review_spec_with(
    driver: &Driver,
    spec: &Path,
    plans: &[PathBuf],
    reuse_saved: bool,
    intake_tree: Option<&Path>,
) -> Result<(), SpecRefusal> {
    if !driver.config().spec_review {
        return Ok(());
    }
    // A preset from before the spec reviewer resolves agent files without it. Without any resolved
    // files the checkout's own agent folder is used, so that case is not judged here.
    if !driver.agent_files().is_empty() && reviewer_file_text(driver).trim().is_empty() {
        return Err(SpecRefusal::ReviewerMissing);
    }
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let plan_texts: Vec<String> = plans.iter().map(|p| read(p)).collect();
    let key = review_key(
        &read(spec),
        &plan_texts,
        reviewer_file_text(driver),
        intake_tree.is_some(),
    );
    let file = driver.layout().spec_reviews_dir().join(format!("{key}.md"));
    let kept = driver
        .last_review
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|r| r.key == key);
    // A failed run is not run again by this process, so a call that validates twice pays once.
    if let Some(reason) = kept.as_ref().and_then(|r| r.failure.clone()) {
        return Err(SpecRefusal::ReviewFailed { reason });
    }
    let mut saved = read(&file);
    let mut on_disk = true;
    if saved.is_empty() {
        if let Some(rec) = kept {
            saved = rec.text;
            on_disk = rec.saved;
        }
    }
    if last_line(&saved) == "VERDICT: APPROVE" || (reuse_saved && !saved.is_empty()) {
        keep(
            driver,
            ReviewRecord {
                key,
                file: file.clone(),
                text: saved.clone(),
                saved: on_disk,
                failure: None,
            },
        );
        return verdict_of(&saved, file);
    }
    let mut agent = driver.spec_reviewer_spec(driver.root(), spec, plans);
    agent.prompt = reviewer_prompt(&agent.prompt, intake_tree);
    // Claude takes its agents from the file. Without resolved files the checkout's own folder is used.
    if driver.config().agent_cli() != "codex" && !driver.agent_files().is_empty() {
        match reviewer_agents_file(driver) {
            Ok(path) => agent.agents_file = Some(path),
            Err(reason) => return Err(SpecRefusal::ReviewFailed { reason }),
        }
    }
    let started = std::time::Instant::now();
    let run = driver.run_agent_for(SPEC_REVIEW_JOB, &agent);
    log_review_usage(driver, &agent, &run, started.elapsed().as_secs());
    let cap = driver.config().spec_review_budget_weighted;
    if let Some(reason) = incomplete_reason(&run, cap) {
        keep(
            driver,
            ReviewRecord {
                key,
                file,
                text: String::new(),
                saved: false,
                failure: Some(reason.clone()),
            },
        );
        return Err(SpecRefusal::ReviewFailed { reason });
    }
    let mut text = run.result_text.trim_end().to_string();
    text.push('\n');
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let written = std::fs::write(&file, &text);
    if let Err(err) = &written {
        eprintln!("{}", unsaved_warning(&file, err));
    }
    keep(
        driver,
        ReviewRecord {
            key,
            file: file.clone(),
            text: text.clone(),
            saved: written.is_ok(),
            failure: None,
        },
    );
    verdict_of(&text, file)
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
        let hint = SPEC_OVERRIDE_HINT;
        let file = PathBuf::from("/s/spec-reviews/k.md");
        for refusal in [
            SpecRefusal::NoUserDecision("the spec touches risky paths (a)".into()),
            SpecRefusal::ReviewChanges {
                file: file.clone(),
                findings: "1. [blocker] x".into(),
            },
            SpecRefusal::ReviewMissing { file },
            SpecRefusal::ReviewFailed {
                reason: "stalled".into(),
            },
        ] {
            assert!(refusal.to_string().ends_with(hint), "{refusal}");
        }
        let failed = SpecRefusal::ReviewFailed {
            reason: "stalled".into(),
        };
        assert!(failed.to_string().contains("did not finish (stalled)"));
    }

    /// A temp tree with `docs/a.md` (3 lines) and `docs/b.md` (1 line).
    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("docs dir");
        std::fs::write(dir.path().join("docs/a.md"), "one\ntwo\nthree\n").expect("write a");
        std::fs::write(dir.path().join("docs/b.md"), "only\n").expect("write b");
        dir
    }

    fn decisions(items: &[&str]) -> String {
        let list: Vec<String> = items.iter().map(|i| format!("- {i}")).collect();
        format!("## Decisions\n\n{}\n", list.join("\n"))
    }

    fn sourced(source: &str) -> String {
        decisions(&[&format!(
            "Use the guide. Source: {source}. Decided by: call"
        )])
    }

    #[test]
    fn check_intake_decisions_accepts_a_sourced_decision() {
        let dir = tree();
        assert_eq!(
            check_intake_decisions(&sourced("docs/a.md:2"), dir.path()),
            Ok(())
        );
        assert_eq!(
            check_intake_decisions(&sourced("docs/a.md"), dir.path()),
            Ok(())
        );
    }

    #[test]
    fn check_intake_decisions_takes_a_range_and_two_refs() {
        let dir = tree();
        let spec = sourced("docs/a.md:2-3, docs/b.md");
        assert_eq!(check_intake_decisions(&spec, dir.path()), Ok(()));
    }

    #[test]
    fn check_intake_decisions_refuses_a_missing_source() {
        let dir = tree();
        let text = "Use the guide for the long reason that goes on and on and on and on and on and on and on";
        let spec = decisions(&[&format!("{text}. Decided by: call")]);
        let err = check_intake_decisions(&spec, dir.path()).expect_err("refused");
        assert!(err.contains(&text[..80]), "{err}");
        assert!(err.contains("decision 1"), "{err}");
        let wrapped = "## Decisions\n\n- Use the guide,\n  Source: docs/a.md. Decided by: call\n";
        check_intake_decisions(wrapped, dir.path()).expect_err("wrapped is refused");
    }

    #[test]
    fn check_intake_decisions_skips_nested_bullets() {
        let dir = tree();
        let spec = "## Decisions\n\n- Use it. Source: docs/a.md. Decided by: call\n  - detail without a source\n";
        assert_eq!(check_intake_decisions(spec, dir.path()), Ok(()));
    }

    #[test]
    fn check_intake_decisions_refuses_a_user_decision() {
        let dir = tree();
        let spec = decisions(&["Use it. Source: docs/a.md. Decided by: user"]);
        check_intake_decisions(&spec, dir.path()).expect_err("refused");
    }

    #[test]
    fn check_intake_decisions_refuses_an_empty_section() {
        let dir = tree();
        for spec in ["## Goal\n\nx\n", "## Decisions\n\nnothing here\n"] {
            let err = check_intake_decisions(spec, dir.path()).expect_err("refused");
            assert!(err.contains("the spec has no decisions"), "{err}");
        }
    }

    #[test]
    fn check_intake_decisions_refuses_a_path_outside_the_tree() {
        let dir = tree();
        for source in ["../x", "/etc/hostname", "docs/../../x"] {
            check_intake_decisions(&sourced(source), dir.path()).expect_err(source);
        }
    }

    #[test]
    fn check_intake_decisions_refuses_a_symlink_escape() {
        let dir = tree();
        let outside = tempfile::tempdir().expect("outside dir");
        std::fs::write(outside.path().join("secret.md"), "x\n").expect("write secret");
        std::os::unix::fs::symlink(outside.path().join("secret.md"), dir.path().join("link.md"))
            .expect("symlink");
        check_intake_decisions(&sourced("link.md"), dir.path()).expect_err("refused");
    }

    #[test]
    fn check_intake_decisions_refuses_a_line_out_of_range() {
        let dir = tree();
        for source in [
            "docs/a.md:0",
            "docs/a.md:4",
            "docs/a.md:3-2",
            "docs/a.md:1-4",
        ] {
            check_intake_decisions(&sourced(source), dir.path()).expect_err(source);
        }
    }

    #[test]
    fn check_intake_decisions_refuses_a_missing_file() {
        let dir = tree();
        check_intake_decisions(&sourced("docs/none.md"), dir.path()).expect_err("refused");
    }

    #[test]
    fn check_intake_decisions_refuses_a_file_that_is_not_utf8() {
        let dir = tree();
        std::fs::write(dir.path().join("bin.md"), [0xff, 0xfe]).expect("write bin");
        check_intake_decisions(&sourced("bin.md:1"), dir.path()).expect_err("refused");
    }

    #[test]
    fn check_intake_decisions_counts_lines_without_the_trailing_newline() {
        let dir = tree();
        std::fs::write(dir.path().join("c.md"), "a\nb\nc\n").expect("write c");
        assert_eq!(
            check_intake_decisions(&sourced("c.md:3"), dir.path()),
            Ok(())
        );
        check_intake_decisions(&sourced("c.md:4"), dir.path()).expect_err("refused");
    }

    #[test]
    fn check_intake_decisions_uses_the_last_source_on_the_line() {
        let dir = tree();
        let spec = decisions(&["Why Source: nope.md. Source: docs/a.md. Decided by: call"]);
        assert_eq!(check_intake_decisions(&spec, dir.path()), Ok(()));
    }

    #[test]
    fn the_reviewer_prompt_in_intake_mode_says_the_spec_was_drafted_unattended() {
        let line = "This spec was drafted unattended from an issue. Every decision cites a source file that exists in /tmp/tree.";
        assert!(reviewer_prompt("base", Some(Path::new("/tmp/tree"))).contains(line));
        assert_eq!(reviewer_prompt("base", None), "base");
    }

    #[test]
    fn an_unsourced_decision_displays_bare() {
        let shown = SpecRefusal::UnsourcedDecision("decision 1 is not sourced".into()).to_string();
        assert_eq!(shown, "decision 1 is not sourced");
    }

    fn stopped(reason: factory_agent::StopReason, error: Option<&str>) -> AgentRun {
        AgentRun {
            stop_reason: reason,
            error: error.map(str::to_string),
            ..AgentRun::done("")
        }
    }

    #[test]
    fn incomplete_reason_names_each_stop() {
        use factory_agent::StopReason::{BudgetStop, Failed, Stalled, TurnLimit};
        assert_eq!(incomplete_reason(&AgentRun::done("x"), 100), None);
        assert_eq!(
            incomplete_reason(&stopped(Failed, Some("boom")), 100).as_deref(),
            Some("failed: boom")
        );
        assert_eq!(
            incomplete_reason(&stopped(Failed, None), 100).as_deref(),
            Some("failed: no error text")
        );
        assert_eq!(
            incomplete_reason(&stopped(Stalled, None), 100).as_deref(),
            Some("stalled")
        );
        assert_eq!(
            incomplete_reason(&stopped(TurnLimit, None), 100).as_deref(),
            Some("stopped at the turn limit")
        );
        assert_eq!(
            incomplete_reason(&stopped(BudgetStop, None), 100).as_deref(),
            Some("stopped at the cap of 100 weighted tokens; raise spec_review_budget_weighted")
        );
    }

    #[test]
    fn the_unsaved_warning_names_the_path_and_error() {
        let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let text = unsaved_warning(Path::new("/s/spec-reviews/k.md"), &err);
        assert_eq!(
            text,
            "warning: could not save the spec review to /s/spec-reviews/k.md: denied"
        );
    }

    #[test]
    fn the_review_key_changes_with_a_plan() {
        let a = review_key("spec", &["p1".into()], "agent", false);
        assert_ne!(a, review_key("spec", &["p1".into()], "agent", true));
        assert_eq!(a.len(), 64);
        assert_eq!(a, review_key("spec", &["p1".into()], "agent", false));
        assert_ne!(a, review_key("spec", &["p2".into()], "agent", false));
        assert_ne!(a, review_key("spec", &["p1".into()], "agent 2", false));
        // Text moved from one part to the next is a different input.
        assert_ne!(
            review_key("ab", &["c".into()], "", false),
            review_key("a", &["bc".into()], "", false)
        );
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

    #[test]
    fn paths_outside_files_blocks_are_ignored() {
        let text = "Touch `before.rs` first.\n\n**Files:**\n- Modify: `in.rs`\n\nSee `after.rs` too.\n- Modify: `late.rs`\n";
        assert_eq!(plan_file_paths(text), vec!["in.rs"]);
    }

    #[test]
    fn two_paths_on_one_line_are_both_read() {
        let text = "**Files:**\n- Modify: `a.rs` and `b.rs`\n";
        assert_eq!(plan_file_paths(text), vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn a_degraded_config_makes_every_path_risky() {
        let cfg = Config {
            risk_paths: vec!["**".to_string()],
            ..Config::default()
        };
        let got = risky_without_user_decision(SPEC_NONE, &[plan("docs/a.md")], &cfg);
        assert!(got.is_some());
        let got =
            risky_without_user_decision(SPEC_NONE, &["### Task 1\n\n- [ ] step\n".into()], &cfg);
        assert_eq!(got, None);
    }
}
