# Harness Hardening, Plan 01: Live Marker Probe

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory probe` builds each role's real agent spec, adds a unique marker, asks the agent (one turn, no tool use) what reaches its context, and reports a pass or fail per channel and role for `claude` or `codex`.

**Architecture:** Pure types, the prompt, the answer parser and `expected_channels` live in `factory-core::probe`. `factory-engine::probe` reuses the existing spec builders in `specs.rs`, `kb.rs`, `commit.rs` and `codex_prep.rs`, replaces only the prompt, puts the marker in the agent text and runs the spec through the runner. `factory-cli` adds the `probe` command, a table and `--json`. The live runs are maintainer steps, not tests.

**Tech Stack:** Rust, serde, clap, `fake-claude` for tests.

**Spec:** `docs/superpowers/specs/2026-10-30-harness-hardening-design.md` (GitHub issue #35, parent #40)

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p <crate> <filter>`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo`; never shell out to git. `gh`, `claude`, `codex` may be spawned.
- Paths reach the driver only through `Layout` helpers; `crates/factory-cli/tests/no_hardcoded_paths.rs` must stay green.
- Board JSON is untouched. The probe never writes the board, the ledger or the lease.
- Tests use `fake-claude` or an injected `AgentRunner`, never the real CLI. No test needs a login or network.
- The probe is never started by `run` or by `cargo test` against a real CLI.
- Comments are short full sentences ending with a period. Docs go in `docs/`.
- The driver makes the commits (with the `dev-workflow-skills:1-git-commit` skill); no task has a commit step.

## Review Focus

1. A role whose real spec has `disable_slash_commands` (reviewer, spec-reviewer on Claude) expects no skills, so a visible skill list fails `no_extra_skills` instead of passing silently (`reviewer_expects_no_skills_on_claude`, Task 1). A Claude builder on a machine with many user skills still passes (`builder_on_claude_expects_bootstrap_and_skills`, Task 1).
2. A model that echoes a guessed or stale marker does not pass (`wrong_marker_is_not_seen`, `prompt_does_not_leak_the_marker`, Task 1).
3. A failed or empty run reports every expected channel as missing with the error in `detail`, and exit 1, never a panic or a pass (`failed_run_marks_all_expected_channels_missing`, Task 2).
4. The probe never leaves tools usable: Claude gets no allowlist and `dontAsk`, Codex gets `read-only` with no network and no writable roots (`probe_spec_cannot_write`, Task 2).
5. `--role` and `--harness` with bad values fail at parse time with exit 2 (`probe_rejects_unknown_role`, Task 3).

## File Structure

- Create `crates/factory-core/src/probe.rs`: types, prompt, parser, `expected_channels`. Re-export from `lib.rs`.
- Modify `crates/factory-core/src/layout.rs`: `Layout::probes_dir()`.
- Create `crates/factory-engine/src/probe.rs`: spec building, marker delivery, `probe_role`, `run_probe`, report writing.
- Modify `crates/factory-engine/src/ports.rs`: split `run_agent`'s spec shaping into `shape_spec`. Modify `compat.rs` and `specs.rs`: share the spec reviewer builder.
- Modify `crates/factory-agent/src/bin/fake-claude.rs`: a `PROBE` mode.
- Create `crates/factory-cli/src/probe.rs`; modify `cli.rs`, `commands.rs`, `lib.rs`, `main.rs`.
- Tests: unit tests in the new files, `crates/factory-engine/tests/probe.rs`, `crates/factory-cli/tests/probe_cmd.rs`.
- Docs: `docs/reference/software-factory.md`, `docs/operations/releasing.md`.

---

### Task 1: Core types, prompt, parser, expected channels

**Files:**
- Create: `crates/factory-core/src/probe.rs`
- Modify: `crates/factory-core/src/lib.rs`, `crates/factory-core/src/layout.rs`, `crates/factory-core/src/config.rs`

**Interfaces:**
- Produces (all serde with lowercase names, `Debug, Clone, PartialEq`):
  - `enum Harness { Claude, Codex }` with `as_str()` and `from_config(&Config) -> Harness` (from `agent_cli()`). Core has no provider enum today (`AgentCli` lives in `factory-cli`), so this is the new one.
  - `enum ProbeRole { Builder, Reviewer, SpecReviewer, Committer, Kb }` with `ALL: [ProbeRole; 5]`, `as_str()` (`builder`, `reviewer`, `spec-reviewer`, `committer`, `kb`), `FromStr`, and `config_role() -> &'static str` (the string `Config::plugins_for` takes).
  - `struct Channel { name: String, expected: bool, present: bool, detail: String }`, `struct RoleProbe { role: ProbeRole, channels: Vec<Channel>, tools: Vec<String>, skills: Vec<String> }`, `struct ProbeReport { harness: Harness, cli_version: String, roles: Vec<RoleProbe>, transcript_dir: PathBuf }`. `RoleProbe::ok()` is true when every expected channel is present; `ProbeReport::ok()` when every role is ok.
  - `struct ProbeAnswer { marker_seen: bool, bootstrap: Option<String>, tools: Vec<String>, skills: Vec<String> }`.
  - `fn probe_prompt(marker: &str) -> String`: first line is exactly `PROBE`; asks for no tool use and four lines `MARKER:`, `BOOTSTRAP:`, `TOOLS:`, `SKILLS:` (`NONE` when empty). It names only the marker prefix (text up to the first `-`), never the full token.
  - `fn parse_probe_answer(text: &str, marker: &str) -> ProbeAnswer`: `marker_seen` only when the `MARKER:` value equals `marker` exactly; `NONE` gives `None` or an empty list; other prose lines are ignored.
  - `fn expected_channels(role: ProbeRole, harness: Harness, config: &Config) -> Vec<&'static str>`.
  - `impl Config { pub fn skill_allow_for(&self, role: &str) -> Vec<String> }` in `config.rs`, next to `plugins_for`: `plugins_for(role)` plus the skill the role's agent text names when the list lacks it (`dev-workflow-skills` for `committer` and `message`, `kb-refresh` for `kb`), each once. It is the one allowlist of the skills a role may see; plan 02 reuses it for Codex runs. `plugins_for` and `enabledPlugins` stay unchanged.
  - `fn allowed_skill_entries(role, harness, config) -> Vec<String>`: empty for `Reviewer` and `SpecReviewer` on Claude (their specs set `--disable-slash-commands`) and for `SpecReviewer` on Codex; otherwise `config.skill_allow_for(role.config_role())`.
  - `fn evaluate(role, harness, config, answer: &ProbeAnswer) -> RoleProbe`: builds the four channels with `expected` from `expected_channels` and `present` and `detail` from the answer. A skill name matches an entry when the part before `:`, or the whole name, equals the entry's part before `@`. `skills` is present when every entry has at least one listed skill; `no_extra_skills` when no listed skill (outside `KNOWN_BUILTIN_SKILLS`) matches no entry; `bootstrap` when the quote contains `You have superpowers`.
  - `const KNOWN_BUILTIN_SKILLS: &[&str] = &[]`, filled after the first live run (Maintainer steps).
  - `Layout::probes_dir(&self) -> PathBuf` = `state_dir.join("probes")`.
