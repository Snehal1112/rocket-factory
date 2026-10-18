# Gate jobs that merge with no checks or no tests

Ticket: Snehal1112/rocket-factory#2.

## Goal

A job that changes code must not merge because no check ran. The driver parks it for the human when no check rule matches a code diff, warns the reviewer when a code diff touches no test file, and records which checks ran. Rocket's `src-tauri` rule also runs tests.

## Why

`VERDICT: APPROVE` is a last-line token from a reviewer that cannot run anything. Two gaps let a job reach auto-merge with weak evidence:

- When no `[[checks]]` rule matches, or every matching command is dropped by `validate_command`, `run_checks` writes `PASS none (no check is mapped to the changed paths)` (`factory-checks/src/run.rs:187-190`). `latest_checks_ok` accepts it (`factory-engine/src/review.rs:94-97`), so zero checks count as green.
- Nothing requires a test. Only the builder and reviewer prompts ask for one.
- Rocket's `src-tauri/**` rule runs `cargo check -j4` only, so a change there merges with no tests run.

## Non-goals

- No proof that each acceptance criterion has a test. That stays a reviewer judgment.
- No change to the reviewer's tools or to how `VERDICT:` is parsed.
- No change to the order in `decide()` for risky diff, budget and size.

## Behaviour

1. **No checks on a code diff parks the job.** After the checks stage, if `checks.md` says `PASS none` and any changed path is a code path, the job parks as needs-human with a new park kind `no-checks`. The reason names the changed code paths (at most 5, then a count). The check runs before the reviewer, so no review tokens are spent. A diff of only non-code paths (docs) keeps passing with `PASS none`.
2. **The final re-run is gated the same way.** `final_checks` on the final head applies the same rule, so a fix round cannot slip past it.
3. **Code paths are configurable.** New setting `code_paths` (globs). The default treats every path as code except `**/*.md`, `docs/**` and `.claude/**`. A missing or broken config makes every path count as code.
4. **A code diff with no test file warns.** New setting `test_paths` (globs). The default covers `**/tests/**`, `**/__tests__/**`, `**/*.test.ts`, `**/*.test.tsx`, `**/*_test.rs`, `**/*_test.go` and `**/*.spec.ts`. When the job diff has a code path and no test path, the driver writes a ledger line and adds one line to the reviewer's input saying no test file changed. It does not park.
5. **`require_tests` parks instead of warning.** New setting `require_tests`, default false. When true, the same condition parks as `no-tests`. A project opts in.
6. **The ledger records the checks that ran.** When the driver reaches the merge decision, it writes one ledger line listing the commands that ran and their result, or `none`.
7. **Rocket's `src-tauri` rule runs tests.** The rule becomes `cargo check -j4 -p rocket` and `cargo test -j4 -p rocket`. Both Rocket fixtures in `factory-core/tests/fixtures/` are updated. The live `~/.factory/projects/rocket/project.toml` is outside this repo and is edited by the user after the merge. This is stated in the job notes.

## Constraints

- Add the new park kinds and settings as optional with defaults, so board JSON and old configs stay compatible with `dashboard/`.
- `no-checks` and `no-tests` go in `decide()` right after the diff-size check and before the checks-failed branch, as inputs on `DecideInput`.
- Engine logic stays behind the ports, so the tests use the fakes.
- No shelling out to `git`. `gh` is not touched.

## Acceptance criteria

- A diff with a code path and `PASS none` parks with kind `no-checks`, and the reviewer is not run.
- A diff of only `*.md` or `docs/**` paths and `PASS none` is not parked.
- With no config, or a config that fails to parse, every changed path counts as code.
- `code_paths` and `test_paths` from the config replace the defaults.
- A code diff with no test path writes a ledger line and a reviewer note, and does not park.
- With `require_tests = true`, a code diff with no test path parks as `no-tests`.
- The `no-checks` rule also applies to `final_checks` on the final head.
- The ledger has one line naming the commands that ran, or `none`.
- The new park kinds have stable `as_str` names `no-checks` and `no-tests`, and the dashboard shows them without error.
- Rocket's fixtures have the new `src-tauri` rule, and the fixture parse tests pass.
- `cargo check -j4` and the targeted crate tests for `factory-core`, `factory-checks` and `factory-engine` pass.

## Follow-ups

- Edit the live Rocket registration `~/.factory/projects/rocket/project.toml` to match the fixture. It is outside the repo, so a human does it.
- Check whether a cargo test with a filter that matches zero tests should count as a pass. Today it does.

## Decisions

- What happens on `PASS none` for a code diff? Options: park as needs-human, fail the checks for a fix round, warn only. Chosen: park as needs-human with a new `no-checks` kind. Decided by: user
- How is a code diff told from a docs-only diff? Options: `code_paths` config with a default, reuse the `[[checks]]` globs, a hardcoded docs list. Chosen: `code_paths` config with a default of everything except docs. Decided by: user
- What happens when a code diff touches no test file? Options: warn and tell the reviewer, always park, reviewer prompt only. Chosen: warn and tell the reviewer, with `require_tests` to park. Decided by: user
- How is a test file recognised? Options: `test_paths` config with a default, a fixed default list. Chosen: `test_paths` config with a default. Decided by: user
- Scope for `src-tauri` and the ledger report? Options: ledger line only with src-tauri as a follow-up, include Rocket's config fix, skip the ledger line. Chosen: include Rocket's config fix. Decided by: user
- The `src-tauri` crate name is `rocket`, from the existing `cargo test -j4 -p rocket` use in Rocket's plans. Decided by: call
- Where `no-checks` sits in `decide()`: after size and before checks-failed, so a risky diff still wins. Decided by: call
- The live `project.toml` is a follow-up, not part of the job, because it is outside the repo. Decided by: call
