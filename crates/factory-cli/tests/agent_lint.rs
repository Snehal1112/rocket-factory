//! Lint tests for the factory agent files, the /factory command and the cutover.
//! They read the repo files and pin the rules the driver relies on.

use std::fs;
use std::path::PathBuf;

type TestResult = Result<(), String>;

/// Walks up from the crate until a folder with `.claude/agents` is found.
fn repo_root() -> Result<PathBuf, String> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(".claude").join("agents").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("repo root with .claude/agents not found".to_string());
        }
    }
}

fn read(rel: &str) -> Result<String, String> {
    let path = repo_root()?.join(rel);
    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parses the key and value lines between the two `---` markers.
fn frontmatter(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err("missing frontmatter".to_string());
    }
    let mut out = Vec::new();
    for line in lines {
        if line == "---" {
            return Ok(out);
        }
        if let Some((key, value)) = line.split_once(':') {
            out.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    Err("unterminated frontmatter".to_string())
}

fn fm_get<'a>(fm: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fm.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn require_all(text: &str, label: &str, phrases: &[&str]) -> TestResult {
    let missing: Vec<&&str> = phrases.iter().filter(|p| !text.contains(**p)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("{label} is missing: {missing:?}"))
    }
}

fn forbid_all(text: &str, label: &str, phrases: &[&str]) -> TestResult {
    let found: Vec<&&str> = phrases.iter().filter(|p| text.contains(**p)).collect();
    if found.is_empty() {
        Ok(())
    } else {
        Err(format!("{label} must not contain: {found:?}"))
    }
}

fn has_line(text: &str, line: &str) -> bool {
    text.lines().any(|l| l == line)
}

fn check_max_turns(fm: &[(String, String)], label: &str) -> TestResult {
    let raw = fm_get(fm, "maxTurns").ok_or(format!("{label}: maxTurns missing"))?;
    let turns: u32 = raw.parse().map_err(|_| format!("{label}: maxTurns not a number"))?;
    if (1..=400).contains(&turns) {
        Ok(())
    } else {
        Err(format!("{label}: maxTurns {turns} out of range"))
    }
}

fn check_description(fm: &[(String, String)], label: &str) -> TestResult {
    let desc = fm_get(fm, "description").ok_or(format!("{label}: description missing"))?;
    if desc.is_empty() || desc.contains(": ") {
        return Err(format!("{label}: description empty or has a colon and space"));
    }
    Ok(())
}

#[test]
fn only_the_known_factory_agents_remain() -> TestResult {
    let dir = repo_root()?.join(".claude").join("agents");
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let name = entry.map_err(|e| e.to_string())?.file_name().to_string_lossy().to_string();
        if name.starts_with("factory-") {
            names.push(name);
        }
    }
    names.sort();
    let want = vec![
        "factory-builder.md".to_string(),
        "factory-code-reviewer.md".to_string(),
        "factory-committer.md".to_string(),
        "factory-kb-refresher.md".to_string(),
    ];
    if names == want {
        Ok(())
    } else {
        Err(format!("factory agents are {names:?}, want {want:?}"))
    }
}

#[test]
fn builder_frontmatter_is_headless() -> TestResult {
    let text = read(".claude/agents/factory-builder.md")?;
    let fm = frontmatter(&text)?;
    if fm_get(&fm, "name") != Some("factory-builder") {
        return Err("builder name is wrong".to_string());
    }
    for banned in ["isolation", "model", "tools"] {
        if fm_get(&fm, banned).is_some() {
            return Err(format!("builder frontmatter must not set {banned}"));
        }
    }
    check_description(&fm, "builder")?;
    check_max_turns(&fm, "builder")
}

#[test]
fn builder_body_pins_the_rules() -> TestResult {
    let text = read(".claude/agents/factory-builder.md")?;
    require_all(
        &text,
        "builder",
        &[
            "superpowers:executing-plans",
            "inline",
            "final whole-branch review",
            "ledger scripts",
            "progress file",
            "every commit step",
            "superpowers:test-driven-development",
            "superpowers:systematic-debugging",
            "superpowers:verification-before-completion",
            "Never run `git commit`",
            "driver commits them",
            "Never amend",
            "Do only Task",
            "Use at most 2 helper agents per task",
            "Never sleep",
            "never poll",
            "Never `cd` out of the worktree",
            "heredoc",
            "Use Edit and Write",
            "Never write the ledger",
            "board",
            "job directory",
            "Do not re-read `CLAUDE.md`",
            "crate `CLAUDE.md`",
            "BLOCKED:",
            "clippy",
            "release build",
            "-j4",
            "CARGO_TARGET_DIR",
            "shadcn",
            "lucide-react",
            "SingleLineEditor",
            "Monaco",
            "Zustand",
            "unwrap",
            "No handoff lines",
        ],
    )?;
    forbid_all(
        &text,
        "builder",
        &[
            "isolation",
            "WORKTREE:",
            "BASE:",
            "HEAD:",
            "git switch -c",
            "SendMessage",
            "dev-workflow-skills:1-git-commit",
            "git commit --only",
            "Never use a helper agent for commits",
        ],
    )
}

