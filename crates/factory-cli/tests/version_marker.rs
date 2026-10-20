//! The version gate of the `factory` binary.

use std::path::Path;
use std::process::Output;

use factory_core::version::FACTORY_VERSION;
use factory_core::Home;

fn factory(home: &Home, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_factory"))
        .args(args)
        .env("FACTORY_HOME", &home.dir)
        .output()
        .expect("run binary")
}

fn home_with(t: &Path, marker: &str) -> Home {
    let home = Home::at(t.join("home"));
    std::fs::create_dir_all(&home.dir).expect("mkdir");
    std::fs::write(home.version_file(), marker).expect("marker");
    home
}

fn marker(home: &Home) -> String {
    std::fs::read_to_string(home.version_file()).expect("read marker")
}

#[test]
fn project_list_stops_on_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "99.0.0\n");
    let out = factory(&home, &["project", "list"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with(&format!(
            "factory: this home was last used by factory 99.0.0, this binary is {FACTORY_VERSION}: install a newer binary, or delete {} to ignore this",
            home.version_file().display()
        )),
        "{stderr}"
    );
}

#[test]
fn presets_refresh_stops_on_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "99.0.0\n");
    let out = factory(&home, &["presets", "refresh"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("last used by factory 99.0.0"), "{stderr}");
    assert!(!home.presets_dir().exists());
}

#[test]
fn a_marker_that_is_not_a_version_stops_project_commands() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "garbage\n");
    let out = factory(&home, &["project", "list"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("does not hold a version (got \"garbage\")"),
        "{stderr}"
    );
}

#[test]
fn project_flag_stops_on_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "99.0.0\n");
    let out = factory(&home, &["--project", "demo", "status"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("last used by factory 99.0.0"), "{stderr}");
}

#[test]
fn project_flag_passes_an_older_marker_through() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "0.0.1\n");
    let out = factory(&home, &["--project", "demo", "status"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(!stderr.contains("last used by"), "{stderr}");
    assert_eq!(marker(&home), "0.0.1\n");
}

#[test]
fn presets_refresh_checks_but_does_not_stamp() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "0.0.1\n");
    let out = factory(&home, &["presets", "refresh"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(marker(&home), "0.0.1\n");
}

#[test]
fn project_list_accepts_an_older_marker_and_does_not_stamp() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "0.0.1\n");
    let out = factory(&home, &["project", "list"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "no projects\n");
    assert_eq!(marker(&home), "0.0.1\n");
}

#[test]
fn commands_without_the_home_ignore_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "99.0.0\n");
    let repo = t.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir");
    git2::Repository::init(&repo).expect("init");
    let cmds = t.path().join("cmds");
    let runs: Vec<Vec<&str>> = vec![
        vec!["--version"],
        vec!["install", "--dir", cmds.to_str().expect("utf8")],
        vec!["--root", repo.to_str().expect("utf8"), "status"],
    ];
    for args in runs {
        let out = factory(&home, &args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(marker(&home), "99.0.0\n");
}
