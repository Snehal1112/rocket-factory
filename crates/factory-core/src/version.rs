//! The version marker in the factory home and the comparison against it.

use crate::{write_atomic, CoreError, Home};

/// The version of this build.
pub const FACTORY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Reads the marker, trimmed. A missing file is `None`.
pub fn read_marker(home: &Home) -> Result<Option<String>, CoreError> {
    let path = home.version_file();
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(&path, e)),
    }
}

/// Parses `major.minor.patch`, ignoring text after the first `-` or `+`.
fn parse(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_current(current: &str) -> Result<(u64, u64, u64), CoreError> {
    parse(current).ok_or_else(|| {
        CoreError::Version(format!("this binary's version {current:?} is not a version"))
    })
}

/// Stops a binary that is older than the marker.
pub fn check(home: &Home, current: &str) -> Result<(), CoreError> {
    let cur = parse_current(current)?;
    let Some(text) = read_marker(home)? else {
        return Ok(());
    };
    match parse(&text) {
        None => Err(CoreError::Version(format!(
            "{} does not hold a version (got {:?}): delete it and run factory upgrade",
            home.version_file().display(),
            text
        ))),
        Some(marker) if marker > cur => Err(CoreError::Version(format!(
            "this home was last used by factory {text}, this binary is {current}: install a newer binary, or delete {} to ignore this",
            home.version_file().display()
        ))),
        Some(_) => Ok(()),
    }
}

/// Writes the marker when it is missing, not a version or older.
/// Returns whether it wrote.
pub fn stamp(home: &Home, current: &str) -> Result<bool, CoreError> {
    let cur = parse_current(current)?;
    if let Some(text) = read_marker(home)? {
        if parse(&text).is_some_and(|marker| marker >= cur) {
            return Ok(false);
        }
    }
    write_atomic(&home.version_file(), format!("{current}\n").as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with(marker: Option<&[u8]>) -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().join("home"));
        if let Some(bytes) = marker {
            std::fs::create_dir_all(&home.dir).expect("mkdir");
            std::fs::write(home.version_file(), bytes).expect("write marker");
        }
        (t, home)
    }

    fn dir_marker() -> (tempfile::TempDir, Home) {
        let (t, home) = home_with(None);
        std::fs::create_dir_all(home.version_file()).expect("mkdir marker");
        (t, home)
    }

    #[test]
    fn check_stops_a_binary_older_than_the_marker() {
        let (_t, home) = home_with(Some(b"0.2.0\n"));
        let err = check(&home, "0.1.2").expect_err("newer marker");
        assert!(matches!(err, CoreError::Version(_)), "{err}");
        assert_eq!(
            err.to_string(),
            format!(
                "this home was last used by factory 0.2.0, this binary is 0.1.2: install a newer binary, or delete {} to ignore this",
                home.version_file().display()
            )
        );
    }

    #[test]
    fn check_accepts_a_missing_marker() {
        let (_t, home) = home_with(None);
        check(&home, "0.1.2").expect("missing is fine");
        assert!(!home.dir.exists());
    }

    #[test]
    fn check_accepts_an_equal_or_older_marker() {
        for m in ["0.1.2\n", "0.1.1\n", "0.0.9"] {
            let (_t, home) = home_with(Some(m.as_bytes()));
            check(&home, "0.1.2").expect("equal or older");
        }
    }

    #[test]
    fn check_rejects_a_marker_that_is_not_a_version() {
        let (_t, home) = home_with(Some(b"garbage\n"));
        let err = check(&home, "0.1.2").expect_err("garbage");
        assert_eq!(
            err.to_string(),
            format!(
                "{} does not hold a version (got \"garbage\"): delete it and run factory upgrade",
                home.version_file().display()
            )
        );
    }

    #[test]
    fn check_rejects_short_and_long_versions() {
        for m in ["0.1", "1.2.3.4", "1.x.3"] {
            let (_t, home) = home_with(Some(m.as_bytes()));
            let err = check(&home, "0.1.2").expect_err("bad shape");
            assert!(err.to_string().contains("does not hold a version"), "{err}");
        }
    }

    #[test]
    fn check_rejects_an_empty_marker() {
        for m in ["", "  \n"] {
            let (_t, home) = home_with(Some(m.as_bytes()));
            let err = check(&home, "0.1.2").expect_err("empty");
            assert!(err.to_string().contains("(got \"\")"), "{err}");
        }
    }

    #[test]
    fn check_rejects_a_marker_that_is_not_utf8() {
        let (_t, home) = home_with(Some(&[0xff, 0xfe, b'\n']));
        let err = check(&home, "0.1.2").expect_err("not utf8");
        assert!(matches!(err, CoreError::Version(_)), "{err}");
    }

    #[test]
    fn check_rejects_an_unparseable_current() {
        let (_t, home) = home_with(Some(b"0.1.2"));
        let err = check(&home, "dev").expect_err("bad current");
        let text = err.to_string();
        assert!(text.contains("\"dev\"") && text.contains("is not a version"), "{text}");
    }

    #[test]
    fn comparison_is_numeric_not_text() {
        let (_t, home) = home_with(Some(b"0.10.0"));
        let err = check(&home, "0.9.0").expect_err("newer");
        assert!(err.to_string().contains("last used by factory 0.10.0"), "{err}");
        assert!(!stamp(&home, "0.9.0").expect("stamp"));
        assert_eq!(std::fs::read(home.version_file()).expect("read"), b"0.10.0");
    }

    #[test]
    fn suffix_after_dash_or_plus_is_ignored() {
        for m in ["0.2.0-rc.1", "0.2.0+build.5"] {
            let (_t, home) = home_with(Some(m.as_bytes()));
            check(&home, "0.2.0").expect("equal");
            assert!(!stamp(&home, "0.2.0").expect("stamp"));
            assert_eq!(std::fs::read(home.version_file()).expect("read"), m.as_bytes());
        }
    }

    #[test]
    fn read_marker_is_none_for_a_missing_file() {
        let (_t, home) = home_with(None);
        assert_eq!(read_marker(&home).expect("read"), None);
    }

    #[test]
    fn read_marker_trims_the_line() {
        let (_t, home) = home_with(Some(b"  0.1.2  \n"));
        assert_eq!(read_marker(&home).expect("read"), Some("0.1.2".to_string()));
    }

    #[test]
    fn read_marker_reports_a_directory_as_io() {
        let (_t, home) = dir_marker();
        let err = read_marker(&home).expect_err("dir");
        assert!(matches!(err, CoreError::Io { .. }), "{err}");
    }

    #[test]
    fn stamp_writes_a_missing_marker_and_creates_the_home() {
        let (_t, home) = home_with(None);
        assert!(stamp(&home, "0.1.2").expect("stamp"));
        assert_eq!(
            std::fs::read_to_string(home.version_file()).expect("read"),
            "0.1.2\n"
        );
        assert!(!stamp(&home, "0.1.2").expect("second stamp"), "equal is not rewritten");
    }

    #[test]
    fn stamp_replaces_an_older_marker() {
        let (_t, home) = home_with(Some(b"0.1.1\n"));
        assert!(stamp(&home, "0.1.2").expect("stamp"));
        assert_eq!(std::fs::read_to_string(home.version_file()).expect("read"), "0.1.2\n");
    }

    #[test]
    fn stamp_replaces_an_unparseable_marker() {
        let (_t, home) = home_with(Some(b"garbage"));
        assert!(stamp(&home, "0.1.2").expect("stamp"));
        assert_eq!(std::fs::read_to_string(home.version_file()).expect("read"), "0.1.2\n");
    }

    #[test]
    fn stamp_never_writes_an_older_version_over_a_newer_one() {
        let (_t, home) = home_with(Some(b"0.2.0\n"));
        assert!(!stamp(&home, "0.1.2").expect("stamp"));
        assert_eq!(std::fs::read_to_string(home.version_file()).expect("read"), "0.2.0\n");
    }

    #[test]
    fn stamp_reports_a_directory_marker_as_io() {
        let (_t, home) = dir_marker();
        let err = stamp(&home, "0.1.2").expect_err("dir");
        assert!(matches!(err, CoreError::Io { .. }), "{err}");
        assert!(home.version_file().is_dir());
    }

    #[test]
    fn factory_version_is_the_workspace_version() {
        assert_eq!(FACTORY_VERSION, env!("CARGO_PKG_VERSION"));
        assert!(parse(FACTORY_VERSION).is_some());
    }
}
