mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use factory_agent::installed_plugin_ids;
use factory_cli::plugin::{
    duplicate_note, install_hint, plugin_installed, switch_note, update_line, PLUGIN_ID,
};
use factory_cli::{install, upgrade};
use factory_core::{claude_commands_dir, Home};

const INSTALLED: &str = r#"[{"id":"factory@rocket-factory","enabled":true}]"#;
const NOT_INSTALLED: &str = "[]";

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Sets environment variables under one lock and restores them on drop.
struct EnvGuard {
    old: Vec<(&'static str, Option<String>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, &str)]) -> EnvGuard {
        let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let old = vars
            .iter()
            .map(|(key, value)| {
                let previous = std::env::var(key).ok();
                std::env::set_var(key, value);
                (*key, previous)
            })
            .collect();
        EnvGuard { old, _lock: lock }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in &self.old {
            match previous {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf8")
}

/// A temp home, the fake program, and `HOME` and `FAKE_PLUGIN_LIST` set for the test.
struct Fixture {
    tmp: tempfile::TempDir,
    program: OsString,
    _env: EnvGuard,
}

impl Fixture {
    fn new(list: &str) -> Option<Fixture> {
        let program = common::fake_claude()?.into_os_string();
        let tmp = tempfile::tempdir().expect("tempdir");
        let env = EnvGuard::set(&[("HOME", s(tmp.path())), ("FAKE_PLUGIN_LIST", list)]);
        Some(Fixture {
            tmp,
            program,
            _env: env,
        })
    }

    fn commands(&self) -> PathBuf {
        claude_commands_dir(self.tmp.path())
    }

    fn old_command(&self) -> PathBuf {
        self.commands().join("factory.md")
    }

    fn write_old(&self, text: &str) {
        std::fs::create_dir_all(self.commands()).expect("mkdir");
        std::fs::write(self.old_command(), text).expect("write");
    }

    fn upgrade(&self, dry_run: bool) -> String {
        let home = Home::at(self.tmp.path().join("factory-home"));
        upgrade::run_with_claude(&home, &self.commands(), dry_run, &self.program).expect("upgrade")
    }

    fn install(&self, dir: Option<&Path>, force: bool) -> String {
        install::run_with_claude(dir, force, &self.program).expect("install")
    }
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| id.to_string()).collect()
}

#[test]
fn detects_the_plugin_by_id() {
    assert!(plugin_installed(&ids(&[PLUGIN_ID])));
    assert!(plugin_installed(&ids(&["a@b", PLUGIN_ID])));
    assert!(!plugin_installed(&ids(&["factory@other"])));
    assert!(!plugin_installed(&[]));
}

#[test]
fn detects_nothing_from_bad_output() {
    let Some(program) = common::fake_claude() else {
        return;
    };
    let _env = EnvGuard::set(&[("FAKE_PLUGIN_LIST", "not json")]);
    assert!(installed_plugin_ids(program.as_os_str()).is_empty());
    let missing = std::path::Path::new("/nonexistent/claude-does-not-exist");
    assert!(installed_plugin_ids(missing.as_os_str()).is_empty());
}

#[test]
fn install_prints_the_plugin_hint_when_not_installed() {
    let Some(fx) = Fixture::new(NOT_INSTALLED) else {
        return;
    };
    let out = fx.install(None, false);
    assert!(out.contains(&install_hint()), "{out}");
    assert!(
        out.contains(&format!("installed {}", fx.old_command().display())),
        "{out}"
    );
    assert!(fx.old_command().is_file());
}

#[test]
fn install_skips_the_command_when_plugin_installed() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    let out = fx.install(None, false);
    assert!(
        out.contains("skipped Claude command: the factory plugin is installed"),
        "{out}"
    );
    assert!(!fx.old_command().exists());
    let forced = fx.install(None, true);
    assert!(fx.old_command().is_file());
    assert!(
        forced.contains(&duplicate_note(&fx.old_command())),
        "{forced}"
    );
}

#[test]
fn install_with_plugin_and_old_command_says_remove() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    fx.write_old("old\n");
    let out = fx.install(None, false);
    assert!(out.contains(&duplicate_note(&fx.old_command())), "{out}");
    assert_eq!(
        std::fs::read_to_string(fx.old_command()).expect("old"),
        "old\n"
    );
}

#[test]
fn install_with_dir_ignores_plugin_state() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    let dir = fx.tmp.path().join("custom");
    let out = fx.install(Some(&dir), false);
    assert!(dir.join("factory.md").is_file(), "{out}");
    assert!(!out.contains("skipped"), "{out}");
}

#[test]
fn upgrade_with_plugin_and_old_command_says_remove() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    fx.write_old("old text\n");
    let out = fx.upgrade(false);
    assert!(out.contains(&duplicate_note(&fx.old_command())), "{out}");
    assert!(out.contains(&format!("rm {}", fx.old_command().display())));
    assert_eq!(std::fs::read(fx.old_command()).expect("old"), b"old text\n");
    assert!(!fx.commands().join("factory.md.bak").exists());
}

#[test]
fn upgrade_with_plugin_only_prints_update_line() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    let out = fx.upgrade(false);
    assert!(out.contains(&update_line()), "{out}");
    assert!(!fx.old_command().exists());
}

#[test]
fn upgrade_without_plugin_adds_the_switch_note() {
    let Some(fx) = Fixture::new(NOT_INSTALLED) else {
        return;
    };
    fx.write_old("old text\n");
    let out = fx.upgrade(false);
    assert!(out.contains("updated "), "{out}");
    assert!(out.contains(&switch_note(&fx.old_command())), "{out}");
    assert_eq!(
        std::fs::read_to_string(fx.old_command()).expect("new"),
        install::GLOBAL_COMMAND
    );
    let again = fx.upgrade(false);
    assert!(
        again.contains(&format!(
            "already installed: {}",
            fx.old_command().display()
        )),
        "{again}"
    );
}

#[test]
fn upgrade_without_plugin_or_command_prints_the_install_hint() {
    let Some(fx) = Fixture::new(NOT_INSTALLED) else {
        return;
    };
    let out = fx.upgrade(true);
    assert!(out.contains(&install_hint()), "{out}");
}

#[test]
fn upgrade_dry_run_with_plugin_writes_nothing() {
    let Some(fx) = Fixture::new(INSTALLED) else {
        return;
    };
    fx.write_old("old text\n");
    let out = fx.upgrade(true);
    assert!(out.contains(&duplicate_note(&fx.old_command())), "{out}");
    assert_eq!(std::fs::read(fx.old_command()).expect("old"), b"old text\n");
    assert!(!fx.commands().join("factory.md.bak").exists());
}
