//! Asks the `factory` binary for its version.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The version `<bin> --version` prints, waiting at most 5 seconds.
pub fn binary_version(bin: &Path) -> Result<String, String> {
    version_within(bin, Duration::from_secs(5))
}

/// A warning text when the binary version differs from `own`, else `None`.
pub fn mismatch(bin: &Path, own: &str) -> Option<String> {
    match binary_version(bin) {
        Ok(v) if v == own => None,
        Ok(v) => Some(format!(
            "factory binary {} is {v}, factoryd is {own}",
            bin.display()
        )),
        Err(why) => Some(format!(
            "cannot read the version of {}: {why}",
            bin.display()
        )),
    }
}

fn version_within(bin: &Path, timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A script written just before it runs can briefly be busy. Retry a few times.
    let mut tries = 0;
    let mut child = loop {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(crate::spawn::TEXT_FILE_BUSY) && tries < 20 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.to_string()),
            Ok(c) => break c,
        }
    };
    let end = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("no answer within {} ms", timeout.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        return Err(match status.code() {
            Some(n) => format!("exited with code {n}"),
            None => "exited with code none".to_string(),
        });
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_string(&mut text).map_err(|e| e.to_string())?;
    }
    let first = text.lines().next().unwrap_or("").trim();
    match first.split_whitespace().last() {
        Some(w) if w.starts_with(|c: char| c.is_ascii_digit()) => Ok(w.to_string()),
        _ => Err(format!("no version in {first:?}")),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::spawn::tests::write_script;
    use std::time::{Duration, Instant};

    #[test]
    fn a_different_version_is_reported() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "echo \"factory 0.1.1\"");
        assert_eq!(binary_version(&bin).expect("version"), "0.1.1");
        let text = mismatch(&bin, "0.1.2").expect("a mismatch");
        assert_eq!(
            text,
            format!(
                "factory binary {} is 0.1.1, factoryd is 0.1.2",
                bin.display()
            )
        );
    }

    #[test]
    fn a_matching_binary_has_no_mismatch() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "echo \"factory 0.1.2\"");
        assert!(mismatch(&bin, "0.1.2").is_none());
    }

    #[test]
    fn a_binary_that_cannot_run_is_reported() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = t.path().join("missing");
        let text = mismatch(&bin, "0.1.2").expect("a mismatch");
        assert!(
            text.starts_with(&format!("cannot read the version of {}: ", bin.display())),
            "{text}"
        );
    }

    #[test]
    fn output_without_a_version_word_cannot_be_read() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "echo factory");
        binary_version(&bin).expect_err("no version");
        let text = mismatch(&bin, "0.1.2").expect("a mismatch");
        assert!(text.starts_with("cannot read the version of "), "{text}");
    }

    #[test]
    fn a_binary_that_fails_is_reported() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "echo \"factory 0.1.2\"; exit 3");
        let e = binary_version(&bin).expect_err("non-zero exit");
        assert!(e.contains("exited with code 3"), "{e}");
    }

    #[test]
    fn a_binary_that_hangs_is_reported() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "sleep 30");
        let start = Instant::now();
        let e = version_within(&bin, Duration::from_millis(300)).expect_err("timeout");
        assert!(e.contains("no answer within"), "{e}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn only_the_first_line_counts() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(
            t.path(),
            "factory",
            "printf 'factory 0.1.2 \\r\\nother 9.9.9\\n'",
        );
        assert_eq!(binary_version(&bin).expect("version"), "0.1.2");
    }

    #[test]
    fn a_pre_release_binary_is_reported() {
        let t = tempfile::tempdir().expect("tempdir");
        let bin = write_script(t.path(), "factory", "echo \"factory 0.1.2-rc.1\"");
        let text = mismatch(&bin, "0.1.2").expect("a mismatch");
        assert!(text.contains("is 0.1.2-rc.1, factoryd is 0.1.2"), "{text}");
    }
}
