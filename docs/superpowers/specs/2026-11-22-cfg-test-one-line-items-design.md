# Path lint: one-line items after `#[cfg(test)]`

## Goal

Make the path lint drop only the one line of a one-line test item after `#[cfg(test)]`, so production code that follows it is still scanned.

## Overview

`crates/factory-cli/tests/no_hardcoded_paths.rs` holds two lints that scan crate sources: one for `/proc` reads and one for hardcoded state paths and the main branch. Both first remove test code with `production_lines`, so a test may name such paths freely.

Job 102's review (issue #141) found that `production_lines` reads a one-line item such as `#[cfg(test)] fn x() {}` as the start of a braced block. It then skips every following line until a `}` at the attribute's indentation. The production code in between is never scanned, so a hardcoded path there would pass the lint unseen.

No crate source has that shape today. Of the 109 `#[cfg(test)]` lines in `crates/*/src`, 106 are followed by a line that opens a block with `{` and 3 by a line that ends in `;`. None is followed by a one-line `{}` item or by a second attribute. This change guards code written later.

## Current state

- `production_lines` (`crates/factory-cli/tests/no_hardcoded_paths.rs:125-152`) walks the lines. A line whose trimmed text is exactly `#[cfg(test)]` starts a test item.
  - When the next line's trimmed text ends with `;` (`:138-142`), it drops the attribute and that line.
  - Otherwise (`:143-149`) it drops lines up to and including the first later line equal to the attribute's indentation followed by `}`.
- The doc comment (`:120-124`) states the rule and the rustfmt layout it relies on.
- `proc_lines` (`:155`) and `path_hits` (`:164`) call it.
- `production_lines_drops_only_test_items` (`:199-213`) pins the current behaviour for a `use` item and a `mod tests` block.

Gaps:

- A next line ending in `{}` (an empty one-line item) takes the braced path and swallows production lines.
- A next line with a trailing comment, for example `use x; // why` or `fn x() {} // stub`, ends in neither `;` nor `{}` and takes the braced path too.

## Non-goals

- Items after stacked attributes (`#[cfg(test)]` then `#[derive(..)]` then the item).
- One-line items with a non-empty body, such as `fn x() { 1 }`. rustfmt splits those over several lines.
- Parsing Rust properly, or adding a dependency to this test.
- Any change to what the two lints look for.

## Design

```
line == "#[cfg(test)]" (trimmed)
  whole = the next line, trimmed
  code  = text of the next line before its first "//", trimmed
  whole or code ends with ";" or "{}"          -> drop the attribute and the next line
  otherwise                                    -> drop up to the closing "<indent>}" line (unchanged)
```

### 1. One-line item rule

In `production_lines`, the two-line case now covers any next line whose whole trimmed text, or whose code part, ends in `;` or in `{}`.

The code part is the text before the line's first `//`, with surrounding whitespace trimmed. Checking the whole line as well keeps today's result for a line whose `//` sits inside a string, such as `const U: &str = "http://x";`.

Add one private helper next to `production_lines`: `fn is_one_line_item(line: &str) -> bool`.

- `#[cfg(test)]` / `fn x() {}` drops 2 lines.
- `#[cfg(test)]` / `impl T for U {}` drops 2 lines.
- `#[cfg(test)]` / `use x; // reason` drops 2 lines.
- `#[cfg(test)]` / `fn x() {} // stub` drops 2 lines.
- `#[cfg(test)]` / `const U: &str = "http://x";` drops 2 lines, as today.
- `#[cfg(test)]` / `mod tests {` keeps the braced path.

A `#[cfg(test)]` on the last line has no next line. It takes the braced path as today, which drops it and nothing else.

### 2. Doc comment

