//! The preset manifest: one SHA-256 hash per preset file the factory wrote.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{write_atomic, CoreError};

/// The manifest file name inside the presets folder.
pub const MANIFEST_FILE: &str = ".manifest";

/// The SHA-256 of the bytes as 64 lower-case hex characters.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Hashes of the preset files, keyed by path relative to the presets folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    entries: BTreeMap<String, String>,
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn parse_line(line: &[u8]) -> Option<(String, String)> {
    let line = std::str::from_utf8(line).ok()?;
    let (hash, path) = line.split_once("  ")?;
    if !is_hash(hash) || path.is_empty() || path.starts_with('/') {
        return None;
    }
    if path.split('/').any(|part| part == "..") {
        return None;
    }
    Some((path.to_string(), hash.to_string()))
}

impl Manifest {
    /// Reads the manifest. A missing file is an empty manifest, bad lines are ignored.
    pub(crate) fn read(presets_dir: &Path) -> Result<Manifest, CoreError> {
        let path = presets_dir.join(MANIFEST_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Manifest::default()),
            Err(e) => return Err(CoreError::io(&path, e)),
        };
        let mut entries = BTreeMap::new();
        let mut duplicates = BTreeSet::new();
        for raw in bytes.split(|b| *b == b'\n') {
            if let Some((rel, hash)) = parse_line(raw) {
                if entries.insert(rel.clone(), hash).is_some() {
                    duplicates.insert(rel);
                }
            }
        }
        for rel in duplicates {
            entries.remove(&rel);
        }
        Ok(Manifest { entries })
    }

    pub(crate) fn get(&self, rel: &str) -> Option<&str> {
        self.entries.get(rel).map(String::as_str)
    }

    pub(crate) fn set(&mut self, rel: String, hash: String) {
        self.entries.insert(rel, hash);
    }

    pub(crate) fn render(&self) -> String {
        self.entries
            .iter()
            .map(|(rel, hash)| format!("{hash}  {rel}\n"))
            .collect()
    }

    /// Writes the manifest. Returns false when the file already holds the same text.
    pub(crate) fn write(&self, presets_dir: &Path) -> Result<bool, CoreError> {
        let path = presets_dir.join(MANIFEST_FILE);
        let text = self.render();
        match std::fs::read(&path) {
            Ok(old) if old == text.as_bytes() => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && self.entries.is_empty() => {
                return Ok(false);
            }
            _ => {}
        }
        write_atomic(&path, text.as_bytes())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(hash: &str, path: &str) -> String {
        format!("{hash}  {path}\n")
    }

    #[test]
    fn write_then_read_round_trips_sorted_lines() {
        let dir = tempfile::tempdir().expect("tmp");
        let a = sha256_hex(b"a");
        let b = sha256_hex(b"b");
        let mut m = Manifest::default();
        m.set("rust/config.toml".to_string(), b.clone());
        m.set("go/config.toml".to_string(), a.clone());
        assert!(m.write(dir.path()).expect("write"));
        let text = std::fs::read_to_string(dir.path().join(MANIFEST_FILE)).expect("read");
        assert_eq!(
            text,
            format!("{a}  go/config.toml\n{b}  rust/config.toml\n")
        );
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![MANIFEST_FILE.to_string()],
            "no temp file is left"
        );
        assert_eq!(Manifest::read(dir.path()).expect("read back"), m);
    }

    #[test]
    fn sha256_hex_matches_the_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_missing_manifest_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(
            Manifest::read(dir.path()).expect("read"),
            Manifest::default()
        );
        let absent = dir.path().join("absent");
        assert_eq!(Manifest::read(&absent).expect("read"), Manifest::default());
    }

    #[test]
    fn write_skips_an_identical_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut m = Manifest::default();
        m.set("rust/config.toml".to_string(), sha256_hex(b"x"));
        assert!(m.write(dir.path()).expect("first"));
        assert!(!m.write(dir.path()).expect("second"));
        let empty_dir = tempfile::tempdir().expect("tmp");
        assert!(!Manifest::default().write(empty_dir.path()).expect("empty"));
        assert!(!empty_dir.path().join(MANIFEST_FILE).exists());
    }

    #[test]
    fn bad_lines_are_ignored_and_good_lines_still_count() {
        let dir = tempfile::tempdir().expect("tmp");
        let good = sha256_hex(b"x");
        let mut text = line(&good, "rust/config.toml");
        text.push_str("nothex  go/config.toml\n");
        text.push_str(&line(&good.to_uppercase(), "go/commit-context.md"));
        text.push_str(&line(&good[..63], "go/agents/factory-builder.md"));
        text.push_str(&format!("{good} rust/commit-context.md\n"));
        text.push_str(&format!("{good}  \n"));
        text.push_str(&line(&good, "/etc/passwd"));
        text.push_str(&line(&good, "rust/../go/config.toml"));
        text.push('\n');
        std::fs::write(dir.path().join(MANIFEST_FILE), text).expect("write");
        let m = Manifest::read(dir.path()).expect("read");
        assert_eq!(m.get("rust/config.toml"), Some(good.as_str()));
        for rel in [
            "go/config.toml",
            "go/commit-context.md",
            "go/agents/factory-builder.md",
            "rust/commit-context.md",
            "/etc/passwd",
            "rust/../go/config.toml",
        ] {
            assert_eq!(m.get(rel), None, "{rel}");
        }
        assert_eq!(m.render().lines().count(), 1);
    }

    #[test]
    fn a_non_utf8_line_is_ignored() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut bytes = b"\xff\xfe  rust/config.toml\n".to_vec();
        bytes.extend_from_slice(line(&sha256_hex(b"y"), "go/config.toml").as_bytes());
        std::fs::write(dir.path().join(MANIFEST_FILE), bytes).expect("write");
        let m = Manifest::read(dir.path()).expect("read");
        assert_eq!(m.get("rust/config.toml"), None);
        assert!(m.get("go/config.toml").is_some());
    }

    #[test]
    fn a_path_listed_twice_counts_as_unmanifested() {
        let dir = tempfile::tempdir().expect("tmp");
        let h = sha256_hex(b"z");
        let mut text = line(&h, "rust/config.toml");
        text.push_str(&line(&h, "rust/config.toml"));
        text.push_str(&line(&h, "go/config.toml"));
        std::fs::write(dir.path().join(MANIFEST_FILE), text).expect("write");
        let m = Manifest::read(dir.path()).expect("read");
        assert_eq!(m.get("rust/config.toml"), None);
        assert!(m.get("go/config.toml").is_some());
    }

    #[test]
    fn an_unreadable_manifest_is_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir(dir.path().join(MANIFEST_FILE)).expect("mkdir");
        let err = Manifest::read(dir.path()).expect_err("a directory is no manifest");
        match err {
            CoreError::Io { path, .. } => {
                assert!(path.ends_with(".manifest"), "{path:?}")
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
