# Builder Permissions Fixes, Plan 1: The validator, the deny rules and the guard

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Close the gaps a code review found in the builder permissions work: `builder_allow` and derived programs that grant a forbidden program or a shell, a writable KB folder, a guard park that offers `approve`, and a guard that is only tested on the first run.

**Architecture:** `factory-core` gets one shared safety check for programs (`builder_program_ok`), a longer never-allowed list, a stricter `builder_allow` validator and a new park kind. `factory-engine` uses the shared check when it derives programs, adds deny rules for every KB folder, widens the path guard, and makes the file guard restore before it propagates an error, park with the new kind and keep the builder session. The engine logic reads only the `Config` and paths and the job folder, as before, so no new port is needed.

**Tech Stack:** Rust 2021, `factory-core`, `factory-engine`.

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- `NEVER_ALLOWED_PROGRAMS` keeps its first nine entries in order (`git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env`, `xargs`) and appends, in this order: `zsh`, `dash`, `ksh`, `fish`, `busybox`, `source`, `.`, `eval`, `exec`, `command`, `timeout`, `nice`, `nohup`, `time`, `stdbuf`, `setsid`. That is 25 names.
- `pub fn builder_program_ok(word: &str) -> bool` is true only when `word` is not empty, holds only letters, digits and `_ - . /`, and its basename (the part after the last `/`), lowercased, is not empty and not in `NEVER_ALLOWED_PROGRAMS`.
- `pub fn is_env_assignment(word: &str) -> bool` moves from `factory-engine` to `factory-core` unchanged in behaviour (a `NAME=value` word, `NAME` of letters, digits and `_`, not starting with a digit).
- A `builder_allow` entry is rejected when its command holds `&`, `|`, `;`, `>`, `<`, a backtick, `$(`, a quote (`"` or `'`), a backslash, `*` or `?`. After leading `NAME=value` words and `-C <path>` pairs are skipped, the program is compared by its lowercased basename. A program in the never list is refused, except `git`, which is allowed only with `status`, `diff`, `log`, `show` or `rev-parse` next (compared in lowercase). Any other program must pass `builder_program_ok`.
- Error texts: `must be one plain command, without shell operators` (operators), `must not contain quotes, backslashes or wildcards`, `<program> is not allowed`, and the existing ones unchanged.
- The deny rules cover the job folder and every KB folder: `Edit(//<canonical dir without the leading slash>/**)` then `Write(...)`, job folder first.
- When the canonical job folder or any KB folder holds one of `* ? [ { ( ) , \`, `with_builder_permissions` returns the spec unchanged.
- A new park kind `ParkKind::BuilderTampered`, name `builder-tampered`, offers `rework` and `cancel` only.
- The guard's park reason stays `builder changed <file>` when every restore worked. When a restore failed it is `builder changed <file>; it could not be restored: <error>`.
- No board, ledger or events format changes; `park.json` gains only a new kind name.

## Review Focus

- `Bash(*)`, `Bash(* :*)`, `Bash(g* push:*)`, `Bash(FOO=1 rm:*)`, `Bash("rm":*)`, `Bash(\rm:*)`, `Bash(RM:*)`, `Bash(Git push:*)` and `Bash(make & rm -rf x:*)` are rejected (Task 1 test `builder_allow_rejects_the_bypass_forms`).
- `timeout 600 cargo test`, `nice cargo test`, `. venv/bin/activate`, `source x`, `eval x`, `exec x`, `command x` and `zsh -c x` derive nothing (Task 2 test `wrapper_and_shell_programs_are_never_derived`).
- A check program that is already in the base list is not repeated, and duplicate `builder_allow` entries collapse (Task 2 tests `a_check_program_already_in_the_base_is_not_repeated`, `duplicate_builder_allow_entries_are_removed`).
- The KB folder is denied for Edit and Write, and a job or KB path with `)` or `,` leaves the spec unchanged (Task 2 tests `kb_folder_is_denied_for_edit_and_write`, `a_path_with_a_rule_character_leaves_the_spec_unchanged`).
- The fail-closed config gives the base allowlist only, and `builder_permissions = false` restores all three builder specs (Task 2 tests `fail_closed_allowlist_is_the_base_list`, `builder_permissions_false_restores_all_three_builder_specs`).
- A tampered fix-round resume does not offer `approve` and keeps the builder session (Task 3 tests `a_tampered_fix_round_parks_with_rework_only`, `a_guard_park_keeps_the_builder_session`).
- The guard holds at the per-task call site and in a fix-round resume (Task 3 tests `guard_holds_in_a_per_task_run`, `guard_holds_in_a_fix_round_resume`).
- A run that fails after the builder changed a job file still gets the file restored (Task 3 test `a_runner_error_still_restores_the_job_files`).
- A restore that cannot write is reported in the park reason (Task 3 test `restore_reports_a_failed_write`).

---

## File Structure

- `crates/factory-core/src/config.rs` (modify): the never list, `builder_program_ok`, `is_env_assignment`, the validator.
- `crates/factory-core/src/lib.rs` (modify): re-exports.
- `crates/factory-core/src/park.rs` (modify): `ParkKind::BuilderTampered`.
- `crates/factory-engine/src/permissions.rs` (modify): derivation, dedupe, deny rules, the guard.
- `crates/factory-engine/src/specs.rs` (modify): `with_builder_permissions`.
- `crates/factory-engine/tests/build_checks.rs`, `driver_files_guard.rs` (modify), `permission_pins.rs` (check only).

### Task 1: The shared check, the validator and the park kind

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-core/src/park.rs`

