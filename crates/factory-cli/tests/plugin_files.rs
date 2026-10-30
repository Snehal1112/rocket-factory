use std::path::{Path, PathBuf};

use factory_cli::{install, plugin};
use factory_core::plugin_rel_files;
use factory_core::version::FACTORY_VERSION;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("read json file");
    serde_json::from_str(&text).expect("parse json file")
}

#[test]
fn plugin_files_match_generated_text() {
    if let Err(error) = plugin::check(&root()) {
        assert!(error.to_string().contains("factory plugin sync"), "{error}");
        panic!("{error}");
    }
}

#[test]
fn check_reports_drift_in_a_temp_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let files = plugin_rel_files();
    for rel in [files[0], files[1]] {
        let target = dir.path().join(rel);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        std::fs::copy(root().join(rel), &target).expect("copy");
    }
    let command = dir.path().join(files[1]);
    let mut text = std::fs::read_to_string(&command).expect("read command");
    text.push_str("drift\n");
    std::fs::write(&command, text).expect("write drift");
    let error = plugin::check(dir.path()).expect_err("drift must fail");
    assert!(error.to_string().contains(files[1]), "{error}");
    plugin::sync(dir.path()).expect("sync");
    plugin::check(dir.path()).expect("check after sync");
    let again = plugin::sync(dir.path()).expect("second sync");
    assert!(
        again.iter().all(|line| line.starts_with("unchanged ")),
        "{again:?}"
    );
}

#[test]
fn plugin_json_version_matches_crate() {
    let json = read_json(&root().join(plugin_rel_files()[0]));
    assert_eq!(json["version"], FACTORY_VERSION);
    assert_eq!(json["name"], "factory");
}

#[test]
fn marketplace_names_the_plugin() {
    let json = read_json(&root().join(".claude-plugin/marketplace.json"));
    assert_eq!(json["name"], "rocket-factory");
    assert_eq!(json["plugins"][0]["name"], "factory");
    assert_eq!(json["plugins"][0]["source"], "./plugins/factory");
    assert!(root()
        .join("plugins/factory/.claude-plugin/plugin.json")
        .is_file());
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            walk(&path, base, out);
        } else {
            let rel = path.strip_prefix(base).expect("prefix");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn plugin_dir_holds_only_declared_files() {
    let base = root();
    let mut found = Vec::new();
    walk(&base.join("plugins/factory"), &base, &mut found);
    found.sort();
    let mut expected: Vec<String> = plugin_rel_files().iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn command_names_the_path_failure() {
    let text = install::claude_command_text();
    assert!(text.contains("factory binaries are not on PATH"));
    assert!(text.contains("FACTORY_BIN"));
}

fn release_script() -> String {
    std::fs::read_to_string(root().join("scripts/release.sh")).expect("read release.sh")
}

fn line_index(lines: &[&str], pred: impl Fn(&str) -> bool) -> usize {
    lines
        .iter()
        .position(|l| pred(l))
        .expect("expected line in release.sh")
}

#[test]
fn release_script_syncs_and_adds_plugin_files() {
    let text = release_script();
    let lines: Vec<&str> = text.lines().collect();
    let bump = line_index(&lines, |l| {
        l.contains("set_section_version \"$DASH_MANIFEST\" package")
    });
    let sync = line_index(&lines, |l| {
        l.starts_with("cargo run") && l.contains("plugin sync") && !l.contains("--check")
    });
    let add = line_index(&lines, |l| {
        l.contains("git add") && l.contains("plugins/factory")
    });
    assert!(sync > bump, "plugin sync must run after the version bump");
    assert!(add > sync, "git add must come after plugin sync");
}

#[test]
fn release_script_gate_checks_plugin_drift() {
    let text = release_script();
    let lines: Vec<&str> = text.lines().collect();
    let check = line_index(&lines, |l| l.contains("plugin sync --check"));
    let gate = line_index(&lines, |l| l.contains("CI gate passed"));
    assert!(check < gate);
}

#[test]
fn release_script_still_parses() {
    let out = match std::process::Command::new("bash")
        .arg("-n")
        .arg(root().join("scripts/release.sh"))
        .output()
    {
        Ok(out) => out,
        Err(err) => {
            eprintln!("bash cannot be spawned, skipping: {err}");
            return;
        }
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn docs_lead_with_the_plugin_commands() {
    let guide = std::fs::read_to_string(root().join("docs/guides/getting-started.md"))
        .expect("read getting-started");
    let add = "claude plugin marketplace add Snehal1112/rocket-factory";
    assert!(guide.contains(add));
    assert!(guide.contains("claude plugin install factory@rocket-factory"));
    let start = guide
        .find("## 6. Install the `/factory` command")
        .expect("section 6");
    let section = &guide[start..];
    let end = section[3..].find("\n## ").map_or(section.len(), |i| i + 3);
    let section = &section[..end];
    let add_at = section.find(add).expect("plugin add inside section 6");
    let install_at = section
        .find("factory install")
        .expect("factory install in section 6");
    assert!(add_at < install_at);
    let readme = std::fs::read_to_string(root().join("README.md")).expect("read README");
    assert!(readme.contains("factory@rocket-factory"));
}
