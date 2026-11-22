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
    for name in workspace_crates() {
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
            hits.extend(path_hits(&rel, &text));
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
    crates_in(&root_text("Cargo.toml"))
}

/// The crate names in the `members` block of a manifest. A member outside `crates/` panics.
fn crates_in(manifest: &str) -> Vec<String> {
    let members = manifest
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("workspace members in Cargo.toml");
    let names: Vec<String> = members
        .lines()
        .filter_map(|line| {
            let path = line.trim().trim_end_matches(',').trim_matches('"');
            if path.is_empty() {
                return None;
            }
            match path.strip_prefix("crates/") {
                Some(name) => Some(name.to_string()),
                None => {
                    panic!("workspace member {path} is not under crates/; update workspace_crates")
                }
            }
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

/// Whether a line is a whole item: it, or its code part, ends in `;` or `{}`.
fn is_one_line_item(line: &str) -> bool {
    let ends_item = |s: &str| {
        let s = s.trim();
        s.ends_with(';') || s.ends_with("{}")
    };
    ends_item(line) || ends_item(line.split("//").next().unwrap_or(line))
}

/// The lines outside test items, with their 1-based numbers.
///
/// A line that is exactly `#[cfg(test)]` after trimming starts a test item. An item whose first
/// line, or that line's code part, ends in `;` or `{}` is that one line. The code part is the
/// text before the first `//`. The whole line is checked too, so a `//` inside a string keeps
/// the old result. A braced item ends at the first later line that is the attribute's
/// indentation followed by `}`, the layout rustfmt gives every file.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim() != "#[cfg(test)]" {
            kept.push((i + 1, line));
            i += 1;
            continue;
        }
        let indent = &line[..line.len() - line.trim_start().len()];
        let close = format!("{indent}}}");
        if lines.get(i + 1).is_some_and(|next| is_one_line_item(next)) {
            i += 2;
        } else {
            i += 1;
            while i < lines.len() && lines[i] != close {
                i += 1;
            }
            i += 1;
        }
    }
    kept
}

/// The non-comment production lines that hold a `/proc` string literal.
fn proc_lines(text: &str) -> Vec<String> {
    production_lines(text)
        .into_iter()
        .filter(|(_, line)| !line.trim_start().starts_with("//") && line.contains("\"/proc"))
        .map(|(_, line)| line.trim_start().to_string())
        .collect()
}

/// The production lines of one file that name a state path or the main branch.
fn path_hits(rel: &str, text: &str) -> Vec<String> {
    production_lines(text)
        .into_iter()
        .filter(|(_, line)| {
            !line.trim_start().starts_with("//")
                && (line.contains("\".superpowers")
                    || line.contains("\".claude")
                    || line.contains("\"main\""))
        })
        .map(|(n, line)| format!("{rel}:{n}: {}", line.trim_start()))
        .collect()
}

#[test]
fn the_proc_lint_catches_a_proc_literal() {
    let src = "//! Reads nothing from /proc.\nfn f() { let _ = std::fs::read(\"/proc/self/stat\"); }\n#[cfg(test)]\nmod tests { const P: &str = \"/proc/1\"; }\n";
    assert_eq!(proc_lines(src).len(), 1);
}

#[test]
fn the_proc_lint_catches_a_read_after_an_early_test_use() {
    let src = "#[cfg(test)]\nuse x;\nfn f() { let _ = std::fs::read(\"/proc/self/stat\"); }\n";
    assert_eq!(proc_lines(src).len(), 1);
}

#[test]
fn the_paths_lint_catches_main_after_an_early_test_use() {
    let src = "#[cfg(test)]\nuse x;\nconst B: &str = \"main\";\n";
    assert_eq!(
        path_hits("a/src/x.rs", src),
        ["a/src/x.rs:3: const B: &str = \"main\";"]
    );
}

#[test]
fn production_lines_drops_only_test_items() {
    let src = "#[cfg(test)]\nuse x::y;\nfn keep_one() {}\nconst S: &str = \"#[cfg(test)]\";\n#[cfg(test)]\nmod tests {\n    fn t() { let a = \"}\"; let b = \"{\"; }\n}\nfn keep_two() {}\n";
    let kept = production_lines(src);
    let texts: Vec<&str> = kept.iter().map(|(_, line)| *line).collect();
    let numbers: Vec<usize> = kept.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        texts,
        [
            "fn keep_one() {}",
            "const S: &str = \"#[cfg(test)]\";",
            "fn keep_two() {}"
        ]
    );
    assert_eq!(numbers, [3, 4, 9]);
}

#[test]
fn production_lines_drops_one_line_test_items() {
    let src = "#[cfg(test)]\nfn x() {}\nconst A: &str = \"keep\";\n#[cfg(test)]\nimpl T for U {}\nfn keep() {}\n";
    let kept = production_lines(src);
    let texts: Vec<&str> = kept.iter().map(|(_, line)| *line).collect();
    let numbers: Vec<usize> = kept.iter().map(|(n, _)| *n).collect();
    assert_eq!(texts, ["const A: &str = \"keep\";", "fn keep() {}"]);
    assert_eq!(numbers, [3, 6]);
}

#[test]
fn production_lines_reads_the_code_before_a_trailing_comment() {
    let src =
        "#[cfg(test)]\nuse x; // reason\nfn a() {}\n#[cfg(test)]\nfn x() {} // stub\nfn b() {}\n";
    let kept = production_lines(src);
    let texts: Vec<&str> = kept.iter().map(|(_, line)| *line).collect();
    let numbers: Vec<usize> = kept.iter().map(|(n, _)| *n).collect();
    assert_eq!(texts, ["fn a() {}", "fn b() {}"]);
    assert_eq!(numbers, [3, 6]);
}

#[test]
fn is_one_line_item_reads_the_whole_line_and_the_code_part() {
    for line in [
        "fn x() {}",
        "use x; // reason",
        "fn x() {} // stub",
        "const U: &str = \"http://x\";",
    ] {
        assert!(is_one_line_item(line), "{line} is one line");
    }
    for line in ["mod tests {", "fn x() { 1 }", "fn x() { // open"] {
        assert!(!is_one_line_item(line), "{line} opens a block");
    }
}

#[test]
fn the_paths_lint_catches_main_after_a_one_line_test_fn() {
    let src = "#[cfg(test)]\nfn x() {}\nconst B: &str = \"main\";\n";
    assert_eq!(
        path_hits("a/src/x.rs", src),
        ["a/src/x.rs:3: const B: &str = \"main\";"]
    );
}

#[test]
#[should_panic(expected = "is not under crates/")]
fn a_member_outside_crates_panics() {
    crates_in("members = [\n    \"crates/a\",\n    \"tools/x\",\n]\n");
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
