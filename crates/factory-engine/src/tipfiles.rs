use factory_core::BaseTipFiles;
use factory_git::Repo;

/// Reads files from the tip of the repo's base branch.
pub struct RepoTip<'a>(pub &'a Repo);

impl BaseTipFiles for RepoTip<'_> {
    fn read_file(&self, rel: &str) -> Result<Option<String>, String> {
        self.0.file_at_branch(self.0.base_branch(), rel).map_err(|e| e.to_string())
    }

    fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
        self.0.dir_at_branch(self.0.base_branch(), rel).map_err(|e| e.to_string())
    }
}
