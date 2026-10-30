//! Prepares an agent spec for a Codex run: the agent text and preface in front of the prompt,
//! and the sandbox, network and writable roots chosen by role.

use factory_agent::AgentSpec;
use factory_core::{codex_worktree_skills_dir, Config};
use factory_git::Repo;

use crate::codex_skills::{allowed, codex_skill_args, discover_skills, FoundSkill};
use crate::{Engine, EngineError};

const PREFACE: &str = "You run under Codex. Do not load or invoke skills and do not start subagents. Where this text names a skill, follow the rules written in short in this text instead.";
const SKILLS_PREFACE: &str = "You run under Codex. Use the skills in your skill catalog when your instructions call for them. Do not start subagents. Where this text names a skill that is not in your catalog, follow the rules written in short in this text instead.";
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

/// The Codex preface. With allowed skills the catalog may be used, subagents stay off either way.
pub(crate) fn codex_preface(has_skills: bool) -> String {
    if has_skills {
        SKILLS_PREFACE.to_string()
    } else {
        PREFACE.to_string()
    }
}

/// True when the role names a skill and no allowed found skill belongs to it.
fn skill_gap(role: &str, found: &[FoundSkill], allow: &[String]) -> bool {
    let Some(named) = Config::named_skill_for(role) else {
        return false;
    };
    let belongs = |s: &FoundSkill| {
        if s.group.is_empty() {
            s.name == named
        } else {
            s.group == named
        }
    };
    !found.iter().any(|s| belongs(s) && allowed(s, allow))
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
        let mut roots = self.codex_skill_roots.clone();
        let worktree = self
            .repo
            .worktree_root_of(&spec.cwd)
            .unwrap_or_else(|| spec.cwd.clone());
        roots.push(codex_worktree_skills_dir(&worktree));
        let found = discover_skills(&roots);
        let allow = self.config.skill_allow_for(&spec.role);
        spec.codex_config = codex_skill_args(&found, &allow);
        if let Some(name) = spec.agent.clone() {
            let file = self
                .agent_files
                .iter()
                .find(|f| f.name == name || f.name == format!("{name}.md"))
                .ok_or_else(|| EngineError::Msg(format!("agent file {name} not found")))?;
            let has_skills = found.iter().any(|s| allowed(s, &allow));
            let mut preface = codex_preface(has_skills);
            if skill_gap(&spec.role, &found, &allow) {
                preface = format!("{preface} {SKILL_SENTENCE}");
            }
            spec.agent_prompt = Some(format!("{preface}\n\n{}", strip_frontmatter(&file.content)));
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
        assert_eq!(
            strip_frontmatter("---\nname: x\n---\nBody\n---\nmore"),
            "Body\n---\nmore"
        );
        assert_eq!(
            strip_frontmatter("Body\n---\nx\n---\n"),
            "Body\n---\nx\n---\n"
        );
        assert_eq!(strip_frontmatter("---\nno end"), "---\nno end");
    }

    fn found(group: &str, name: &str) -> FoundSkill {
        FoundSkill {
            group: group.to_string(),
            name: name.to_string(),
            path: std::path::PathBuf::from(format!("/p/{group}/{name}")),
        }
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn preface_with_skills_allows_the_catalog_and_forbids_subagents() {
        let p = codex_preface(true);
        assert!(p.contains("skill catalog"), "{p}");
        assert!(p.contains("Do not start subagents"), "{p}");
        assert!(!p.contains("Do not load or invoke skills"), "{p}");
    }

    #[test]
    fn preface_without_skills_keeps_the_ban() {
        assert_eq!(codex_preface(false), PREFACE);
    }

    #[test]
    fn skill_gap_is_true_only_for_a_missing_named_skill() {
        let commit = found("dev-workflow-skills", "1-git-commit");
        let kb = found("", "kb-refresh");
        let other = found("superpowers", "tdd");
        assert!(!skill_gap("builder", &[], &list(&["superpowers"])));
        assert!(!skill_gap(
            "committer",
            std::slice::from_ref(&commit),
            &list(&["dev-workflow-skills"])
        ));
        assert!(skill_gap(
            "committer",
            std::slice::from_ref(&other),
            &list(&["dev-workflow-skills"])
        ));
        assert!(skill_gap("message", &[], &list(&["dev-workflow-skills"])));
        assert!(skill_gap("committer", &[commit], &[]));
        assert!(!skill_gap("kb", &[kb], &list(&["kb-refresh"])));
        assert!(skill_gap("kb", &[other], &list(&["kb-refresh"])));
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
