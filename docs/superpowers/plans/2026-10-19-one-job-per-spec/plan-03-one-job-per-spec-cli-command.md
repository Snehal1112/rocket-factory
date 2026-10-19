# One job per spec, CLI flag and /factory command Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory new --one-job` creates the one job, and the global `/factory` command accepts `--one-job` and asks the user when the driver reports `needs consent:`.

**Architecture:** Two optional clap flags on `New` route `new_job` to `create_one_job`. The first output line stays `created job <id> in <dir>` because `factoryd` parses it. The over budget refusal reaches the user through the existing error line. The global command text, its test pins and the docs are updated together.

**Tech Stack:** Rust, cargo with `-j4` per crate, clap, Markdown.

**Spec:** `docs/superpowers/specs/2026-10-19-one-job-per-spec-design.md`

## Global Constraints

- Every cargo call has `-j4` and `-p <crate>`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Follow each task's steps in order: write the failing tests as given, run the stated command and confirm the failure, implement from the signatures, then confirm the pass.
- Without `--one-job` every command, output line and exit code is unchanged. The first output line of `new` stays `created job <id> in <dir>...`.
- `factoryd` and its API are not changed in this plan. `--series` on a job with no series stays harmless.
- The Rocket copy `.claude/commands/factory.md` is not changed. The global asset `crates/factory-core/assets/global-command.md` is.
- Paths reach the driver only through `Layout`. The command never edits `project.toml`.
- Tests use `.expect("...")` and `expect_err`. Never write the literal text of a panicking unwrap call in a plan, doc or Rust file. No `git` CLI, no commit steps. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. **`--accept-over-budget` without `--one-job`.** clap must reject it. Pinned in Task 1 (`accept_over_budget_needs_one_job`).
2. **`--one-job` without `--plans`.** clap must reject it. Pinned in Task 1 (`one_job_needs_plans`).
3. **The created line stays first.** `factoryd` parses it, so notes and warnings come after it. Pinned in Task 1 (`one_job_output_starts_with_the_created_line`).
4. **The refusal is readable.** An over budget `new` exits non-zero with a `needs consent:` line and creates nothing. Pinned in Task 1 (`over_budget_new_prints_needs_consent_and_creates_nothing`).
5. **The command strips the token.** The `--one-job` token never lands in the feature text or the slug, and the consent flow re-runs `new` with `--accept-over-budget`. Pinned in Task 2 (`global_command_describes_one_job_and_consent`).

## File Structure

- Modify `crates/factory-cli/src/cli.rs`: the two flags and their parse tests.
- Modify `crates/factory-cli/src/commands.rs`: `new_job` and the dispatch.
- Create `crates/factory-cli/tests/one_job.rs`: the CLI tests, set up like `tests/e2e.rs`.
- Modify `crates/factory-core/assets/global-command.md`, `crates/factory-cli/tests/global_command.rs`.
- Modify `README.md` and `docs/software-factory.md`: a short "One job per spec" section.

---

### Task 1: The CLI flags

**Files:**
- Modify: `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs`
- Create: `crates/factory-cli/tests/one_job.rs`

**Interfaces:**
- Consumes: `create_one_job(...)  -> Result<OneJob, EngineError>` from plan 2.
- Produces: on `Command::New`, `#[arg(long, requires = "plans")] one_job: bool` and `#[arg(long, requires = "one_job")] accept_over_budget: bool`. `new_job` gains both parameters. With `one_job` it calls `create_one_job` and prints `created job {id} in {dir}{estimate_lines}`, then one `note: {line}` line per note and one `warning: {line}` line per warning.

- [ ] **Step 1: Write the failing tests:** in `cli.rs` `#[cfg(test)]`: `one_job_needs_plans` (`new s f --one-job` is a usage error), `accept_over_budget_needs_one_job` (`new s f --plans p --accept-over-budget` is a usage error), `one_job_flags_parse` (`new s f --plans p --one-job --accept-over-budget` parses). In `tests/one_job.rs`: `one_job_output_starts_with_the_created_line` (a 2 plan fixture with a 2.0M budget: the first output line starts with `created job `), `over_budget_new_prints_needs_consent_and_creates_nothing` (a 4 plan, 12 task fixture at the default budget: the call fails, the message starts with `needs consent:` and the board has no jobs), and `without_the_flag_new_is_unchanged` (the same plans without the flag give the series output as before).
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-cli one_job`. Expected: failure, since the flags do not exist.
- [ ] **Step 3: Implement** the two flags and the `new_job` branch.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-cli one_job`, then `cargo test -j4 -p factory-cli` and `cargo check -j4 -p factory-cli`. Expected: pass, including every existing CLI test.

### Task 2: The /factory command text and docs

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `crates/factory-cli/tests/global_command.rs`, `README.md`, `docs/software-factory.md`

**Interfaces:**
- Consumes: the flags from Task 1.
- Produces: in `global-command.md`: the `argument-hint` gains `[--one-job]`. A feature may start or end with the single token `--one-job`, handled like `--opus` (it is removed from the feature text and the slug). Step 4 adds `--one-job` to the `new` call when it was given. When `new` prints a `needs consent:` line, the command tells the user the estimate and the budget in one or two plain sentences, asks whether to go ahead over the budget (raising `budget_weighted` is the other way out), re-runs the same `new` with `--accept-over-budget` on a yes, and stops on a no without creating a job. It never edits `project.toml`. Plan writing is unchanged: at most 3 tasks per plan.

- [ ] **Step 1: Write the failing test** `global_command_describes_one_job_and_consent` in `tests/global_command.rs`, in the style of its existing pins: the asset contains `[--one-job]`, `--one-job`, `needs consent:`, `--accept-over-budget`, and still contains `at most 3 tasks`, and the lint tests of the Rocket copy still pass untouched.
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-cli --test global_command`. Expected: the new test fails.
- [ ] **Step 3: Edit** the asset as described, then add a short "One job per spec" section to `README.md` and `docs/software-factory.md`: what the flag does, the estimate rule `110K + 100K per task`, the consent flag, and that the default stays one job per plan.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-cli --test global_command --test agent_lint`, then `cargo test -j4 -p factory-cli` and `cargo check -j4 -p factory-cli`. Expected: pass.

Next plan: none
