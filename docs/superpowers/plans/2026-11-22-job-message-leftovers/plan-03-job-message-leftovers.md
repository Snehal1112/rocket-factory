# Index, config, command and recovery test gaps Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-job-message-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Pin the index, ignored-key and recovery rules with tests, fix the old batch wording, and make `/factory` check a one-job spec's Commit line.

**Commit:** test(factory): pin recovery, index and config

**Architecture:** Mostly tests. `staged_tree_is` gets a test in `crates/factory-git/tests/stage.rs`, `ignored_keys_in` in `crates/factory-core/src/config.rs`, and the machine wait in `crates/factory-engine/tests/builder_recover.rs`, behind the `AgentRunner` port with the `BuilderScript` and `fake_machine` fakes. There are three text changes: one ruling in `build.rs`, one sentence in `global-command.md` and its generated plugin copy.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No new board, ledger or event field.
- Git state in tests is read with `git2` on `Repository::open(&wt.path)`, as `inside` in `stage.rs` does. Production code never shells out to git.
- `plugins/factory/commands/factory.md` is never edited by hand. It is regenerated with `cargo run -j4 -q -p factory-cli --bin factory -- plugin sync`.

## Review Focus

1. `staged_tree_is` on a worktree with an unstaged file: a user expects the file left unstaged. Pinned by `staged_tree_is_leaves_the_index_and_files_alone`.
2. A key set in `[settings]` and at the top level with different values: a user expects one name in the list. Pinned by `ignored_keys_in_lists_a_key_set_in_both_places_once`.
3. A disk failure whose disk frees by the first probe: a user expects one probe at 60 s, then one rerun. Pinned by `a_failed_disk_run_waits_for_the_disk_then_reruns`, which checks the probe times.
4. The Codex skill text: a user expects it to carry the new sentence too. It is generated from `GLOBAL_COMMAND` at install, so it needs no copy. The plugin copy is pinned by `plugin_files_match_generated_text` after the sync.
5. The old ruling text in older ledgers: a user expects no test to depend on it. Pinned by `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings`, which checks the new text.

## File Structure

- Modify: `crates/factory-git/tests/stage.rs`
- Modify: `crates/factory-core/src/config.rs` (`mod tests` only)
- Modify: `crates/factory-engine/src/build.rs` (`keep_unsent_note`) and `crates/factory-engine/tests/build_checks.rs`
- Modify: `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md` (generated), `crates/factory-cli/tests/global_command.rs`
- Modify: `crates/factory-engine/tests/builder_recover.rs`
- Modify: `crates/factory-engine/src/drafts.rs` and `crates/factory-engine/src/park.rs` (`mod tests` config only)

---

### Task 1: index, ignored keys and the old batch wording

**Files:**
- Modify: `crates/factory-git/tests/stage.rs`, `crates/factory-core/src/config.rs`
- Modify: `crates/factory-engine/src/build.rs`, `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Port: `factory_git::Repo::staged_tree_is(&self, wt: &Worktree, rev: &str) -> Result<bool>` and `pub fn ignored_keys_in(text: &str) -> Vec<&'static str>` are unchanged. `keep_unsent_note` is engine code behind `AgentRunner`, and its test uses `two_task_fixture` and `park_a_batch`.
- Produces: no new items.

**Tests added:** `staged_tree_is_leaves_the_index_and_files_alone`, `ignored_keys_in_lists_a_key_set_in_both_places_once`, `ignored_keys_in_skips_commented_keys`, and the new wording asserts in `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings`.

- [ ] **Step 1: Read the code**

Read these:
- `staged_tree_is` in `crates/factory-git/src/stage.rs`;
- the helpers `job_worktree` and `inside`, and the test `staged_tree_is_compares_the_index_with_a_commit`, in `crates/factory-git/tests/stage.rs`;
- `untracked` in `crates/factory-git/tests/exclude.rs`;
- `ignored_keys_in` and `ignored_keys_in_lists_the_set_keys_in_order` in `crates/factory-core/src/config.rs`;
- `keep_unsent_note` in `crates/factory-engine/src/build.rs`;
- `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings` in `crates/factory-engine/tests/build_checks.rs`.

