# Path lint one-line test items Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make the path lint drop only the one line of a one-line test item after `#[cfg(test)]`, so production code after it is still scanned.

**Commit:** fix(lint): skip one-line items after cfg(test)

**Architecture:** `production_lines` in the path lint test file gains a small helper, `is_one_line_item`, that decides whether the line after `#[cfg(test)]` is a whole item. The braced-block path stays as it is.

**Tech Stack:** Rust, an integration test file in `factory-cli` with no new dependency.

> **Spec:** docs/superpowers/specs/2026-11-22-cfg-test-one-line-items-design.md

## Global Constraints

- Only `crates/factory-cli/tests/no_hardcoded_paths.rs` changes. No new dependency.
- Cargo commands are per crate with `-j4`: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences.
- The code part of a line is its text before the first `//`, trimmed. A line counts as a one-line item when its whole trimmed text, or its code part, ends in `;` or `{}`.

## File Structure

- `crates/factory-cli/tests/no_hardcoded_paths.rs`: add `is_one_line_item`, use it in `production_lines`, update the doc comment on `production_lines`, and add four tests.

## Review Focus

1. A line whose `//` sits inside a string, such as `const U: &str = "http://x";`, must still count as a one-line item, as it does today. Pinned by `is_one_line_item_reads_the_whole_line_and_the_code_part`.
2. A block opener with a trailing comment, such as `fn x() { // open`, must not count as a one-line item, or a whole test module would be scanned. Pinned by the same test.
3. A `mod tests {` block must still be dropped up to its closing brace. Pinned by the existing `production_lines_drops_only_test_items`.
4. Line numbers in the kept lines must stay the original 1-based numbers, since `path_hits` prints them. Pinned by `production_lines_drops_one_line_test_items` and `the_paths_lint_catches_main_after_a_one_line_test_fn`.

---

### Task 1: One-line items after `#[cfg(test)]`

**Files:**
- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs:120-152` (doc comment and `production_lines`)
- Test: `crates/factory-cli/tests/no_hardcoded_paths.rs` (four new tests next to `production_lines_drops_only_test_items` at `:199`)

**Interfaces:**
- Consumes: `production_lines(text: &str) -> Vec<(usize, &str)>` and `path_hits(rel: &str, text: &str) -> Vec<String>`, both in this file.
- Produces: `fn is_one_line_item(line: &str) -> bool`, private to this file.

Tests added: `production_lines_drops_one_line_test_items`, `production_lines_reads_the_code_before_a_trailing_comment`, `is_one_line_item_reads_the_whole_line_and_the_code_part`, `the_paths_lint_catches_main_after_a_one_line_test_fn`.

- [ ] **Step 1: Read** `crates/factory-cli/tests/no_hardcoded_paths.rs:120-213`: the doc comment, `production_lines`, `proc_lines`, `path_hits`, `production_lines_drops_only_test_items` and `the_paths_lint_catches_main_after_an_early_test_use`. Then read the spec's Design and edge-case table.

- [ ] **Step 2: Write the failing tests** next to `production_lines_drops_only_test_items`:

```rust
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
```

`production_lines_reads_the_code_before_a_trailing_comment` has the same shape as `production_lines_drops_one_line_test_items`, with these values:

| Source | Kept texts | Kept numbers |
|---|---|---|
| `"#[cfg(test)]\nuse x; // reason\nfn a() {}\n#[cfg(test)]\nfn x() {} // stub\nfn b() {}\n"` | `["fn a() {}", "fn b() {}"]` | `[3, 6]` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Expected: the file does not compile, with `cannot find function 'is_one_line_item' in this scope`.

- [ ] **Step 4: Implement** `fn is_one_line_item(line: &str) -> bool` above `production_lines`. It takes the trimmed line and the trimmed text before its first `//`, and returns true when either ends in `;` or `{}`. Give it a one-sentence doc comment. In `production_lines`, replace the `next.trim().ends_with(';')` check with `is_one_line_item(next)`. Rewrite the doc comment on `production_lines` so it says:
  - an item whose first line, or that line's code part, ends in `;` or `{}` is that one line;
  - the code part is the text before the first `//`;
  - the whole line is checked too, so a `//` inside a string keeps the old result;
  - a braced item still ends at the first later line that is the attribute's indentation followed by `}`, the layout rustfmt gives every file.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Expected: 13 passed, 0 failed (the 9 existing tests and the 4 new ones).

- [ ] **Step 6: Run** `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings` and `cargo fmt --check`. Expected: no warnings and no formatting changes.

Next plan: none
