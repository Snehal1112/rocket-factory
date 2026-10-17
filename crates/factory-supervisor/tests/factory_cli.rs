//! `ProcessFactoryCli` runs a child process. It lives in its own test binary, because a fork inside the
//! library tests could briefly hold the instance lock file of a parallel test.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use factory_supervisor::{CliSpawnError, FactoryCli, ProcessFactoryCli};

fn script(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("factory");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn arguments_stay_separate_and_the_home_is_passed() {
    let t = tempfile::tempdir().expect("tempdir");
    let bin = script(t.path(), "echo \"home=$FACTORY_HOME\"; for a in \"$@\"; do echo \"[$a]\"; done; echo oops >&2; exit 3");
    let cli = ProcessFactoryCli::new(bin, PathBuf::from("/some/home"));
    let args = vec!["rework".to_string(), "001".to_string(), "a b; echo $HOME\nsecond".to_string()];
    let out = cli.run("proj", &args).expect("run");
    assert_eq!(out.code, 3);
    assert_eq!(out.stderr.trim(), "oops");
    assert!(
        out.stdout.starts_with("home=/some/home\n[--project]\n[proj]\n[rework]\n[001]\n[a b; echo $HOME\n"),
        "{}",
        out.stdout
    );
}

#[test]
fn a_missing_binary_is_a_start_error() {
    let cli = ProcessFactoryCli::new(PathBuf::from("/nonexistent/factory"), PathBuf::from("/h"));
    assert!(matches!(cli.run("p", &[]), Err(CliSpawnError::Start { .. })));
}