Then grep `crates/` for `because the batch commit ran no builder`, so that no other test matches the old text.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn staged_tree_is_leaves_the_index_and_files_alone() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    let sha = commit_files(&inside(&wt), &[("one.txt", "1\n")], "add one").to_string();
    std::fs::write(wt.path.join("two.txt"), "2\n").expect("write");
    std::fs::write(wt.path.join("one.txt"), "changed\n").expect("edit");
    let entries = |r: &git2::Repository| -> Vec<(String, git2::Oid)> {
        r.index().expect("index").iter()
            .map(|e| (String::from_utf8_lossy(&e.path).into_owned(), e.id))
            .collect()
    };
    let before = entries(&inside(&wt));
    assert!(repo.staged_tree_is(&wt, &sha).expect("same tree"));
    assert_eq!(entries(&inside(&wt)), before);
    assert_eq!(std::fs::read_to_string(wt.path.join("one.txt")).expect("one"), "changed\n");
    let status = inside(&wt).status_file(std::path::Path::new("two.txt")).expect("status");
    assert!(status.contains(git2::Status::WT_NEW), "{status:?}");
}
```

| Test | Input | Assertion |
|---|---|---|
| `ignored_keys_in_lists_a_key_set_in_both_places_once` | `"commit_batch = false\n[settings]\ncommit_batch = true\n"` | equals `["commit_batch"]` |
| `ignored_keys_in_skips_commented_keys` | `"# commit_model = \"x\"\nfix_rounds = 3\n"`, then `"[settings]\n# commit_batch = false\n"` | both `is_empty()` |

In `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings`, add: the ledger contains `rework note not sent to a builder because the old batch recovery ran no builder: please look at the batch`, and does not contain `because the batch commit ran no builder`.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-git --test stage`, then `cargo test -j4 -p factory-core ignored_keys_in`
Expected: they pass, because they pin today's behaviour. If the stage test fails, the doc claim of `staged_tree_is` is wrong: stop and report it in the ledger, without changing `stage.rs`.

Run: `cargo test -j4 -p factory-engine --test build_checks a_rework_note_left_by_a_batch`
Expected: it fails on the new wording.

- [ ] **Step 4: Implement**

In `keep_unsent_note`, change the ruling text to `"rework note not sent to a builder because the old batch recovery ran no builder: {kept}"`.

- [ ] **Step 5: Run them**

Run: `cargo test -j4 -p factory-engine --test build_checks`
Expected: all pass, 0 failed.

### Task 2: `/factory` step 3 checks the one-job spec Commit line

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md` (generated)
- Modify: `crates/factory-cli/tests/global_command.rs`

**Interfaces:**
- Port: command text only. `factory_core::GLOBAL_COMMAND` includes the asset, `factory plugin sync` writes the plugin copy, and `plugin_files_match_generated_text` checks it.
- Produces: no new items.

**Tests added:** a step 3 assertion in `the_command_asks_for_a_commit_line_in_every_plan`.

- [ ] **Step 1: Read the code**

Read step 3 (`3. Always validate`) and the one-job spec sentence in step 1 of `crates/factory-core/assets/global-command.md`. Read `the_command_asks_for_a_commit_line_in_every_plan` and `global_command_describes_one_job_and_consent` in `crates/factory-cli/tests/global_command.rs`. Read `command_is_thin_and_pins_the_shapes` in `crates/factory-cli/tests/agent_lint.rs`, and `plugin_files_match_generated_text` in `crates/factory-cli/tests/plugin_files.rs`.

- [ ] **Step 2: Write the failing test**

In `the_command_asks_for_a_commit_line_in_every_plan`, the test already slices step 3 as the text between `3. Always validate` and ``4. Run `<factory>``. Add:

```rust
assert!(
    validate.contains("With `--one-job`, also check the spec's `**Commit:**` line under its title by the same rule."),
    "step 3 must check the one-job spec line"
);
```

The test names its step 3 slice `validate`.

- [ ] **Step 3: Run it and see it fail**

Run: `cargo test -j4 -p factory-cli --test global_command the_command_asks_for_a_commit_line_in_every_plan`
Expected: it fails with `step 3 must check the one-job spec line`.

- [ ] **Step 4: Implement**

