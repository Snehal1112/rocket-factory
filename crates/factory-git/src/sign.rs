//! Signs commit buffers the way `git commit -S` would, with no prompt.

use crate::merge::signature;
use git2::Repository;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long a signing program may run before it is killed.
pub(crate) const SIGN_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest reason text, in characters.
const MAX_REASON: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignFormat {
    OpenPgp,
    X509,
    Ssh,
}

#[derive(Debug, Clone)]
pub(crate) struct Signer {
    pub(crate) format: SignFormat,
    pub(crate) program: String,
    pub(crate) key: String,
    pub(crate) timeout: Duration,
}

/// None when `commit.gpgsign` is not true. Err with a reason when signing is
/// on but cannot be set up.
pub(crate) fn signer_for(repo: &Repository) -> Option<Result<Signer, String>> {
    let cfg = repo.config().ok()?;
    if !cfg.get_bool("commit.gpgsign").unwrap_or(false) {
        return None;
    }
    let get = |name: &str| cfg.get_string(name).ok().filter(|v| !v.is_empty());
    let format = match get("gpg.format").as_deref() {
        None | Some("openpgp") => SignFormat::OpenPgp,
        Some("x509") => SignFormat::X509,
        Some("ssh") => SignFormat::Ssh,
        Some(other) => return Some(Err(limit(format!("unknown gpg.format {other}")))),
    };
    let program = match format {
        SignFormat::OpenPgp => get("gpg.openpgp.program")
            .or_else(|| get("gpg.program"))
            .unwrap_or_else(|| "gpg".to_string()),
        SignFormat::X509 => get("gpg.x509.program").unwrap_or_else(|| "gpgsm".to_string()),
        SignFormat::Ssh => get("gpg.ssh.program").unwrap_or_else(|| "ssh-keygen".to_string()),
    };
    let key = match get("user.signingkey") {
        Some(key) => key,
        None if format == SignFormat::Ssh => {
            return Some(Err("no user.signingkey for ssh signing".to_string()));
        }
        None => match signature(repo) {
            Ok(sig) => format!(
                "{} <{}>",
                sig.name().unwrap_or_default(),
                sig.email().unwrap_or_default()
            ),
            Err(e) => return Some(Err(limit(format!("no committer for signing: {e}")))),
        },
    };
    Some(Ok(Signer {
        format,
        program,
        key,
        timeout: SIGN_TIMEOUT,
    }))
}

impl Signer {
    /// The armored signature for `buffer` with one trailing newline removed,
    /// or a one-line reason.
    pub(crate) fn sign(&self, buffer: &str) -> Result<String, String> {
        let raw = match self.format {
            SignFormat::OpenPgp | SignFormat::X509 => self.sign_gpg(buffer)?,
            SignFormat::Ssh => self.sign_ssh(buffer)?,
        };
        Ok(raw.strip_suffix('\n').unwrap_or(&raw).to_string())
    }

    fn sign_gpg(&self, buffer: &str) -> Result<String, String> {
        let mut cmd = Command::new(&self.program);
        cmd.args([
            "--status-fd=2",
            "--pinentry-mode",
            "error",
            "-bsau",
            &self.key,
        ]);
        let out = self.run(cmd, Some(buffer))?;
        let created = out
            .stderr
            .lines()
            .any(|line| line.starts_with("[GNUPG:] SIG_CREATED "));
        if !created {
            return Err(limit(format!("{} made no signature", self.program)));
        }
        Ok(out.stdout)
    }

    fn sign_ssh(&self, buffer: &str) -> Result<String, String> {
        let mut temps = TempFiles::default();
        let buffer_file = temps.create(buffer)?;
        let sig_file = PathBuf::from(format!("{}.sig", buffer_file.display()));
        temps.0.push(sig_file.clone());
        let literal = self
            .key
            .strip_prefix("key::")
            .or_else(|| self.key.starts_with("ssh-").then_some(self.key.as_str()));
        let (key_file, agent) = match literal {
            Some(public) => (temps.create(public)?, true),
            None => (PathBuf::from(expand_home(&self.key)), false),
        };
        let mut cmd = Command::new(&self.program);
        cmd.args(["-Y", "sign", "-n", "git", "-f"]).arg(&key_file);
        if agent {
            cmd.arg("-U");
        }
        cmd.arg(&buffer_file).env("SSH_ASKPASS_REQUIRE", "never");
        self.run(cmd, None)?;
        std::fs::read_to_string(&sig_file)
            .map_err(|_| limit(format!("{} made no signature", self.program)))
    }