#[test]
fn reviewer_frontmatter_is_read_only() -> TestResult {
    let text = read(".claude/agents/factory-code-reviewer.md")?;
    let fm = frontmatter(&text)?;
    if fm_get(&fm, "name") != Some("factory-code-reviewer") {
        return Err("reviewer name is wrong".to_string());
    }
    if fm_get(&fm, "tools") != Some("Read, Grep, Glob") {
        return Err("reviewer tools must be exactly Read, Grep, Glob".to_string());
    }
    for banned in ["model", "isolation"] {
        if fm_get(&fm, banned).is_some() {
            return Err(format!("reviewer frontmatter must not set {banned}"));
        }
    }
    check_description(&fm, "reviewer")?;
    check_max_turns(&fm, "reviewer")
}

#[test]
fn reviewer_body_pins_the_rules() -> TestResult {
    let text = read(".claude/agents/factory-code-reviewer.md")?;
    require_all(
        &text,
        "reviewer",
        &[
            "correctness",
            "DDD placement",
            "serde",
            "error handling",
            "persistence",
            "tests",
            "shadcn",
            "lucide",
            "SingleLineEditor",
            "Monaco",
            "Zustand",
            "unwrap",
            "git CLI",
            "offset",
            "limit 1500",
            "at most 60 lines",
            "[blocker]",
            "[minor]",
            "Do not re-read `CLAUDE.md`",
            "crate `CLAUDE.md`",
        ],
    )?;
    if !has_line(&text, "VERDICT: APPROVE") || !has_line(&text, "VERDICT: CHANGES") {
        return Err("reviewer must list both VERDICT lines on their own line".to_string());
    }
    // `SingleLineEditor` contains "Edit", so it is removed before the tool names are checked.
    let without_editor = text.replace("SingleLineEditor", "");
    forbid_all(&without_editor, "reviewer", &["Bash", "Write", "Edit"])
}

#[test]
fn committer_frontmatter_is_headless() -> TestResult {
    let text = read(".claude/agents/factory-committer.md")?;
    let fm = frontmatter(&text)?;
    if fm_get(&fm, "name") != Some("factory-committer") {
        return Err("committer name is wrong".to_string());
    }
    for banned in ["isolation", "model", "tools"] {
        if fm_get(&fm, banned).is_some() {
            return Err(format!("committer frontmatter must not set {banned}"));
        }
    }
    check_description(&fm, "committer")?;
    check_max_turns(&fm, "committer")
}

#[test]
fn committer_body_pins_the_rules() -> TestResult {
    let text = read(".claude/agents/factory-committer.md")?;
    require_all(
        &text,
        "committer",
        &[
            "dev-workflow-skills:1-git-commit",
            "standing answers",
            "inline",
            "floor",
            "Never push",
            "Never ask the user",
            "GIT_SEQUENCE_EDITOR=true",
            "git add -A",
            "at most 4 commits",
            "git rebase --abort",
            "HARNESS_GATE",
            "BLOCKED:",
            "message-only",
            "---BEGIN MESSAGE---",
            "---END MESSAGE---",
        ],
    )?;
    forbid_all(&text, "committer", &["SendMessage", "--force-with-lease"])
}

#[test]
fn kb_refresher_frontmatter_is_headless() -> TestResult {
    let text = read(".claude/agents/factory-kb-refresher.md")?;
    let fm = frontmatter(&text)?;
    if fm_get(&fm, "name") != Some("factory-kb-refresher") {
        return Err("kb refresher name is wrong".to_string());
    }
    for banned in ["isolation", "model", "tools"] {
        if fm_get(&fm, banned).is_some() {
            return Err(format!("kb refresher frontmatter must not set {banned}"));
        }
    }
    check_description(&fm, "kb refresher")?;
    check_max_turns(&fm, "kb refresher")
}

#[test]
fn kb_refresher_body_pins_the_rules() -> TestResult {
    let text = read(".claude/agents/factory-kb-refresher.md")?;
    require_all(
        &text,
        "kb refresher",
        &["kb-refresh", "Workflow", "only files under the KB path", "BLOCKED:", "No relevant changes", "Never write git state", "Never ask the user"],
    )?;
    forbid_all(&text, "kb refresher", &["SendMessage", "--force-with-lease"])
}

