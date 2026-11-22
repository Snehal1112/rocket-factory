# Tidy docs and guards

## Goal

Close seven size S tickets, #43, #55, #95, #96, #118, #140 and #145. Make the board survive unknown review states, keep paths behind `Layout` with a lint that catches them, guard the settings reference with a test, and tidy the command text, tests and docs those tickets name.

## Overview

All seven tickets came from audits and job reviews. A code check on 2026-11-23 (HEAD 616ab25) found which items still hold. Items that are already fixed are left out:

- #55 item 4: the reviewer row no longer names Rocket rules.
- #145: the park-kind map covers every kind.
- #43 item 1: `HARNESS_WORDS` is gone from the reference doc. It survives only in old specs and plans, which stay as history.

What is left is a mix of one board-compatibility fix, one lint gap, one missing doc test, and small text and test tidies.

## Current state

| Ticket | Where | Today |
|---|---|---|
| #118 | `crates/factory-core/src/board.rs:10-20` | `ReviewState { Pending, Running, Pass, Changes, Skipped, Failed }`, `rename_all = "lowercase"`, no `#[serde(other)]`. One unknown value fails `Board::load` for the whole board. `missing_reviews` (`board.rs:442-451`) counts any state except pass, changes and skipped as missing. `has_no_verdict` (`crates/factory-engine/src/sync.rs:161-168`) lists pending, running and failed. `crates/factory-cli/src/status.rs:65-70` matches every variant. |
| #95 | `crates/factory-cli/tests/no_hardcoded_paths.rs:5-14` | Scans each crate's `src/` for the substrings `".superpowers`, `".claude` and `"main"` only, and skips six whole files. The `config.rs` and `risk.rs` entries say "plan 3 moves them into presets". |
| #95 | `crates/factory-core/src/resolve.rs:13-16` | `OVERRIDE_FILE = ".factory/override.toml"`, `AGENTS_DIR = ".factory/agents"`, `FACTORY_RISK = ".factory/**"`. |
| #95 | `crates/factory-core/src/preset.rs:451` | Error text `cannot read .factory/agents from the base branch`; `AGENTS_DIR` is used at `preset.rs:446`. |
| #95 | `crates/factory-core/src/home.rs:99` | `.join(".factory")`, the default home. |
| #95 | `crates/factory-core/src/project.rs:235`, `project.rs:416-422` | `for sub in ["agents", "state", "worktrees"]`; `build_layout` joins `"state"`, `"jobs"` and `"worktrees"`. |
| #95 | `crates/factory-cli/src/project.rs:422-437` | `dir.join("worktrees")`, `dir.join("state")` (twice), `.join("state").join("board.json")`. |
| #95 | `crates/factory-git/src/repo.rs:49` | `root.join(".claude").join("worktrees")`. `factory-git` does not depend on `factory-core`, so it cannot use `Layout`. |
| #95 | `crates/factory-core/src/layout.rs:134-136` | `board_file()` joins `"board.json"` inline. |
| #96 | `crates/factory-core/src/config.rs:117-119` | `#[serde(default, deny_unknown_fields)] pub struct Config`. The five `commit_*` keys are `skip_serializing`, `degraded` is `skip`. `settings_block.rs:327-330` `config_keys()` derives keys from `serde_json::to_value(Config::default())`. |
| #96 | `docs/reference/project-toml-reference.md` | `###` sections, each with a `\| Key \| Type \| Default \| Effect \|` table whose rows start with `` | `key` | ``. `cost_markers` has its own section. The `commit_*` keys sit under "Ignored settings". No test checks any of this. |
| #43 | `crates/factory-core/assets/global-command.md:76,93` | "commit with an exact pathspec" has no pointer to the `Commit` note at `global-command.md:134-136`. |
| #43 | `crates/factory-core/assets/tool-map-claude.md:29` | The Plan writing note replaces the shared executor line with the `using superpowers:subagent-driven-development` variant. |
| #43 | `crates/factory-core/assets/tool-map-codex.md:13,37` | `Use only the skill catalog the run preface allows.` and the filler `Skill invocation as $factory follows the shared text.` |
| #43 | `crates/factory-cli/tests/agent_lint.rs:375` | The kb refresher pin is `workflow scripts`; the preset text is `Never use workflow scripts.` |
| #55 | `crates/factory-cli/tests/agent_lint.rs:9`, `agent_lint.rs:1184-1204` | Its own `PRESETS` list repeats `factory_core::preset::PRESET_NAMES` (`preset.rs:12`). The forbid list `["shadcn", "lucide", "Zustand", "rocket-infra", "Rocket"]` is case-sensitive, and `tauri` is missing. |
| #55 | `docs/reference/software-factory.md:64`, `CLAUDE.md:57-59` | The agent-file layers omit README's rule that a tracked `.claude/agents/<name>.md` on the base branch still wins (`README.md:629-632`). CLAUDE.md says the `.claude/` files "are copies of Rocket's factory files. Tests read them." Only `factory/config.toml` is read by tests (`config.rs:1937-1960`); the folder also holds `agents/factory-spec-reviewer.md` and settings files. |
| #140 | `crates/factory-core/assets/global-command.md:45` | The retry bullet says "report a refusal as it is"; a factoryd refusal names shell forms such as `factory approve 007 --without-review`. Pinned by `crates/factory-cli/tests/global_command.rs:380-395`. |
| #145 | `crates/factory-engine/src/park.rs:5-9`, `checks_stage.rs:6-12`, `merge.rs:1-4` | Two `use factory_core` blocks in one order in park and checks_stage, another order in merge. |
| #145 | `crates/factory-engine/src/gate_record.rs:73-93,113-119` | Private `now_at()`. `now().max(0.0) as u64` is repeated in `park.rs` and `merge.rs`. `Engine::gate` loads the job per line. `record_gate` passes `job.round.max(1)`, and `GateLine::new` already applies `.max(1)`. |
| #145 | `crates/factory-engine/src/review.rs` (`risk_line`) | Tells "cleared" from "normal" by `note.is_some()`. |
| #145 | `dashboard/src/gates.rs:129`, `dashboard/README.md:39`, `docs/reference/software-factory.md:387,424` | The working stages list `"checks"`, which is no stage (`crates/factory-core/src/stage.rs:8-15`). A blank line splits the README file list. The gate summary text omits `cancelled` and the skipped count (`dashboard/static/board.js:779-796`). The `waiting` sentence does not say which stages are working. |