The doc comment on `production_lines` says that an item whose first line, or that line's code part, ends in `;` or `{}` is that one line, that the code part is the text before the first `//`, and that the whole line is checked too so a `//` inside a string keeps the old result.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Next line `fn x() {}` | The attribute and that line are dropped; the following lines are kept. |
| Next line `fn x() {} // stub` | Same as above. |
| Next line `use x; // reason` | Same as above (previously took the braced path). |
| Next line `mod tests {` | Braced path, unchanged. |
| Next line `fn x() { 1 }` | Braced path, unchanged (a non-goal; rustfmt never writes it). |
| Next line holds `//` inside a string, e.g. `const U: &str = "http://x";` | The whole line ends in `;`, so the two lines are dropped, as today. |
| Next line opens a block and its trailing comment ends in `;` or `{}`, e.g. `mod tests { // see foo;` | The whole line ends in `;`, so it is read as a one-line item and the block body is scanned as production code. This can only add lint hits, never hide one, and the current rule does the same for `;`. Accepted as a known limit; no source has this shape. |
| `#[cfg(test)]` is the last line | Dropped; nothing else changes. |
| `#[cfg(test)]` with text after it on the same line | Not a test-item start (the trimmed line must equal `#[cfg(test)]`), unchanged. |

## Testing

In `crates/factory-cli/tests/no_hardcoded_paths.rs`:

- `production_lines_drops_one_line_test_items`: a source with `#[cfg(test)]` / `fn x() {}`, then `const A: &str = "keep";`, then `#[cfg(test)]` / `impl T for U {}`, then `fn keep() {}`. It asserts that the kept texts are the `const A` line and the `fn keep` line, and that their numbers are 3 and 6.
- `production_lines_reads_the_code_before_a_trailing_comment`: a source with `#[cfg(test)]` / `use x; // reason`, then `fn a() {}`, then `#[cfg(test)]` / `fn x() {} // stub`, then `fn b() {}`. It asserts that the kept texts are `fn a() {}` and `fn b() {}`, with numbers 3 and 6.
- `is_one_line_item_reads_the_whole_line_and_the_code_part`: true for `fn x() {}`, `use x; // reason`, `fn x() {} // stub` and `const U: &str = "http://x";`; false for `mod tests {`, `fn x() { 1 }` and `fn x() { // open`.
- `the_paths_lint_catches_main_after_a_one_line_test_fn`: `path_hits("a/src/x.rs", ...)` on `#[cfg(test)]` / `fn x() {}` / `const B: &str = "main";` returns exactly `["a/src/x.rs:3: const B: &str = \"main\";"]`.
- The existing `production_lines_drops_only_test_items`, `the_paths_lint_catches_main_after_an_early_test_use` and the source-scanning lints still pass unchanged.

Command: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`.

## Files changed

- `crates/factory-cli/tests/no_hardcoded_paths.rs`

## Constraints

- Only the one test file changes. No new dependency.
- Cargo runs with `-j4 -p factory-cli`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences.

## Acceptance criteria

- A `#[cfg(test)]` followed by a line whose code part ends in `{}` drops exactly those two lines.
- A `#[cfg(test)]` followed by a line whose code part ends in `;`, with or without a trailing `//` comment, drops exactly those two lines.
- A `#[cfg(test)]` followed by a line that ends in `;` only because a `//` sits inside a string still drops exactly those two lines.
- A `#[cfg(test)]` followed by a line that opens a block keeps the braced behaviour.
- `path_hits` reports a `"main"` literal on the line after a one-line test item.
- The doc comment on `production_lines` states the whole-line and code-part rule.
- `cargo test -j4 -p factory-cli --test no_hardcoded_paths` passes, including the four new tests and all existing ones.

## Out of scope

- Stacked attributes after `#[cfg(test)]`.
- Non-empty one-line bodies.
- Making the lint panic on shapes it does not recognise.

## Decisions

- How far should the test-code skip be hardened? Options: one-line items only, one-line items plus stacked attributes, fail loudly on unknown shapes. Chosen: one-line items only. Decided by: user
- Which next lines count as a one-line item? Options: ends in `{}`, braces balance on the line, contains `{` and ends in `}`. Chosen: ends in `{}`. Decided by: user
- How is a trailing comment handled? Options: also match when the text before the first `//` ends in `{}`, only a bare `{}` ending, panic on that shape. Chosen: also match the text before the first `//`. Decided by: user
- Does the code-part rule also apply to the `;` case? Options: yes, no. Chosen: yes, because `use x; // reason` has the same flaw and one rule is simpler than two. Decided by: call
- The whole line is checked as well as the code part, so a `//` inside a string keeps today's result. Decided by: call
- A braced opener whose trailing comment ends in `;` or `{}` is a known limit, because its error only adds lint hits. Decided by: call
- The two new `production_lines` tests are separate from the existing one rather than folded into it. Decided by: call
