//! The risk verdict of a whole job diff, with the Cargo lock check that may clear Cargo files.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use factory_checks::{locked_path_packages, workspace_root};
use factory_core::{
    cargo_clearable, check_path_packages, classify_paths, compare_locks, manifest_keys_diff,
    write_atomic, CargoVerdict, Config, CoreError, DiffClass, LockDiff,
};

use crate::ports::one_line;
use crate::Engine;

/// The stored verdict in the job folder, keyed by head and config.
pub(crate) const CARGO_CHECK_FILE: &str = "cargo-check.txt";

/// The ledger line of a check that cleared the Cargo files.
const PASS_NOTE: &str = "cargo lock check: no outside code changed, Cargo files are not risky";

/// The cargo config files that cargo reads from parent folders.
const PARENT_CARGO_FILES: [&str; 3] = ["Cargo.toml", ".cargo/config", ".cargo/config.toml"];

/// Makes each export folder name unique within this process.
static EXPORT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The risk of a job diff: its class, the paths that keep it risky and the check's ledger line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Risk {
    pub class: DiffClass,
    pub risky: Vec<String>,
    pub note: Option<String>,
}

impl Risk {
    pub fn is_risky(&self) -> bool {
        self.class == DiffClass::Risky
    }
}

/// The ledger line of a check that could not clear the diff.
fn stays(reason: impl Display) -> String {
    one_line(&format!("cargo lock check: {reason}, diff stays risky"))
}

/// The ledger line of a lock that gained or lost outside crates.
fn outside_note(added: &[String], removed: &[String], changed: &[String]) -> String {
    let list = |names: &[String]| {
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    };
    stays(format!(
        "outside crates added: {}; removed: {}; source or checksum changed: {}",
        list(added),
        list(removed),
        list(changed)
    ))
}

/// Turns a structural verdict into a stop with its ledger line.
fn verdict(v: CargoVerdict) -> Result<(), String> {
    match v {
        CargoVerdict::Same => Ok(()),
        CargoVerdict::Changed(reason) | CargoVerdict::Unknown(reason) => Err(stays(reason)),
    }
}

/// The config text the stored verdict is keyed by. Each glob is length-prefixed so no glob text can fake a boundary.
fn config_text(config: &Config) -> String {
    let globs: String = config
        .risk_paths
        .iter()
        .map(|g| format!("{}:{g}", g.len()))
        .collect();
    format!(
        "cargo_lock_check={}; risk_paths={globs}",
        config.cargo_lock_check
    )
}

/// Line 1 of `cargo-check.txt` for this head and config.
pub(crate) fn store_key(head: &str, config: &Config) -> String {
    format!("key {head} {}", config_text(config))
}

/// The stored verdict, `(same, note)`, when `cargo-check.txt` holds this key.
pub(crate) fn read_store(job_dir: &Path, key: &str) -> Option<(bool, String)> {
    let text = std::fs::read_to_string(job_dir.join(CARGO_CHECK_FILE)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != key {
        return None;
    }
    let same = match lines.next()? {
        "same" => true,
        "risky" => false,
        _ => return None,
    };
    Some((same, lines.next().unwrap_or("").to_string()))
}

/// Writes the verdict to `cargo-check.txt` atomically.
pub(crate) fn write_store(
    job_dir: &Path,
    key: &str,
    same: bool,
    note: &str,
) -> Result<(), CoreError> {
    let word = if same { "same" } else { "risky" };
    let text = format!("{key}\n{word}\n{}\n", one_line(note));
    write_atomic(&job_dir.join(CARGO_CHECK_FILE), text.as_bytes())
}

/// The cargo home, whose own config files are allowed above the export folder.
fn cargo_home() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    Some(home.canonicalize().unwrap_or(home))
}

/// True when `dir` or a parent of it holds a Cargo file that cargo would read.
fn cargo_file_above(dir: &Path) -> bool {
    let home = cargo_home();
    dir.ancestors().any(|d| {
        PARENT_CARGO_FILES.iter().any(|f| {
            let path = d.join(f);
            path.exists() && !home.as_ref().is_some_and(|h| path.starts_with(h))
        })
    })
}