    /// Runs the program with stdin from `input` (or null), and kills it after
    /// the timeout.
    fn run(&self, mut cmd: Command, input: Option<&str>) -> Result<Output, String> {
        cmd.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| limit(format!("could not run {}: {e}", self.program)))?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            let text = text.to_string();
            // A program that exits early closes the pipe, so a write error is fine.
            std::thread::spawn(move || {
                let _ = stdin.write_all(text.as_bytes());
            });
        }
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "signing timed out after {} s",
                        self.timeout.as_secs()
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(limit(format!("could not run {}: {e}", self.program))),
            }
        };
        let out = Output {
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        };
        if !status.success() {
            let code = status
                .code()
                .map_or_else(|| "a signal".to_string(), |c| c.to_string());
            let last = out
                .stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.starts_with("[GNUPG:]"))
                .unwrap_or("");
            let reason = if last.is_empty() {
                format!("{} exited with {code}", self.program)
            } else {
                format!("{} exited with {code}: {}", self.program, last.trim())
            };
            return Err(limit(reason));
        }
        Ok(out)
    }
}

struct Output {
    stdout: String,
    stderr: String,
}

/// Reads a pipe to the end on its own thread, so a full pipe never blocks the child.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn limit(reason: String) -> String {
    let line = reason.lines().next().unwrap_or_default();
    line.chars().take(MAX_REASON).collect()
}

fn expand_home(key: &str) -> String {
    match (key.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => key.to_string(),
    }
}

