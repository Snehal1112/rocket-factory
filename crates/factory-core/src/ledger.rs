use crate::error::CoreError;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Dispatched,
    ImplementerDone,
    Complete,
}

impl TaskState {
    fn label(self) -> &'static str {
        match self {
            TaskState::Dispatched => "dispatched",
            TaskState::ImplementerDone => "implementer done",
            TaskState::Complete => "complete",
        }
    }
}

/// An append-only `progress.md`. Only the driver writes it.
#[derive(Debug, Clone)]
pub struct Ledger {
    pub path: PathBuf,
}

fn single_line(text: &str) -> Result<&str, CoreError> {
    let t = text.trim();
    if t.is_empty() || t.contains('\n') || t.contains('\r') {
        return Err(CoreError::LedgerLine);
    }
    Ok(t)
}

impl Ledger {
    pub fn new(path: impl Into<PathBuf>) -> Ledger {
        Ledger { path: path.into() }
    }

    /// Writes the header only when the file does not exist yet.
    pub fn create(&self, header: &str) -> Result<(), CoreError> {
        if self.path.exists() {
            return Ok(());
        }
        let header = single_line(header)?;
        self.append(&format!("{header}\n"))
    }

    pub fn task(&self, n: u32, state: TaskState, detail: Option<&str>) -> Result<(), CoreError> {
        let mut line = format!("Task {n}: {}", state.label());
        if let Some(d) = detail {
            line.push_str(&format!(" ({})", single_line(d)?));
        }
        self.append(&format!("{line}\n"))
    }

    pub fn ruling(&self, text: &str) -> Result<(), CoreError> {
        self.append(&format!("Ruling: {}\n", single_line(text)?))
    }

    /// Parses only the strict `Task N: <state>` lines, in file order. Other lines are ignored.
    pub fn tasks(&self) -> Result<Vec<(u32, TaskState)>, CoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CoreError::io(&self.path, e)),
        };
        Ok(text.lines().filter_map(parse_task_line).collect())
    }

    fn append(&self, text: &str) -> Result<(), CoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| CoreError::io(&self.path, e))?;
        file.write_all(text.as_bytes())
            .map_err(|e| CoreError::io(&self.path, e))
    }
}

fn parse_task_line(line: &str) -> Option<(u32, TaskState)> {
    let rest = line.strip_prefix("Task ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u32 = digits.parse().ok()?;
    let after = rest[digits.len()..].strip_prefix(": ")?;
    for state in [
        TaskState::ImplementerDone,
        TaskState::Dispatched,
        TaskState::Complete,
    ] {
        if let Some(tail) = after.strip_prefix(state.label()) {
            if tail.is_empty() || tail.starts_with(' ') {
                return Some((n, state));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> (tempfile::TempDir, Ledger) {
        let dir = tempfile::tempdir().expect("tempdir");
        let ledger = Ledger::new(dir.path().join("progress.md"));
        (dir, ledger)
    }

    #[test]
    fn writes_strict_lines_and_reads_them_back() {
        let (_dir, l) = ledger();
        l.create("# SDD ledger").expect("create");
        l.task(1, TaskState::Dispatched, None).expect("t1");
        l.task(1, TaskState::ImplementerDone, None).expect("t1b");
        l.task(1, TaskState::Complete, Some("commits a..b"))
            .expect("t1c");
        l.ruling("keep the cap at 4000 lines").expect("ruling");
        let text = std::fs::read_to_string(&l.path).expect("read");
        assert!(text.contains("\nTask 1: dispatched\n"));
        assert!(text.contains("\nTask 1: implementer done\n"));
        assert!(text.contains("\nTask 1: complete (commits a..b)\n"));
        assert!(text.contains("\nRuling: keep the cap at 4000 lines\n"));
        let tasks = l.tasks().expect("parse");
        assert_eq!(tasks.last(), Some(&(1, TaskState::Complete)));
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn create_never_truncates_and_appends_survive() {
        let (_dir, l) = ledger();
        l.create("# head").expect("create");
        l.task(2, TaskState::Complete, None).expect("t2");
        l.create("# other head").expect("second create is a no-op");
        let text = std::fs::read_to_string(&l.path).expect("read");
        assert!(text.starts_with("# head\n"));
        assert!(text.contains("Task 2: complete"));
    }

    #[test]
    fn rejects_multiline_and_empty_text() {
        let (_dir, l) = ledger();
        assert!(l.ruling("a\nTask 9: complete").is_err());
        assert!(l.ruling("   ").is_err());
        assert!(l.task(1, TaskState::Complete, Some("x\ny")).is_err());
    }

    #[test]
    fn missing_ledger_has_no_tasks_and_loose_lines_are_ignored() {
        let (_dir, l) = ledger();
        assert!(l.tasks().expect("missing ok").is_empty());
        std::fs::write(
            &l.path,
            "Task 3: complete\ntask 4: complete\nTask 5: completed\nTask x: complete\n",
        )
        .expect("w");
        assert_eq!(l.tasks().expect("parse"), vec![(3, TaskState::Complete)]);
    }
}