## Non-goals

- No `Unknown` fallback for `Stage` or other board enums.
- No change to risk patterns or protected paths, only where their text lives.
- No change to the old specs and plans that still name `HARNESS_WORDS`.

## Design

```
plan 1 code    : ReviewState::Unknown -> path constants + lint -> engine and dashboard tidy
plan 2 guards  : settings doc test -> agent_lint fixes -> docs fixes
plan 3 commands: global-command and tool-map text, including slash forms for retry
```

### 1. Unknown review states (#118)

Add `#[serde(other)] Unknown` as the last `ReviewState` variant. It serializes as `"unknown"`. `missing_reviews` already counts it as missing. `has_no_verdict` lists it with pending, running and failed. `status.rs` prints `unknown`. When an older binary saves a board it loaded with `Unknown`, it writes `"unknown"`. That loses the newer value, but the board stays readable.

### 2. Paths behind Layout (#95)

New constants in `crates/factory-core/src/layout.rs`:

```rust
pub const FACTORY_OVERRIDE_REL_FILE: &str = ".factory/override.toml";
pub const FACTORY_AGENTS_REL_DIR: &str = ".factory/agents";
pub const FACTORY_RISK_GLOB: &str = ".factory/**";
pub const PROJECT_AGENTS_DIR: &str = "agents";
pub const PROJECT_STATE_DIR: &str = "state";
pub const PROJECT_WORKTREES_DIR: &str = "worktrees";
pub const STATE_JOBS_DIR: &str = "jobs";
pub const BOARD_FILE: &str = "board.json";
```

- `resolve.rs` uses the three `.factory` constants and keeps its private names as aliases or drops them.
- `preset.rs:451` formats the error with `AGENTS_DIR`.
- `build_layout`, the creation loops at `project.rs:235` and `:501` (the test one may stay), `Layout::board_file` and `factory-cli/src/project.rs:422-437` use the project constants.
- The lint adds the needle `".factory`.
- `ALLOWED` gains `factory-core/src/home.rs` with the reason "the default home folder name". Its `config.rs` and `risk.rs` reasons drop "plan 3 moves them" and say they are risk policy. `repo.rs` keeps its entry with the reason "factory-git does not depend on factory-core; the in-repo worktree folder mirrors Layout::in_repo".
- Bare words such as `"state"` are not scanned.

### 3. Settings reference test (#96)

New `crates/factory-cli/tests/project_toml_reference.rs` with `every_config_key_is_documented`:

- Keys come from `serde_json::to_value(Config::default())`, which leaves out `skip_serializing` and `skip` fields.
- Rows are read only from tables whose header line is `| Key | Type | Default | Effect |`; the second table in `### Cost markers` (`name`, `job`) is not read. Cells are split on ` | `, so a `|` inside backticks in the Effect cell does not shift the Default cell (split at most into four cells from the left).
- A key is documented when such a row starts with `` | `<key>` | ``. `cost_markers` is the one exception: it is documented by the `### Cost markers` heading.
- The five ignored keys (`commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch`, `commit_plugins`) are a fixed list in the test, because serde skips them; each must appear in backticks in the text of the `### Ignored settings` section.
- Defaults: for a bool, number or string default, the Default cell is compared only when, after stripping backticks and quotes, it is one token; a cell with prose is skipped. A `None` default (JSON `null`) must have a cell that starts with `unset`.
- A failure names the key and, for a wrong default, the doc line number and both values.