- `expected_channels` rules: always `agent_text`; add `skills` when `allowed_skill_entries` is non-empty; on Claude add `bootstrap` when an entry's name part is `superpowers`. `no_extra_skills` is expected on every Codex role (plan 02 adds the filter) and, on Claude, only for `Reviewer` and `SpecReviewer`: Claude has no per-run skill filter, so a builder that also sees the user's own skills is not a failure, only shown in `Skills seen`. Otherwise every release probe would fail on a machine with user skills. With the default config, Claude builder gives `agent_text, skills, bootstrap`; committer and kb give `agent_text, skills`; reviewer and spec-reviewer give `agent_text, no_extra_skills`. Codex never expects `bootstrap`.

- [ ] **Step 1: Write the failing tests** in `probe.rs`: `builder_on_claude_expects_bootstrap_and_skills` (and not `no_extra_skills`), `committer_on_claude_expects_skills_only`, `kb_expects_kb_refresh` (default config: `allowed_skill_entries` is `["kb-refresh"]`), `reviewer_expects_no_skills_on_claude` (also with `reviewer_plugins = ["superpowers"]`: still only `agent_text`, `no_extra_skills`), `codex_expects_no_extra_skills_for_every_role`, `codex_never_expects_bootstrap`, `prompt_starts_with_probe_and_asks_four_lines`, `prompt_does_not_leak_the_marker` (`!probe_prompt("FPROBE-abc123").contains("abc123")`), `parse_reads_four_lines`, `none_values_parse_as_empty`, `wrong_marker_is_not_seen`, `skills_with_group_match_the_entry` (`superpowers:brainstorming` matches `superpowers`; `imagegen` is extra), `evaluate_marks_missing_expected_channels`, `report_round_trips_json`, `role_parses_from_str_and_rejects_unknown`. In `layout.rs`: `probes_dir_is_under_the_state_dir`. In `config.rs`: `skill_allow_for_adds_the_named_skill_once` (kb gives `["kb-refresh"]`; committer with the default `commit_plugins = ["dev-workflow-skills"]` gives it once; builder gives `["superpowers"]`; `kb_plugins = ["kb-refresh@x"]` does not add a second entry).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-core probe`. Expected: FAIL, module missing.
- [ ] **Step 3: Implement** the items above. Keep `probe.rs` free of I/O.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-core`. Expected: PASS.

