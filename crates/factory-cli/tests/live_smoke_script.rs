use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the smoke script with the `claude` shim first on PATH and a temp HOME.
/// Returns the process output and the temp dir that holds `out/` and `home/`.
fn run_script(args: &[&str], envs: &[(&str, &str)]) -> (Output, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let shim = bin.join("claude");
    std::fs::copy(
        repo_root().join("crates/factory-cli/tests/support/claude-shim.sh"),
        &shim,
    )
    .expect("copy shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
            .expect("chmod shim");
    }
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join("keys")).expect("home dir");
    // A key file for tests that copy key settings; the runner must find it readable.
    let key = home.join("keys/test.key");
    std::fs::write(&key, "sk-test-not-a-real-key\n").expect("write key");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).expect("chmod key");
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"));
    let out = tmp.path().join("out");
    let mut cmd = Command::new("bash");
    cmd.arg("scripts/live-smoke.sh")
        .args(args)
        .arg("--out")
        .arg(&out)
        .current_dir(repo_root())
        .env("FACTORY_BIN", env!("CARGO_BIN_EXE_factory"))
        .env("HOME", &home)
        .env("PATH", path)
        .env("TMPDIR", tmp.path())
        .env_remove("FACTORY_HOME");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    (cmd.output().expect("run script"), tmp)
}

