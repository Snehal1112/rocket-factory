//! The agent text a job runs on, and the `agents.json` that carries it to `claude --agents`.

use std::path::PathBuf;

use factory_core::{agent_files_rel_dir, agents_file, agents_json, write_atomic, AgentFile};

use crate::ports::core_err;
use crate::{Engine, EngineError};

impl Engine<'_> {
    /// The resolved agent files, each replaced by the base branch's text when the base branch tracks it.
    /// A tracked file that cannot be read stops the run, the resolved text never stands in for it.
    pub(crate) fn effective_agent_files(&self) -> Result<Vec<AgentFile>, EngineError> {
        let rel_dir = agent_files_rel_dir();
        self.agent_files
            .iter()
            .map(|file| {
                let rel = rel_dir
                    .join(&file.name)
                    .to_string_lossy()
                    .replace('\\', "/");
                let tracked = self
                    .repo
                    .file_at_branch(self.repo.base_branch(), &rel)
                    .map_err(|e| {
                        EngineError::Msg(format!("cannot read tracked agent file {rel}: {e}"))
                    })?;
                Ok(AgentFile {
                    name: file.name.clone(),
                    content: tracked.unwrap_or_else(|| file.content.clone()),
                })
            })
            .collect()
    }

    /// Writes `agents.json` in the job folder when its text differs, and returns the path.
    pub(crate) fn write_agents_json(&self) -> Result<PathBuf, EngineError> {
        // Every name becomes a key, so the same check as for written files applies.
        self.check_agent_names()?;
        let path = agents_file(&self.job_dir);
        let text = agents_json(&self.effective_agent_files()?);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == text) {
            return Ok(path);
        }
        write_atomic(&path, text.as_bytes()).map_err(core_err)?;
        Ok(path)
    }
}
