//! The commit notes the builders write, saved under the job folder for the merge message.

use std::path::Path;

use factory_core::{check_note, fallback_note, parse_commit_note, CommitNote};

use crate::{Engine, EngineError};

/// The folder under the job folder that holds the notes.
pub const NOTES_DIR: &str = "commit-notes";

/// The notes of one job, read back from disk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    /// The note of a whole-plan (legacy) build.
    pub build: Option<CommitNote>,
    /// Task notes, sorted by task number.
    pub tasks: Vec<(u32, CommitNote)>,
    /// Fix and rework round notes, sorted by round.
    pub rounds: Vec<(u32, CommitNote)>,
}

impl Notes {
    /// True when no note exists.
    pub fn is_empty(&self) -> bool {
        self.build.is_none() && self.tasks.is_empty() && self.rounds.is_empty()
    }
}

/// Reads every note under `<job_dir>/commit-notes`. Unreadable or unparsable files are skipped.
pub fn read_notes(job_dir: &Path) -> Notes {
    let mut notes = Notes::default();
    let Ok(entries) = std::fs::read_dir(job_dir.join(NOTES_DIR)) else {
        return notes;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".md") else {
            continue;
        };
        let Some(note) = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|text| CommitNote::from_file_text(&text))
        else {
            continue;
        };
        if stem == "build" {
            notes.build = Some(note);
        } else if let Some(n) = stem.strip_prefix("task-").and_then(|n| n.parse().ok()) {
            notes.tasks.push((n, note));
        } else if let Some(n) = stem.strip_prefix("round-").and_then(|n| n.parse().ok()) {
            notes.rounds.push((n, note));
        }
    }
    notes.tasks.sort_by_key(|(n, _)| *n);
    notes.rounds.sort_by_key(|(n, _)| *n);
    notes
}

impl Engine<'_> {
    /// Saves the checked note of a builder reply as `commit-notes/<file>`. A missing or refused
    /// note is replaced by a fallback and one ledger ruling. Only a ledger error is returned.
    pub fn save_commit_note(
        &self,
        file: &str,
        label: &str,
        reply: &str,
        fallback_title: &str,
        files: &[String],
    ) -> Result<(), EngineError> {
        let checked = match parse_commit_note(reply) {
            None => Err("missing".to_string()),
            Some(note) => check_note(&note),
        };
        let note = match checked {
            Ok(note) => note,
            Err(why) => {
                self.ruling(&format!("commit note for {label}: {why}, fallback used"))?;
                fallback_note(fallback_title, files)
            }
        };
        let dir = self.job_dir.join(NOTES_DIR);
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(file), note.to_file_text()));
        if let Err(e) = written {
            self.ruling(&format!("commit note for {label} not saved: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_text(subject: &str) -> String {
        CommitNote {
            subject: subject.to_string(),
            body: "- Do it".to_string(),
        }
        .to_file_text()
    }

    #[test]
    fn read_notes_sorts_by_number_and_skips_bad_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join(NOTES_DIR);
        std::fs::create_dir_all(&dir).expect("mkdir");
        for name in [
            "task-10.md",
            "task-2.md",
            "round-3.md",
            "round-2.md",
            "build.md",
        ] {
            std::fs::write(dir.join(name), note_text(name)).expect("write");
        }
        std::fs::write(dir.join("task-4.md"), "").expect("write empty");
        let notes = read_notes(tmp.path());
        let tasks: Vec<u32> = notes.tasks.iter().map(|(n, _)| *n).collect();
        let rounds: Vec<u32> = notes.rounds.iter().map(|(n, _)| *n).collect();
        assert_eq!(tasks, vec![2, 10]);
        assert_eq!(rounds, vec![2, 3]);
        assert_eq!(notes.build.expect("build").subject, "build.md");
    }

    #[test]
    fn an_absent_folder_gives_no_notes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(read_notes(tmp.path()).is_empty());
    }
}
