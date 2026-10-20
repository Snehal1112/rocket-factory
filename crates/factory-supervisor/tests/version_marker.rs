use std::path::Path;
use std::process::{Command, Output};

use factory_core::Home;

const V: &str = env!("CARGO_PKG_VERSION");

fn factoryd(home: &Home, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_factoryd"))
        .args(args)
        .env("FACTORY_HOME", &home.dir)
        .env("FACTORY_BIN", "/nonexistent/factory")
        .output()
        .expect("run factoryd")
}

fn home_with(t: &Path, marker: Option<&str>) -> Home {
    let home = Home::at(t.join("home"));
    std::fs::create_dir_all(&home.dir).expect("home dir");
    if let Some(m) = marker {
        std::fs::write(home.version_file(), m).expect("marker");
    }
    home
}

fn marker(home: &Home) -> String {
    std::fs::read_to_string(home.version_file()).expect("marker")
}

#[test]
fn run_once_stamps_a_missing_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), None);
    let out = factoryd(&home, &["run", "--once"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(marker(&home), format!("{V}\n"));
}

#[test]
fn run_once_stamps_over_an_older_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), Some("0.0.1\n"));
    let out = factoryd(&home, &["run", "--once"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(marker(&home), format!("{V}\n"));
}

#[test]
fn run_once_refuses_a_newer_marker_and_leaves_it() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), Some("99.0.0\n"));
    let out = factoryd(&home, &["run", "--once"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    let want = format!("factoryd: this home was last used by factory 99.0.0, this binary is {V}");
    assert!(err.starts_with(&want), "{err}");
    assert_eq!(marker(&home), "99.0.0\n");
    assert!(!home.run_dir().exists());
}

#[test]
fn status_stop_and_enqueue_refuse_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), Some("99.0.0\n"));
    let cases: [&[&str]; 3] = [
        &["status"],
        &["stop"],
        &["enqueue", "--project", "p", "--job", "001"],
    ];
    for args in cases {
        let out = factoryd(&home, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("last used by factory 99.0.0"), "{args:?}: {err}");
    }
    assert!(!home.queue_dir().exists());
}

#[test]
fn status_accepts_an_older_marker_and_does_not_stamp() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), Some("0.0.1\n"));
    let out = factoryd(&home, &["status"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.ends_with(&format!("factoryd {V}\n")), "{text}");
    assert_eq!(marker(&home), "0.0.1\n");
}

#[test]
fn is_running_ignores_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), Some("99.0.0\n"));
    let out = factoryd(&home, &["is-running"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
}
