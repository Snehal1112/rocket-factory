use factory_core::{GLOBAL_COMMAND, TOOL_MAP_CLAUDE, TOOL_MAP_CODEX};

const NOTE_NAMES: [&str; 9] = [
    "Task list",
    "Shell and background runs",
    "Skills",
    "Subagents",
    "File tools",
    "Interview",
    "Plan writing",
    "Commit",
    "Model option",
];

/// Claude-only words, matched as whole words.
const CLAUDE_WORDS: [&str; 15] = [
    "TaskCreate",
    "TaskUpdate",
    "run_in_background",
    "Bash",
    "Skill tool",
    "Agent tool",
    "Read tool",
    "Workflow tool",
    "Edit and Write",
    "AskUserQuestion",
    "Grep",
    "Glob",
    "WebFetch",
    "--opus",
    "claude-opus-5-5",
];

/// Codex-only words, matched as whole words.
const CODEX_WORDS: [&str; 9] = [
    "apply_patch",
    "exec_command",
    "write_stdin",
    "update_plan",
    "spawn_agent",
    "wait_agent",
    "followup_task",
    "request_user_input",
    "$factory",
];

fn harness_words() -> impl Iterator<Item = &'static str> {
    CLAUDE_WORDS.into_iter().chain(CODEX_WORDS)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Returns the 1-based line of the first whole-word match of `word`, scanning every match.
fn find_word(text: &str, word: &str) -> Option<usize> {
    text.lines().enumerate().find_map(|(index, line)| {
        line.match_indices(word).find_map(|(at, _)| {
            let before = line[..at].chars().next_back();
            let after = line[at + word.len()..].chars().next();
            (!before.is_some_and(is_word_char) && !after.is_some_and(is_word_char))
                .then_some(index + 1)
        })
    })
}

/// Returns the first harness word in `text` and its 1-based line.
fn find_harness_word(text: &str) -> Option<(&'static str, usize)> {
    harness_words().find_map(|word| find_word(text, word).map(|line| (word, line)))
}

fn after_frontmatter(text: &str) -> &str {
    text.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(text, |(_, body)| body)
}

fn assert_no_harness_word(name: &str, text: &str) {
    if let Some((word, line)) = find_harness_word(after_frontmatter(text)) {
        panic!("{name} names the harness word {word} near body line {line}");
    }
}

#[test]
fn agent_files_name_no_harness_tool() {
    let agents = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/agents");
    let mut seen = 0;
    for entry in std::fs::read_dir(&agents).expect("read agents folder") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with("factory-") && name.ends_with(".md") {
            let text = std::fs::read_to_string(&path).expect("read agent file");
            assert_no_harness_word(&path.display().to_string(), &text);
            seen += 1;
        }
    }
    assert!(seen > 0, "no agent files found");
    for preset in factory_core::preset::PRESET_NAMES {
        let preset = factory_core::preset::embedded(preset).expect("embedded preset");
        for (file, text) in preset.agents {
            assert_no_harness_word(&format!("{}/{file}", preset.name), text);
        }
    }
}

fn assert_notes(map: &str, heading: &str) {
    assert!(map.starts_with(heading), "map must start with {heading}");
    for name in NOTE_NAMES {
        assert!(
            map.contains(&format!("### {name}\n")),
            "{heading} lacks the note {name}"
        );
    }
}

#[test]
fn maps_name_their_own_tools() {
    for word in ["TaskCreate", "run_in_background", "Skill"] {
        assert!(TOOL_MAP_CLAUDE.contains(word), "Claude map lacks {word}");
    }
    for word in ["update_plan", "exec_command", "$factory"] {
        assert!(TOOL_MAP_CODEX.contains(word), "Codex map lacks {word}");
    }
    assert_notes(TOOL_MAP_CLAUDE, "## Harness notes: Claude Code");
    assert_notes(TOOL_MAP_CODEX, "## Harness notes: Codex");
}

#[test]
fn claude_map_is_the_command_tail() {
    assert!(
        GLOBAL_COMMAND.ends_with(TOOL_MAP_CLAUDE),
        "edit tool-map-claude.md and the tail of global-command.md together"
    );
}

fn shared_command_text() -> &'static str {
    let body = GLOBAL_COMMAND
        .split_once("\n---\n")
        .map(|(_, body)| body)
        .expect("command frontmatter");
    body.strip_suffix(TOOL_MAP_CLAUDE)
        .expect("command ends with the Claude map")
}

#[test]
fn shared_text_names_no_harness_tool() {
    if let Some((word, line)) = find_harness_word(shared_command_text()) {
        panic!("shared command text names the harness word {word} near body line {line}");
    }
}

#[test]
fn codex_skill_has_no_claude_words() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let old = std::env::var("HOME").ok();
    std::env::set_var("HOME", tmp.path());
    let result = factory_cli::install::run_with_claude(
        None,
        false,
        std::ffi::OsStr::new("/nonexistent/claude"),
    );
    match old {
        Some(home) => std::env::set_var("HOME", home),
        None => std::env::remove_var("HOME"),
    }
    result.expect("install");
    let skill = factory_core::codex_skills_dir(tmp.path())
        .join("factory")
        .join("SKILL.md");
    let text = std::fs::read_to_string(skill).expect("Codex skill");
    let (shared, _) = text
        .split_once("## Harness notes: Codex")
        .expect("Codex map in the skill");
    for word in CLAUDE_WORDS {
        assert!(
            find_word(shared, word).is_none(),
            "Codex skill names the Claude word {word}"
        );
    }
}
