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

#[test]
fn ci_and_release_list_every_workspace_crate() {
    let manifest = root_text("Cargo.toml");
    let ci = root_text(".github/workflows/ci.yml");
    let release = root_text("scripts/release.sh");
    let crates_block = release
        .split("CRATES=(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("CRATES list in scripts/release.sh");
    let release_names: Vec<&str> = crates_block.split_whitespace().collect();
    let members = manifest
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("workspace members in Cargo.toml");
    let mut seen = 0;
    for line in members.lines() {
        let path = line.trim().trim_end_matches(',').trim_matches('"');
        let Some(name) = path.strip_prefix("crates/") else {
            continue;
        };
        seen += 1;
        assert!(
            ci.contains(&format!("-p {name}\n")),
            "{name} is missing from PKGS in .github/workflows/ci.yml"
        );
        assert!(
            release_names.contains(&name),
            "{name} is missing from CRATES in scripts/release.sh"
        );
    }
    assert!(seen > 0, "no workspace members parsed from Cargo.toml");
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