**Interfaces:**
- Produces: `pub const NEVER_ALLOWED_PROGRAMS: [&str; 25]`; `pub fn builder_program_ok(word: &str) -> bool`; `pub fn is_env_assignment(word: &str) -> bool`; `ParkKind::BuilderTampered` with `as_str() == "builder-tampered"` and `next(id) == [rework, cancel]`. All three functions and the constant are re-exported from `factory_core`.
- Changes: private `check_builder_allow` follows the Global Constraints.

- [ ] **Step 1: Read** `check_builder_allow` and its tests, `NEVER_ALLOWED_PROGRAMS`, `READ_ONLY_GIT` in `crates/factory-core/src/config.rs`; `ParkKind`, `as_str`, `next` and the park tests in `crates/factory-core/src/park.rs`; `is_env_assignment` and `program_of` in `crates/factory-engine/src/permissions.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `builder_allow_rejects_the_bypass_forms` in `config.rs`: for each entry below, `Config::parse(&format!("builder_allow = [{entry:?}]\n"))` is `Err` and the text contains `builder_allow:`: `Bash(*)`, `Bash(* :*)`, `Bash(g* push:*)`, `Bash(FOO=1 rm:*)`, `Bash(GIT_DIR=x git push:*)`, `Bash("rm":*)`, `Bash('git' push)`, `Bash(\rm:*)`, `Bash(RM:*)`, `Bash(Git push:*)`, `Bash(make & rm -rf x:*)`, `Bash(timeout 5 make:*)`, `Bash(Timeout 5 make:*)`, `Bash(zsh:*)`, `Bash(.:*)`, `Bash(source x:*)`, `Bash(eval x:*)`, `Bash(exec x:*)`, `Bash(command x:*)`, `Bash(nohup x:*)`.
  - `builder_allow_still_accepts_plain_commands`: `Bash(make:*)`, `Bash(Make:*)`, `Bash(FOO=1 make test:*)`, `Bash(git log:*)`, `Bash(git -C x log)`, `Bash(./scripts/test.sh:*)`, `Bash(cargo test -j4 -p x:*)`, `Bash(shellcheck:*)` parse `Ok`.
  - `builder_program_ok_checks_chars_and_the_lowercased_basename`: `builder_program_ok("cargo")`, `("./scripts/t.sh")` true; `("")`, `("a*b")`, `("a b")`, `("a(b)")`, `("RM")`, `("/bin/Rm")`, `(".")`, `("timeout")` false.
  - `never_allowed_and_read_only_lists_are_exact` (existing test): update the expected `NEVER_ALLOWED_PROGRAMS` to the 25 names in the order of the Global Constraints.
  - In `park.rs`, extend the kind-name test with `ParkKind::BuilderTampered.as_str() == "builder-tampered"`, and add `builder_tampered_offers_rework_and_cancel_only`: `ParkKind::BuilderTampered.next("007") == vec!["factory rework 007 <note>", "factory cancel 007"]`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_` and `cargo test -j4 -p factory-core park` and expect FAIL (compile errors: `cannot find function 'builder_program_ok'`, `no variant 'BuilderTampered'`).