#[test]
fn passes_on_the_fake_agent_and_saves_transcripts() {
    let (output, tmp) = run_script(&["--no-probe"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}\n{stderr}");
    let out = tmp.path().join("out");
    let summary = std::fs::read_to_string(out.join("summary.txt")).expect("summary");
    assert!(summary.starts_with("PASS"), "{summary}");
    for name in ["board.json", "events.jsonl"] {
        let len = std::fs::metadata(out.join(name)).expect(name).len();
        assert!(len > 0, "{name} is empty");
    }
    // A clean first pass has no builder-r1.md, so the review file and the usage line stand in.
    let job_dirs: Vec<PathBuf> = std::fs::read_dir(out.join("jobs"))
        .expect("jobs dir")
        .map(|e| e.expect("entry").path())
        .collect();
    assert!(job_dirs
        .iter()
        .any(|d| d.join("review-code-r1.md").is_file()));
    assert!(job_dirs.iter().any(|d| {
        std::fs::read_to_string(d.join("usage.jsonl"))
            .is_ok_and(|u| u.contains("\"role\":\"builder\""))
    }));
    let show = std::fs::read_to_string(out.join("show-001.json")).expect("show");
    assert!(show.contains("\"stage\": \"merged\""), "{show}");
    assert!(!tmp.path().join("home/.factory").exists());
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn fails_when_the_job_does_not_reach_merged() {
    let (output, tmp) = run_script(&["--no-probe"], &[("SHIM_REVIEW", "changes")]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("FAIL: job 001 ended in needs-human"),
        "{stderr}"
    );
    assert!(stderr.contains("changes-remain"), "{stderr}");
    assert!(tmp.path().join("out/board.json").is_file());
    assert!(tmp.path().join("out/jobs").is_dir());
}

#[test]
fn fails_when_the_probe_fails() {
    let (output, tmp) = run_script(&[], &[]);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("FAIL: marker probe failed"), "{stderr}");
    assert!(tmp.path().join("out/probe").is_dir());
    assert!(!tmp.path().join("out/jobs").exists());
}

#[test]
fn unknown_option_exits_2_and_help_exits_0() {
    let (output, _tmp) = run_script(&["--bogus"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("--bogus"));
    let (output, _tmp) = run_script(&["--help"], &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for word in ["--interactive", "--harness", "--out"] {
        assert!(stdout.contains(word), "{word} missing in {stdout}");
    }
}

#[test]
fn missing_agent_program_exits_2() {
    let (output, _tmp) = run_script(
        &["--no-probe"],
        &[("LIVE_SMOKE_AGENT_PROGRAM", "definitely-missing-claude")],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("Missing tool: definitely-missing-claude"));
}

/// Registers a temp git repo as `keysrc` in a fresh "real" factory home and returns the home.
fn real_home_with_keysrc(tmp: &Path, key_lines: &str) -> PathBuf {
    let home = tmp.join("real-home");
    let repo = tmp.join("keysrc-repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&repo)
            .status()
            .expect("git");
        assert!(status.success());
    }
    let status = Command::new(env!("CARGO_BIN_EXE_factory"))
        .args(["project", "add"])
        .arg(&repo)
        .args(["--preset", "node", "--name", "keysrc"])
        .env("FACTORY_HOME", &home)
        .status()
        .expect("project add");
    assert!(status.success());
    let toml = home.join("projects/keysrc/project.toml");
    let text = std::fs::read_to_string(&toml).expect("read toml");
    let text = text.replacen("[settings]\n", &format!("[settings]\n{key_lines}"), 1);
    std::fs::write(&toml, text).expect("write toml");
    home
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn copies_the_api_key_settings_from_the_key_project() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = real_home_with_keysrc(
        tmp.path(),
        "api_key_file = \"~/keys/test.key\"\nallow_api_key = true\n",
    );
    let (output, run_tmp) = run_script(
        &["--no-probe", "--key-from", "keysrc"],
        &[("FACTORY_HOME", real.to_str().expect("utf8"))],
    );
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let home = run_tmp.path().join("home");
    let key = format!("{}/keys/test.key", home.display());
    let out = run_tmp.path().join("out");
    let project = std::fs::read_to_string(out.join("project.toml")).expect("project.toml");
    assert!(
        project.contains(&format!("api_key_file = \"{key}\"")),
        "{project}"
    );
    assert!(project.contains("allow_api_key = true"), "{project}");
    let summary = std::fs::read_to_string(out.join("summary.txt")).expect("summary");
    assert!(
        summary.contains(&format!("credentials: api_key_file {key} (from keysrc)")),
        "{summary}"
    );
    // The script never writes the key file; its content is still the test's own.
    assert_eq!(
        std::fs::read_to_string(&key).expect("key"),
        "sk-test-not-a-real-key\n"
    );
    for entry in walk(&out) {
        let name = entry
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if name == "project.toml" || name == "summary.txt" {
            continue;
        }
        let bytes = std::fs::read(&entry).expect("read evidence");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("test.key") && !text.contains("sk-test-not-a-real-key"),
            "{} names the key",
            entry.display()
        );
    }
}

#[test]
fn no_key_project_uses_the_login() {
    let real = tempfile::tempdir().expect("tempdir");
    let home = real.path().join("empty-home");
    std::fs::create_dir_all(&home).expect("home");
    let (output, tmp) = run_script(
        &["--no-probe", "--key-from", "nosuch"],
        &[("FACTORY_HOME", home.to_str().expect("utf8"))],
    );
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let out = tmp.path().join("out");
    let project = std::fs::read_to_string(out.join("project.toml")).expect("project.toml");
    // The template lists every key as a comment, so only a set line counts.
    assert!(
        !project.lines().any(|l| l.starts_with("api_key_file")),
        "{project}"
    );
    let summary = std::fs::read_to_string(out.join("summary.txt")).expect("summary");
    assert!(summary.contains("credentials: login"), "{summary}");
}

#[test]
fn missing_tmux_exits_2() {
    let (output, tmp) = run_script(
        &["--no-probe", "--interactive"],
        &[("TMUX_BIN", "definitely-missing-tmux")],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("Missing tool: definitely-missing-tmux"));
    assert!(!tmp.path().join("out/board.json").exists());
}

#[test]
fn interactive_with_codex_exits_2() {
    let (output, tmp) = run_script(&["--harness", "codex", "--interactive"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("--interactive supports claude only"));
    assert!(!tmp.path().join("out/board.json").exists());
}
