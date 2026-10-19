use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MAX_PLANS: usize = 6;

#[derive(Debug, thiserror::Error)]
pub enum SeriesError {
    #[error("plan file not found: {0}")]
    Missing(PathBuf),
    #[error("{0}: the last line must be 'Next plan: <path|none>'")]
    NoNextLine(PathBuf),
    #[error("{path}: a plan needs 1 to 3 tasks, found {found}")]
    TaskCount { path: PathBuf, found: usize },
    #[error("cycle in the Next plan chain at {0}")]
    Cycle(PathBuf),
    #[error("the chain has more than {MAX_PLANS} plans, stopped at {0}")]
    TooMany(PathBuf),
    #[error("plan file is not in the chain: {0}")]
    Orphan(PathBuf),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct SeriesPlan {
    pub path: PathBuf,
    pub tasks: Vec<u32>,
    pub next: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Series {
    pub plans: Vec<SeriesPlan>,
}

/// A relative target is looked up from the plan's own checkout first, then the driver root, then beside the plan.
fn resolve(root: &Path, from: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        return PathBuf::from(rel);
    }
    if let Some(found) = from.ancestors().skip(1).map(|dir| dir.join(rel)).find(|p| p.is_file()) {
        return found;
    }
    let a = root.join(rel);
    if a.exists() {
        return a;
    }
    from.parent().map(|p| p.join(rel)).filter(|p| p.exists()).unwrap_or(a)
}

fn read_plan(root: &Path, path: &Path, max_tasks: Option<usize>) -> Result<SeriesPlan, SeriesError> {
    let text = std::fs::read_to_string(path).map_err(|_| SeriesError::Missing(path.to_path_buf()))?;
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    let target = last
        .strip_prefix("Next plan:")
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.contains(char::is_whitespace))
        .ok_or_else(|| SeriesError::NoNextLine(path.to_path_buf()))?;
    let tasks: Vec<u32> = crate::tasks::parse_checkbox_tasks(&text).into_iter().map(|t| t.n).collect();
    if tasks.is_empty() || max_tasks.is_some_and(|max| tasks.len() > max) {
        return Err(SeriesError::TaskCount { path: path.to_path_buf(), found: tasks.len() });
    }
    let next = (target != "none").then(|| resolve(root, path, target));
    Ok(SeriesPlan { path: path.to_path_buf(), tasks, next })
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Validates the `Next plan:` chain from `start` (a plan file, or a folder of `plan-*.md` files).
pub fn parse_series(root: &Path, start: &Path) -> Result<Series, SeriesError> {
    parse_series_with(root, start, Some(3))
}

/// `max_tasks` of `None` lifts the upper bound on tasks per plan. A plan still needs at least one task.
pub fn parse_series_with(root: &Path, start: &Path, max_tasks: Option<usize>) -> Result<Series, SeriesError> {
    let (first, folder_names) = if start.is_dir() {
        let mut names: Vec<PathBuf> = std::fs::read_dir(start)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("plan-") && n.ends_with(".md"))
            })
            .collect();
        names.sort();
        let first = names.first().cloned().ok_or_else(|| SeriesError::Missing(start.join("plan-01-*.md")))?;
        (first, Some(names))
    } else {
        (start.to_path_buf(), None)
    };
    let mut plans = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(first);
    while let Some(path) = current {
        if !seen.insert(canon(&path)) {
            return Err(SeriesError::Cycle(path));
        }
        if plans.len() == MAX_PLANS {
            return Err(SeriesError::TooMany(path));
        }
        let plan = read_plan(root, &path, max_tasks)?;
        current = plan.next.clone();
        plans.push(plan);
    }
    if let Some(names) = folder_names {
        for name in names {
            if !seen.contains(&canon(&name)) {
                return Err(SeriesError::Orphan(name));
            }
        }
    }
    Ok(Series { plans })
}
