# Subfolder project tests Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-harness-ci-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Pin the subfolder project behaviour that is correct only by reading the code today. That covers the builder and reviewer folders, the overlap, disabled-show and top-folder texts, scoped crash-resume, path mapping, the worktree name clash and `worktree_root_of`.

**Commit:** test(scope): pin subfolder project behaviour

**Architecture:** Mostly tests over `Repo::with_scope`. There are two text changes: the inner-folder overlap error in `crates/factory-core/src/project.rs`, and the clash error in `create_worktree` in `crates/factory-git/src/worktree.rs`. The engine tests sit behind the `AgentRunner` port with the scripted fakes, and git stays behind `factory_git::Repo`.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, which is a risk path.
- Worktree and branch names do not change.
- The reviewer prompt text does not change, because golden prompt tests pin it.
- No board, ledger or event field.

## Review Focus

1. A top-folder project: a user expects every cwd at the worktree root, as today. Pinned by the existing `top_folder_project_cwd_is_unchanged`, which must pass unchanged.
2. A new subfolder inside an existing subfolder project: a user expects today's text. Pinned by a row of `overlap_names_the_inner_folder`.
3. The name clash: a user expects no stray branch left behind. Pinned by `a_worktree_name_clash_across_projects_fails_cleanly`, which checks the branch list.
4. A rename from outside into the project: a user expects the outside side passed through unchanged. Pinned by `scoped_paths_maps_both_sides_of_a_rename`.
5. A scoped job whose base is merged: a user expects no out-of-scope refusal. Pinned by `a_scoped_job_with_a_merged_base_resumes`.

## File Structure

- Modify: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-engine/tests/gates.rs`, `crates/factory-engine/tests/merge.rs`
- Modify: `crates/factory-core/src/project.rs` (`check_overlap` and its tests)
- Modify: `crates/factory-cli/tests/project_cmd.rs`
- Modify: `crates/factory-git/src/repo.rs` (doc comments), `crates/factory-git/src/worktree.rs` (`create_worktree` error), `crates/factory-git/tests/repo_basics.rs`, `crates/factory-git/tests/worktree.rs`

---

### Task 1: every builder kind and the reviewer start in the project folder

**Files:**
- Modify: `crates/factory-engine/tests/agent_files.rs`

**Interfaces:**
- Port: behind `AgentRunner`. The tests use `scoped_fixture()`, `Fixture` and the scripted runners in `crates/factory-engine/tests/common/mod.rs`.
- Consumes: the `workdir_in` call sites in `crates/factory-engine/src/specs.rs`, which are read only.

**Tests added:** `every_builder_and_the_reviewer_start_in_the_project_folder`, one test covering every kind. The kinds are the task builder, task resume, task carry, the resumed fix builder, the fresh fix builder and the reviewer.

- [ ] **Step 1: Read the code**

Read `scoped_fixture`, `builder_starts_in_the_project_folder` and `top_folder_project_cwd_is_unchanged` in `crates/factory-engine/tests/agent_files.rs`. Read every `workdir_in` call site in `crates/factory-engine/src/specs.rs`, and the reviewer prompt in `reviewer_spec_with_model` (`the worktree at {cwd}`). Read how these tests drive each kind:
- `a_crash_session_beats_a_carry` in `crates/factory-engine/tests/task_carry.rs` (task resume: it sets `task_session`);
- `task_two_continues_the_session_when_gap_and_context_allow` and `two_task_fixture` in `crates/factory-engine/tests/task_carry.rs` (task and carry builders);
- `reach_fix_round` and `an_idle_large_session_gets_a_fresh_fix_builder` in `crates/factory-engine/tests/fix_fresh.rs` (resumed and fresh fix builders);
- a review run in `crates/factory-engine/tests/gates.rs`, such as `risk_paths_are_project_relative`.

- [ ] **Step 2: Write the tests**

- Reuse those flows on `scoped_fixture()`, and record each spec with the runner's `specs()` or `calls()`.
- Assert `spec.cwd == <worktree>.join("apps/web")` for each kind, where the worktree path comes from the existing `worktree(&fx)` helper in `agent_files.rs`. Name the kind in the message.
- For the reviewer, also assert that `spec.prompt` contains `format!("the worktree at {}. ", wt.display())`. The trailing `. ` keeps it from matching `<wt>/apps/web`.
- The reviewer needs `checks_stage()` to run before `review_stage()`, or the review fails with an Io error.

When a flow needs a two-task plan, write it into `fx.plan_file` as `two_task_fixture` does.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-engine --test agent_files`
Expected: all pass, because they pin today's behaviour. If one fails, check the setup first. A real cwd bug goes in the ledger as a finding, without changing `specs.rs`.

