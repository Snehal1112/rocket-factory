//! The secret of the local API: a random token in a file only the owner can read.

use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

use factory_core::Home;

const TOKEN_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token file {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("token file {path} is not a valid token")]
    Invalid { path: String },
}

fn io_err(home: &Home, e: std::io::Error) -> TokenError {
    TokenError::Io { path: home.token_file().display().to_string(), reason: e.to_string() }
}

fn random_hex() -> std::io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads the token. A missing or malformed file is an error.
pub fn read(home: &Home) -> Result<String, TokenError> {
    let text = std::fs::read_to_string(home.token_file()).map_err(|e| io_err(home, e))?;
    let token = text.trim().to_string();
    if token.len() == TOKEN_BYTES * 2 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(token)
    } else {
        Err(TokenError::Invalid { path: home.token_file().display().to_string() })
    }
}

/// Returns the token, creating the file (mode 0600) only when it is absent. An existing file is never rewritten.
pub fn ensure(home: &Home) -> Result<String, TokenError> {
    let path = home.token_file();
    std::fs::create_dir_all(&home.dir).map_err(|e| io_err(home, e))?;
    let token = random_hex().map_err(|e| io_err(home, e))?;
    let created = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path);
    match created {
        Ok(mut f) => {
            f.write_all(token.as_bytes()).map_err(|e| io_err(home, e))?;
            Ok(token)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read(home),
        Err(e) => Err(io_err(home, e)),
    }
}

/// Compares in constant time for equal lengths. Different lengths are never equal.
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn home() -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let h = Home::at(t.path().to_path_buf());
        (t, h)
    }

    #[test]
    fn ensure_creates_64_hex_chars_with_mode_0600() {
        let (_t, h) = home();
        let tok = ensure(&h).expect("ensure");
        assert_eq!(tok.len(), 64);
        assert!(tok.bytes().all(|b| b.is_ascii_hexdigit()));
        let mode = std::fs::metadata(h.token_file()).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_second_ensure_returns_the_same_value() {
        let (_t, h) = home();
        let a = ensure(&h).expect("first");
        let b = ensure(&h).expect("second");
        assert_eq!(a, b);
        assert_eq!(read(&h).expect("read"), a);
    }

    #[test]
    fn read_of_a_missing_file_is_an_error() {
        let (_t, h) = home();
        assert!(read(&h).is_err());
    }

    #[test]
    fn matches_rejects_length_empty_and_one_byte_differences() {
        assert!(matches("abcd", "abcd"));
        assert!(!matches("abcd", "abc"));
        assert!(!matches("abcd", "abcde"));
        assert!(!matches("", ""));
        assert!(!matches("abcd", ""));
        assert!(!matches("abcd", "abce"));
    }
}