The doc is fixed in the same plan: `risky_fix_rounds` becomes `unset (same as fix_rounds)` and `log_level` becomes `unset (logs at info)`, plus any other row the test flags.

### 4. agent_lint fixes (#55, #43 item 5)

- `PRESETS` is replaced by `factory_core::preset::PRESET_NAMES`.
- The forbid check lowercases the text and compares with `["shadcn", "lucide", "zustand", "rocket-infra", "tauri"]`.
- `Rocket` stays case-sensitive, because the lowercase word `rocket` may be fine in preset text.
- The kb refresher pin becomes `Never use workflow scripts`.

### 5. Docs fixes (#55, #145)

- `software-factory.md:64` gains README's sentence that a tracked `.claude/agents/<name>.md` on the base branch still wins.
- CLAUDE.md's "Carried over from Rocket" section says that only `.claude/factory/config.toml` is read by tests, and names the other files it holds.
- `software-factory.md:387` lists `cancelled` and the skipped count in the summary forms.
- The `waiting` sentence names the working stages `spec`, `build` and `review`.
- `dashboard/README.md:39`, the blank line, is removed.
- The `Gate record` section names the review detail words `did not finish` and `gave no verdict` (`review.rs:522,524`).

### 6. Engine and dashboard tidy (#145)

- One `use factory_core::{...}` block per file, with `gate_record::{GateKey, GateResult}` inside it, in `park.rs`, `checks_stage.rs` and `merge.rs`.
- `pub(crate) fn now_whole_secs() -> u64` next to `now()` in `ports.rs:20`, used by `gate_record.rs` and `park.rs:213`. `now_at` is removed. `merge.rs:87` works on a parameter and the `build.rs` copies stay, to keep the diff small.
- `record_gate` passes `job.round`, because `GateLine::new` already applies `.max(1)`.
- `Engine::gate_lines(&self, lines: &[(GateKey, GateResult, &str)])` returns at once when `gate_record` is off, else loads the job once. `gate_checks_run` and the park's gate calls use it, and `gate` calls it with one line. `gate_machine` keeps its own read of the record and calls `gate` as today.
- `risk_line(risky, paths, cleared_by_lock: Option<&str>)` is renamed and documented. It keeps the same output.
- `dashboard/src/gates.rs:129` drops `"checks"`.

### 7. Command text (#43, #140)

- `global-command.md:76` and `:93` end their commit sentence with `See the Commit note.` The `Interview` and `Plan writing` pointers move to the end of their bullet.
- `tool-map-claude.md` Plan writing drops the executor sentence, so Claude keeps the shared line.
- `tool-map-codex.md` replaces the filler with `Invoke this workflow as $factory; the shared text's /factory examples are written as $factory here.`, and the Skills note starts with `In factory runs,`.
- The retry bullet in `global-command.md:45` becomes: report a refusal, naming each `factory <sub> <id> ...` it lists as `/factory <sub> <id> ...`.
- The pins in `agent_lint.rs`, `global_command.rs` and `tool_map.rs` follow.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Board job with review `"reviewing"` | Loads as `Unknown`, counts as missing, `factory status` prints `unknown`. |
| Board with every known state | Round-trips unchanged. |
| A new `".factory/x"` literal in engine code | `no_hardcoded_paths` fails and names the file and line. |
| A new `Config` field with no doc row | `every_config_key_is_documented` fails and names the key. |
| Doc default `"claude-sonnet-5-5"` vs `claude-sonnet-5-5` | Equal after stripping quotes and backticks. |
| `Option` setting documented as `unset, which means ...` | Passes; a cell not starting with `unset` fails. |
| Default cell with prose (`19 built-in globs`) | Not compared. |
| Preset text with `Lucide` | `preset_agent_files_name_no_rocket_only_term` fails. |
| Refusal `use factory rework 7 <note>, factory cancel 7` | The command reports `/factory rework 7 <note>` and `/factory cancel 7`. |
| `risky` and a lock note both set | Still the `risky:` note line. |

## Testing