### Task 2: overlap, disabled show and the top-folder text

**Files:**
- Modify: `crates/factory-core/src/project.rs`
- Modify: `crates/factory-cli/tests/project_cmd.rs`

**Interfaces:**
- Port: no engine port. `check_overlap(home, canon, subpath) -> Result<(), CoreError>` keeps its signature.
- Consumes: `make_monorepo`, `add_refuses_overlap_and_root_after_subfolder`, `list_shows_subfolder_project_disabled_when_folder_is_gone` and `show_of_a_project_with_a_missing_repo_is_disabled_like_list` in `crates/factory-cli/tests/project_cmd.rs`.

**Tests added:** `overlap_names_the_inner_folder` (in the `crates/factory-core/src/project.rs` tests, next to `overlap_is_refused`), `show_of_a_subfolder_project_with_a_missing_folder_is_disabled`, `a_registered_subfolder_repo_path_names_the_top_folder_rule`, and the full text asserts in `add_refuses_overlap_and_root_after_subfolder`.

- [ ] **Step 1: Read the code**

Read `check_overlap` in `crates/factory-core/src/project.rs`, `disabled_reason` and `show` in `crates/factory-cli/src/project.rs`, the `NotTopFolder` error (`is not the top folder of a git repository`) in `crates/factory-git/src/error.rs` and where the CLI reports it. Read the four tests named above.

- [ ] **Step 2: Write the failing tests**

| Test | Setup | Assertion |
|---|---|---|
| `overlap_names_the_inner_folder` (`crates/factory-core/src/project.rs` tests) | with that file's helpers, add `web` at `apps/web`, then on the canonical repo: `inner` at `apps/web/inner`, a top-folder project, and `apps` at `apps` | each errors: the first ends with `project web already covers <canon>/apps/web/inner`, the second with `project web already covers <canon>/apps/web, inside <canon>`, the third with `project web already covers <canon>/apps/web, inside <canon>/apps` |
| `add_refuses_overlap_and_root_after_subfolder` | unchanged, after `web` at `apps/web`. Create `apps/web/x` with `create_dir_all` first, because `project add` canonicalizes the path, and pass `--name x` | the error for `apps/web/x` ends with `project web already covers <repo>/apps/web/x`; the error for the repo root ends with `project web already covers <repo>/apps/web, inside <repo>`, with no trailing slash after `<repo>`. The CLI prefixes `config error: ` |
| `show_of_a_subfolder_project_with_a_missing_folder_is_disabled` | as `list_shows_subfolder_project_disabled_when_folder_is_gone`, then `project show web` | output contains `disabled: project folder is missing` and `subpath: apps/web` |
| `a_registered_subfolder_repo_path_names_the_top_folder_rule` | add `apps/api` as `make_monorepo` tests do, then rewrite its `project.toml`: drop the `subpath = "apps/api"` line and set `repo` to `<canon>/apps/api` | `project show api` contains `disabled: <canon>/apps/api is not the top folder of a git repository`, and `project list` names the same reason. `project add` itself cannot reach this text, because it accepts a subfolder as a subfolder project |

Build every expected text with `format!` from `std::fs::canonicalize(&repo).expect("canon")`, never from the tempdir path, because macOS resolves `/var` to `/private/var`. Check the real `add` argument names in `project_cmd.rs`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli --test project_cmd`, then `cargo test -j4 -p factory-core overlap`
Expected: the root-after-subfolder assert fails, and the second and third rows of `overlap_names_the_inner_folder` fail. The other new and changed tests pass.

- [ ] **Step 4: Implement**

In `check_overlap`, when `other.starts_with(new)` and `other != new`, return `project {name} already covers {canon.join(other)}, inside {outer}`, where `outer` is `canon` itself when `new` is empty and `canon.join(new)` otherwise. Every other overlap keeps today's text.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-cli --test project_cmd`, then `cargo test -j4 -p factory-core project`.
Expected: all pass, 0 failed.

### Task 3: scoped crash-resume, path mapping, the name clash and `worktree_root_of`

**Files:**
- Modify: `crates/factory-git/src/repo.rs`, `crates/factory-git/src/worktree.rs`
- Modify: `crates/factory-git/tests/repo_basics.rs`, `crates/factory-git/tests/worktree.rs`
- Modify: `crates/factory-engine/tests/gates.rs`, `crates/factory-engine/tests/merge.rs`

