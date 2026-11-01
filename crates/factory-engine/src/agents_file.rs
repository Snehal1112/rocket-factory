//! The agent text a job runs on, and the `agents.json` that carries it to `claude --agents`.

use std::path::{Path, PathBuf};

use factory_core::{agent_files_rel_dir, agents_file, agents_json, write_atomic, AgentFile};
use factory_git::Repo;

use crate::ports::core_err;
use crate::{Driver, Engine, EngineError};

/// The resolved agent files, each replaced by the base branch's text when the base branch tracks it.
/// A tracked file that cannot be read stops the run, the resolved text never stands in for it.
fn effective_files(repo: &Repo, files: &[AgentFile]) -> Result<Vec<AgentFile>, EngineError> {
    let rel_dir = agent_files_rel_dir();
    files
        .iter()
        .map(|file| {
            let rel = rel_dir
                .join(&file.name)
                .to_string_lossy()
                .replace('\\', "/");
            let tracked = repo.file_at_branch(repo.base_branch(), &rel).map_err(|e| {
                EngineError::Msg(format!("cannot read tracked agent file {rel}: {e}"))
            })?;
            Ok(AgentFile {
                name: file.name.clone(),
                content: tracked.unwrap_or_else(|| file.content.clone()),
            })
        })
        .collect()
}

impl Driver {
    /// The resolved agent files with the base branch's tracked text, for work outside a job.
    pub(crate) fn effective_agent_files(&self) -> Result<Vec<AgentFile>, EngineError> {
        effective_files(self.repo(), self.agent_files())
    }
}

impl Engine<'_> {
    /// The resolved agent files with the base branch's tracked text.
    pub(crate) fn effective_agent_files(&self) -> Result<Vec<AgentFile>, EngineError> {
        effective_files(self.repo, &self.agent_files)
    }

    /// Writes `agents.json` in the job folder when its text differs, and returns the path.
    pub(crate) fn write_agents_json(&self) -> Result<PathBuf, EngineError> {
        // Every name becomes a key, so the same check as for written files applies.
        self.check_agent_names()?;
        let path = agents_file(&self.job_dir);
        write_agents_json_at(&path, &self.effective_agent_files()?).map_err(core_err)?;
        Ok(path)
    }
}

/// Writes `agents.json` for `files` at `path` when its text differs.
pub(crate) fn write_agents_json_at(
    path: &Path,
    files: &[AgentFile],
) -> Result<(), factory_core::CoreError> {
    let text = agents_json(files);
    if std::fs::read_to_string(path).is_ok_and(|current| current == text) {
        return Ok(());
    }
    write_atomic(path, text.as_bytes())
}