### Task 2: Shared spec shaping and the fake-claude PROBE mode

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`, `compat.rs`, `specs.rs`
- Modify: `crates/factory-agent/src/bin/fake-claude.rs`

**Interfaces:**
- Port: the spec builders and `shape_spec` sit in front of the `AgentRunner` port; the fake-claude mode is how tests exercise it. No new trait.
- Produces:
  - `impl Engine<'_> { pub(crate) fn shape_spec(&self, spec: &mut AgentSpec) -> Result<(), String> }`: the provider-specific part of `run_agent` (Codex: `prepare_for_codex`; Claude: cache TTL env, `spec.plugins = Some(config.plugins_for(&spec.role))`, `CLAUDE_CODE_SUBAGENT_MODEL`). `run_agent` calls it and turns `Err` into `AgentRun::failed`. Behavior unchanged.
  - `pub(crate) fn spec_reviewer_agent(repo: &Repo, config: &Config, cwd: &Path, spec: &Path, plans: &[PathBuf]) -> AgentSpec` in `specs.rs`: the body of `Driver::spec_reviewer_spec`, which now calls it. The probe needs it because the spec reviewer is built on `Driver`, not `Engine`.
  - fake-claude mode `PROBE` (first stdin line). It finds the line `Probe marker: <token>` in this order: the file `<cwd>/.claude/agents/<name>.md` when `--agent <name>` is in the args, the `--append-system-prompt` value, the rest of stdin. It answers, as the `result` text, `MARKER: <token or NONE>`, `BOOTSTRAP: <env FAKE_PROBE_BOOTSTRAP or NONE>`, `TOOLS: Read,Grep,Glob`, `SKILLS: <env FAKE_PROBE_SKILLS or NONE>`, one per line.

- [ ] **Step 1: Write the failing test** next to the existing fake-claude tests in `crates/factory-agent/tests` (follow their style): `fake_claude_probe_reads_the_marker_from_each_delivery_path` runs the binary for the agent file (temp cwd), `--append-system-prompt` and stdin, and asserts `MARKER: FPROBE-t1` each time, and `MARKER: NONE` when no marker is given. The existing engine tests are the regression check for the refactor.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-agent probe`. Expected: FAIL.
- [ ] **Step 3: Implement** the three items. The refactors move code only.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent` and `cargo test -j4 -p factory-engine`. Expected: PASS.

#### Then: Engine probe

**Files:**
- Create: `crates/factory-engine/src/probe.rs`; Modify: `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/tests/probe.rs`

**Interfaces:**
- Port: `AgentRunner` (`factory-agent/src/runner.rs`). The probe calls `self.runner.run` only; tests use `FakeRunner`. It never touches the board, so no other port is involved.
- Consumes: Task 1 types, `shape_spec`, `spec_reviewer_agent`, and the existing `builder_first_spec`, `reviewer_spec`, `committer_spec`, `kb_spec`, `Engine.agent_files`.
- Produces:
  - `pub enum Delivery { Agent, AppendSystemPrompt, Stdin }` (default `Agent`).
  - `impl Engine<'_> { pub fn probe_role(&self, role: ProbeRole, harness: Harness) -> Result<RoleProbe, EngineError>; pub fn probe_role_with(&self, role: ProbeRole, harness: Harness, delivery: Delivery, marker: &str, work_dir: &Path) -> Result<(RoleProbe, String), EngineError> }`. `probe_role` uses `Delivery::Agent`, a fresh marker `FPROBE-<12 hex>` and a scratch dir under `job_dir`. The second element is the raw answer text for the transcript.
  - `pub fn run_probe(engine: &Engine<'_>, harness: Harness, roles: &[ProbeRole], delivery: Delivery, cli_version: &str, probes_dir: &Path, stamp: &str) -> Result<ProbeReport, EngineError>`: writes `<probes_dir>/<stamp>-<harness>.json` (`stamp` is the UTC `yyyymmddThhmmssZ` the caller passes) and `<probes_dir>/<stamp>-<harness>/<role>.txt` (prompt, spec args, answer); sets `transcript_dir`.
