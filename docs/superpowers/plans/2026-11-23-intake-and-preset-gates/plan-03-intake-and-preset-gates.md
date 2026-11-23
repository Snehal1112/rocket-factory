# Intake and preset gates, plan 3: preset format and lint gates

> **Spec:** docs/superpowers/specs/2026-11-23-intake-and-preset-gates-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The go, csharp and ruby presets ship format and lint gates that work without project setup, node's builder rules say how to add a format gate (#68), and the settings reference lists which preset ships which gates.

**Commit:** feat(presets): ship format and lint gates

**Architecture:** Edits to four embedded preset `config.toml` files, one pinning test in `preset.rs`, and doc text.

**Tech Stack:** TOML, Rust test in factory-core, Markdown.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- No change to the rust preset or to any risk path list.
- Check commands run without a shell, split on whitespace, and fail only on a non-zero exit (`crates/factory-checks/src/run.rs:143-160`).
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/presets/go/config.toml` | `commands`, `format`, `lint` (lines 21-23). |
| `crates/factory-core/presets/csharp/config.toml` | `format` (lines 26-28). |
| `crates/factory-core/presets/ruby/config.toml` | `format`, `lint` (lines 25-28). |
| `crates/factory-core/presets/node/config.toml` | One `builder_rules` line. |
| `crates/factory-core/src/preset.rs` | `presets_ship_their_gates` next to `the_node_preset_is_bun_and_script_driven` (line ~491). |
| `docs/reference/project-toml-reference.md` | A table of preset gates under `### Checks and gates`, with the refresh and gofmt notes. |

## Review Focus

1. A project that already sets `[[settings.checks]]`: the user expects its checks unchanged. This holds by design (a project value replaces the preset list), so it only needs a sentence in the docs, no test.
2. The node `commands` and `lint`: unchanged. Pinned by the existing `the_node_preset_is_bun_and_script_driven`.

---

### Task 1: Preset gates and docs

**Files:**
- Modify: the four preset `config.toml` files, `docs/reference/project-toml-reference.md`
- Test: `crates/factory-core/src/preset.rs` (`mod tests`)

**Interfaces:**
- Consumes: `embedded(name)`, `Config::parse`, `CheckRule { commands, format, lint, .. }`.
- Produces: no new item.
- Port: none.

- [ ] **Step 1: Read.** Read the four preset `config.toml` files, `crates/factory-core/src/preset.rs:480-575`, and `docs/reference/project-toml-reference.md:151-255`.

- [ ] **Step 2: Write the failing test:**

```rust
#[test]
fn presets_ship_their_gates() {
    let lists = |name: &str| {
        let cfg = Config::parse(embedded(name).expect("preset").config).expect("parses");
        let pick = |f: fn(&crate::CheckRule) -> &Vec<String>| -> Vec<String> {
            cfg.checks.iter().flat_map(|c| f(c).clone()).collect()
        };
        (pick(|c| &c.commands), pick(|c| &c.format), pick(|c| &c.lint))
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(lists("go"), (s(&["go test ./..."]), s(&["gofmt -d ."]), s(&["go vet ./..."])));
    assert_eq!(lists("csharp"), (s(&["dotnet build --nologo", "dotnet test --nologo"]), s(&["dotnet format --verify-no-changes --nologo"]), s(&[])));
    assert_eq!(lists("ruby"), (s(&["bundle exec rspec"]), s(&["bundle exec rubocop --only Layout"]), s(&["bundle exec rubocop --except Layout"])));
    assert_eq!(lists("node").1, s(&[]));
    let node = Config::parse(embedded("node").expect("node").config).expect("parses");
    assert!(node.builder_rules.iter().any(|r| r.contains("format:check")), "node builder rule");
}
```

If `CheckRule` is not reachable as `crate::CheckRule`, use its real path. If the field types are not `Vec<String>`, compare with `iter().map(String::as_str)` instead.

- [ ] **Step 3: Run it.** `cargo test -j4 -p factory-core presets_ship_their_gates`. Expected: fails on the go assertion.

- [ ] **Step 4: Edit.**
  - **go:** `commands = ["go test ./..."]`, `format = ["gofmt -d ."]`, `lint = ["go vet ./..."]`.
  - **csharp:** add `format = ["dotnet format --verify-no-changes --nologo"]`.
  - **ruby:** add `format = ["bundle exec rubocop --only Layout"]` and change `lint` to `["bundle exec rubocop --except Layout"]`.
  - **node:** add the builder rule "To get a format gate, add a `format:check` script and list `bun run format:check` under `format` in the project's check rules."
  - **Docs:** in `project-toml-reference.md`, add a `Preset | format | lint` table for all five presets. Then say three things:
    - a project's own `[[settings.checks]]` replaces the preset's;
    - `factory presets refresh` updates only a preset `config.toml` the user has not edited, so an edited one needs `--force`;
    - the go `format` gate relies on `gofmt -d` exiting 1 on a difference.

- [ ] **Step 5: Run them.** `cargo test -j4 -p factory-core preset` (all pass), then `cargo test -j4 -p factory-cli --test presets_refresh` (all pass).

Next plan: none
