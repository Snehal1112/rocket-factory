# Gate settings, park kinds and helpers Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory-core` knows the `no-checks` and `no-tests` park kinds, the `code_paths`, `test_paths`, `test_markers` and `require_tests` settings, and pure helpers that tell a code path, a test path and an added test line.

**Architecture:** Two park kinds join `ParkKind`. Four optional keys join `Config` with defaults that mean "use the built-in rule". A new `gate.rs` module holds three pure functions the engine calls in plan 2. Rocket's `src-tauri` check rule is fixed in the two fixtures. Nothing in the engine changes in this plan.

**Tech Stack:** Rust, cargo with `-j4` per crate, serde, toml.

**Spec:** `docs/superpowers/specs/2026-10-18-no-check-no-test-gate-design.md`

## Global Constraints

- Every cargo call has `-j4` and `-p <crate>`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Follow each task's steps in order: write the failing tests exactly as given, run the stated command and confirm the stated failure, implement from the signatures, then confirm the stated pass.
- Tests use `.expect("...")` and `expect_err`. Never write the literal text of a panicking unwrap call in a plan, doc or Rust file.
- `Config` keeps `deny_unknown_fields`. The new keys are optional, so an old config parses unchanged.
- A degraded config makes every path count as code. A missing or broken config stays restrictive.
- `ParkKind` names are stable strings. `park.json` and board JSON stay readable by older code and by `dashboard/`.
- No `git` CLI, no new dependency, no commit steps. Code comments are short full sentences ending with a punctuation mark.

## Review Focus

1. **A path with different case or a leading `./`.** `./Crates/A/Tests/X.rs` must classify like `crates/a/tests/x.rs`. Pinned in Task 2 (`paths_match_ignoring_case_and_a_leading_dot_slash`).
2. **An empty `code_paths`, `test_paths` or `test_markers` list.** It means the built-in rule, never "nothing is code" or "nothing is a test". Pinned in Task 2 (`an_empty_code_paths_list_means_the_default`, `an_empty_test_paths_list_means_the_default`, `an_empty_test_markers_list_means_the_default`).
3. **A removed test line, a `+++` header and a context line in a diff.** None may count as an added test. Pinned in Task 2 (`an_added_test_marker_line_counts_only_when_added`).
4. **A degraded config.** Every path is code, markers and test paths keep their defaults, `require_tests` stays false. Pinned in Task 2 (`a_degraded_config_counts_every_path_as_code`).
5. **An old `park.json`.** A record with kind `checks-failed` still parses. Pinned in Task 1 (`an_old_park_record_still_parses`).

## File Structure

- Modify `crates/factory-core/src/park.rs`: two kinds, `as_str`, `next`, tests.
- Modify `crates/factory-core/src/config.rs`: four fields, defaults, `fail_closed`, tests.
- Modify `crates/factory-core/src/risk.rs`: `is_doc_path` becomes `pub(crate)`, `normalize` too.
- Create `crates/factory-core/src/gate.rs`: three helpers and their tests. Modify `crates/factory-core/src/lib.rs`: `mod gate;` and re-exports.
- Modify `dashboard/static/board.js`: two entries in `PARK_LABEL`.
- Modify `crates/factory-core/tests/fixtures/rocket-config.toml`, `rocket-project.toml` and `crates/factory-core/tests/rocket_parity.rs`.

---

### Task 1: Park kinds

**Files:**
- Modify: `crates/factory-core/src/park.rs`, `dashboard/static/board.js`

**Interfaces:**
- Produces: `ParkKind::NoChecks` (`"no-checks"`) and `ParkKind::NoTests` (`"no-tests"`). `next(id)` returns `["factory approve <id>", "factory rework <id> <note>"]` for both. `next_for(id, true)` returns the `--without-review` form already defined for kinds that offer approve.

- [ ] **Step 1: Write the failing tests** in the `tests` module of `park.rs`, next to the existing `as_str` and `next` tests:

```rust
    #[test]
    fn no_checks_and_no_tests_have_stable_names() {
        assert_eq!(ParkKind::NoChecks.as_str(), "no-checks");
        assert_eq!(ParkKind::NoTests.as_str(), "no-tests");
    }

    #[test]
    fn no_checks_and_no_tests_offer_approve_and_rework() {
        for kind in [ParkKind::NoChecks, ParkKind::NoTests] {
            assert_eq!(
                kind.next("012"),
                vec!["factory approve 012".to_string(), "factory rework 012 <note>".to_string()],
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_park_before_the_review_offers_approve_without_review() {
        let next = ParkKind::NoChecks.next_for("012", true);
        assert!(next.iter().any(|n| n.contains("factory approve 012 --without-review")), "{next:?}");
        assert!(!next.contains(&"factory approve 012".to_string()), "{next:?}");
    }

    #[test]
    fn an_old_park_record_still_parses() {
        let json = r#"{"kind":"checks-failed","reason":"r","at":1,"round":1,"next":[],"parts":[]}"#;
        let rec: ParkRecord = serde_json::from_str(json).expect("old record parses");
        assert_eq!(rec.kind, ParkKind::ChecksFailed);
    }

    #[test]
    fn the_new_kinds_round_trip_through_park_json() {
        for kind in [ParkKind::NoChecks, ParkKind::NoTests] {
            let rec = ParkRecord::new(kind, "why", "012", 5, 1, Vec::new());
            let text = serde_json::to_string(&rec).expect("serialize");
            assert!(text.contains(kind.as_str()), "{text}");
            let back: ParkRecord = serde_json::from_str(&text).expect("parse");
            assert_eq!(back.kind, kind);
        }
    }
```

- [ ] **Step 2: Run to verify they fail:** `cargo test -j4 -p factory-core no_checks`. Expected: compile error, `no variant named NoChecks`.

- [ ] **Step 3: Implement.** Add `NoChecks` and `NoTests` to `ParkKind` (keep the serde form that the other kinds use, so the JSON string equals `as_str`). Add the two `as_str` arms. Add both to the `ChangesRemain | RiskyDiff | DiffTooLarge` arm of `next`. In `dashboard/static/board.js` add `"no-checks": "No checks ran"` and `"no-tests": "No tests added"` to `PARK_LABEL`. Check the `ParkRecord::new` argument order in the file and adjust the last test to it.

- [ ] **Step 4: Run to verify they pass:** `cargo test -j4 -p factory-core park`. Expected: all `park` tests pass, five of them new.

### Task 2: Settings and path helpers

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/risk.rs`, `crates/factory-core/src/lib.rs`
- Create: `crates/factory-core/src/gate.rs`

**Interfaces:**
- Produces: `Config { code_paths: Vec<String>, test_paths: Vec<String>, test_markers: Vec<String>, require_tests: bool }`. Defaults: empty, empty, empty, `false`. `Config::fail_closed()` sets `code_paths` to `["**"]`.
- Produces (in `gate.rs`, re-exported from the crate root):

```rust
pub fn code_paths_of<S: AsRef<str>>(paths: &[S], config: &Config) -> Vec<String>;
pub fn has_test_path<S: AsRef<str>>(paths: &[S], config: &Config) -> bool;
pub fn has_test_marker(diff_text: &str, config: &Config) -> bool;
```

- Rules. `code_paths_of`: empty `config.code_paths` means "not a doc path" (`is_doc_path` after `normalize`); a non-empty list is a positive glob list matched with `glob_match` on the lower-cased normalized path. It returns the original path strings that matched, in input order. `has_test_path`: empty `test_paths` means the defaults `**/tests/**`, `**/__tests__/**`, `**/*.test.ts`, `**/*.test.tsx`, `**/*.spec.ts`, `**/*_test.rs`, `**/*_test.go`. `has_test_marker`: only lines starting with `+` and not `+++` are looked at; true when one contains a marker. Empty `test_markers` means `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, `describe(`, `it(`, `test(`, `func Test`.