- Design:
  - The real spec comes from the role's builder: `builder_first_spec` with a `JobView` for branch `probe`, `reviewer_spec` with empty diff and names paths and round 1, `spec_reviewer_agent`, `committer_spec`, `kb_spec` (kb dir from `config.kb_dir()`, else the scratch dir). Then `shape_spec`. Only `prompt` is replaced, by `probe_prompt(marker)`.
  - Marker delivery. Agent text is the matching `agent_files` entry (`factory-builder`, `factory-code-reviewer`, `factory-spec-reviewer`, `factory-committer`, `factory-kb-refresher`; a missing file is an `EngineError`) with `\n\nProbe marker: <marker>\n` appended, frontmatter kept. `Agent` (Claude default): write it to `<work_dir>/.claude/agents/<name>.md`, set `cwd = work_dir`, keep `spec.agent`. `AppendSystemPrompt`: `agent = None`, `agent_prompt = Some(text)`. `Stdin`: same spec as append; the CLI builds the runner with `Caps { agent: false, append_system_prompt: false, .. }` so the runner prepends it to stdin. Codex: `prepare_for_codex` has already put the preface and the body without frontmatter in `agent_prompt`; the marker is appended there and `Delivery` is ignored, since Codex carries agent text on stdin only.
  - Tools off. Claude: keep the role's `tools` and `disable_slash_commands` (so the tool and skill lists match the real run), clear `allowed_tools` and `disallowed_tools`, `permission_mode = Some("dontAsk")` (every tool call is denied), `max_turns = Some(1)`, `resume = None`, `budget_weighted = Some(50_000)`. Removing the tool list with `--tools ""` is not used, because it may also hide the skill list. Codex has no tool switch: `sandbox = Some("read-only")`, `network = false`, `writable_roots` cleared, `cwd = work_dir`. The prompt also says to call no tool.
  - A run with `stop_reason != Done` or empty text gives a `RoleProbe` with all expected channels `present: false` and `detail` set to the error or `empty answer`.
  - The probe calls `self.runner.run` directly; it never calls `job()` or `run_agent`, so no board is needed.

- [ ] **Step 1: Write the failing tests** in `tests/probe.rs` with `FakeRunner`: `probe_spec_cannot_write` (all five roles, both harnesses: Claude spec has empty `allowed_tools`, `permission_mode == Some("dontAsk")`, `max_turns == Some(1)`; Codex spec `sandbox == Some("read-only")`, `!network`, no `writable_roots`), `prompt_is_replaced_and_marker_is_in_agent_text` (the agent file under `work_dir/.claude/agents` holds the marker and the original body; `spec.prompt` starts with `PROBE`), `append_delivery_puts_the_marker_in_agent_prompt`, `codex_marker_rides_agent_prompt_after_the_preface`, `reviewer_spec_keeps_disable_slash_commands` (and builder does not), `all_channels_present_when_the_answer_echoes_everything` (answer with marker, `You have superpowers.`, `superpowers:brainstorming`), `failed_run_marks_all_expected_channels_missing`, `missing_agent_file_is_an_error`, `run_probe_writes_report_and_transcripts` (stamp `20261030T120000Z`; the report parses back equal).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test probe`. Expected: FAIL.
- [ ] **Step 3: Implement** as designed.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine`. Expected: PASS.

### Task 3: CLI command, table and JSON

