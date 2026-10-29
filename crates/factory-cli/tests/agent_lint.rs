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
    let turns: u32 = raw
        .parse()
        .map_err(|_| format!("{label}: maxTurns not a number"))?;
    if (1..=400).contains(&turns) {
        Ok(())
    } else {
        Err(format!("{label}: maxTurns {turns} out of range"))
    }
}

fn check_description(fm: &[(String, String)], label: &str) -> TestResult {
    let desc = fm_get(fm, "description").ok_or(format!("{label}: description missing"))?;
    if desc.is_empty() || desc.contains(": ") {
        return Err(format!(
            "{label}: description empty or has a colon and space"
        ));
    }
    Ok(())
}

#[test]
fn only_the_known_factory_agents_remain() -> TestResult {
    let dir = repo_root()?.join(".claude").join("agents");
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .to_string();
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
            "superpowers:subagent-driven-development",
            "Status:",
            "DONE_WITH_CONCERNS",
            "NEEDS_CONTEXT",
            "Ruling:",
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
        ],
    )?;
    forbid_all(
        &text,
        "builder",
        &[
            "superpowers:executing-plans",
            "inline fallback",
            "No handoff lines",
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
        &[
            "kb-refresh",
            "Workflow",
            "only files under the KB path",
            "BLOCKED:",
            "No relevant changes",
            "Never write git state",
            "Never ask the user",
        ],
    )?;
    forbid_all(
        &text,
        "kb refresher",
        &["SendMessage", "--force-with-lease"],
    )
}

const COMMAND: &str = "crates/factory-core/assets/global-command.md";

#[test]
fn command_is_thin_and_pins_the_shapes() -> TestResult {
    let text = read(COMMAND)?;
    require_all(
        &text,
        "command",
        &[
            "<factory> --project <name> new",
            "<factory> --project <name> run",
            "--series",
            "--opus",
            "--plans",
            "<factory> --project <name> status",
            "<factory> --project <name> approve",
            "<factory> --project <name> rework",
            "<factory> --project <name> cancel",
            "<factory> --project <name> cleanup",
            "exact shape",
            "next button for pagination",
            "status page for jobs",
            "approve button for forms",
            "superpowers:brainstorming",
            "superpowers:writing-plans",
            "run_in_background",
            "Do not poll",
            "at most 3 tasks",
            "## Goal",
            "## Acceptance criteria",
            "explicit approval",
            "Do not write any plan before that approval",
            "Do not run `<factory> --project <name> new` before that approval",
            "no placeholders",
            "Review Focus",
            "Next plan:",
            "Next plan: none",
            "validate",
            "rework needs a note",
            "escape",
            "Never edit the board",
            // The interview and the committed spec and plans.
            "--plans docs/superpowers/plans/<date>-<slug> --spec docs/superpowers/specs/<date>-<slug>-design.md",
            "docs/superpowers/specs/<date>-<slug>-design.md",
            "docs/superpowers/plans/<date>-<slug>/",
            // The 7.0.0 interview and the draft card.
            "<factory> --project <name> spec start <slug> '<feature>'",
            "<factory> --project <name> spec stage <slug> writing-spec",
            "<factory> --project <name> spec stage <slug> writing-plans",
            "<factory> --project <name> spec resume <slug>",
            "`file <repo-relative path>",
            "never only digits",
            "the same slug",
            "recommended answer",
            "`Decided by: user`",
            "`Decided by: call`",
            "builder check",
            "opt-out",
            "dev-workflow-skills:1-git-commit",
            "git commit --only -- <spec path>",
            "git commit --only -- <plan paths>",
            "## Decisions",
            // The 7.0.0 plans, run by the driver.
            "superpowers:subagent-driven-development",
            "one fresh factory-builder implementer per task",
            "File Structure",
            "`**Interfaces:**`",
            "plan self-review",
            "at most 6 plans",
            "ask for approval again",
            "repo-relative",
            "Keep a visible task list",
            "skip the skill's execution handoff",
            "only when that branch merges",
            // The overrides of the brainstorming skill, the drafts and a split feature's ticket.
            "Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words.",
            "because open questions were too hard to answer",
            "Red Flags row \"Options steer\"",
            "Always size it as a project with a written design",
            "If they still decline, run `<factory> --project <name> cancel <slug>` to remove the draft card and stop without building anything.",
            "Survey the code and the ticket first",
            "`<factory> --project <name> cancel <slug>`",
            "When the `file` line says `exists`, read the file and continue from it, never overwrite it.",
            "Only the last series carries `#N` or the issue URL in its feature text",
            "Report every `warning:` line",
            "refuses a plan with `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences",
        ],
    )?;
    let pos = |needle: &str| {
        text.find(needle)
            .ok_or_else(|| format!("command is missing: {needle}"))
    };
    if pos("Run `<factory> --project <name> status`")?
        > pos("<factory> --project <name> spec start <slug>")?
    {
        return Err("the draft check must come before `spec start`".to_string());
    }
    if pos("After it, stage the spec path")?
        > pos("<factory> --project <name> spec stage <slug> writing-plans")?
    {
        return Err(
            "the spec commit must come before `spec stage <slug> writing-plans`".to_string(),
        );
    }
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
            // The 6.4.1 rules that 7.0.0 removed, and the inline executor.
            "executing-plans",
            "architectural path",
            "never classify",
            "spec self-review",
            "Coverage gate",
            "Propose 2 to 3",
            "approval after each section",
            "2 to 5 minute",
            "one at a time",
            "purpose and who benefits",
            "n/a: <reason>",
            "read-only exploration subagents",
            "Everything else in the skill stands",
        ],
    )
}

