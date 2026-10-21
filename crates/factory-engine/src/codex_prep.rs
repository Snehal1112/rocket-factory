//! Prepares an agent spec for a Codex run: the agent text and preface in front of the prompt,
//! and the sandbox, network and writable roots chosen by role.

use factory_agent::AgentSpec;
use factory_git::Repo;

use crate::{Engine, EngineError};

const PREFACE: &str = "You run under Codex. Do not load or invoke skills and do not start subagents. Where this text names a skill, follow the rules written in short in this text instead.";
const SKILL_SENTENCE: &str = "The skill your instructions name is not available; the analysis in this prompt replaces its first steps, and the agent text gives the rules.";

/// Removes a leading `---` block, such as the frontmatter of an agent file.
pub(crate) fn strip_frontmatter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return rest[offset..].trim_start_matches('\n');
        }
    }
    text
}

fn has_role(role: &str, prefix: &str) -> bool {
    role.starts_with(prefix)
}

/// The Codex preface for a role. Committer, message and KB roles get the skill sentence too.
pub(crate) fn codex_preface(role: &str) -> String {
    if has_role(role, "committer") || has_role(role, "message") || has_role(role, "kb") {
        format!("{PREFACE} {SKILL_SENTENCE}")
    } else {
        PREFACE.to_string()
    }
}

/// The sandbox mode and network switch of a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SandboxPlan {
    pub mode: &'static str,
    pub network: bool,
}

pub(crate) fn sandbox_for(role: &str) -> SandboxPlan {
    if has_role(role, "builder") {
        SandboxPlan {
            mode: "workspace-write",
            network: true,
        }
    } else if has_role(role, "committer") || has_role(role, "message") || has_role(role, "kb") {
        SandboxPlan {
            mode: "workspace-write",
            network: false,
        }
    } else {
        SandboxPlan {
            mode: "read-only",
            network: false,
        }
    }
}

impl Engine<'_> {
    /// Rewrites a spec for the Codex runner. A missing agent file is an error, and the caller fails the run.
    pub(crate) fn prepare_for_codex(&self, spec: &mut AgentSpec) -> Result<(), EngineError> {
        if let Some(name) = spec.agent.clone() {
            let file = self
                .agent_files
                .iter()
                .find(|f| f.name == name || f.name == format!("{name}.md"))
                .ok_or_else(|| EngineError::Msg(format!("agent file {name} not found")))?;
            spec.agent_prompt = Some(format!(
                "{}\n\n{}",
                codex_preface(&spec.role),
                strip_frontmatter(&file.content)
            ));
            spec.agent = None;
        }
        spec.max_turns = None;
        let plan = sandbox_for(&spec.role);
        spec.sandbox = Some(plan.mode.to_string());
        spec.network = plan.network;
        spec.writable_roots.clear();
        if has_role(&spec.role, "committer") || has_role(&spec.role, "message") {
            spec.writable_roots.push(self.repo.common_git_dir());
        }
        if has_role(&spec.role, "kb") {
            if let Some(kb_dir) = self.config.kb_dir() {
                if self.config.kb_auto_commit {
                    if let Ok(kb_repo) = Repo::open(&kb_dir) {
                        spec.writable_roots.push(kb_repo.common_git_dir());
                    }
                }
                spec.cwd = kb_dir;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_frontmatter_removes_only_a_leading_block() {
        assert_eq!(strip_frontmatter("---\nname: x\n---\nBody\n---\nmore"), "Body\n---\nmore");
        assert_eq!(strip_frontmatter("Body\n---\nx\n---\n"), "Body\n---\nx\n---\n");
        assert_eq!(strip_frontmatter("---\nno end"), "---\nno end");
    }

    #[test]
    fn the_preface_adds_the_skill_sentence_for_committer_message_and_kb() {
        for role in ["committer", "message", "kb"] {
            let p = codex_preface(role);
            assert!(p.starts_with(PREFACE), "{role}");
            assert!(p.ends_with(SKILL_SENTENCE), "{role}");
        }
        for role in ["builder", "reviewer"] {
            assert_eq!(codex_preface(role), PREFACE, "{role}");
        }
    }

    #[test]
    fn sandbox_follows_the_role() {
        let table = [
            ("builder", "workspace-write", true),
            ("committer", "workspace-write", false),
            ("message", "workspace-write", false),
            ("kb", "workspace-write", false),
            ("reviewer", "read-only", false),
            ("something-else", "read-only", false),
        ];
        for (role, mode, network) in table {
            assert_eq!(sandbox_for(role), SandboxPlan { mode, network }, "{role}");
        }
    }
}
