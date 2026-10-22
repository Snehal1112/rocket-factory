//! `factory install`: writes the global `/factory` slash command and Codex skill.

use std::path::{Path, PathBuf};

pub use factory_core::GLOBAL_COMMAND;
use factory_core::{claude_commands_dir, codex_skills_dir, write_atomic};

use crate::CliError;

const FILE_NAME: &str = "factory.md";
const SKILL_DIR_NAME: &str = "factory";
const SKILL_FILE_NAME: &str = "SKILL.md";
const SKILL_DESCRIPTION: &str = "Run the software factory workflow for a project: brainstorm, plan, then build, check, review, and merge through the factory driver.";

/// Installs into `dir` when given, else installs the Claude command and Codex skill under `$HOME`.
pub fn run(dir: Option<&Path>, force: bool) -> Result<String, CliError> {
    if let Some(folder) = dir {
        return install_file(folder, FILE_NAME, GLOBAL_COMMAND, force);
    }
    let home = user_home()?;
    let claude = install_file(
        &claude_commands_dir(&home),
        FILE_NAME,
        GLOBAL_COMMAND,
        force,
    )?;
    let codex_folder = codex_skills_dir(&home).join(SKILL_DIR_NAME);
    let codex_target = codex_folder.join(SKILL_FILE_NAME);
    let skill = codex_skill().map_err(|error| {
        CliError::Refused(format!("{claude}\nCodex skill install failed: {error}"))
    })?;
    if !force {
        match std::fs::read_to_string(&codex_target) {
            Ok(existing) if existing != skill => {
                return Ok(format!(
                    "{claude}\nnote: Codex skill not installed: {} exists and differs; use --force to replace it",
                    codex_target.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::Refused(format!(
                    "{claude}\nCodex skill install failed: cannot read {}: {error}",
                    codex_target.display()
                )));
            }
        }
    }
    let codex = install_file(
        &codex_folder,
        SKILL_FILE_NAME,
        &skill,
        force,
    )
    .map_err(|error| CliError::Refused(format!("{claude}\nCodex skill install failed: {error}")))?;
    Ok(format!("{claude}\n{codex}"))
}

/// The Claude commands folder, used by `factory upgrade`.
pub fn commands_folder(dir: Option<&Path>) -> Result<PathBuf, CliError> {
    match dir {
        Some(d) => Ok(d.to_path_buf()),
        None => Ok(claude_commands_dir(&user_home()?)),
    }
}

/// Updates the Codex skill when the command folder is the standard `~/.claude/commands` path.
pub fn upgrade_codex_skill(commands_dir: &Path, dry_run: bool) -> Result<Option<String>, CliError> {
    let Some(home) = commands_dir.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if claude_commands_dir(home).as_path() != commands_dir {
        return Ok(None);
    }
    Ok(Some(upgrade_file(
        &codex_skills_dir(home).join(SKILL_DIR_NAME),
        SKILL_FILE_NAME,
        &codex_skill()?,
        dry_run,
    )?))
}

/// Updates one command file in an explicit folder. Kept for callers that manage one integration.
pub fn upgrade_into(folder: &Path, dry_run: bool) -> Result<String, CliError> {
    upgrade_file(folder, FILE_NAME, GLOBAL_COMMAND, dry_run)
}

fn user_home() -> Result<PathBuf, CliError> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| CliError::Refused("$HOME is not set, pass --dir".into()))?;
    Ok(PathBuf::from(home))
}

fn codex_skill() -> Result<String, CliError> {
    build_codex_skill(GLOBAL_COMMAND)
}

fn replace_required(body: String, from: &str, to: &str) -> Result<String, CliError> {
    if !body.contains(from) {
        let head: String = from.chars().take(60).collect();
        return Err(CliError::Refused(format!(
            "global command text changed: {head}"
        )));
    }
    Ok(body.replace(from, to))
}

