use factory_git::Repo;

use crate::ports::git_err;
use crate::EngineError;

/// The most out-of-scope paths named in a reason.
const MAX_LISTED: usize = 10;

/// The reason text for a job that changed files outside the project folder, or `None` when it did not.
/// A repo without a scope has no such folder, so it never has a violation.
pub fn scope_violation(repo: &Repo, base: &str, head: &str) -> Result<Option<String>, EngineError> {
    let Some(sub) = repo.subpath() else {
        return Ok(None);
    };
    let changed = repo.changed_paths(base, head).map_err(git_err)?;
    let outside = repo.out_of_scope(&changed);
    if outside.is_empty() {
        return Ok(None);
    }
    let mut reason = format!(
        "the job changed files outside {}: {}",
        sub.display(),
        outside
            .iter()
            .take(MAX_LISTED)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    if outside.len() > MAX_LISTED {
        reason.push_str(&format!(" and {} more", outside.len() - MAX_LISTED));
    }
    Ok(Some(reason))
}
