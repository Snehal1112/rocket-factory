# Run the code and security reviewers in parallel

Date: 2026-11-21. Item 6 of the 2026-11-19 efficiency adviser brief. A small change.

## Why

A review round that holds both roles runs the code reviewer, then the security reviewer. Running them at the same time shortens the round by the shorter of the two runs. The log of the rocket-factory project has 27 such rounds: the saving is about 12 s in the median, 85 s at most, about 6.5 min over all 88 jobs. The gain is small; the user chose to build it because the change is contained and the review result does not change.

## Goals

- In `review_stage_as` (`crates/factory-engine/src/review.rs`), the agent runs of all roles in `to_run` run at the same time.
- Everything the round produces stays the same as today: the verdicts, the review states on the board, the `review-{role}-*.raw.md` files, the merged `review-code-*.md`, `last_review`, the ruling lines and the decision.

## Non-goals

- No token saving, no change to prompts, models or which roles run.
- No async runtime. No parallel builders, checks or jobs.
- A sync review runs only the code reviewer, so it is unchanged.

## Anti-goals

- The order of the merged review file or the verdicts depends on which reviewer finishes first.
- A reviewer starts after the driver's lost flag is set.
- A log line, usage row or event loses its job or role, or carries the other reviewer's duration.

## Approaches

- **A. Thread only the agent runs (picked).** Prepare each run one at a time on the main thread, run the `runner.run` calls inside `std::thread::scope`, then handle the replies one at a time in `to_run` order. The engine is not `Sync` (it holds a `Cell` and a `git2` repo), and this way only the runner crosses threads.
- **B. Close the item without building it.** Fair, given the 6.5 min total; not picked.
- **Dropped: async.** Nothing in the driver is async; a big rewrite for seconds.

## Design

- **Split `Engine::run_agent`** (`ports.rs`) into three steps that the existing single call still chains: prepare (lost-flag check, `sync_provider`, model, `shape_spec`, `agents.json`, `agent.start` log), run (`self.runner.run(&spec)` and its elapsed time), finish (`agent.end` log, token add on the board, `log_usage`). The builder and every other caller keep calling `run_agent` unchanged.
- **The review loop** first prepares every role and sets each to `Running` on the board, then runs them in parallel, then finishes each and handles its reply in `to_run` order (code, then security), as the loop body does today. With one role in `to_run`, no thread is spawned.
- **A prepare error** (lost flag, provider sync) returns before any run starts, as today. A `shape_spec` failure gives that role a failed run, as today, and the other role still runs.
- **Thread safety:** `AgentRunner` gets `Send + Sync` as supertraits. `ClaudeRunner`, `CodexRunner` and `FakeRunner` already are (`OnceLock`, `AtomicU64`, `Mutex`). Test runners that are not get a `Mutex` or atomic in place of their `Cell`/`RefCell`.
- **Tracing:** each thread runs under the parent's dispatcher and enters the span that was current when it was spawned (both captured before the scope), so lines logged inside the runner keep the job. The dispatcher matters because `factory_telemetry::capture` installs its subscriber for the current thread only.
- **Durations:** each run measures its own elapsed time inside its thread and hands it to finish, so `agent.end` `duration_ms` and the usage row stay per role.
- **Cancel and shutdown:** each run is its own process group, already registered under the registry mutex (`factory-procgroup`), so a cancel or signal kills both.
- **Events:** `EventSink::emit` writes one line per `write_all` in append mode, so lines from two runs do not mix.

## Tests

- The existing `multi_review.rs` tests pass. Where a test scripts replies in call order with `FakeRunner`, it moves to a runner that answers by role (`RoleRunner` or `ByRole` in the test helpers), because the start order is no longer fixed.
- New: both reviewers overlap. A test runner blocks each run on a barrier for two parties; the round finishes only if both runs are in flight together.
- New: the merged review file lists code before security even when the security run finishes first.
- New: each role's `agent.end` carries its own duration and role.

## Left to the builder

- Names of the split functions and where the run step's result type lives.
- Which test runners need a `Mutex` to become `Sync`.