fn build_codex_skill(command: &str) -> Result<String, CliError> {
    let body = command
        .split_once("\n---\n")
        .map(|(_, body)| body.trim_start())
        .unwrap_or(command);
    let mut body = body
        .strip_prefix("# Factory\n\nArguments: $ARGUMENTS\n")
        .unwrap_or(body)
        .replace("/factory", "$factory");
    for (claude, codex) in [
        (
            "Keep a visible task list (TaskCreate and TaskUpdate) for steps 1 to 6 and update it at each milestone.",
            "Keep a visible task list for steps 1 to 6 and update it at each milestone.",
        ),
        (
            "Invoke `superpowers:brainstorming` yourself, with the user, in this session, and follow it fully. Spec quality comes from the interview, so do not shorten it.",
            "Interview the user about the design before writing the spec. Cover the goal, alternatives, tradeoffs, edge cases and testing. Spec quality comes from the interview, so do not shorten it.",
        ),
        (
            "These overrides win where the skill says otherwise:",
            "Interview rules:",
        ),
        (
            "Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words. The user asked for this because open questions were too hard to answer. It replaces the skill's open first question, its \"a single sentence with no list of example answers\", its \"Offer a short menu only when they're stuck\" and its Red Flags row \"Options steer\".",
            "Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words. Ask one question at a time and record each answer in the spec.",
        ),
        (
            "If the user takes the skill's opt-out and skips the questions,",
            "If the user skips the questions,",
        ),
        (
            "The skill goes from approval straight to `superpowers:writing-plans`. Here the commit after the spec approval comes first (the last bullet of this step), then step 2.",
            "After spec approval, commit the approved spec as the last bullet of this step, then write the plans in step 2.",
        ),
        (
            "The skill's builder check reads the spec before the user sees it. It is the one subagent this step uses: do the interview and the writing yourself, and do not do the builder's or reviewer's work.",
            "Before showing the spec to the user, review it against the quality bar and check that a builder can implement it without asking questions. Do the interview and writing yourself; do not do the builder's or reviewer's work.",
        ),
        (
            "commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec",
            "commit with an exact pathspec",
        ),
        (
            "Invoke `superpowers:writing-plans` yourself and write the plans in its style. The factory driver is the execution method: skip the skill's execution handoff question and do not offer to run the plans yourself.",
            "Write the plans using the rules below. The factory driver is the execution method; do not offer to run the plans yourself.",
        ),
        (
            "the header is the skill's template, with the repo-relative spec path (`docs/superpowers/specs/<date>-<slug>-design.md`) as its Spec line and its line for agentic workers, which offers a choice of executor, replaced by this executor line: `> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.`",
            "the header names the plan and includes `> **Spec:** docs/superpowers/specs/<date>-<slug>-design.md` and this executor line: `> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.`",
        ),
        (
            "Review Focus as the skill defines it, as a numbered list:",
            "Review Focus as a numbered list:",
        ),
        (
            "the skill's plan self-review is run and its findings fixed",
            "review each plan against these rules and fix every finding",
        ),
        (
            "The Bash output shows a line starting `needs consent:`",
            "The command output shows a line starting `needs consent:`",
        ),
        (
            "run `<factory> --project <name> run <id> --series` as a Bash command with `run_in_background` set to true, and tell the user in one line that the factory is running. Then stop. Do not poll, do not sleep, do not read logs. The supervisor or the harness reports the end.",
            "start `<factory> --project <name> run <id> --series` as a long-running command using the available command execution tool. If it returns a still-running session or process handle, keep the handle for harness notification when it finishes. Tell the user in one line that the factory is running. Then stop. Do not poll, do not sleep, do not read logs. If it returns a final exit/completion result, including a non-zero exit, continue to step 6 immediately. Do not report a completed or failed process as running.",
        ),
        (
            "When the run ends (the harness notifies you for a background run; with the supervisor the user asks, or the dashboard shows it),",
            "When the run ends (the command tool returns a final result, the harness notifies you that a live session ended, the user asks, or the dashboard shows it),",
        ),
    ] {
        body = replace_required(body, claude, codex)?;
    }
    let opus_instruction = "A feature may start or end with the single token `--opus`. Remove that token from the text and pass `--model opus` to `<factory> --project <name> new`. It may likewise start or end with the single token `--one-job`, in any order together with `--opus` (either token may be at the start or the end), handled the same way: remove it from the feature text and from the slug, and pass `--one-job` to `<factory> --project <name> new`.";
    body = replace_required(body, opus_instruction, "`--opus` is a Claude-only option. If it appears at the start or end of a feature request, explain that this Codex skill does not accept it and ask the user to select a Codex model in the project configuration; stop before creating a draft or job. A feature may start or end with the single token `--one-job`: remove it from the feature text and slug, and pass `--one-job` to `<factory> --project <name> new`.")?;
    let new_instruction = "adding `--model opus` when `--opus` was given and `--one-job` when `--one-job` was given";
    body = replace_required(body, new_instruction, "adding `--one-job` when `--one-job` was given")?;
    Ok(format!(
        "---\nname: factory\ndescription: {SKILL_DESCRIPTION}\n---\n\n# Factory\n\nUse the user's accompanying request as the factory arguments.\n\n{body}"
    ))
}

fn install_file(
    folder: &Path,
    file_name: &str,
    content: &str,
    force: bool,
) -> Result<String, CliError> {
    let target = folder.join(file_name);
    match std::fs::read_to_string(&target) {
        Ok(existing) if existing == content => {
            return Ok(format!("already installed: {}", target.display()));
        }
        Ok(_) if !force => {
            return Err(CliError::Refused(format!(
                "{} exists and differs from the shipped command, use --force to replace it",
                target.display()
            )));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(CliError::Refused(format!(
                "cannot read {}: {e}",
                target.display()
            )))
        }
    }
    write_atomic(&target, content.as_bytes())?;
    Ok(format!("installed {}", target.display()))
}

fn upgrade_file(
    folder: &Path,
    file_name: &str,
    content: &str,
    dry_run: bool,
) -> Result<String, CliError> {
    let target = folder.join(file_name);
    let backup = folder.join(format!("{file_name}.bak"));
    let prefix = if dry_run { "would " } else { "" };
    match std::fs::read(&target) {
        Ok(existing) if existing == content.as_bytes() => {
            Ok(format!("already installed: {}", target.display()))
        }
        Ok(existing) => {
            if !dry_run {
                write_atomic(&backup, &existing).map_err(|e| {
                    CliError::Refused(format!("cannot save {}: {e}", backup.display()))
                })?;
                write_atomic(&target, content.as_bytes())?;
            }
            Ok(format!(
                "{prefix}updated {} (old copy saved as {})",
                target.display(),
                backup.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !dry_run {
                write_atomic(&target, content.as_bytes())?;
            }
            Ok(format!("{prefix}installed {}", target.display()))
        }
        Err(e) => Err(CliError::Refused(format!(
            "cannot read {}: {e}",
            target.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_command_builds_a_codex_skill() {
        let skill = codex_skill().expect("skill");
        assert!(skill.contains("name: factory"));
    }

    #[test]
    fn codex_skill_reports_a_changed_command() {
        let error = build_codex_skill("# Factory\n\nnothing the rewrite expects\n")
            .expect_err("changed command must be an error");
        assert!(
            error.to_string().contains("global command text changed"),
            "{error}"
        );
    }
}