**Interfaces:**
- Port: `factory_git::Repo` (`scoped_paths`, `worktree_root_of`, `create_worktree`, `with_scope`, `job_ref`). Merge goes through `merge_stage` with `common::approved_scoped_job`.
- Produces: no new items. `create_worktree` keeps its signature, with a new error text.

**Tests added:** `scoped_paths_maps_both_sides_of_a_rename`, `scoped_paths_of_nothing_is_nothing`, `out_of_scope_paths_pass_through_scoped_paths`, `worktree_root_of_is_lexical` (in `repo_basics.rs`), `a_worktree_name_clash_across_projects_fails_cleanly` (in `worktree.rs`), `a_scoped_job_with_a_merged_base_resumes` (in `merge.rs`), and a `checks.md` assert in `checks_map_project_relative_paths` (in `gates.rs`).

- [ ] **Step 1: Read the code**

Read these:
- `scoped_paths`, `out_of_scope` and `worktree_root_of` in `crates/factory-git/src/repo.rs`, and `workdir_in_and_worktree_root_of` in `crates/factory-git/tests/repo_basics.rs`;
- `create_worktree` and `dir_name` in `crates/factory-git/src/worktree.rs`, and `same_id_and_slug_in_two_projects_do_not_collide` in `crates/factory-git/tests/worktree.rs`;
- the `BaseState::Merged` path in `crates/factory-engine/src/merge.rs`, `approved_scoped_job`, `an_in_scope_head_merges` in `crates/factory-engine/tests/merge.rs`, and the helper `common::merge_by_hand`;
- `checks_map_project_relative_paths` and `only_src_checks` in `crates/factory-engine/tests/gates.rs`.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn a_worktree_name_clash_across_projects_fails_cleanly() {
    let fx = Fixture::new(); // factory-git tests: the repo path field is `fx.path`
    let a001 = fx.repo().with_scope("a-001", std::path::Path::new("apps/a1"));
    let a = fx.repo().with_scope("a", std::path::Path::new("apps/a"));
    a001.create_worktree(&a001.job_ref("001", "x")).expect("first");
    let err = a.create_worktree(&a.job_ref("001", "001-x")).expect_err("clash");
    assert!(err.to_string().contains("another job or project uses the worktree name factory-a-001-001-x"), "{err}");
    let repo = git2::Repository::open(&fx.path).expect("open");
    assert!(repo.find_branch(&a.job_ref("001", "001-x").branch_name(), git2::BranchType::Local).is_err());
}
```

Check the `Fixture` fields, `job_ref` and `branch_name` against the file.

| Test | Input | Assertion |
|---|---|---|
| `scoped_paths_maps_both_sides_of_a_rename` | scope `apps/web`; paths `["apps/web/old.rs", "apps/web/new.rs", "lib/x.rs"]` | equals `["old.rs", "new.rs", "lib/x.rs"]` |
| `scoped_paths_of_nothing_is_nothing` | scope `apps/web`; `[]` | empty |
| `out_of_scope_paths_pass_through_scoped_paths` | scope `apps/web`; `["apps/webx/a.rs"]` | unchanged |
| `worktree_root_of_is_lexical` | `<worktrees>/a/../b` | `Some(<worktrees>/a)` |
| `checks_map_project_relative_paths` | unchanged | also `std::fs::read_to_string(fx.job_dir.join("checks.md"))` contains `FAIL false` |
| `a_scoped_job_with_a_merged_base_resumes` | `approved_scoped_job(&["apps/web/a.txt"])`, then `common::merge_by_hand(&fx)`, which leaves the base merged, then `merge_stage(&fx.driver, &fx.id)` | it returns `Merged` without an out-of-scope refusal, and main's tip is the one `merge_by_hand` made |

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-git --test repo_basics`, `cargo test -j4 -p factory-git --test worktree`, `cargo test -j4 -p factory-engine --test gates`, `cargo test -j4 -p factory-engine --test merge`.
Expected: only `a_worktree_name_clash_across_projects_fails_cleanly` fails, on its text. The others pin today's behaviour.

- [ ] **Step 4: Implement**

- The `path.exists()` refusal in `create_worktree` says `{path} already exists; another job or project uses the worktree name {name}`.
- Add to the doc comment of `scoped_paths`: "A path outside the project folder passes through unchanged."
- Add to the doc comment of `worktree_root_of`: "The match is lexical: `..` is not resolved."

- [ ] **Step 5: Run them and see them pass**

Run the same four commands.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-git`, `cargo fmt -p factory-core`, `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-git --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-harness-ci-leftovers/plan-04-harness-ci-leftovers.md