#[test]
fn docs_describe_the_new_design() -> TestResult {
    let doc = read("docs/reference/software-factory.md")?;
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
            "Decided by: user",
            "builder check",
            "superpowers:subagent-driven-development",
        ],
    )?;
    forbid_all(
        &doc,
        "software-factory.md",
        &[
            "factory-ctl",
            "factory-approver",
            "factory-spec-writer",
            "log-event",
            "n/a: <reason>",
            "required coverage list",
            "one question at a time",
        ],
    )?;
    let root_doc = read("CLAUDE.md")?;
    require_all(
        &root_doc,
        "CLAUDE.md",
        &["docs/reference/software-factory.md"],
    )?;
    forbid_all(&root_doc, "CLAUDE.md", &["factory-ctl"])
}

#[test]
fn docs_describe_the_per_project_api_key() -> TestResult {
    let reference = read("docs/reference/project-toml-reference.md")?;
    require_all(
        &reference,
        "project-toml-reference.md",
        &[
            "`allow_api_key`",
            "`api_key_file`",
            "chmod 600",
            "never falls back",
        ],
    )?;
    let readme = read("README.md")?;
    require_all(
        &readme,
        "README.md",
        &["`allow_api_key = true`", "`api_key_file"],
    )?;
    let pipeline = read("docs/reference/software-factory.md")?;
    require_all(
        &pipeline,
        "software-factory.md",
        &["`allow_api_key = true`", "`api_key_file`"],
    )
}

#[test]
fn reference_documents_the_builder_permission_keys() -> TestResult {
    let doc = read("docs/reference/project-toml-reference.md")?;
    require_all(
        &doc,
        "project-toml-reference.md",
        &[
            "`builder_permissions`",
            "`builder_allow`",
            "acceptEdits",
            "Bash(<program>:*)",
            "shell operators",
            "rmdir",
            "xargs",
            "fails closed",
            "timeout",
            "quotes",
            "wildcards",
            "duplicates",
            "Bash(<command>:*)",
        ],
    )
}

#[test]
fn spec_matches_the_builder_permission_fixes() -> TestResult {
    let doc = read("docs/superpowers/specs/2026-10-19-builder-permissions-design.md")?;
    require_all(
        &doc,
        "builder-permissions-design.md",
        &[
            "builder-tampered",
            "timeout",
            "Edit(//<kb dir>/**)",
            "shell operators",
            "quotes",
        ],
    )
}

#[test]
fn software_factory_doc_describes_builder_permissions() -> TestResult {
    let doc = read("docs/reference/software-factory.md")?;
    require_all(
        &doc,
        "software-factory.md",
        &[
            "`builder_permissions`",
            "`builder_allow`",
            "`--disallowedTools`",
            "Edit(//",
            "builder changed",
            "acceptEdits",
            "claude 2.1.303",
            "2026-10-19",
            "Deny rule plus an allow",
            "`find`",
            "Bash(cd:*)",
            "Bash(git status:*)",
            "builder-tampered",
            "knowledge base",
            "not a sandbox",
            "--output",
            "has not been probed",
            "cd web && bun test",
            "duplicates are removed",
        ],
    )
}

#[test]
fn readme_documents_upgrading() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "### Upgrading",
            "factory upgrade --dry-run",
            "no running jobs",
            "factory.md.bak",
            "refused files stay as they are",
            "factory presets refresh --force",
            "shadowed: <project> <project-layer|repo-layer> <file>",
            "do not follow preset changes",
            "factoryd stop",
            "cargo build -j4 --manifest-path dashboard/Cargo.toml",
            "<home>/VERSION",
            "version: ",
        ],
    )?;
    forbid_all(
        &doc,
        "README.md",
        &["After an upgrade also run `factory install --force`"],
    )
}

