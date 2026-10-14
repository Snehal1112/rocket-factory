use crate::error::Result;
use crate::repo::{short, Repo};
use crate::stage::staged_entries;
use crate::worktree::Worktree;
use git2::{Diff, DiffFormat, DiffStatsFormat, Repository, Sort};

/// The output of the commit skill's step 1 commands for a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedReport {
    /// What `git status` would print for the staged changes.
    pub status: String,
    /// `git diff --staged --stat`.
    pub stat: String,
    /// `git diff --staged`.
    pub diff: String,
    /// `git log --oneline -N`, newest first, as `<short sha> <subject>`.
    pub log: Vec<String>,
}

/// Renders a diff as patch text, with the `+`, `-` and space prefixes git prints.
fn render_patch(diff: &Diff<'_>) -> Result<String> {
    let mut out: Vec<u8> = Vec::new();
    diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
        if matches!(line.origin(), '+' | '-' | ' ') {
            out.push(line.origin() as u8);
        }
        out.extend_from_slice(line.content());
        true
    })?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn recent_log(inside: &Repository, count: usize) -> Result<Vec<String>> {
    let mut walk = inside.revwalk()?;
    walk.push_head()?;
    walk.set_sorting(Sort::TOPOLOGICAL)?;
    let mut lines = Vec::new();
    for oid in walk.take(count) {
        let commit = inside.find_commit(oid?)?;
        let id = commit.id().to_string();
        lines.push(format!("{} {}", short(&id), commit.summary().unwrap_or("")));
    }
    Ok(lines)
}

impl Repo {
    /// Computes, through `git2`, what the skill's step 1 commands print for the
    /// staged changes of a job worktree, so the driver can put them in the
    /// committer prompt.
    pub fn staged_report(&self, wt: &Worktree, log_count: usize) -> Result<StagedReport> {
        let inside = Repository::open(&wt.path)?;
        let head = inside.head()?;
        let branch = head.shorthand().unwrap_or("HEAD").to_string();
        let mut status = format!("On branch {branch}\n");
        let entries = staged_entries(&inside)?;
        if entries.is_empty() {
            status.push_str("nothing to commit\n");
        } else {
            status.push_str("Changes to be committed:\n");
            for (label, path) in &entries {
                status.push_str(&format!("\t{label:<12}{path}\n"));
            }
        }
        let head_tree = head.peel_to_commit()?.tree()?;
        let index = inside.index()?;
        let diff = inside.diff_tree_to_index(Some(&head_tree), Some(&index), None)?;
        let stat = if entries.is_empty() {
            String::new()
        } else {
            let buf = diff.stats()?.to_buf(DiffStatsFormat::FULL, 80)?;
            String::from_utf8_lossy(&buf[..]).into_owned()
        };
        Ok(StagedReport { status, stat, diff: render_patch(&diff)?, log: recent_log(&inside, log_count)? })
    }
}
