use crate::error::Result;
use crate::repo::{paths_of, short, Repo};
use git2::{DiffFormat, DiffLine, DiffStatsFormat};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

/// Paths and counts of the files `diff_to_files` wrote. The diff text itself
/// is never returned, so it never enters an LLM context by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFiles {
    pub diff: PathBuf,
    pub stat: PathBuf,
    pub names: PathBuf,
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl DiffFiles {
    /// Changed lines, the number the diff size cap is checked against.
    pub fn changed_lines(&self) -> usize {
        self.insertions + self.deletions
    }
}

fn write_line(out: &mut impl Write, line: &DiffLine<'_>) -> io::Result<()> {
    if matches!(line.origin(), '+' | '-' | ' ') {
        out.write_all(&[line.origin() as u8])?;
    }
    out.write_all(line.content())
}

/// Counts of a diff, without writing any file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffCounts {
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl Repo {
    /// File and line counts between two commits. Nothing is written.
    pub fn diff_counts(&self, base: &str, head: &str) -> Result<DiffCounts> {
        let diff = self.tree_diff(base, head)?;
        let stats = diff.stats()?;
        Ok(DiffCounts {
            files: stats.files_changed(),
            insertions: stats.insertions(),
            deletions: stats.deletions(),
        })
    }

    /// Writes `review-<base7>..<head7>.diff`, `.stat` and `.names` into `dir`.
    pub fn diff_to_files(&self, base: &str, head: &str, dir: &Path) -> Result<DiffFiles> {
        let base_id = self.commit_of(base)?.id().to_string();
        let head_id = self.commit_of(head)?.id().to_string();
        let diff = self.tree_diff(base, head)?;
        fs::create_dir_all(dir)?;
        let stem = format!("review-{}..{}", short(&base_id), short(&head_id));
        let diff_path = dir.join(format!("{stem}.diff"));
        let stat_path = dir.join(format!("{stem}.stat"));
        let names_path = dir.join(format!("{stem}.names"));

        let mut out = BufWriter::new(fs::File::create(&diff_path)?);
        let mut failure: Option<io::Error> = None;
        let printed = diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
            match write_line(&mut out, &line) {
                Ok(()) => true,
                Err(e) => {
                    failure = Some(e);
                    false
                }
            }
        });
        if let Some(e) = failure {
            return Err(e.into());
        }
        printed?;
        out.flush()?;

        let stats = diff.stats()?;
        let stat_buf = stats.to_buf(DiffStatsFormat::FULL, 80)?;
        fs::write(&stat_path, &stat_buf[..])?;

        let mut names = String::new();
        for path in paths_of(&diff) {
            names.push_str(&path);
            names.push('\n');
        }
        fs::write(&names_path, names)?;

        Ok(DiffFiles {
            diff: diff_path,
            stat: stat_path,
            names: names_path,
            files: stats.files_changed(),
            insertions: stats.insertions(),
            deletions: stats.deletions(),
        })
    }
}