/// A fresh export folder that is removed when it goes out of scope.
struct ExportDir(PathBuf);

impl ExportDir {
    fn create(temp_root: &Path, job_id: &str) -> Result<ExportDir, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = EXPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "factory-cargo-check-{job_id}-{}-{nanos}-{n}",
            std::process::id()
        );
        let path = temp_root.join(name);
        std::fs::create_dir(&path)
            .map_err(|e| stays(format!("cannot make the export folder: {e}")))?;
        Ok(ExportDir(path))
    }
}

impl Drop for ExportDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            tracing::warn!(
                event = "cargo_check.cleanup",
                msg = %format!("cannot remove {}: {e}", self.0.display())
            );
        }
    }
}

/// The file name of a scoped path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A path relative to `root` for a message, `.` for the root itself.
fn shown(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

impl Engine<'_> {
    /// The risk of the whole job diff for scoped `paths`. It never fails: a check it cannot settle stays risky.
    pub(crate) fn risk(&self, paths: &[String]) -> Risk {
        self.risk_in(paths, &std::env::temp_dir())
    }

    /// Like `risk`, with the export folder made under `temp_root`.
    pub(crate) fn risk_in(&self, paths: &[String], temp_root: &Path) -> Risk {
        let class = classify_paths(paths, &self.config);
        if class != DiffClass::Risky {
            return Risk {
                class,
                risky: Vec::new(),
                note: None,
            };
        }
        let risky: Vec<String> = paths
            .iter()
            .filter(|p| classify_paths(std::slice::from_ref(*p), &self.config) == DiffClass::Risky)
            .cloned()
            .collect();
        let stays_risky = |note: Option<String>| Risk {
            class: DiffClass::Risky,
            risky: risky.clone(),
            note,
        };
        if !self.config.cargo_lock_check || !risky.iter().all(|p| cargo_clearable(p, &self.config))
        {
            return stays_risky(None);
        }
        // The git diff runs only after the cheap gate, so a diff that stays risky without cargo never pays for it.
        let outside = self
            .job_changed_paths_raw()
            .map(|raw| self.repo.out_of_scope(&raw));
        match outside {
            Ok(list) if list.is_empty() => {}
            Ok(_) => return stays_risky(Some(stays("a changed path is outside the project"))),
            Err(e) => return stays_risky(Some(stays(e))),
        }
        let (start, head) = match self.job_range() {
            Ok(range) => range,
            Err(e) => return stays_risky(Some(stays(e))),
        };
        let key = store_key(&head, &self.config);
        let (same, note) = match read_store(&self.job_dir, &key) {
            Some(found) => found,
            None => {
                let (same, note) = match self.cargo_check(paths, &start, &head, temp_root) {
                    Ok(()) => (true, PASS_NOTE.to_string()),
                    Err(note) => (false, note),
                };
                // The store only saves a later cargo run, so a failed write never changes the verdict.
                if let Err(e) = write_store(&self.job_dir, &key, same, &note) {
                    tracing::warn!(
                        event = "cargo_check.store",
                        msg = %format!("cannot write {CARGO_CHECK_FILE}: {e}")
                    );
                }
                (same, note)
            }
        };
        let note = (!note.is_empty()).then_some(note);
        if same {
            Risk {
                class: DiffClass::Normal,
                risky: Vec::new(),
                note,
            }
        } else {
            stays_risky(note)
        }
    }

    /// The text of a scoped path at `commit`, read from the repo.
    fn cargo_file(&self, commit: &str, scoped: &str) -> Result<Option<String>, String> {
        let rel = self.repo.workdir_in(Path::new("")).join(scoped);
        let rel = rel.to_string_lossy();
        self.repo
            .file_at(commit, &rel)
            .map_err(|e| stays(format!("cannot read {rel}: {e}")))
    }

    /// Runs the lock, cargo and structural checks on a clean export of `head`. `Err` holds the ledger line.
    fn cargo_check(
        &self,
        paths: &[String],
        start: &str,
        head: &str,
        temp_root: &Path,
    ) -> Result<(), String> {
        let modules = self
            .repo
            .file_at(head, ".gitmodules")
            .map_err(|e| stays(format!("cannot read .gitmodules: {e}")))?;
        if modules.is_some() {
            return Err(stays("the repo has git submodules"));
        }
        let temp_root = temp_root
            .canonicalize()
            .map_err(|e| stays(format!("cannot resolve the temp folder: {e}")))?;
        // A shared temp folder can gain a Cargo file between this check and the cargo calls. That gap is accepted.
        if cargo_file_above(&temp_root) {
            return Err(stays("a Cargo file sits above the export folder"));
        }
        let export = ExportDir::create(&temp_root, &self.job_id)?;
        self.repo
            .export_commit(head, &export.0)
            .map_err(|e| stays(format!("cannot export the head commit: {e}")))?;
        let project = self.repo.workdir_in(&export.0);
        let project = project
            .canonicalize()
            .map_err(|e| stays(format!("the project folder is missing in the export: {e}")))?;

        // The changed manifests present at head, and the manifests that name the touched workspaces.
        let mut manifests: BTreeMap<&str, String> = BTreeMap::new();
        let mut new_manifests: BTreeSet<PathBuf> = BTreeSet::new();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        let mut locks: Vec<&str> = Vec::new();
        let mut deleted: Vec<&str> = Vec::new();
        for path in paths {
            let name = file_name(path);
            if name.eq_ignore_ascii_case("Cargo.toml") {
                if let Some(text) = self.cargo_file(head, path)? {
                    if self.cargo_file(start, path)?.is_none() {
                        new_manifests.insert(project.join(path));
                    }
                    manifests.insert(path, text);
                    touched.insert(path.clone());
                } else {
                    deleted.push(path);
                }
            } else if name.eq_ignore_ascii_case("Cargo.lock") {
                let dir = Path::new(path).parent().unwrap_or(Path::new(""));
                let manifest = dir.join("Cargo.toml").to_string_lossy().into_owned();
                if self.cargo_file(head, &manifest)?.is_none() {
                    return Err(stays(format!("{path} has no Cargo.toml beside it at head")));
                }
                locks.push(path);
                touched.insert(manifest);
            }
        }

        // The workspace roots, relative to the project folder.
        let mut roots: BTreeSet<PathBuf> = BTreeSet::new();
        for manifest in &touched {
            let root = workspace_root(&project.join(manifest)).map_err(stays)?;
            let root = root
                .canonicalize()
                .map_err(|e| stays(format!("cannot resolve the workspace root: {e}")))?;
            let rel = root.strip_prefix(&project).map_err(|_| {
                stays(format!(
                    "workspace root {} is outside the project",
                    shown(&root, &export.0)
                ))
            })?;
            roots.insert(rel.to_path_buf());
        }
        if roots.is_empty() {
            return Err(stays("no Cargo manifest or lock is left to check"));
        }
        for path in &deleted {
            let dir = Path::new(path).parent().unwrap_or(Path::new(""));
            if !roots.iter().any(|r| dir.starts_with(r)) {
                return Err(stays(format!(
                    "{path} was deleted outside the checked workspaces"
                )));
            }
        }
        let root_locks: BTreeSet<String> = roots
            .iter()
            .map(|r| r.join("Cargo.lock").to_string_lossy().into_owned())
            .collect();
        if let Some(lock) = locks.iter().find(|l| !root_locks.contains(**l)) {
            return Err(stays(format!(
                "{lock} is not the lock of a touched workspace"
            )));
        }

        // The lock check comes first, so cargo never fetches a crate the job added.
        for lock in &root_locks {
            let base = self
                .cargo_file(start, lock)?
                .ok_or_else(|| stays(format!("the base {lock} is missing")))?;
            let tip = self
                .cargo_file(head, lock)?
                .ok_or_else(|| stays(format!("the head {lock} is missing")))?;
            match compare_locks(&base, &tip) {
                LockDiff::Same => {}
                LockDiff::Changed {
                    added,
                    removed,
                    changed,
                } => return Err(outside_note(&added, &removed, &changed)),
                LockDiff::Unknown(reason) => return Err(stays(reason)),
            }
        }

        let changed: BTreeSet<PathBuf> = paths.iter().map(|p| project.join(p)).collect();
        for root in &roots {
            let packages =
                locked_path_packages(&project.join(root).join("Cargo.toml")).map_err(stays)?;
            verdict(check_path_packages(
                &project,
                &packages,
                &changed,
                &new_manifests,
            ))?;
        }
        for (path, text) in &manifests {
            let base = self.cargo_file(start, path)?;
            verdict(manifest_keys_diff(base.as_deref(), text))?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use factory_core::{Board, Job};
    use git2::{Repository, Signature};
    use std::process::Command;

    /// A repo with a job whose base and head commits hold the given files, and what an engine needs.
    pub(crate) struct JobRepo {
        pub dir: tempfile::TempDir,
        pub repo: factory_git::Repo,
        pub runner: factory_agent::FakeRunner,
        pub events: factory_agent::EventSink,
        pub job_dir: PathBuf,
        pub head: String,
    }

    impl JobRepo {
        pub(crate) fn engine(&self, config: Config) -> Engine<'_> {
            Engine::new(
                self.dir.path().join("board.json"),
                self.job_dir.clone(),
                "001".to_string(),
                self.job_dir.join("plan.md"),
                &self.repo,
                &self.runner,
                config,
                &self.events,
            )
            .expect("engine")
        }
    }

    fn commit(
        repo: &Repository,
        update: &str,
        files: &[(&str, String)],
        parent: Option<&git2::Commit>,
    ) -> String {
        let workdir = repo.workdir().expect("workdir").to_path_buf();
        let mut index = repo.index().expect("index");
        for (name, text) in files {
            let path = workdir.join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, text).expect("write");
            index.add_path(Path::new(name)).expect("add");
        }
        index.write().expect("write index");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("find tree");
        let sig = Signature::now("t", "t@example.com").expect("sig");
        let parents: Vec<&git2::Commit> = parent.into_iter().collect();
        repo.commit(Some(update), &sig, &sig, "c", &tree, &parents)
            .expect("commit")
            .to_string()
    }

    /// Inits a repo on main with the base files, commits the head files on a side branch and saves a job.
    pub(crate) fn engine_with_job(
        base_files: &[(&str, String)],
        head_files: &[(&str, String)],
    ) -> JobRepo {
        let dir = tempfile::tempdir().expect("temp dir");
        let main = dir.path().join("repo");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        let git = Repository::init_opts(&main, &opts).expect("init");
        let base = commit(&git, "HEAD", base_files, None);
        let base_commit = git
            .find_commit(git2::Oid::from_str(&base).expect("oid"))
            .expect("base commit");
        let head = commit(&git, "refs/heads/job", head_files, Some(&base_commit));
        let job_dir = dir.path().join("job");
        std::fs::create_dir(&job_dir).expect("job dir");
        let mut job = Job::new(
            "001",
            "2026-11-21-001-demo",
            "demo",
            "2026-11-21",
            "demo",
            0.0,
        );
        job.base = Some(base);
        job.head = Some(head.clone());
        let mut board = Board::default();
        board.jobs.push(job);
        board.save(&dir.path().join("board.json")).expect("board");
        let repo = factory_git::Repo::open(&main).expect("open repo");
        let events = factory_agent::EventSink::new(job_dir.join("events.jsonl"), "001");
        JobRepo {
            dir,
            repo,
            runner: factory_agent::FakeRunner::new(Vec::new()),
            events,
            job_dir,
            head,
        }
    }

    fn package(name: &str, deps: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}")
    }

    /// The lock cargo writes, without the network, for a workspace of these files.
    fn generated_lock(files: &[(&str, String)]) -> String {
        let dir = tempfile::tempdir().expect("temp dir");
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, text).expect("write");
        }
        let out = Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(dir.path())
            .output()
            .expect("run cargo");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read_to_string(dir.path().join("Cargo.lock")).expect("lock")
    }

    fn workspace(a_deps: &str) -> Vec<(&'static str, String)> {
        let mut files = vec![
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n".to_string(),
            ),
            ("a/Cargo.toml", package("a", a_deps)),
            ("a/src/lib.rs", "\n".to_string()),
            ("b/Cargo.toml", package("b", "")),
            ("b/src/lib.rs", "\n".to_string()),
        ];
        let lock = generated_lock(&files);
        files.push(("Cargo.lock", lock));
        files
    }

    fn config() -> Config {
        Config {
            risk_paths: vec!["**/Cargo.toml".into(), "**/Cargo.lock".into()],
            ..Config::default()
        }
    }

    /// A job whose head adds an internal path dependency, which the check clears.
    fn passing_job() -> JobRepo {
        let head = workspace("b = { path = \"../b\" }\n");
        let head: Vec<(&str, String)> = head
            .into_iter()
            .filter(|(n, _)| *n == "a/Cargo.toml" || *n == "Cargo.lock")
            .collect();
        engine_with_job(&workspace(""), &head)
    }

    /// A job whose head lock gains an outside crate, which keeps it risky.
    fn outside_job() -> JobRepo {
        let base = workspace("");
        let mut lock = base
            .iter()
            .find(|(n, _)| *n == "Cargo.lock")
            .expect("lock")
            .1
            .clone();
        lock.push_str("\n[[package]]\nname = \"libc\"\nversion = \"0.2.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n");
        engine_with_job(&base, &[("Cargo.lock", lock)])
    }

    fn is_empty(dir: &Path) -> bool {
        std::fs::read_dir(dir).expect("read dir").next().is_none()
    }

    fn risk_of(job: &JobRepo, temp_root: &Path) -> Risk {
        let engine = job.engine(config());
        let paths = engine.job_changed_paths().expect("paths");
        engine.risk_in(&paths, temp_root)
    }

    #[test]
    fn a_changed_path_outside_the_project_stays_risky() {
        let job = engine_with_job(
            &[("apps/web/x.txt", "x\n".to_string())],
            &[("Cargo.lock", "# lock\n".to_string())],
        );
        let repo = factory_git::Repo::open(&job.dir.path().join("repo"))
            .expect("open")
            .with_scope("web", Path::new("apps/web"));
        let engine = Engine::new(
            job.dir.path().join("board.json"),
            job.job_dir.clone(),
            "001".to_string(),
            job.job_dir.join("plan.md"),
            &repo,
            &job.runner,
            config(),
            &job.events,
        )
        .expect("engine");
        let root = tempfile::tempdir().expect("temp root");
        let risk = engine.risk_in(&["Cargo.lock".to_string()], root.path());
        assert!(risk.is_risky());
        let note = risk.note.expect("note");
        assert!(
            note.contains("a changed path is outside the project"),
            "{note}"
        );
        assert!(is_empty(root.path()));
    }

    #[test]
    fn the_export_folder_is_gone_after_a_pass_and_an_error() {
        let root = tempfile::tempdir().expect("temp root");
        let pass = risk_of(&passing_job(), root.path());
        assert_eq!(pass.class, DiffClass::Normal, "{:?}", pass.note);
        assert!(is_empty(root.path()));

        let broken = engine_with_job(&workspace(""), &[("a/Cargo.toml", "x [".to_string())]);
        let fail = risk_of(&broken, root.path());
        assert!(fail.is_risky());
        let note = fail.note.expect("note");
        assert!(note.ends_with(", diff stays risky"), "{note}");
        assert!(is_empty(root.path()));
    }

    #[test]
    fn a_cargo_file_above_the_export_folder_is_unknown() {
        let outer = tempfile::tempdir().expect("temp root");
        std::fs::write(outer.path().join("Cargo.toml"), "[workspace]\n").expect("write");
        let inner = outer.path().join("inner");
        std::fs::create_dir(&inner).expect("mkdir");
        let risk = risk_of(&passing_job(), &inner);
        assert!(risk.is_risky());
        assert_eq!(risk.risky, vec!["Cargo.lock", "a/Cargo.toml"]);
        let note = risk.note.expect("note");
        assert!(
            note.contains("a Cargo file sits above the export folder"),
            "{note}"
        );
        assert!(is_empty(&inner));
    }

    #[test]
    fn a_stored_verdict_for_the_same_key_is_reused() {
        let job = outside_job();
        let key = store_key(&job.head, &config());
        write_store(&job.job_dir, &key, true, "stored note").expect("store");
        let root = tempfile::tempdir().expect("temp root");
        let risk = risk_of(&job, root.path());
        assert_eq!(risk.class, DiffClass::Normal);
        assert_eq!(risk.note.as_deref(), Some("stored note"));
        assert!(is_empty(root.path()));
    }

    #[test]
    fn a_stored_verdict_for_another_head_is_ignored() {
        let job = outside_job();
        let other = "0".repeat(40);
        write_store(&job.job_dir, &store_key(&other, &config()), true, "old").expect("store");
        let root = tempfile::tempdir().expect("temp root");
        let risk = risk_of(&job, root.path());
        assert!(risk.is_risky());
        let note = risk.note.expect("note");
        assert!(note.contains("outside crates added: libc 0.2.1"), "{note}");
        let stored = std::fs::read_to_string(job.job_dir.join(CARGO_CHECK_FILE)).expect("store");
        let mut lines = stored.lines();
        assert_eq!(lines.next(), Some(store_key(&job.head, &config()).as_str()));
        assert_eq!(lines.next(), Some("risky"));
        assert_eq!(lines.next(), Some(note.as_str()));
    }

    #[test]
    fn store_key_length_prefixes_each_glob() {
        let with = |globs: &[&str]| Config {
            risk_paths: globs.iter().map(|g| g.to_string()).collect(),
            ..Config::default()
        };
        assert_eq!(
            store_key("abc", &with(&["**/Cargo.toml", "**/Cargo.lock"])),
            "key abc cargo_lock_check=true; risk_paths=13:**/Cargo.toml13:**/Cargo.lock"
        );
        assert_ne!(
            store_key("abc", &with(&["a,b"])),
            store_key("abc", &with(&["a", "b"]))
        );
    }

    #[test]
    fn store_key_counts_bytes() {
        let config = Config {
            risk_paths: vec!["é/**".to_string()],
            ..Config::default()
        };
        assert!(store_key("h", &config).ends_with("risk_paths=5:é/**"));
    }

    #[test]
    fn the_outside_note_names_changed_crates() {
        let note = outside_note(&[], &[], &["libc 0.2.190".to_string()]);
        assert_eq!(
            note,
            "cargo lock check: outside crates added: none; removed: none; source or checksum changed: libc 0.2.190, diff stays risky"
        );
    }

    #[test]
    fn a_park_card_uses_the_cargo_verdict() {
        let job = outside_job();
        let engine = job.engine(config());
        let paths = engine.job_changed_paths().expect("paths");
        let key = store_key(&job.head, &config());

        write_store(&job.job_dir, &key, true, PASS_NOTE).expect("store");
        assert!(!engine.is_risky());
        assert!(engine.risk_part(&paths).is_none());

        let note =
            "cargo lock check: outside crates added: libc 0.2.1; removed: none, diff stays risky";
        write_store(&job.job_dir, &key, false, note).expect("store");
        assert!(engine.is_risky());
        let part = engine.risk_part(&paths).expect("risk part");
        assert!(part.text.ends_with(note), "{}", part.text);
    }
}