- [ ] **Step 1: Write the failing tests.** In `gate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn docs_only_paths_are_not_code_by_default() {
        let paths = ["README.md", "docs/a.md", ".claude/x.toml"];
        assert!(code_paths_of(&paths, &cfg()).is_empty());
    }

    #[test]
    fn a_source_file_is_code_by_default() {
        let paths = ["crates/a/src/lib.rs", "docs/a.md"];
        assert_eq!(code_paths_of(&paths, &cfg()), vec!["crates/a/src/lib.rs".to_string()]);
    }

    #[test]
    fn code_paths_from_config_replace_the_default() {
        let mut c = cfg();
        c.code_paths = vec!["src/**".into()];
        let paths = ["docs/a.md", "build.rs", "src/a.ts"];
        assert_eq!(code_paths_of(&paths, &c), vec!["src/a.ts".to_string()]);
    }

    #[test]
    fn an_empty_code_paths_list_means_the_default() {
        let mut c = cfg();
        c.code_paths = Vec::new();
        assert_eq!(code_paths_of(&["a.rs"], &c), vec!["a.rs".to_string()]);
    }

    #[test]
    fn a_degraded_config_counts_every_path_as_code() {
        let c = Config::fail_closed();
        assert_eq!(code_paths_of(&["docs/a.md"], &c), vec!["docs/a.md".to_string()]);
        assert!(!c.require_tests);
    }

    #[test]
    fn test_paths_default_covers_the_common_layouts() {
        for p in [
            "crates/a/tests/x.rs",
            "src/a/__tests__/b.ts",
            "src/a.test.tsx",
            "src/a.spec.ts",
            "crates/a/src/x_test.rs",
            "pkg/x_test.go",
        ] {
            assert!(has_test_path(&[p], &cfg()), "{p}");
        }
        assert!(!has_test_path(&["crates/a/src/lib.rs"], &cfg()));
    }

    #[test]
    fn an_empty_test_paths_list_means_the_default() {
        let mut c = cfg();
        c.test_paths = Vec::new();
        assert!(has_test_path(&["a/tests/b.rs"], &c));
    }

    #[test]
    fn test_paths_from_config_replace_the_default() {
        let mut c = cfg();
        c.test_paths = vec!["spec/**".into()];
        assert!(has_test_path(&["spec/a.rb"], &c));
        assert!(!has_test_path(&["crates/a/tests/x.rs"], &c));
    }

    #[test]
    fn paths_match_ignoring_case_and_a_leading_dot_slash() {
        assert!(has_test_path(&["./Crates/A/Tests/X.rs"], &cfg()));
        assert_eq!(code_paths_of(&["./Src/A.RS"], &cfg()), vec!["./Src/A.RS".to_string()]);
    }

    #[test]
    fn an_added_test_marker_line_counts_only_when_added() {
        let added = "+++ b/src/a.rs\n@@ -1,1 +1,3 @@\n+    #[test]\n+    fn x() {}\n";
        assert!(has_test_marker(added, &cfg()));
        assert!(has_test_marker("+  it(\"works\", () => {});\n", &cfg()));
        assert!(has_test_marker("+func TestThing(t *testing.T) {\n", &cfg()));
        assert!(!has_test_marker("-    #[test]\n", &cfg()), "a removed line");
        assert!(!has_test_marker(" #[test]\n", &cfg()), "a context line");
        assert!(!has_test_marker("+++ b/tests/it(.rs\n", &cfg()), "a file header");
    }

    #[test]
    fn an_empty_test_markers_list_means_the_default() {
        let mut c = cfg();
        c.test_markers = Vec::new();
        assert!(has_test_marker("+#[cfg(test)]\n", &c));
    }

    #[test]
    fn test_markers_from_config_replace_the_default() {
        let mut c = cfg();
        c.test_markers = vec!["@Test".into()];
        assert!(has_test_marker("+@Test\n", &c));
        assert!(!has_test_marker("+#[test]\n", &c));
    }
}
```

  In the `tests` module of `config.rs`:

