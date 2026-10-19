//! Fixtures for the router tests: a home with one working and one broken project.

use std::path::Path;

use factory_core::Home;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::api::ApiDeps;
use crate::factory_cli::{CliOutput, CliSpawnError, FactoryCli};

pub const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub const NOW_MS: u64 = 1_700_000_000_000;

/// Records every call and answers with the queued replies, else with the last one set.
pub struct FakeCli {
    calls: Mutex<Vec<(String, Vec<String>)>>,
    replies: Mutex<VecDeque<CliOutput>>,
    last: Mutex<CliOutput>,
}

impl Default for FakeCli {
    fn default() -> FakeCli {
        FakeCli {
            calls: Mutex::new(Vec::new()),
            replies: Mutex::new(VecDeque::new()),
            last: Mutex::new(CliOutput {
                code: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
        }
    }
}

impl FakeCli {
    pub fn reply(&self, code: i32, stdout: &str, stderr: &str) {
        *self.last.lock().expect("lock") = CliOutput {
            code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        };
    }

    pub fn reply_stdout(&self, stdout: &str) {
        self.reply(0, stdout, "");
    }

    pub fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().expect("lock").clone()
    }
}

impl FactoryCli for FakeCli {
    fn run(&self, project: &str, args: &[String]) -> Result<CliOutput, CliSpawnError> {
        self.calls
            .lock()
            .expect("lock")
            .push((project.to_string(), args.to_vec()));
        let queued = self.replies.lock().expect("lock").pop_front();
        Ok(queued.unwrap_or_else(|| self.last.lock().expect("lock").clone()))
    }
}

pub fn deps_with(cli: Arc<FakeCli>) -> ApiDeps {
    ApiDeps {
        now_ms: Arc::new(|| NOW_MS),
        cli,
    }
}

pub fn deps() -> ApiDeps {
    deps_with(Arc::new(FakeCli::default()))
}

pub fn headers(token: Option<&str>, host: &str) -> Vec<(String, String)> {
    let mut h = vec![("Host".to_string(), host.to_string())];
    if let Some(t) = token {
        h.push(("Authorization".to_string(), format!("Bearer {t}")));
    }
    h
}

fn register(home: &Home, name: &str, repo: &Path) {
    let dir = home.project_dir(name);
    std::fs::create_dir_all(&dir).expect("project dir");
    let text = format!(
        "repo = {:?}\npreset = \"rust\"\n[settings]\nbase_branch = \"trunk\"\n",
        repo.display().to_string()
    );
    std::fs::write(dir.join("project.toml"), text).expect("project.toml");
}

/// A home with `good` (a real git repo) and `broken` (a repo folder that does not exist), and a token file.
pub fn fixture() -> (tempfile::TempDir, Home) {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    std::fs::create_dir_all(&home.dir).expect("home");
    std::fs::write(home.token_file(), TOKEN).expect("token");
    let repo = t.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git2::Repository::init(&repo).expect("git init");
    register(&home, "good", &repo);
    register(&home, "broken", &t.path().join("missing-repo"));
    (t, home)
}

/// Writes a board for `good` with one job whose folder is `001-first`.
pub fn write_board(home: &Home, json: &str) {
    let state = home.project_dir("good").join("state");
    std::fs::create_dir_all(&state).expect("state");
    std::fs::write(state.join("board.json"), json).expect("board");
}

pub fn job_json(id: &str, dir: &str) -> String {
    format!(
        r#"{{"id":"{id}","dir":"{dir}","slug":"first","created":"2026-10-17","feature":"A feature","branch":"factory/{id}-first","stage":"build","round":1,"stage_since":1.0,"reviews":{{}},"tokens":{{"input":10,"cache_creation":0,"output":5,"cache_read":100}}}}"#
    )
}

pub fn write_usage(home: &Home, dir: &str, text: &str) {
    let d = home
        .project_dir("good")
        .join("state")
        .join("jobs")
        .join(dir);
    std::fs::create_dir_all(&d).expect("job dir");
    std::fs::write(d.join("usage.jsonl"), text).expect("usage");
}
