use factory_cli::{run_log, CliCtx, CliError};
use git2::Repository;

fn ctx_with_log(lines: Option<&[&str]>) -> (tempfile::TempDir, CliCtx) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ctx = CliCtx::from_env(Some(tmp.path().to_path_buf())).expect("ctx");
    if let Some(lines) = lines {
        std::fs::create_dir_all(&ctx.layout.state_dir).expect("state dir");
        std::fs::write(ctx.layout.log_file(), lines.join("\n") + "\n").expect("write log");
    }
    (tmp, ctx)
}

fn line(level: &str, job: &str, n: u32) -> String {
    format!(r#"{{"level":"{level}","job_id":"{job}","n":{n}}}"#)
}

#[test]
fn log_prints_a_jobs_lines() {
    let (a, b, c) = (
        line("info", "001", 1),
        line("info", "002", 2),
        line("info", "001", 3),
    );
    let (_t, ctx) = ctx_with_log(Some(&[&a, &b, &c]));
    let out = run_log(&ctx, Some("001"), None, None, 100).expect("log");
    assert_eq!(out, format!("{a}\n{c}"));
}

#[test]
fn log_level_filter() {
    let (a, b, c) = (
        line("info", "001", 1),
        line("warn", "001", 2),
        line("error", "001", 3),
    );
    let (_t, ctx) = ctx_with_log(Some(&[&a, &b, &c]));
    let out = run_log(&ctx, None, Some("warn"), None, 100).expect("log");
    assert_eq!(out, format!("{b}\n{c}"));
}

#[test]
fn log_tail_keeps_the_last_n() {
    let (a, b, c) = (
        line("info", "001", 1),
        line("info", "001", 2),
        line("info", "001", 3),
    );
    let (_t, ctx) = ctx_with_log(Some(&[&a, &b, &c]));
    assert_eq!(
        run_log(&ctx, None, None, None, 2).expect("log"),
        format!("{b}\n{c}")
    );
    assert_eq!(
        run_log(&ctx, None, None, None, 0).expect("log"),
        format!("{a}\n{b}\n{c}")
    );
}

#[test]
fn log_unknown_job_prints_nothing() {
    let a = line("info", "001", 1);
    let (_t, ctx) = ctx_with_log(Some(&[&a]));
    assert_eq!(
        run_log(&ctx, Some("999"), None, None, 100).expect("log"),
        ""
    );
}

#[test]
fn no_log_yet_exits_zero() {
    let (_t, ctx) = ctx_with_log(None);
    assert_eq!(
        run_log(&ctx, None, None, None, 100).expect("log"),
        "no log yet"
    );
}

#[test]
fn log_rejects_an_unknown_level() {
    let a = line("info", "001", 1);
    let (_t, ctx) = ctx_with_log(Some(&[&a]));
    let err = run_log(&ctx, None, Some("loud"), None, 100).expect_err("bad level");
    assert!(matches!(err, CliError::Refused(_)), "{err}");
}

#[test]
fn log_event_empty_is_refused() {
    let a = line("info", "001", 1);
    let (_t, ctx) = ctx_with_log(Some(&[&a]));
    let err = run_log(&ctx, None, None, Some(""), 100).expect_err("empty event");
    assert!(matches!(err, CliError::Refused(_)), "{err}");
}

#[test]
fn log_event_filters_by_prefix() {
    let a = r#"{"event":"task.start","job_id":"001","level":"info"}"#;
    let b = r#"{"event":"task.end","job_id":"001","level":"info"}"#;
    let c = r#"{"event":"agent.end","job_id":"001","level":"info"}"#;
    let (_t, ctx) = ctx_with_log(Some(&[a, b, c]));
    let out = run_log(&ctx, None, None, Some("task"), 100).expect("log");
    assert_eq!(out, format!("{a}\n{b}"));
}

#[test]
fn bad_factory_log_is_a_note_not_a_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    Repository::init(tmp.path()).expect("init");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_factory"))
        .arg("--root")
        .arg(tmp.path())
        .arg("status")
        .env("FACTORY_LOG", "bogus!!")
        .output()
        .expect("run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("FACTORY_LOG ignored"), "{stderr}");
}
