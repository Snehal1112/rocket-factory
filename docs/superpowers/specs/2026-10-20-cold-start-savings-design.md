# Cold-start savings design

## Goal

Cut the tokens the factory spends on cold-started agent runs by removing rules that agent files and prompts both carry, and measure what a cold start really costs with a new COLD STARTS section in factory tokens.

## Overview

Almost every agent run of a job starts cold. A typical job (3 tasks, 1 fix round, no sync) makes 9 cold starts and 1 resumed run: three task builders, the batch committer, two reviewer rounds, the fix committer, the merge message and the KB refresher. The usage logs of this machine (268 rows) show that first-turn cache writes are 52% of all cache creation, and 82% for committers. Only about 9k to 12k tokens of each 15k to 30k start context is ever read from cache.

Two causes are in the factory's hands:

1. The builder and reviewer prompts repeat rules that their agent files already carry. The agent file is the fixed system layer, so the copy in the prompt is paid again on every run, and again for every task of a plan.
2. `factory tokens --cache` reports resumed runs only. It cannot say how much of a fresh run's cache creation is the first turn, which is the number needed to judge any further cold-start work.

The user decided to ship the measurement first and to leave resuming the builder across tasks to a follow-up (see Follow-ups).

## Current state

| Area | Today | Where |
|---|---|---|
| Crash resume (`on_session`) | Done and merged in commit 941e44c. Not part of this job. | `crates/factory-engine/src/build.rs` `session_hook` |
| Builder prompts | `builder_first_spec`, `builder_task_spec` and `builder_resume_spec` each repeat the implementer rules, the `NOT_CONTROLLER` sentence, test-first, the no-commit and ledger ban, and the reply format | `crates/factory-engine/src/specs.rs:132-156`, `:205-251`, `:272-314` |
| Reviewer prompt | `reviewer_spec_with_model` repeats the paging rule, the check list, the vision-document rule, the read-only rule, the 60 line limit, the grading and the `VERDICT` line | `specs.rs:363-456` |
| Agent files | `factory-builder.md` and `factory-code-reviewer.md` carry the same rules (sections Work, Commits, Sandbox, Blockers, Reply; How to read, Check, Judging, Report) | `crates/factory-core/presets/{rust,go}/agents/`, `.claude/agents/` |
| Where the agent file comes from | The driver writes the resolved agent files into each worktree at `.claude/agents/<name>`. A file tracked on the base branch shadows the resolved one. | `build.rs:249-310` (`write_agent_files`) |
| Preset refresh | A manifest replaces unmodified seeded files on `factory upgrade`. A modified or shadowing copy keeps the old text. | `crates/factory-core/src/preset.rs:78-142`, README "Upgrading" |
| Usage line | `usage_line` writes `start_ctx` (first message creation plus read) and `usage_v` 2. It does not write the two parts. | `crates/factory-engine/src/ports.rs:246-280` |
| First message usage | `StreamState` keeps `first_usage`. `start_ctx()` sums its creation and read. | `crates/factory-agent/src/stream.rs:38,108,184` |
| Usage record | `UsageRecord` has `start_ctx`, `resumed`, `cache_creation_1h`, `secs`. | `crates/factory-cli/src/tokens.rs:20-40`, parsed at `:330-355` |
| Cache report | `cache_report` and `render_cache` give per-role totals and the resumed-run list. | `tokens.rs:129-320`, README "Cache report" |

Gaps: the prompt copies add roughly 1 KB per builder task and 1.2 KB per reviewer round; the driver cannot tell a cold start's cost from the logs.

## Non-goals