```rust
    #[test]
    fn a_config_without_the_new_keys_parses_with_defaults() {
        let cfg = Config::parse("diff_cap_lines = 100\n").expect("parses");
        assert!(cfg.code_paths.is_empty() && cfg.test_paths.is_empty() && cfg.test_markers.is_empty());
        assert!(!cfg.require_tests);
    }

    #[test]
    fn the_new_keys_parse() {
        let cfg = Config::parse(
            "code_paths = [\"src/**\"]\ntest_paths = [\"spec/**\"]\ntest_markers = [\"@Test\"]\nrequire_tests = true\n",
        )
        .expect("parses");
        assert_eq!(cfg.code_paths, vec!["src/**".to_string()]);
        assert_eq!(cfg.test_paths, vec!["spec/**".to_string()]);
        assert_eq!(cfg.test_markers, vec!["@Test".to_string()]);
        assert!(cfg.require_tests);
    }

    #[test]
    fn fail_closed_makes_every_path_code() {
        let cfg = Config::fail_closed();
        assert_eq!(cfg.code_paths, vec!["**".to_string()]);
        assert!(cfg.test_paths.is_empty() && !cfg.require_tests);
    }
```

- [ ] **Step 2: Run to verify they fail:** `cargo test -j4 -p factory-core gate`. Expected: compile errors, unresolved `gate` items and no field `code_paths` on `Config`.

- [ ] **Step 3: Implement.** Add the four fields to `Config` with doc comments and the defaults above (the struct already has `#[serde(default, deny_unknown_fields)]`). In `fail_closed` add `code_paths: vec!["**".to_string()]`. Make `normalize` and `is_doc_path` in `risk.rs` `pub(crate)`. In `gate.rs` write the three functions to the rules above, with the default lists as `const` slices. Add `mod gate;` and `pub use gate::{code_paths_of, has_test_marker, has_test_path};` to `lib.rs`.

- [ ] **Step 4: Run to verify they pass:** `cargo test -j4 -p factory-core gate` (12 tests) then `cargo test -j4 -p factory-core config`. Expected: pass. Then `cargo check -j4 -p factory-core`.

### Task 3: Rocket's src-tauri rule in the fixtures

**Files:**
- Modify: `crates/factory-core/tests/fixtures/rocket-config.toml`, `crates/factory-core/tests/fixtures/rocket-project.toml`
- Test: `crates/factory-core/tests/rocket_parity.rs`

**Interfaces:**
- Consumes: `resolved()` and `old()` helpers already in `rocket_parity.rs`, and `CheckRule::matches(path)`.

- [ ] **Step 1: Write the failing test** at the end of `rocket_parity.rs`:

```rust
#[test]
fn the_src_tauri_rule_runs_the_tests() {
    let commands = |cfg: &Config| -> Vec<String> {
        cfg.checks
            .iter()
            .filter(|c| c.matches("src-tauri/src/lib.rs"))
            .flat_map(|c| c.commands.clone())
            .collect()
    };
    let (_t, r) = resolved();
    for cmds in [commands(&r.config), commands(&old())] {
        assert!(cmds.contains(&"cargo check -j4 -p rocket".to_string()), "{cmds:?}");
        assert!(cmds.contains(&"cargo test -j4 -p rocket".to_string()), "{cmds:?}");
        assert!(!cmds.contains(&"cargo check -j4".to_string()), "a bare workspace check: {cmds:?}");
    }
}
```

- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-core --test rocket_parity the_src_tauri_rule`. Expected: FAIL, the commands are `["cargo check -j4"]`.

- [ ] **Step 3: Implement.** In both fixtures change the rule whose paths are `["src-tauri/**"]` to `commands = ["cargo check -j4 -p rocket", "cargo test -j4 -p rocket"]`. Do not touch the sample data in `factory-checks/src/plan.rs` tests. The package name `rocket` comes from Rocket's `src-tauri/Cargo.toml`.

- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-core --test rocket_parity` (every test passes, the new one included), then `cargo test -j4 -p factory-core` and `cargo check -j4 -p factory-core`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-no-check-no-test-gate/plan-02-no-check-no-test-gate-engine.md