const COMMAND: &str = ".claude/commands/factory.md";

#[test]
fn command_is_thin_and_pins_the_shapes() -> TestResult {
    let text = read(COMMAND)?;
    require_all(
        &text,
        "command",
        &[
            "<factory> new",
            "<factory> run",
            "--series",
            "--opus",
            "--plans",
            "<factory> status",
            "<factory> approve",
            "<factory> rework",
            "<factory> cancel",
            "<factory> cleanup",
            "exact shape",
            "next button for pagination",
            "status page for jobs",
            "approve button for forms",
            "superpowers:brainstorming",
            "superpowers:writing-plans",
            "run_in_background",
            "Do not poll",
            "at most 3 tasks",
            "architectural path",
            "never classify the feature as a spike",
            "## Goal",
            "## Acceptance criteria",
            "explicit approval",
            "Do not write any plan before that approval",
            "Do not run `<factory> new` before that approval",
            "no placeholders",
            "Review Focus",
            "Next plan:",
            "Next plan: none",
            "validate",
            "rework needs a note",
            "cargo build -j4",
            "escape",
            "Never edit the board",
            // The interview and the committed spec and plans.
            "--plans docs/superpowers/plans/<date>-<slug> --spec docs/superpowers/specs/<date>-<slug>-design.md",
            "docs/superpowers/specs/<date>-<slug>-design.md",
            "docs/superpowers/plans/<date>-<slug>/",
            "one at a time",
            "until the user confirms",
            "read-only exploration subagents",
            "purpose and who benefits",
            "edge cases and failure behavior",
            "non-goals",
            "rejected",
            "Propose 2 to 3 approaches",
            "approval after each section",
            "spec self-review",
            "dev-workflow-skills:1-git-commit",
            "git commit --only -- <spec path>",
            "git commit --only -- <plan paths>",
            "Coverage gate",
            "n/a: <reason>",
            "## Decisions",
            "ask for approval again",
            "repo-relative",
            "Keep a visible task list",
            "skip the skill's execution handoff",
            "only when that branch merges",
        ],
    )?;
    forbid_all(
        &text,
        "command",
        &[
            "factory-ctl",
            "python3",
            "factory-spec-writer",
            "factory-approver",
            "set-stage",
            "SendMessage",
            // Nothing is drafted in a gitignored folder any more.
            ".superpowers/sdd/draft-",
        ],
    )
}

#[test]
fn docs_describe_the_new_design() -> TestResult {
    let doc = read(".claude/software-factory.md")?;
    require_all(
        &doc,
        "software-factory.md",
        &[
            "Rust driver",
            "tools/factory/",
            "measured baseline",
            "estimate",
            "12.75M",
            "2.2M weighted",
            "needs-human",
            "VERDICT: APPROVE",
            "factory run",
            "tools/factory-server/",
            "risk_paths",
            "reviewed sha",
            "Acceptance criteria",
            "every job's folder",
            "only an exact `## Goal` line",
            "`--spec` needs `--plans`",
            "FACTORY_ALLOW_API_KEY",
            "n/a: <reason>",
        ],
    )?;
    forbid_all(&doc, "software-factory.md", &["factory-ctl", "factory-approver", "factory-spec-writer", "log-event"])?;
    let root_doc = read("CLAUDE.md")?;
    require_all(&root_doc, "CLAUDE.md", &[".claude/software-factory.md"])?;
    forbid_all(&root_doc, "CLAUDE.md", &["factory-ctl"])
}

#[test]
fn agent_max_turns_match_the_driver_constants() -> TestResult {
    let files = [
        (".claude/agents/factory-builder.md", factory_engine::BUILDER_MAX_TURNS),
        ("crates/factory-core/presets/rust/agents/factory-builder.md", factory_engine::BUILDER_MAX_TURNS),
        ("crates/factory-core/presets/go/agents/factory-builder.md", factory_engine::BUILDER_MAX_TURNS),
        (".claude/agents/factory-code-reviewer.md", factory_engine::REVIEWER_MAX_TURNS),
        ("crates/factory-core/presets/rust/agents/factory-code-reviewer.md", factory_engine::REVIEWER_MAX_TURNS),
        ("crates/factory-core/presets/go/agents/factory-code-reviewer.md", factory_engine::REVIEWER_MAX_TURNS),
    ];
    for (path, want) in files {
        let fm = frontmatter(&read(path)?)?;
        let raw = fm_get(&fm, "maxTurns").ok_or(format!("{path}: maxTurns missing"))?;
        let got: u32 = raw.parse().map_err(|_| format!("{path}: maxTurns not a number"))?;
        if got != want {
            return Err(format!("{path}: maxTurns is {got}, the driver constant is {want}"));
        }
    }
    Ok(())
}