- [ ] **Step 4: Implement** the constant, the two functions, the validator rewrite and the park kind (`next`: add it to the arm of `NothingChanged`'s `[rework, cancel]` or give it its own arm). Add the re-exports in `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and expect PASS; `cargo check -j4 -p factory-engine` will fail until Task 2 only if a name moved, so keep `is_env_assignment` in the engine until Task 2 switches over.

### Task 2: Derivation, deny rules and the spec wiring

**Files:**
- Modify: `crates/factory-engine/src/permissions.rs`, `crates/factory-engine/src/specs.rs`, `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Changes: `fn program_of` uses `factory_core::{builder_program_ok, is_env_assignment}` (the engine copy of `is_env_assignment` is deleted); `pub fn builder_allowed_tools(config: &Config) -> Vec<String>` removes duplicates over the whole list, keeping the first occurrence; `pub fn builder_denied_tools(job_dir: &Path, read_only_dirs: &[PathBuf]) -> Vec<String>`.
- Changes: `with_builder_permissions` passes `self.kb_dirs()` canonicalized as `read_only_dirs`, applies the character guard to the job folder and every KB folder, and returns the spec unchanged when any holds a guarded character.

- [ ] **Step 1: Read** `permissions.rs`, `with_builder_permissions` and `kb_dirs` in `specs.rs`, `with_kb` in `crates/factory-engine/tests/kb.rs`, and the permission tests in `build_checks.rs`.
- [ ] **Step 2: Write the failing tests.** In `permissions.rs`:
  - `wrapper_and_shell_programs_are_never_derived`: commands `timeout 600 cargo test`, `nice cargo test`, `. venv/bin/activate`, `source x`, `eval x`, `exec x`, `command x`, `zsh -c x`, `nohup x`, `time cargo t`, `stdbuf -o0 x`, `setsid x`, `busybox sh` give the base list only.
  - `a_check_program_already_in_the_base_is_not_repeated`: commands `grep -r x`, `ls` give the base list.
  - `duplicate_builder_allow_entries_are_removed`: `builder_allow = ["Bash(make:*)", "Bash(make:*)", "Bash(cd:*)"]` gives the base plus one `Bash(make:*)`.
  - `fail_closed_allowlist_is_the_base_list`: `builder_allowed_tools(&Config::fail_closed()) == base()`.
  - `allowlist_never_contains_env_xargs_rmdir_or_shells`: over every preset as `allowlist_never_contains_commit_add_push_rm_curl` does, no entry has a program equal to any of the 25 names except the base `git status|diff|log|show` rules.
  - `denied_tools_cover_the_job_folder`: update to the two-argument call with an empty slice; add `denied_tools_list_each_read_only_folder_after_the_job_folder`: a job dir and two folders give six rules in the order job Edit, job Write, first Edit, first Write, second Edit, second Write.
  In `build_checks.rs` (use `with_kb`'s setup of `fx.config.kb_path`):
  - `kb_folder_is_denied_for_edit_and_write`: run `build_stage` and assert the first builder spec's `disallowed_tools` contains `Edit(//<canonical kb without the leading slash>/**)` and the `Write` twin.
  - `a_path_with_a_rule_character_leaves_the_spec_unchanged`: for a job folder renamed to `job)1` and to `job,1`, and for a KB folder named `kb)1`, the first builder spec has `permission_mode == None` and empty `allowed_tools`.
  - `builder_permissions_false_restores_all_three_builder_specs`: extend the existing test so the task spec (`two_task_fixture`) and the resume spec (second `build_stage` after a rework note) have `permission_mode == None` and empty `allowed_tools` and `disallowed_tools`, and their `args(&Caps::default())` contain none of `--permission-mode`, `--allowedTools`, `--disallowedTools`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine permissions` and `cargo test -j4 -p factory-engine --test build_checks permission` and expect FAIL.
- [ ] **Step 4: Implement** the changes above. Delete the engine's `is_env_assignment`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-agent` and expect PASS, then `cargo clippy -j4 -p factory-core -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-engine -- --check`.

### Task 3: The guard

**Files:**
- Modify: `crates/factory-engine/src/permissions.rs`, `crates/factory-engine/tests/driver_files_guard.rs`

**Interfaces:**
- Changes: `run_builder_guarded` runs the agent, restores the changed files whether the run succeeded or not, and only then propagates an `Err`; it parks with `ParkKind::BuilderTampered`; before parking it records the run's session in `builder_id` the way the call sites do (`run.session_id` or the earlier value); it logs a failed restore with `tracing::warn!` (event name in the style of `crates/factory-engine/src/ports.rs`) instead of `eprintln!`.
- Produces: `fn restore_file(path: &Path, old: Option<&[u8]>) -> Result<(), String>`: writes `old` back, or removes the file when `old` is `None`. A failure is the message returned.

- [ ] **Step 1: Read** `run_builder_guarded` in `permissions.rs`, the two builder call sites in `build.rs` (`let session = ...` and `builder_id`), the tests in `driver_files_guard.rs`, `the_fix_resume_prompt_names_receiving_code_review_and_the_needs_context_rule` in `build_checks.rs` (how a round-2 resume is set up), and `ParkRecord::read` in `crates/factory-core/src/park.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `guard_holds_in_a_per_task_run`: `two_task_fixture`, first builder effect rewrites `plan.md`; `build_stage` parks with `builder changed plan.md` and the file is restored.
  - `guard_holds_in_a_fix_round_resume`: one clean first build, then set up round 2 as the fix-resume test does (write `changes-r2.md`, set `round = 2`, stage `build`), and make the resumed builder rewrite `plan.md`; the stage parks with `builder changed plan.md` and the file is restored.
  - `a_tampered_fix_round_parks_with_rework_only`: in that fix-round case, `ParkRecord::read(&fx.job_dir)` has kind `builder-tampered` and its `next` list holds no `approve`.
  - `a_guard_park_keeps_the_builder_session`: after a guard park on a first run, `fx.job().builder_id` equals the session id the scripted runner returned (read it from the scripted run's spec or reply as the neighbouring session tests do).
  - `a_runner_error_still_restores_the_job_files`: the effect rewrites `plan.md` and then deletes the board file, so the driver's next board update returns `Err`; assert `build_stage()` is `Err` and `plan.md` has its original text.
  - `restore_reports_a_failed_write`: `restore_file(&tmp.path().join("missing-dir").join("plan.md"), Some(b"x"))` is `Err` with a non-empty message; and `restore_file` of an existing path with `None` removes it.
  - Update the existing guard tests so any park-kind or next-command assertion matches the new kind; the reason strings stay `builder changed <file>`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test driver_files_guard` and expect FAIL.
- [ ] **Step 4: Implement** the changes above.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-core`, then `cargo clippy -j4 -p factory-core -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-engine -- --check`. Expect PASS.

Next plan: docs/superpowers/plans/2026-10-20-builder-permissions-fixes/plan-02-builder-permissions-fixes.md
