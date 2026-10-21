# Codex support, Plan 3: Provider in the reports, the dashboard and the docs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The provider of every run shows in `factory tokens <id>`, `factory tokens --cache` and `factory costs`, on the dashboard card, and in the docs that describe `agent_cli` and its Codex limits.

**Architecture:** `UsageRecord` reads the `provider` key that plan 2 writes (absent means `claude`). The cache report groups its role tables by provider and role and adds a `PROVIDER` column. `usage_summary` in `factory-core` also sums weighted tokens per provider, and `render_costs` prints a `PROVIDERS` line when more than one provider appears. The dashboard `Job` model reads `provider` and `board.js` shows a chip.

**Tech Stack:** Rust, vanilla JS, `node --test`.

**Spec:** docs/superpowers/specs/2026-10-21-codex-support-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Per crate: `cargo test -j4 -p <crate> <filter>`; dashboard: `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test`.
- A usage line without `provider` is `claude`. A line with a non-string `provider` is `claude` too.
- New fields in `costs.json` and in the dashboard JSON are optional (`#[serde(default)]`); the dashboard reads a missing `provider` as none and shows no chip.
- The `PROVIDER` column is always printed in the three tables that have a role column (role table, resumed list, cold-start role table), left of `ROLE`. The per-job cold-start table is unchanged.
- `factory costs` prints the `PROVIDERS` line only when two or more providers have weighted tokens; the percentages are of the weighted total, rounded like the `ROLES` line.
- Existing tests that pin the old table headers or the old run-row text are updated in the same task that changes the output.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences ending with a punctuation mark. All text put in HTML goes through `escapeHtml`.

## Review Focus

1. A project that used Claude for 10 jobs and then Codex for 1. Expected: the cache role table shows both providers with their own rows, never merged. Pinned by `tokens_cache_groups_by_provider_and_role` (Task 1).
2. Old `usage.jsonl` lines with no `provider`. Expected: they read as `claude` everywhere. Pinned by `old_lines_without_provider_read_as_claude` (Task 1) and `costs_with_only_claude_prints_no_providers_line` (Task 2).
3. A run row with a very long provider string. Expected: the table keeps its layout (the value is cut to 8 characters). Pinned by `long_provider_names_are_cut_to_eight_characters` (Task 1).
4. A job JSON with `provider: 5`. Expected: the dashboard drops the bad value and shows no chip. Pinned by `a_bad_provider_value_is_dropped` (Task 3).
5. A provider value with HTML in it. Expected: escaped in the chip. Pinned by `the_provider_chip_escapes_its_text` (Task 3).

---

## File Structure

- `crates/factory-cli/src/tokens.rs` (modify): `UsageRecord.provider`, `render_detail`, the three tables.
- `crates/factory-core/src/cost.rs` (modify): `UsageSummary.providers`, `JobCost.providers`, `GroupStats.providers`.
- `crates/factory-engine/src/costs.rs`, `crates/factory-cli/src/costs.rs` (modify): carry and print the providers.
- `dashboard/src/model.rs`, `dashboard/static/board.js`, `dashboard/static/dashboard.css` (modify): the chip.
- `dashboard/tests/provider.test.js` (create).
- `README.md`, `docs/project-toml-reference.md`, `docs/software-factory.md` (modify).

