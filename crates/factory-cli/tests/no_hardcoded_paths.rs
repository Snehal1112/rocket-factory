//! The engine, the git crate and the CLI take every path and the base branch from outside.

use std::path::{Path, PathBuf};

const ALLOWED: &[&str] = &[
    "factory-core/src/layout.rs",
    "factory-git/src/repo.rs",
    "factory-cli/src/bin/factory-shots.rs",
    // The fake Claude reads the agent file from the folder the real `claude --agent` searches.
    "factory-agent/src/bin/fake-claude.rs",
    // The default risk patterns are a policy, not a path the driver opens. Plan 3 moves them into presets.
    "factory-core/src/config.rs",
    // The docs-only rule names Rocket folders. It is a policy too, and plan 3 moves it into presets.
    "factory-core/src/risk.rs",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_names_a_state_path_or_the_main_branch() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut hits = Vec::new();
    for name in [
        "factory-engine",
        "factory-git",
        "factory-cli",
        "factory-agent",
        "factory-checks",
        "factory-core",
        "factory-supervisor",
    ] {
        let mut files = Vec::new();
        rust_files(&crates.join(name).join("src"), &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&crates)
                .expect("under crates")
                .to_string_lossy()
                .replace('\\', "/");
            if ALLOWED.contains(&rel.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read");
            // Everything from the first test module on is test code.
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for (n, line) in code.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") {
                    continue;
                }
                if line.contains("\".superpowers")
                    || line.contains("\".claude")
                    || line.contains("\"main\"")
                {
                    hits.push(format!("{rel}:{}: {}", n + 1, t));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "hardcoded paths or branch:\n{}",
        hits.join("\n")
    );
}

fn root_text(rel: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel),
    )
    .unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The crate names in the `members` block of the root `Cargo.toml`, read as text.
fn workspace_crates() -> Vec<String> {
    let manifest = root_text("Cargo.toml");
    let members = manifest
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("workspace members in Cargo.toml");
    let names: Vec<String> = members
        .lines()
        .filter_map(|line| {
            let path = line.trim().trim_end_matches(',').trim_matches('"');
            path.strip_prefix("crates/").map(str::to_string)
        })
        .collect();
    assert!(
        !names.is_empty(),
        "no workspace members parsed from Cargo.toml"
    );
    names
}

#[test]
fn ci_and_release_list_every_workspace_crate() {
    let ci = root_text(".github/workflows/ci.yml");
    let release = root_text("scripts/release.sh");
    let crates_block = release
        .split("CRATES=(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("CRATES list in scripts/release.sh");
    let release_names: Vec<&str> = crates_block.split_whitespace().collect();
    for name in workspace_crates() {
        assert!(
            ci.contains(&format!("-p {name}\n")),
            "{name} is missing from PKGS in .github/workflows/ci.yml"
        );
        assert!(
            release_names.contains(&name.as_str()),
            "{name} is missing from CRATES in scripts/release.sh"
        );
    }
}

/// The non-comment lines before the test module that hold a `/proc` string literal.
fn proc_lines(text: &str) -> Vec<String> {
    // Everything from the first test module on is test code.
    let code = text.split("#[cfg(test)]").next().unwrap_or("");
    code.lines()
        .filter(|line| !line.trim_start().starts_with("//") && line.contains("\"/proc"))
        .map(|line| line.trim_start().to_string())
        .collect()
}

#[test]
fn the_proc_lint_catches_a_proc_literal() {
    let src = "//! Reads nothing from /proc.\nfn f() { let _ = std::fs::read(\"/proc/self/stat\"); }\n#[cfg(test)]\nmod tests { const P: &str = \"/proc/1\"; }\n";
    assert_eq!(proc_lines(src).len(), 1);
}

#[test]
fn no_source_reads_proc() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut hits = Vec::new();
    for name in workspace_crates() {
        let mut files = Vec::new();
        rust_files(&crates.join(&name).join("src"), &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read");
            for line in proc_lines(&text) {
                hits.push(format!("{}: {line}", file.display()));
            }
        }
    }
    assert!(hits.is_empty(), "source reads /proc:\n{}", hits.join("\n"));
}

#[test]
fn ci_runs_on_linux_and_macos() {
    let ci = root_text(".github/workflows/ci.yml");
    for needle in [
        "os: [ubuntu-latest, macos-latest]",
        "fail-fast: false",
        "runs-on: ${{ matrix.os }}",
    ] {
        assert_eq!(
            ci.matches(needle).count(),
            2,
            "{needle} must appear once per job"
        );
    }
}