- Resuming the builder across tasks of a plan. It is a follow-up that needs the new data first.
- Any change to the committer, message or KB prompts and agent files.
- Reordering prompts so fixed text comes first. Removing the duplicates comes first.
- Changing `cache_ttl`, the budget formula or the per-role TTL (ticket #3).
- Stripping ambient context (user settings, hooks, plugins) from headless runs.

## Design

Flow: the engine builds a builder or reviewer spec, checks the worktree's agent file for the rules marker, and picks the slim or the full prompt; the agent run logs the first turn's cache numbers; `factory tokens --cache` reads them.

### 1. The rules marker

- Each shipped builder and reviewer agent file gets one line directly after the frontmatter: `<!-- factory-rules: 1 -->`. This covers `presets/rust/agents`, `presets/go/agents` and the repo's `.claude/agents` copies. No other text of those files changes, because the rules already live there.
- `pub const RULES_VERSION: u32 = 1;` in `specs.rs`, re-exported from `factory-engine`.
- `fn agent_rules_current(&self, cwd: &Path, agent: &str) -> bool` on `Engine`: reads `<cwd>/.claude/agents/<agent>.md`; true when a line equals `<!-- factory-rules: N -->` with `N >= RULES_VERSION`. A missing or unreadable file, a missing marker and an unparsable number give false.
- When it is false for a builder or reviewer run, the engine writes one ruling per job and agent, for example `agent file factory-builder.md has no current rules marker, prompts keep the full rules`, and uses the full prompt. It never fails the run.
- A project that edited its copy keeps working. `factory upgrade` replaces unmodified copies, so the marker reaches them on the next upgrade.

### 2. Slim prompts

Used only when `agent_rules_current` is true. The full prompts stay exactly as they are today.

- Builder (first run, task, fix round): drop the implementer-rules sentence with `NOT_CONTROLLER`, the test-first sentence, the no-`git add`/`git commit`/push/ledger sentence and the reply-format sentence. Keep the branch, the read line, `Never edit the plan.`, the spec line, the excerpt, base and tip, the earlier commits, `Do only Task N: ...`, the config rules line, and the KB line. The task prompt also keeps its `Never sleep or poll, never run a command in the background and wait for it ... ten minutes` line, because the agent file only says `Never sleep and never poll`. The first-run prompt has no such line today and gets none.
- Fix round builder: drop the `git add`/`git commit`/amend ban, the implementer-rules sentence and `Never push, never merge.` Keep the KB line, the findings path, the branch, the `receiving-code-review` instruction, the push-back and `NEEDS_CONTEXT` instructions and the check hints.
- Reviewer: drop the paging sentence, the `Check:` list, the vision-document sentence, the read-only sentence, the 60 line and grading sentence, the `Declined to judge` sentence and the `VERDICT` sentence. Keep the inputs line (diff, names, plan, worktree, round), the spec line, the earlier findings and builder reply lines, the scoped re-review block, the context block, the config rules line and the KB line.
- The interrupted prompt of a crash resume (`builder_task_resume_spec`) is unchanged. The fix round prompt is `builder_resume_spec`. Its check reads the worktree agent file when the fix round starts, which is the file the resumed session already loaded.
- The full prompts are not refactored into shared fragments in a way that could change bytes. Golden tests that pin today's full prompts are written first (Testing), and the slim variants are built beside them.

Example, builder task prompt, slim: `You are the builder in a software factory. Your working directory is the job worktree, already on branch factory/003-x.` then the read line, the excerpt, base and tip, `Do only Task 2: parse the header.`, the rules line, the sleep line and the KB line.

### 3. First-turn cache fields

- `factory-agent`: `StreamState::start_cache(&self) -> (u64, u64)` returns the first message's cache creation and cache read, `(0, 0)` when there is none. `AgentRun` gets `start_cc: u64` and `start_cr: u64`. `start_ctx` stays the sum.
- `factory-engine`: `usage_line` adds `start_cc` and `start_cr` and sets `usage_v` to 3. Readers that test `usage_v < 2` (`factory-core/src/cost.rs:103`) are unaffected.
- `factory-cli`: `UsageRecord` gets `start_cc: Option<u64>` and `start_cr: Option<u64>`, `None` on lines without the keys.

### 4. COLD STARTS section of `factory tokens --cache`

`CacheReport` gets two lists: per role and per job. A row has start fields when `start_cc` is `Some`. Only rows with start fields feed the first-turn columns. `--days N` limits both lists the same way it limits the rest.

```
COLD STARTS
ROLE       FRESH  RESUMED  AVG-START  FIRST-CC  FIRST-CR  FIRST-CC%
builder    122    4        29033      2720000   1050000   54.1%
...
start fields cover 96 of 168 fresh runs

JOB   COLD  RESUMED  FIRST-CC
013   9     1        200100
```

- FRESH and RESUMED count runs of the role, `resumed == false` and `true`. AVG-START is the mean `start_ctx` of fresh runs with start fields.
- FIRST-CC and FIRST-CR sum `start_cc` and `start_cr` over fresh runs with start fields. FIRST-CC% is FIRST-CC divided by the `cache_creation` of the same runs, `-` when that is 0.
- The note `start fields cover X of Y fresh runs` appears only when X is below Y, where Y counts the fresh runs of all roles in range and X those with start fields. When X is 0 the section says `no runs with start fields yet`.
- The job table lists every job with at least one counted run, ordered by job id.
- The section prints even when there are no resumed runs. The README "Cache report" paragraph is updated.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Agent file missing, no marker, older marker | Full prompt, one ruling per job and agent |
| Marker newer than `RULES_VERSION` | Counts as current |
| Marker line inside a code fence or with other text | Not a marker; only an exact line counts |
| Repo agent file shadows the resolved one | The worktree file is read, so the shadowing copy decides |
| Reviewer worktree has no `.claude/agents` file | Full prompt |
| Run killed before the first message | `start_cc` and `start_cr` are 0 and the row still counts as fresh |
| Old `usage.jsonl` lines | No start fields; they appear in the run counts and in the coverage note only |
| `--days` leaves no rows | `no runs` as today |
| Role with no fresh rows that have start fields | Row shows `-` in the first-turn columns |
| `usage.jsonl` line with a non-numeric `start_cc` | Treated as absent, never an error |

## Testing

- `factory-engine`, new file `crates/factory-engine/tests/slim_prompts.rs`, fixtures from `tests/common/mod.rs`:
  - `a_current_agent_file_gives_the_slim_builder_prompt`
  - `a_missing_marker_keeps_the_full_builder_prompt`
  - `an_older_marker_keeps_the_full_builder_prompt`
  - `a_newer_marker_counts_as_current`
  - `the_slim_fix_round_prompt_keeps_findings_hints_and_the_kb_line`
  - `the_slim_reviewer_prompt_keeps_inputs_and_the_rereview_block`
  - `the_slim_reviewer_prompt_drops_the_verdict_and_check_text`
  - `the_fallback_is_noted_once_per_job_and_agent`
  - `the_slim_builder_prompt_is_shorter_than_the_full_one`
  - `the_full_builder_prompts_match_golden_text` and `the_full_reviewer_prompt_matches_golden_text`, written first on unchanged code with a `GOLDEN` constant that starts empty, pin today's full prompts byte for byte. They run with no agent file in the worktree.
  - existing prompt tests such as `the_builder_prompts_tell_it_not_to_commit` stay unchanged and keep pinning the full prompt.
  - `the_fix_round_checks_the_agent_file_when_it_starts` (a current file gives the slim fix prompt; removing the file gives the full one).
- `factory-engine` unit tests in `ports.rs`: `usage_line_carries_start_cc_and_start_cr`; the two assertions on `usage_v` in that file and the one in `crates/factory-engine/tests/runloop.rs:233` become 3.
- `factory-agent`, `stream.rs`: `start_cache_is_the_first_message_creation_and_read`, `start_cache_is_zero_without_a_message`.
- `factory-cli`, `tokens.rs`: `load_usage_reads_the_start_fields`, `old_lines_have_no_start_fields`, `cold_report_counts_fresh_runs_only`, `cold_report_counts_cold_and_resumed_per_job`, `cold_report_respects_days`, `cold_section_prints_without_resumed_runs`, `cold_section_notes_runs_without_start_fields`.
- `factory-cli/tests/tokens_cache.rs`: `tokens_cache_prints_the_cold_starts_section`.
- `factory-cli/tests/agent_lint.rs`: `shipped_builder_and_reviewer_files_carry_the_rules_marker` (presets rust and go, `.claude/agents`), `the_marker_version_matches_the_engine_constant`.
- Commands: `cargo test -j4 -p factory-engine --test slim_prompts`, `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 --all-targets -- -D warnings`, `cargo fmt --check`.

## Files changed

- `crates/factory-engine/src/specs.rs`, `ports.rs`, `lib.rs`; `tests/slim_prompts.rs`, `tests/runloop.rs`, `tests/build_checks.rs` and `tests/common/mod.rs` (new fields in the `AgentRun` literals)
- `crates/factory-agent/src/stream.rs`, `run.rs`, `claude.rs`, and its tests where they build an `AgentRun`
- `crates/factory-cli/src/tokens.rs`; `tests/tokens_cache.rs`, `tests/agent_lint.rs`
- `crates/factory-core/presets/rust/agents/factory-builder.md`, `factory-code-reviewer.md`, and the same two files under `go`
- `.claude/agents/factory-builder.md`, `.claude/agents/factory-code-reviewer.md`
- `README.md` (Cache report)

## Constraints

- The board, ledger and events stay driver-only. Usage fields are added optionally; older readers ignore them.
- No `unwrap` in production code. Always `-j4`; never `--workspace`, `--all` or `CARGO_TARGET_DIR`. No git shell-outs.
- A missing or odd marker must fall back to the full prompt, never to a prompt without rules.
- Run `agent_lint` after editing the agent files.

## Acceptance criteria

- With a worktree agent file whose marker is current, the builder prompts (first, task, fix round) and the reviewer prompt omit the duplicated rules listed in Design 2 and keep everything else.
- With no marker or an older marker, the prompts are byte-for-byte what they are today, and one ruling per job and agent records the fallback.
- The shipped builder and reviewer files for rust, go and `.claude/agents` carry `<!-- factory-rules: 1 -->`, and the engine constant matches.
- `usage.jsonl` lines carry `start_cc`, `start_cr` and `usage_v` 3 for new runs.
- `factory tokens --cache` prints a COLD STARTS section with the role and job tables, the coverage note and the empty message, also when there are no resumed runs, and `--days` applies to it.
- Old `usage.jsonl` lines still load and show up without start fields.
- The tests named in Testing pass, and clippy and fmt are clean.

## Out of scope

- Resuming the builder across tasks (Follow-ups).
- Prompt reordering, committer, message and KB cleanup, TTL changes, ambient-context trimming.
- The dashboard. It does not read the usage fields.

## Follow-ups

- Resume the builder across tasks of one plan, behind a setting that is off by default. Revisit when the COLD STARTS section, on at least 20 builder runs with start fields, shows builder FIRST-CC% above 30% and most gaps between consecutive tasks of a plan under 5 minutes.
- Reorder the builder and reviewer prompts so fixed text comes first and volatile text last, and measure it with the same report.
- Strip ambient context (user settings, hooks, plugins, skills) from headless runs and measure the start context before and after.

## Decisions

- Scope of this job. Options: items 3, 5 and 6 only; items 3, 5, 6 plus a real-crash check for item 4; items 3, 5, 6 with item 5 as its own series. Chosen: items 3, 5 and 6 only. Decided by: user
- Which copy of the duplicated rules stays? Options: keep them in the agent files and cut them from the prompts; keep them in the prompts and cut them from the agent files; cut only text identical in both. Chosen: keep them in the agent files and cut them from the prompts. Decided by: user
- Should this job build resume of the builder across tasks? Options: a setting off by default with a context cap and gap check; always resume when on; on by default; skip it and let the cold-start report decide. Chosen: skip it, list it under Follow-ups with the condition to revisit. Decided by: user
- What should the cold-start report show, and where? Options: extend the cache report and log the first turn's creation and read as new fields; extend it from the fields already logged; a separate flag. Chosen: extend `factory tokens --cache` with a COLD STARTS section and log `start_cc` and `start_cr` (usage_v 3), older rows show n/a. Decided by: user
- Which roles does the duplicate-rule cleanup cover? Options: builder and reviewer only; all roles; builder only. Chosen: builder and reviewer only. Decided by: user
- What happens when a project's agent file is old? Options: version marker with fallback to the full old prompt; marker and refuse to start the job; no check. Chosen: version marker with fallback to the full old prompt and a warning. Decided by: user
- The marker is the line `<!-- factory-rules: N -->` directly after the frontmatter, checked in the worktree file. Decided by: call
- The fallback is noted as one ledger ruling per job and agent. Decided by: call
- The `usage_v` value becomes 3 and old lines show no start fields instead of being estimated. Decided by: call
- No `presets/legacy` files are added. A seeded home has a manifest, so `factory upgrade` refreshes an unedited agent file; a copy the user edited, or a repo file that shadows it, keeps its old text and gets the full prompt. Rocket's own agent copies live outside this repo and take the slim prompt after their next upgrade or edit. Decided by: call
- `start_cc` and `start_cr` come from the first-seen snapshot of the first message, the same source as `start_ctx`. Decided by: call
- The fallback ruling is deduplicated with the existing `progress.md` contains-check, run on every builder and reviewer spec. Decided by: call