#[test]
fn readme_documents_the_job_lease_and_resume() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "### The job lease",
            "every 60 seconds",
            "`driver-<pid>-<secs>`",
            "counts as stale at once, for the job lease too",
            "resumes that session",
            "one fresh builder runs on the worktree as it is",
            "`resumed` or `fresh`",
            "`factoryd enqueue`",
            "`nohup`",
            "`tmux`",
            "survive a dropped terminal",
        ],
    )?;
    forbid_all(
        &doc,
        "README.md",
        &["the job lease is judged by expiry only"],
    )
}

#[test]
fn getting_started_upgrades_with_one_command() -> TestResult {
    let doc = read("docs/guides/getting-started.md")?;
    require_all(
        &doc,
        "getting-started.md",
        &["factory upgrade", "factoryd stop"],
    )?;
    forbid_all(&doc, "getting-started.md", &["do all four steps"])
}

#[test]
fn readme_points_to_builder_permissions() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "`builder_permissions`",
            "docs/reference/project-toml-reference.md",
            "docs/reference/software-factory.md",
            "check_hints",
        ],
    )?;
    forbid_all(&doc, "README.md", &["never blocks on a prompt"])
}

#[test]
fn readme_documents_the_rules_marker() -> TestResult {
    let text = read("README.md")?;
    require_all(
        &text,
        "README",
        &[
            "## Agent file rules marker",
            "<!-- factory-rules: 1 -->",
            "no current rules marker",
            "factory upgrade",
            "prompts keep the full rules",
        ],
    )
}