- factory-core: `cargo test -j4 -p factory-core board`. New tests: `an_unknown_review_state_loads_as_unknown`, `an_unknown_review_state_counts_as_missing`, `known_review_states_round_trip`.
- factory-engine: `cargo test -j4 -p factory-engine`. New tests: `has_no_verdict_counts_unknown` (sync.rs), `record_gate_keeps_round_zero_as_one`, `gate_lines_write_every_line_once` (gate_record.rs), `risk_line_cleared_by_lock_says_so`.
- factory-cli:
  - `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. New test: `path_hits_flag_a_factory_literal`, a unit check of `path_hits` on a `".factory/x"` line.
  - `cargo test -j4 -p factory-cli --test project_toml_reference`. New tests: `every_config_key_is_documented`, `a_wrong_default_names_the_line` (built on a string fixture).
  - `cargo test -j4 -p factory-cli --test agent_lint --test global_command --test tool_map --test skill_drift --test plugin_files`. New test: `retry_bullet_maps_refusals_to_slash_forms`.
- dashboard: `cargo test -j4 --manifest-path dashboard/Cargo.toml gates`. New test: `waiting_in_review_is_stale_and_checks_is_no_stage`.
- `cargo test -j4 -p factory-supervisor` and `-p factory-git` keep passing.

## Files changed

- `crates/factory-core/src/board.rs`, `layout.rs`, `resolve.rs`, `preset.rs`, `project.rs`
- `crates/factory-engine/src/sync.rs`, `gate_record.rs`, `ports.rs`, `park.rs`, `merge.rs`, `checks_stage.rs`, `review.rs`
- `crates/factory-cli/src/status.rs`, `project.rs`
- `crates/factory-cli/tests/no_hardcoded_paths.rs`, `agent_lint.rs`, `global_command.rs`, `tool_map.rs`, new `project_toml_reference.rs`
- `crates/factory-core/assets/global-command.md`, `tool-map-claude.md`, `tool-map-codex.md`, plus the copies `skill_drift.rs` and `plugin_files.rs` compare against
- `dashboard/src/gates.rs`, `dashboard/README.md`
- `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md` (only where the new test finds a gap), `CLAUDE.md`

## Constraints

- Board JSON stays readable by `dashboard/`, which reads review states as strings.
- No change to risk patterns, protected paths or any default value.
- No git shell-out. Paths go through `Layout` or the new constants.
- Engine tidy items keep their output; existing tests are the check.

## Acceptance criteria

- A board with an unknown review state loads, and the job counts as not reviewed.
- `no_hardcoded_paths` fails on a `".factory` literal outside the allowed files, and passes on main after the move.
- `resolve.rs`, `preset.rs`, both `project.rs` files and `Layout::board_file` use the new constants.
- A `Config` key with no doc row, or with a wrong scalar default, fails `every_config_key_is_documented`.
- agent_lint uses `PRESET_NAMES`, forbids `tauri` and catches `Lucide`, and pins `Never use workflow scripts`.
- The retry bullet tells the reader to show refusals with `/factory` forms.
- The Claude tool map no longer replaces the shared executor line.
- `software-factory.md`, CLAUDE.md and `dashboard/README.md` carry the fixes listed in Design 5.
- `gates.rs` has no `"checks"` stage, and `record_gate` no longer repeats `.max(1)`.
- Every listed test command passes.

## Out of scope

- Group C tickets #68, #75 and #76.
- `Stage` fallback values.

## Follow-ups

- Add a release-notes line for the next release: boards that hold the review state `failed` cannot be read by factory or factoryd older than this change (#118).

## Decisions

- #118: how are future ReviewState values handled? Options: an Unknown variant with serde(other), release notes only, Unknown for every board enum. Chosen: ReviewState::Unknown with #[serde(other)], plus a release-notes line. Decided by: user
- #140: should the command text map factoryd refusals to slash forms? Options: map in the command text, close as won't fix, map in factoryd. Chosen: map in the command text. Decided by: user
- #95: how far does the path fix go? Options: constants plus a `".factory` needle, also scan bare names, lint only. Chosen: constants plus a `".factory` needle. Decided by: user
- #96: what does the doc test check? Options: keys plus scalar defaults, keys only, keys plus all defaults. Chosen: keys plus scalar defaults. Decided by: user
- #96: how strict is the default check? Options: single-token cells only with an `unset` prefix for None, exact cells, keys only. Chosen: single-token cells, None starts with `unset`; reword risky_fix_rounds and log_level. Decided by: user
- The ignored `commit_*` keys are a fixed list in the test, and `cost_markers` is matched by its section heading. Decided by: call
- `now_whole_secs` replaces the copies in gate_record.rs and park.rs only. Decided by: call
- #43: which executor line do Claude plans carry? Options: the shared line, the Claude variant. Chosen: the shared line. Decided by: user
- How is Group B packaged? Options: three plans as one job, three plans as a series, code then text. Chosen: three plans as a series. Decided by: user
- `factory-git/src/repo.rs` stays allowed with a reason, because factory-git does not depend on factory-core. Decided by: call
- `Rocket` stays a case-sensitive forbid word; the others are matched lowercase. Decided by: call
- The `.claude/agents` sentence is copied from README into software-factory.md rather than linked. Decided by: call
- The release-notes line for #118 is filed as a follow-up, because releases go through `scripts/release.sh`. Decided by: call