### Task 1: Provider in the token reports

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs` (`UsageRecord`, `load_usage`, `render_detail`, `RoleCache`, `ResumedRun`, `RoleCold`, `cache_report`, `render_cache`, `render_cold`)
- Test: `crates/factory-cli/src/tokens.rs` (unit tests), `crates/factory-cli/tests/tokens_cache.rs`

**Interfaces:**
- Produces: `UsageRecord.provider: String` (default `"claude"`); `RoleCache.provider`, `ResumedRun.provider`, `RoleCold.provider` (all `String`); role rows are keyed by `(provider, role)` and sorted by provider then role.

- [ ] **Step 1: Read** `tokens.rs` from `UsageRecord` through `render_cold`, and the tests that pin headers or run rows (`render_shows_roles_totals_and_baseline_percent`, the cache report tests, `tests/tokens_cache.rs`).
- [ ] **Step 2: Write the failing tests.** Full example:

```rust
#[test]
fn tokens_cache_groups_by_provider_and_role() {
    let mut codex = run("020", "builder", false, 1_000, Some(50), 100, Some(0), 900);
    codex.provider = "codex".into();
    let claude = run("020", "builder", false, 900, Some(50), 100, Some(0), 900);
    let r = cache_report(&[vec![claude, codex]], None, 2_000);
    let keys: Vec<(&str, &str)> = r.roles.iter().map(|x| (x.provider.as_str(), x.role.as_str())).collect();
    assert_eq!(keys, vec![("claude", "builder"), ("codex", "builder")]);
    let text = render_cache(&r);
    assert!(text.lines().any(|l| l.starts_with("PROVIDER")), "{text}");
}
```

  The others, each with its own inputs:

  | Test | Inputs | Assertion |
  |---|---|---|
  | `old_lines_without_provider_read_as_claude` | a usage line with no `provider`; one with `"provider": 5` | both `provider == "claude"` |
  | `load_usage_reads_the_provider` | a line with `"provider":"codex"` | `"codex"` |
  | `run_rows_show_the_provider` | `render_detail` of a codex and a claude record | each row holds its provider after the role |
  | `resumed_rows_and_cold_roles_carry_the_provider` | a resumed codex run | `ResumedRun.provider` and the `RoleCold.provider` are `codex`, and both tables print a `PROVIDER` column |
  | `long_provider_names_are_cut_to_eight_characters` | provider `averyveryverylongname` | the printed cell is `averyver` |

  In `tests/tokens_cache.rs`, extend `cache_report_reads_every_job_of_the_project` with a second line that has `"provider":"codex"` and assert both providers appear in the output.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli tokens`. Expected: compile failure, no field `provider` on `UsageRecord`.
- [ ] **Step 4: Implement** the field and its loading, the keyed grouping and sorting, and the columns (`{:<8}` for `PROVIDER`, cut to 8 characters). Update every existing test that pins an old header or run row.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli`. Expected: all pass.

### Task 2: Provider in `factory costs`

**Files:**
- Modify: `crates/factory-core/src/cost.rs:66-110,140-240,297-316`, `crates/factory-engine/src/costs.rs:13-45`, `crates/factory-cli/src/costs.rs:20-95`
- Test: `crates/factory-core/src/cost.rs`, `crates/factory-engine/tests/costs.rs`, `crates/factory-cli/src/costs.rs`

**Interfaces:**
- Produces:
  - `UsageSummary.providers: BTreeMap<String, u64>` (weighted tokens per provider).
  - `JobCost.providers: BTreeMap<String, u64>`.
  - `GroupStats.providers: BTreeMap<String, u32>` (percent of the group's weighted tokens, `#[serde(default)]` in `CostsFile`).
  - `render_costs` prints `PROVIDERS  claude 80%  codex 20%` below the `ROLES` line when the `all` group has two or more providers.

- [ ] **Step 1: Read** `usage_summary`, `JobCost`, the role percentage code near `role_totals`, `gather_costs`, `render_costs` and its tests.
- [ ] **Step 2: Write the failing tests.** Full example:

