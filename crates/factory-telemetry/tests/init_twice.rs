use factory_telemetry::{init, LogTarget, Scope, TelemetryError};

#[test]
fn init_twice_returns_already_installed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = || LogTarget {
        path: dir.path().join("log.jsonl"),
        project: None,
        scope: Scope::Cli,
    };
    assert!(init(target(), "info", None).is_ok());
    assert_eq!(
        init(target(), "info", None).err(),
        Some(TelemetryError::AlreadyInstalled)
    );
}
