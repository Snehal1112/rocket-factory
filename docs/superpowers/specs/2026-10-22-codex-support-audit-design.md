# Codex support audit

## Intent

Rocket Factory already has broad Codex CLI support, but it has not been verified end to end. Audit the shipped support against `2026-10-21-codex-support-design.md`, including the current uncommitted Codex skill install and upgrade work, then close confirmed defects and coverage gaps so users can rely on Codex for a complete factory run.

## Goals

- Verify configuration, project setup, runner invocation and JSONL parsing, sandboxing, session resume and provider switching, usage and budget accounting, commit fallback, reports, dashboard, and install/upgrade flows.
- Run the relevant existing test suites and the real Codex smoke test when the local CLI/login is available; distinguish deterministic coverage from environment-dependent verification.
- Add focused regression coverage and fix defects found, preserving the existing uncommitted changes and default Claude behavior.
- Update documentation where observed behavior differs from documented Codex limits or installation behavior.

## Non-goals

- Add provider-per-role selection, Codex review subcommands, or new Codex capabilities.
- Redesign Codex prompts, model defaults, or the Codex support architecture absent a defect that blocks the stated behavior.
- Commit, publish, or deploy the work.

## Constraints

- Use the existing Codex design and acceptance criteria as the baseline; record any changed conclusion with evidence.
- Do not require Codex credentials for deterministic tests. The real smoke test remains optional if the CLI or a usable login is unavailable.
- Keep changes scoped to demonstrated gaps and do not discard or overwrite the user's current work.

## Approaches

1. Run only the listed tests and report results. This is quick, but it leaves uncovered integration gaps unresolved.
2. Audit each Codex path against the existing design, run deterministic suites plus the real smoke test when available, and fix confirmed defects with focused tests. **Selected:** the user chose this full audit and gap-closing scope because support is already implemented but not fully tested.

## Decisions

- Audit scope includes all existing Codex support and the current uncommitted install, upgrade, and token-report changes.
- Keep one provider per project and preserve Claude as the default.
- Treat unavailable external Codex access as a reported limitation, not a test failure; all behavior that can be tested locally should have deterministic coverage.

## Builder's discretion

- Choose exact test commands and order based on workspace structure and test runtime.
- Add only regression tests needed to establish a reproduced defect or an uncovered acceptance criterion.
- Make small documentation corrections that follow verified behavior.

## Audit results (2026-10-22)

- Codex runner and engine lifecycle: `cargo test -j4 -p factory-agent` passed; `cargo test -j4 -p factory-engine --test codex_prep` passed (14 tests after this audit). Regression coverage confirms nonzero exits after a completion fail with diagnostics, JSONL failures remain sticky when a later completion arrives, and provider switches clear saved task and legacy resume ids before spec construction.
- Core configuration and compatibility: `cargo test -j4 -p factory-core` passed (311 unit tests and all core integration tests). Cost summaries now retain provider/role joint totals; legacy usage still defaults to Claude. CLI tests cover provider-split role reports, install, upgrade, and old data.
- The full relevant package command `cargo test -j4 -p factory-agent -p factory-engine -p factory-cli -p factory-core` exited 101 in `factory-engine/tests/slim_prompts.rs`: three golden tests compare a canonical `/private/tmp` worktree path with an expected `/tmp` path on macOS. The Codex engine tests, engine cost tests, agent tests, core tests, and CLI tests passed before that unrelated failure.
- CLI install and upgrade now have direct Codex skill assertions. The upgrade helper uses the shared `claude_commands_dir` layout function rather than spelling `.claude` in CLI code; this keeps the helper compatible with the repository's path policy. Token report tests reflect the uncommitted column heading and totals layout.
- `cargo test -j4 -p factory-cli` passed across all targets after the above changes. Its screenshot helper needs local port access, so the successful full run was performed with the sandbox restriction lifted. The `factory costs` report now shows provider-specific role shares whenever history includes multiple providers.
- `cargo test -j4 --manifest-path dashboard/Cargo.toml` passed (146 tests) with local socket access. `node --test` in `dashboard` passed (140 tests).
- The ignored real smoke test passed on `codex-cli 0.163.0` with an authenticated local session: `cargo test -j4 -p factory-agent real_codex_smoke -- --ignored`. The sandbox blocked Codex's in-process app-server client, so the final successful run was performed outside the sandbox.

The final reviews found and this audit fixed four important gaps: nonzero child exits after completion, failure JSONL events being overwritten by later completion events, provider switching after stale session ids had already been copied into a builder spec, and loss of the joint provider/role distribution in cost reports. README upgrade guidance now documents the Codex skill backup. The audit also fixed the CLI path-policy regression and stale token report tests associated with the current uncommitted formatting changes, then added direct skill install and upgrade coverage.

The generated Codex `$factory` skill was updated to replace Claude-only task tracking, skill plugin, commit plugin, and background parameter instructions while preserving approvals and exact pathspec commits. A user-directed follow-up then added distinct handling for live run handles and immediate final results. Live handles are retained for harness notification without polling; completed or failed commands continue to the single status check and report the job's actual line, including the `needs-human` review exception. New generated-content assertions cover both branches. The follow-up re-review approved the fix with no new findings. `cargo test -j4 -p factory-cli --test global_command` passed (17 tests).