#[test]
fn agent_max_turns_match_the_driver_constants() -> TestResult {
    let files = [
        (
            ".claude/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/rust/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/go/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/ruby/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/csharp/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/node/agents/factory-builder.md",
            factory_engine::BUILDER_MAX_TURNS,
        ),
        (
            ".claude/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
        (
            "crates/factory-core/presets/node/agents/factory-code-reviewer.md",
            factory_engine::REVIEWER_MAX_TURNS,
        ),
    ];
    for (path, want) in files {
        let fm = frontmatter(&read(path)?)?;
        let raw = fm_get(&fm, "maxTurns").ok_or(format!("{path}: maxTurns missing"))?;
        let got: u32 = raw
            .parse()
            .map_err(|_| format!("{path}: maxTurns not a number"))?;
        if got != want {
            return Err(format!(
                "{path}: maxTurns is {got}, the driver constant is {want}"
            ));
        }
    }
    Ok(())
}

#[test]
fn reviewer_files_carry_the_vision_rule_and_declined_to_judge() -> TestResult {
    for path in [
        ".claude/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
    ] {
        let text = read(path)?;
        require_all(
            &text,
            path,
            &[
                "vision document",
                "reasonable person",
                "Declined to judge",
                "Critical",
                "Important",
                "Minor",
                "you do not dispatch subagents",
                "Out-of-scope observations",
                "[blocker]",
                "[minor]",
                "Any `[blocker]` means CHANGES.",
            ],
        )?;
        if !has_line(&text, "VERDICT: APPROVE") || !has_line(&text, "VERDICT: CHANGES") {
            return Err(format!(
                "{path}: both VERDICT lines must stay on their own line"
            ));
        }
        require_all(
            &text,
            path,
            &["Your last line, alone, must be exactly one of:"],
        )?;
    }
    Ok(())
}

const PRESET_BUILDERS: [&str; 5] = [
    "crates/factory-core/presets/rust/agents/factory-builder.md",
    "crates/factory-core/presets/go/agents/factory-builder.md",
    "crates/factory-core/presets/ruby/agents/factory-builder.md",
    "crates/factory-core/presets/csharp/agents/factory-builder.md",
    "crates/factory-core/presets/node/agents/factory-builder.md",
];

#[test]
fn presets_name_subagent_driven_development_and_not_executing_plans() -> TestResult {
    for path in PRESET_BUILDERS {
        let text = read(path)?;
        require_all(
            &text,
            path,
            &[
                "superpowers:subagent-driven-development",
                "Status:",
                "DONE_WITH_CONCERNS",
                "NEEDS_CONTEXT",
                "Ruling:",
            ],
        )?;
        forbid_all(
            &text,
            path,
            &[
                "superpowers:executing-plans",
                "inline fallback",
                "No handoff lines",
            ],
        )?;
    }
    Ok(())
}

#[test]
fn reviewer_files_name_the_review_focus_and_the_builder_rulings() -> TestResult {
    for path in [
        ".claude/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
    ] {
        require_all(&read(path)?, path, &["Review Focus", "rulings", "concerns"])?;
    }
    Ok(())
}

#[test]
fn reviewer_files_tag_fixed_findings() -> TestResult {
    for path in [
        ".claude/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/node/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
    ] {
        require_all(&read(path)?, path, &["[fixed]", "keeps no other tag"])?;
    }
    Ok(())
}

/// The builder follows the implementer rules of subagent-driven-development and never acts as its controller.
const NOT_CONTROLLER: &str =
    "You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review.";

#[test]
fn builder_files_say_the_builder_is_the_implementer_not_the_controller() -> TestResult {
    for path in [
        ".claude/agents/factory-builder.md",
        PRESET_BUILDERS[0],
        PRESET_BUILDERS[1],
        PRESET_BUILDERS[2],
        PRESET_BUILDERS[3],
        PRESET_BUILDERS[4],
    ] {
        require_all(
            &read(path)?,
            path,
            &[
                "Follow the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md)",
                NOT_CONTROLLER,
                "only to read or search code",
            ],
        )?;
        forbid_all(
            &read(path)?,
            path,
            &["as its implementer: one implementer per task"],
        )?;
    }
    Ok(())
}

#[test]
fn builder_files_carry_the_quality_bar() -> TestResult {
    for path in [".claude/agents/factory-builder.md"]
        .into_iter()
        .chain(PRESET_BUILDERS)
    {
        require_all(
            &read(path)?,
            path,
            &[
                "## Quality bar",
                "Before `Status: DONE`, read your own diff once",
                "Edge cases:",
                "Tests:",
                "Reuse:",
                "No dead code:",
                "No needless work:",
            ],
        )?;
    }
    Ok(())
}

#[test]
fn reviewer_files_check_edge_cases_reuse_and_efficiency() -> TestResult {
    for path in [
        ".claude/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
        "crates/factory-core/presets/node/agents/factory-code-reviewer.md",
    ] {
        require_all(
            &read(path)?,
            path,
            &[
                "edge cases and error paths:",
                "reuse and simplicity:",
                "efficiency:",
                "Grade a reuse, simplicity or efficiency finding `[minor]`",
            ],
        )?;
    }
    Ok(())
}

// The `.claude/agents` copies are marked by the maintainer, so they are not listed here.
const MARKED: [&str; 10] = [
    "crates/factory-core/presets/rust/agents/factory-builder.md",
    "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
    "crates/factory-core/presets/go/agents/factory-builder.md",
    "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
    "crates/factory-core/presets/ruby/agents/factory-builder.md",
    "crates/factory-core/presets/ruby/agents/factory-code-reviewer.md",
    "crates/factory-core/presets/csharp/agents/factory-builder.md",
    "crates/factory-core/presets/csharp/agents/factory-code-reviewer.md",
    "crates/factory-core/presets/node/agents/factory-builder.md",
    "crates/factory-core/presets/node/agents/factory-code-reviewer.md",
];

#[test]
fn shipped_builder_and_reviewer_files_carry_the_rules_marker() -> TestResult {
    for path in MARKED {
        let text = read(path)?;
        let after_frontmatter = text.splitn(3, "---\n").nth(2).unwrap_or_default();
        if after_frontmatter.lines().next() != Some("<!-- factory-rules: 1 -->") {
            return Err(format!(
                "{path} has no marker on the line after the frontmatter"
            ));
        }
    }
    Ok(())
}

#[test]
fn the_marker_version_matches_the_engine_constant() -> TestResult {
    let marker = format!("<!-- factory-rules: {} -->", factory_engine::RULES_VERSION);
    for path in MARKED {
        require_all(&read(path)?, path, &[marker.as_str()])?;
    }
    Ok(())
}

#[test]
fn docs_describe_the_builder_carry() -> TestResult {
    for path in ["README.md", "docs/reference/software-factory.md"] {
        require_all(
            &read(path)?,
            path,
            &[
                "builder_carry_session",
                "builder_carry_max_ctx",
                "continued",
                "cache_ttl",
            ],
        )?;
    }
    Ok(())
}

#[test]
fn the_committer_and_kb_files_have_no_rules_marker() -> TestResult {
    for path in [
        ".claude/agents/factory-committer.md",
        ".claude/agents/factory-kb-refresher.md",
    ] {
        if read(path)?.contains("factory-rules") {
            return Err(format!("{path} must not carry a rules marker"));
        }
    }
    Ok(())
}

#[test]
fn builder_files_carry_the_rules_digest() -> TestResult {
    for path in [
        ".claude/agents/factory-builder.md",
        PRESET_BUILDERS[0],
        PRESET_BUILDERS[1],
        PRESET_BUILDERS[2],
        PRESET_BUILDERS[3],
        PRESET_BUILDERS[4],
    ] {
        require_all(
            &read(path)?,
            path,
            &[
                "Rules in short",
                "no production code without a failing test first",
                "no fix without a root cause",
                "no completion claim without fresh evidence",
                "watch it fail",
                "read the output and exit code",
            ],
        )?;
    }
    Ok(())
}