/// Temporary files that are removed on drop, also when signing fails.
#[derive(Default)]
struct TempFiles(Vec<PathBuf>);

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempFiles {
    fn create(&mut self, content: &str) -> Result<PathBuf, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "factory-sign-{}-{}-{nanos}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| file.write_all(content.as_bytes()));
        // Track the path first, so a partial file is removed too.
        self.0.push(path.clone());
        written.map_err(|e| limit(format!("could not write {}: {e}", path.display())))?;
        Ok(path)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::TempDir;

    fn repo_with(gpgsign: bool, settings: &[(&str, &str)]) -> (TempDir, Repository) {
        let dir = TempDir::new().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");
        let mut cfg = repo.config().expect("config");
        cfg.set_bool("commit.gpgsign", gpgsign).expect("gpgsign");
        // Empty values hide the machine's global signing settings.
        for name in [
            "gpg.format",
            "gpg.program",
            "gpg.openpgp.program",
            "user.signingkey",
        ] {
            cfg.set_str(name, "").expect("blank setting");
        }
        for (name, value) in settings {
            cfg.set_str(name, value).expect("setting");
        }
        drop(cfg);
        (dir, repo)
    }

    fn script(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path.display().to_string()
    }

    /// Signs "x", trying again while a parallel test's fork still holds the fresh script open for writing.
    fn sign_retrying(signer: &Signer) -> Result<String, String> {
        let mut result = signer.sign("x");
        for _ in 0..20 {
            match &result {
                Err(reason) if reason.contains("Text file busy") => {
                    std::thread::sleep(Duration::from_millis(50));
                    result = signer.sign("x");
                }
                _ => break,
            }
        }
        result
    }

    fn signer(format: SignFormat, program: String, key: &str, timeout: Duration) -> Signer {
        Signer {
            format,
            program,
            key: key.to_string(),
            timeout,
        }
    }

    #[test]
    fn signer_reads_openpgp_settings() {
        let (_dir, repo) = repo_with(
            true,
            &[
                ("gpg.openpgp.program", "/opt/gpg2"),
                ("gpg.program", "/usr/bin/other"),
                ("user.signingkey", "ABC"),
            ],
        );
        let signer = signer_for(&repo).expect("signing on").expect("signer");
        assert_eq!(signer.format, SignFormat::OpenPgp);
        assert_eq!(signer.program, "/opt/gpg2");
        assert_eq!(signer.key, "ABC");
        assert_eq!(signer.timeout, SIGN_TIMEOUT);
    }

    #[test]
    fn signer_falls_back_to_gpg_program_then_gpg() {
        let (_a, repo) = repo_with(
            true,
            &[("gpg.program", "/usr/bin/g2"), ("user.signingkey", "ABC")],
        );
        let signer = signer_for(&repo).expect("signing on").expect("signer");
        assert_eq!(signer.program, "/usr/bin/g2");
        let (_b, repo) = repo_with(true, &[("user.signingkey", "ABC")]);
        let signer = signer_for(&repo).expect("signing on").expect("signer");
        assert_eq!(signer.program, "gpg");
    }

    #[test]
    fn signer_uses_the_committer_when_no_key_is_set() {
        let (_dir, repo) = repo_with(true, &[("user.name", "Ann"), ("user.email", "ann@x.io")]);
        let signer = signer_for(&repo).expect("signing on").expect("signer");
        assert_eq!(signer.key, "Ann <ann@x.io>");
    }

    #[test]
    fn ssh_without_a_key_is_an_error() {
        let (_dir, repo) = repo_with(true, &[("gpg.format", "ssh")]);
        let got = signer_for(&repo).expect("signing on");
        assert_eq!(
            got.expect_err("no key"),
            "no user.signingkey for ssh signing"
        );
    }

    #[test]
    fn an_unknown_format_is_an_error() {
        let (_dir, repo) = repo_with(true, &[("gpg.format", "pgp2")]);
        let got = signer_for(&repo).expect("signing on");
        assert_eq!(got.expect_err("bad format"), "unknown gpg.format pgp2");
    }

    #[test]
    fn no_signer_without_gpgsign() {
        let (_dir, repo) = repo_with(false, &[("user.signingkey", "ABC")]);
        assert!(signer_for(&repo).is_none());
    }

    #[test]
    fn a_hanging_program_times_out() {
        let dir = TempDir::new().expect("tempdir");
        let program = script(dir.path(), "slow", "sleep 5");
        let s = signer(SignFormat::OpenPgp, program, "ABC", Duration::from_secs(1));
        let start = Instant::now();
        let err = sign_retrying(&s).expect_err("timeout");
        assert_eq!(err, "signing timed out after 1 s");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn no_sig_created_line_is_no_signature() {
        let dir = TempDir::new().expect("tempdir");
        let program = script(dir.path(), "junk", "cat >/dev/null; echo junk-signature");
        let s = signer(SignFormat::OpenPgp, program, "ABC", SIGN_TIMEOUT);
        let err = sign_retrying(&s).expect_err("no signature");
        assert!(err.ends_with("made no signature"), "{err}");
    }

    #[test]
    fn a_status_line_makes_a_signature_without_its_final_newline() {
        let dir = TempDir::new().expect("tempdir");
        let program = script(
            dir.path(),
            "good",
            "cat >/dev/null; echo '[GNUPG:] SIG_CREATED D 1 8 00 1 X' >&2; echo SIG",
        );
        let s = signer(SignFormat::OpenPgp, program, "ABC", SIGN_TIMEOUT);
        assert_eq!(sign_retrying(&s).expect("signature"), "SIG");
    }

    #[test]
    fn the_reason_skips_gnupg_status_lines() {
        let dir = TempDir::new().expect("tempdir");
        let program = script(
            dir.path(),
            "fail",
            "echo \"gpg: signing failed: No pinentry\" >&2; echo \"[GNUPG:] FAILURE sign 67108949\" >&2; exit 2",
        );
        let s = signer(SignFormat::OpenPgp, program.clone(), "ABC", SIGN_TIMEOUT);
        let err = sign_retrying(&s).expect_err("failure");
        assert_eq!(
            err,
            format!("{program} exited with 2: gpg: signing failed: No pinentry")
        );
    }

    #[test]
    fn ssh_temp_files_are_removed_on_failure() {
        let dir = TempDir::new().expect("tempdir");
        let args = dir.path().join("args");
        let program = script(
            dir.path(),
            "fake-keygen",
            &format!("echo \"$@\" > {}\nexit 1", args.display()),
        );
        let s = signer(
            SignFormat::Ssh,
            program,
            "key::ssh-ed25519 AAAA x",
            SIGN_TIMEOUT,
        );
        sign_retrying(&s).expect_err("failure");
        let recorded = std::fs::read_to_string(&args).expect("args file");
        let words: Vec<&str> = recorded.split_whitespace().collect();
        let after_f = words
            .iter()
            .position(|w| *w == "-f")
            .and_then(|i| words.get(i + 1))
            .expect("key file argument");
        let last = words.last().expect("buffer file argument");
        assert!(!Path::new(after_f).exists());
        assert!(!Path::new(last).exists());
    }
}