```rust
#[test]
fn usage_summary_sums_weighted_tokens_per_provider() {
    let text = concat!(
        "{\"job\":\"001\",\"role\":\"builder\",\"input\":700,\"output\":0,\"cache_creation\":0,\"cache_read\":0,\"usage_v\":2}\n",
        "{\"job\":\"001\",\"role\":\"builder\",\"provider\":\"codex\",\"input\":300,\"output\":0,\"cache_creation\":0,\"cache_read\":0,\"usage_v\":2}\n",
    );
    let s = usage_summary(text, "001");
    assert_eq!(s.providers.get("claude"), Some(&700));
    assert_eq!(s.providers.get("codex"), Some(&300));
}
```

  | Test | Inputs | Assertion |
  |---|---|---|
  | `group_stats_hold_provider_percentages` | two jobs, 700 claude and 300 codex weighted | `providers["claude"] == 70`, `["codex"] == 30` |
  | `an_old_costs_file_without_providers_still_loads` | a `costs.json` text with no `providers` | loads, empty map |
  | `costs_with_two_providers_prints_a_providers_line` | the stats above | output contains `PROVIDERS  claude 70%  codex 30%` |
  | `costs_with_only_claude_prints_no_providers_line` | only claude | no `PROVIDERS` in the output |
  | `gather_costs_carries_the_providers_of_each_job` | engine fixture with a codex line in `usage.jsonl` | `JobCost.providers` has `codex` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core cost`. Expected: compile failure, no field `providers` on `UsageSummary`.
- [ ] **Step 4: Implement** the fields (`provider` from the line, `claude` when absent or not a string), the percentage code next to the role percentages, `gather_costs` passing `usage.providers`, and the `PROVIDERS` line.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `-p factory-engine costs`, `-p factory-cli`. Expected: all pass.

### Task 3: Dashboard chip and docs

**Files:**
- Modify: `dashboard/src/model.rs:133`, `dashboard/static/board.js` (`renderCard` header), `dashboard/static/dashboard.css`, `dashboard/README.md`, `README.md`, `docs/project-toml-reference.md`, `docs/software-factory.md`
- Create: `dashboard/tests/provider.test.js`
- Test: `dashboard/src/model.rs` (unit tests), `dashboard/tests/provider.test.js`

**Interfaces:**
- Consumes: `Job.provider` on the board (plan 2).
- Produces: `Job.provider: Option<String>` in the dashboard model (lenient: a non-string value is dropped); `providerChip(provider)` in `board.js`, exported, returning `<span class="chip chip-provider">…</span>` (escaped) or an empty string for a missing or empty value.

- [ ] **Step 1: Read** the `Job` model with its `lenient` helpers, `seriesChip` and the `renderCard` header in `board.js`, `makeJob` in `dashboard/tests/board.test.js`.
- [ ] **Step 2: Write the failing tests.** `model.rs`: `a_job_provider_is_read_and_optional` (present, missing) and `a_bad_provider_value_is_dropped` (`"provider": 5` parses, `provider == None`). `provider.test.js` (copy `makeJob` and `makeState` from `board.test.js`):

```js
test("a card shows a provider chip only when provider is set", () => {
  const withChip = board.renderCard(makeJob({ provider: "codex" }));
  assert.match(withChip, /chip-provider[^>]*>codex</);
  assert.doesNotMatch(board.renderCard(makeJob({})), /chip-provider/);
});
```

  and `the_provider_chip_escapes_its_text` (`provider: "<img src=x>"` gives `&lt;img`, never `<img`), `an_empty_provider_shows_no_chip`.
- [ ] **Step 3: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml provider` and `node --test dashboard/tests/provider.test.js`. Expected: failures, no field `provider` and `board.providerChip is not a function`.
- [ ] **Step 4: Implement** the model field, `providerChip`, the chip placed after `seriesChip` in the card header, and a `.chip-provider` rule next to the other chips. Docs: `docs/project-toml-reference.md` gets an `agent_cli` row (`"claude"` or `"codex"`, default unset meaning claude, what Codex does, `project add --agent-cli codex`); `docs/software-factory.md` and `README.md` get a short section "Using Codex" with the limits from the spec (no `max_turns`, no mid-run budget stop, no `cache_ttl`, no cold-start data, `builder_permissions` and `builder_allow` ignored, carry off, the sandbox table, `--ignore-user-config`, the ignored smoke test command); `dashboard/README.md` lists `provider` on `jobs[]`.
- [ ] **Step 5: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test`. Expected: all pass. Also run `cargo test -j4 -p factory-cli --test agent_lint` only if an agent file changed (none should).

Next plan: none