**Files:**
- Create: `crates/factory-cli/src/probe.rs`; Modify: `cli.rs`, `commands.rs`, `lib.rs`, `main.rs`
- Test: `crates/factory-cli/tests/probe_cmd.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `Command::Probe { harness: Option<Harness>, role: Vec<ProbeRole>, json: bool, delivery: Delivery }`, giving `factory [--project <name>] probe [--harness claude|codex] [--role <role>]... [--json] [--delivery agent|append|stdin]`. `--delivery` is an addition to the fixed interface (default `agent`), needed to cover all three Claude paths. Default harness is `Harness::from_config`; default roles are `ProbeRole::ALL`. Parsing uses clap value parsers, not strings.
  - `pub fn render_probe(report: &ProbeReport) -> String`: a header `harness <h>  cli <version>`, then one row per role and one column per channel (`ok`, `MISSING`, `-` when not expected and absent, `extra` when present but not expected), a `Skills seen:` line per role (so the `--disable-slash-commands` answer is readable), and the report path. `--json` prints `serde_json::to_string_pretty(&report)` and nothing else.
  - A failing report returns `CliError::ProbeFailed(String)` with the rendered text; `main.rs` prints it to stdout and exits 1. Exit 0 otherwise.
  - Without `ctx.runner`, the CLI builds `ClaudeRunner::new("claude", caps, sink)` (with `api_key_source`) or `CodexRunner::new("codex", sink)`; the sink writes into the transcript folder, not `events.jsonl`. `cli_version` is the first line of `<program> --version`, or `unknown`.

- [ ] **Step 1: Write the failing tests.** In `cli.rs`: `probe_parses_flags` (`--harness codex --role builder --role kb --json`), `probe_rejects_unknown_role` (`--role nope` and `--harness gpt` give `ErrorKind::InvalidValue`). In `probe_cmd.rs`, with `CliCtx::with_layout` and a scripted runner (see `tests/common/mod.rs`): `probe_prints_a_table_and_exits_zero`, `probe_reports_missing_channel_and_fails` (answer `MARKER: NONE` gives `Err(CliError::ProbeFailed(..))` whose text has `MISSING`), `probe_json_is_one_parseable_report` (the whole output parses as `ProbeReport`), `probe_role_filter_limits_rows`, `probe_saves_the_report_under_probes_dir`, and `probe_through_fake_claude_finds_the_marker` (a real `ClaudeRunner` on the `fake-claude` binary; delivery `agent`, `append` and `stdin` each give `agent_text` present).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli probe`. Expected: FAIL.
- [ ] **Step 3: Implement** the command, `render_probe` and the error mapping. Take the UTC stamp from the clock helper in `factory-engine/src/clock.rs`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli probe` and `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Expected: PASS.

#### Then: Docs

**Files:**
- Modify: `docs/reference/software-factory.md`, `docs/operations/releasing.md`

- [ ] **Step 1:** Add a short `## Checking what agents see` section to `software-factory.md` after `## Using Codex`: what `factory probe` does, the four channels and what each proves, the command line, where reports live (`<state dir>/probes/`), the cost note (one short turn per role, real credentials, never run by `cargo test`), and a `Findings` line saying the `--disable-slash-commands` and bootstrap answers are recorded after the first live run (maintainer step 2).
- [ ] **Step 2:** In `releasing.md`, under `## Cutting a release`, add a step before tagging: run `factory probe --harness claude` and `factory probe --harness codex`; a failing channel blocks the release unless the notes say why; paste the CLI versions and tables into the release notes.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test skill_drift --test global_command`. Expected: PASS.

## Maintainer steps after merge

These need the real CLIs and cost tokens. They are not tasks and no test depends on them.

1. Run `factory probe --harness claude` in a project with the default plugin lists. Record in `software-factory.md` under `Findings`: the CLI version, whether the builder row shows `bootstrap` (does `--settings` enabledPlugins fire the SessionStart hook in `-p` mode), and the skills seen.
2. Read the reviewer and spec-reviewer rows. If `Skills seen` is not `NONE`, `--disable-slash-commands` does not block skills; write that down and open a follow-up. If it is `NONE`, write that down. This answers acceptance criterion 3 of issue #35.
3. Run the builder role with `--delivery agent`, `append` and `stdin`. Record which paths show `agent_text`.
4. If Claude built-in skills show under `no_extra_skills`, add their names to `KNOWN_BUILTIN_SKILLS` in `crates/factory-core/src/probe.rs` in a small follow-up.
5. Run `factory probe --harness codex`. Expect `no_extra_skills` to fail until plan 02 lands (Codex sees every user skill); record the skills seen as the baseline.
6. Put the tables in the next release notes and comment the results on #35.

Next plan: docs/superpowers/plans/2026-10-30-harness-hardening/plan-02-codex-skills.md
