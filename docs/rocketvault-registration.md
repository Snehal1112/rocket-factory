# Registering RocketVault in the factory home

The steps below repeat plan 10, Task 2. They touch `~/.factory`, never RocketVault's checkout, and start
no agent. The fixture `crates/factory-core/tests/fixtures/rocketvault-project.toml` is guarded by
`crates/factory-core/tests/rocketvault_registration.rs`.

```bash
VAULT=/home/numericlabs/data/rocket/rocketvault
FACTORY=/home/numericlabs/data/rocket/rocket-factory
cd "$FACTORY"
```

1. Register the project and replace the generated file with the fixture.

   ```bash
   target/debug/factory project add "$VAULT" --preset go --name rocketvault --base v-4.0.0
   cp crates/factory-core/tests/fixtures/rocketvault-project.toml ~/.factory/projects/rocketvault/project.toml
   ```

   Write `~/.factory/projects/rocketvault/commit-context.md` for RocketVault's commit style: short
   imperative subject, no type prefix, a body that says why, footer `Fixes #N` or `Relates to: #N`.

2. Check the settings. No problems, base `v-4.0.0`, `tickets on merge: true`, the go preset's agents.

   ```bash
   target/debug/factory project show rocketvault
   ```

3. Verify the check commands in a disposable clone, never in RocketVault. Time each one.

   ```bash
   git clone --local "$VAULT" ~/.factory/tmp/rocketvault-check
   cd ~/.factory/tmp/rocketvault-check
   go build ./... && go vet ./... && go test ./...
   bun install --cwd web --frozen-lockfile && bun --cwd web run typecheck && bun --cwd web run lint
   bun install --cwd journeybook --frozen-lockfile && bun --cwd journeybook run typecheck && bun --cwd journeybook run lint
   cd "$FACTORY" && rm -rf ~/.factory/tmp/rocketvault-check
   ```

4. Dry run with no agent.

   ```bash
   git -C "$VAULT" status --short                        # note it, 8 untracked plan docs
   target/debug/factory --project rocketvault new dryrun "Dry run"
   target/debug/factory --project rocketvault status
   target/debug/factory --project rocketvault run <id>   # parks: no plan.md in the job folder
   target/debug/factory --project rocketvault cancel <id>
   target/debug/factory --project rocketvault cleanup <id>
   git -C "$VAULT" status --short                        # unchanged
   git -C "$VAULT" branch --list 'factory/*'             # no new branch
   ```

## Decisions to confirm

- `go test ./...` runs without `-race` (CI uses `-race`, which is slower). Choose after the timing in step 3.
- No `golangci-lint` check: CI has it switched off and it mixes in linters (`gosec`) that fail on old code.
- Frontend checks run only when `web/**` or `journeybook/**` changed, and install first, because a fresh
  worktree has no `node_modules`.
- `budget_weighted` is left at the default (3000000), the same as the rocket project.
- Risk globs `internal/db/migration_runner.go` and `tls/**` were dropped: nothing in RocketVault matches them.
- Which ticket is the first real job.