In step 3 of `global-command.md`, right after the sentence that ends `refuses a broken one.`, add: "With `--one-job`, also check the spec's `**Commit:**` line under its title by the same rule. If the line is missing, the subject is `feat: <spec title>`." Then run `cargo run -j4 -q -p factory-cli --bin factory -- plugin sync`.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-cli --test global_command`, `cargo test -j4 -p factory-cli --test plugin_files` and `cargo test -j4 -p factory-cli --test agent_lint`.
Expected: all pass, 0 failed. `git status` shows `plugins/factory/commands/factory.md` changed.

### Task 3: the machine wait tests and the unit test disk check

**Files:**
- Modify: `crates/factory-engine/tests/builder_recover.rs`
- Modify: `crates/factory-engine/src/drafts.rs`, `crates/factory-engine/src/park.rs`

**Interfaces:**
- Port: behind `AgentRunner`. Tests use `BuilderScript`, `failed_run`, `fake_machine`, `disk_fixture`, `short_on_third_call` and `change_effect` from `builder_recover.rs` and `tests/common/mod.rs`. `Engine::with_machine_wait` replaces the disk check and the clock.
- Produces: no new items.

**Tests added:** `a_failed_disk_run_waits_for_the_disk_then_reruns`, `an_other_failure_waits_for_the_disk_before_its_rerun`, and a `machine_wait` assert in `wait_ends_on_cancel`.

- [ ] **Step 1: Read the code**

Read these in `crates/factory-engine/src/recover.rs`:
- `run_builder_recovering`, `setback`, `Rerun::run`, `wait_while_down` and `wait_for_machine`;
- `classify_failure` and the `DISK` patterns in `crates/factory-agent/src/failure.rs`.

In `crates/factory-engine/tests/builder_recover.rs`, read `fake_machine`, `disk_fixture`, `short_on_third_call`, `script`, `step`, `OTHER`, `wait_ends_on_cancel`, `disk_blocked_report_waits_then_reruns_with_note` and `other_failure_rerun_succeeds`. Also read the `driver` helper in `drafts.rs` `mod tests` and `with_unreadable_board` in `park.rs` `mod tests`.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn a_failed_disk_run_waits_for_the_disk_then_reruns() {
    let fx = disk_fixture();
    let m = fake_machine(short_on_third_call(), |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![step(Some(failed_run("write: No space left on device", 3, 50, 500)))],
    );
    let out = fx.engine(&s).with_machine_wait(m.wait).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review), "{}", ledger(&fx));
    assert_eq!(s.specs().len(), 2);
    assert!(ledger(&fx).contains("builder failed (disk), rerunning once"), "{}", ledger(&fx));
    assert!(m.disk_calls.lock().expect("calls").contains(&60.0));
    assert_eq!(fx.job().machine_wait, None);
}
```

| Test | Input | Assertion |
|---|---|---|
| `an_other_failure_waits_for_the_disk_before_its_rerun` | `disk_fixture()`, `fake_machine(short_on_third_call(), ..)`, one step `failed_run(OTHER, 3, 50, 500)`, inner `vec![change_effect()]` | `Advanced(Stage::Review)`; `s.specs().len() == 2`; `disk_calls` contains `60.0`; the ledger contains `builder failed (claude reported success: it broke), rerunning once`; `machine_wait == None` |
| `wait_ends_on_cancel` | unchanged | add `assert_eq!(fx.job().machine_wait, None);` at the end |

Before relying on `short_on_third_call`, count the disk checks on each path in `recover.rs`: two before the first run, and one after the failure or before the rerun. If the count differs, write a counter closure that is short only on the check after the failed run, and say why in a comment.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-engine --test builder_recover`
Expected: all pass, because they pin today's behaviour. A failure means the setup does not reach the wait: fix the setup, not `recover.rs`.

- [ ] **Step 4: Unit test configs**

- In `drafts.rs` `mod tests`, `driver` passes `Config { min_free_disk_mb: 0, ..Config::default() }`.
- In `park.rs` `mod tests`, `with_unreadable_board` passes `factory_core::Config { min_free_disk_mb: 0, ..factory_core::Config::default() }`.
- Add a short comment on each saying that unit tests skip the real disk check.

- [ ] **Step 5: Run them**

Run: `cargo test -j4 -p factory-engine --lib drafts`, then `cargo test -j4 -p factory-engine --lib park`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-engine`, `cargo fmt -p factory-core`, `cargo fmt -p factory-git`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-git --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: none
