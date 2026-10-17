# Reviewer model and job budget Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Add an opt-in stronger reviewer model for risky or large diffs and lower the default per-job budget to 1M weighted tokens.

**Architecture:** Two new config keys choose the reviewer model per review. The per-job budget default becomes one named constant; the existing warn-at-80-percent and stop-at-100-percent logic is unchanged.

**Tech Stack:** Rust, cargo with `-j4`, `factory-checks`, `factory-agent` (`AgentSpec`, `fake-claude`).

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 3 per-task checks, Design 4).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- No commit steps in any task. The driver commits.
- The default per-job budget is 1,000,000 weighted tokens; warn at 80 percent and stop at 100 percent as today. A config that sets `budget_weighted` is unchanged. The daily budget stays off by default.

## Review Focus

- A diff exactly at `reviewer_large_over_lines` must use the normal model, and one line over must use the large model.
- A risky diff that is also tiny must still use the large model, and a project with no `reviewer_model_large` must see no change at all.
- An existing project config with its own `budget_weighted` must keep that value, and `rocket_parity` must still hold.

---

### Task 1: Stronger reviewer model on risky or large diffs

**Files:**
- Modify: `crates/factory-core/src/config.rs` (new keys), `crates/factory-engine/src/review.rs` (model choice), `crates/factory-engine/src/specs.rs` (`with_limits` call for the reviewer), `README.md`, `.claude/software-factory.md`
- Test: `crates/factory-core/src/config.rs` (inline), `crates/factory-engine/tests/review_decide.rs`

**Interfaces:**
- Produces: `Config.reviewer_model_large: Option<String>` (default `None`, meaning off) and `Config.reviewer_large_over_lines: u32` (default 800, must be above 0). Invalid values fail closed like the other config keys.
- Produces: `pub(crate) fn reviewer_model_for(&self, changed_lines: u32, risky: bool) -> &str` on the engine: returns `reviewer_model_large` when it is set and (`risky` or `changed_lines > reviewer_large_over_lines`), otherwise `reviewer_model`. `review` calls it with the round diff's line count and the same risk classification `decide` uses on the changed paths, and writes a ledger line `review model: <name> (<reason>)` when the large model is chosen.
- Consumes: the round diff stat already computed in `review.rs`, the risk classifier in `factory-core` (`risk` module), `with_limits`.

- [ ] **Step 1: Write the failing tests.** `config.rs`: `the_large_reviewer_model_is_off_by_default`, `reviewer_large_over_lines_rejects_zero`, `a_config_can_set_both_keys`. `review_decide.rs`: `a_risky_diff_uses_the_large_model_when_set`, `a_diff_over_the_line_threshold_uses_the_large_model`, `a_small_safe_diff_uses_the_normal_model`, `with_no_large_model_set_nothing_changes` (the spec's `model` equals `reviewer_model` for every size and risk), `the_choice_is_written_to_the_ledger`. Run `cargo test -j4 -p factory-core config` and `cargo test -j4 -p factory-engine --test review_decide model` and confirm failure.
- [ ] **Step 2: Implement** the two keys, `reviewer_model_for` and the call in `review.rs`. Document both keys in the README and the design doc, and say that the weighted-token figure ignores the model, so a stronger model's extra cost does not show in `factory tokens`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-core -p factory-engine`. Expected: pass, with no change for a config that sets neither key.

### Task 2: Lower the default per-job budget

**Files:**
- Modify: `crates/factory-core/src/config.rs` (default for `budget_weighted`), `crates/factory-core/presets/{rust,go}/config.toml` if either sets it, `README.md`, `.claude/software-factory.md`
- Test: `crates/factory-core/src/config.rs` (inline), `crates/factory-core/tests/rocket_parity.rs`, `crates/factory-engine/tests/runloop.rs`

**Interfaces:**
- Produces: `Config::default().budget_weighted == 1_000_000` (named constant `DEFAULT_JOB_BUDGET_WEIGHTED`); a config file that sets `budget_weighted` keeps its value; docs state the default and the 80 percent warning.
- Consumes: the existing warn-at-80-percent and stop-at-100-percent logic in `budget.rs` and `claude.rs`, unchanged.

- [ ] **Step 1: Write the failing tests.** `config.rs`: `the_default_job_budget_is_one_million` and `an_explicit_budget_is_kept`. Check `rocket_parity.rs` and the Rocket fixtures: if the fixture does not set `budget_weighted`, the parity assertion must compare against the explicit old value, so add that value to the fixture or assert the new default. Run `cargo test -j4 -p factory-core config rocket_parity` and confirm the default test fails.
- [ ] **Step 2: Implement** the constant and default; update preset configs and docs that state 3M. Search the repo for the old figure and fix every mention.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-core -p factory-engine`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-06-review-skills-in-prompts.md
