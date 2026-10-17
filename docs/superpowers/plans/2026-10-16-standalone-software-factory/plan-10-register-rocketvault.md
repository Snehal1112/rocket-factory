# Plan 10: Register RocketVault (go preset, bun checks, base `v-4.0.0`, ignored `.claude/`)

Part of the standalone software factory series. Read `00-plan-index.md` first.

Plan 9 registered Rocket. This plan registers the second project, `~/data/rocket/rocketvault` (Go plus two bun frontends, GitHub `Snehal1112/rocketvault`, working and default branch `v-4.0.0`, no local `main`, `.claude/` ignored by git). It has 3 tasks. Tasks 1 and 2 are safe and spend no tokens. Task 3 runs a real job in RocketVault and starts only when the user says go and names the ticket.

## Survey facts (2026-10-17, read-only, re-check before use)

- Base branch `v-4.0.0` tracks `origin/v-4.0.0` and is GitHub's default branch. `origin/main` exists but is stale. The checkout has 8 untracked plan documents under `docs/superpowers/plans/` and no modified tracked file.
- Go module `rocketvault`, `go 1.25.0`. CI runs `go build -v ./...` and `go test -race -coverprofile=coverage.out ./...`. `build.sh --lint` runs `go vet ./...` and `staticcheck`. `.golangci.yml` exists and CI has the lint job switched off. There is no Makefile.
- Frontends: `web/` and `journeybook/`, package manager bun (`bun.lock`, bun 1.3.14 installed), no test runner. Scripts `typecheck` (`tsc -b --noEmit`) and `lint` (`biome lint --error-on-warnings`) exist in both. `format` mutates files and `build` rewrites `dist`, so neither is a check. `node_modules` exists in the main checkout but a fresh job worktree has none, so a frontend check must install first.
- `.claude/` is ignored (`.gitignore:127`). Factory agent files are written into job worktrees and git-excluded by the driver, so nothing from the factory is committed.
- Commit style: a short imperative subject with no type prefix, a body that says why, footers `Fixes #N` or `Relates to: #N`.
- `CLAUDE.md` and `go-reviewer.agent.md` are partly stale (the Go version, `internal/domain/`). Rules from `CLAUDE.md` that matter to a builder: new CLI commands call `vaultcli.RequireDataAction`; scopes come only from `model.NewVaultScope` or `model.NewAdminScope`; no HS256 JWT fallback; repositories do CRUD only; bun only.

## Rules for this plan

- RocketVault is read-only until Task 3. Tasks 1 and 2 only read it, plus one disposable clone (deleted afterwards). Never write in `~/data/rocket/rocketvault` before Task 3, and never push anything.
- `cargo` always with `-j2` on this machine, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Commits go through the `dev-workflow-skills:1-git-commit` skill. Use explicit paths.
- The shared command and presets stay generic: RocketVault specifics live in its `project.toml`, never in `crates/factory-core/presets/go/`.

### Task 1: A registration fixture and parity test for RocketVault

**Files:**
- Create: `crates/factory-core/tests/fixtures/rocketvault-project.toml`
- Create: `crates/factory-core/tests/rocketvault_registration.rs`
- Create: `docs/rocketvault-registration.md`

- [ ] **Step 1: Write the fixture.** A full `project.toml`: `repo = "/home/numericlabs/data/rocket/rocketvault"`, `preset = "go"`, and `[settings]` with:
  - `base_branch = "v-4.0.0"`, `tickets_on_merge = true`, the same numeric limits as the `rocket` project except `budget_weighted` (copy from `~/.factory/projects/rocket/project.toml` at write time).
  - `risk_paths`: the preset's list plus `internal/db/migrations/**`, `internal/db/migration_runner.go`, `internal/crypto/**`, `internal/signing/**`, `internal/services/auth/**`, `internal/services/authorization/**`, `internal/services/oauth2/**`, `internal/middleware/**`, `internal/keycache/**`, `model/scope.go`, `cmd/vaultcli/**`, `tls/**`, `Caddyfile`, `Caddyfile.local`, `docker-compose.yml`, `docker-entrypoint.sh`, `fly.toml`, `railway.json`, `release.sh`, `install.sh`, `build.sh`, `.golangci.yml`, `scripts/hooks/**`. Check each glob against `git ls-files` in RocketVault and drop the ones that match nothing.
  - `read_first = ["CLAUDE.md", ".claude/known-bugs.md", ".claude/multi-vault.md"]`.
  - `builder_rules` (one sentence each): new CLI commands call `vaultcli.RequireDataAction` or the package's own authorization helper; build scopes only with `model.NewVaultScope` or `model.NewAdminScope` and never use `NewOwnerScope`, `ownerScopeFromRequest`, `InVault`, `ReadByOwner`, `ListByUser(` or a `*Scoped` name in non-test code; never add an HS256 JWT fallback; repositories do CRUD only and services own encryption and business logic; no secrets in logs or errors; SQL is parameterized; new migrations go in `internal/db/migrations/` with a test; use bun and never npm, yarn or pnpm; trust the code and CI over `CLAUDE.md` where they differ.
  - `check_hints` and `reviewer_rules`: the Go and bun commands below, and the same scope and authorization rules for the reviewer.
  - `[[settings.checks]]`: `paths = ["**/*.go", "go.mod", "go.sum"]` with `go build ./...`, `go vet ./...`, `go test ./...`; `paths = ["web/**"]` with `bun install --cwd web --frozen-lockfile`, `bun run --cwd web typecheck`, `bun run --cwd web lint`; `paths = ["journeybook/**"]` with the same three for `journeybook`.
- [ ] **Step 2: Write the failing test.** `rocketvault_registration.rs` builds a temp `Home` with the real `go` preset, writes the fixture, resolves it and asserts: `base_branch` is `v-4.0.0`; the resolved config is not degraded; every glob of the list above is in `risk_paths`; the three check rules exist with exactly the commands above; no command contains `--workspace`, `-race` or `format`; `read_first` has the three files; the builder rules name `RequireDataAction`, `NewVaultScope` and bun. Mirror `crates/factory-core/tests/rocket_parity.rs` for the helpers. Run `cargo test -j2 -p factory-core --test rocketvault_registration` and see it fail for the right reason with a bare fixture.
- [ ] **Step 3: Make it pass.** Fix the fixture, not the test.
- [ ] **Step 4: Write `docs/rocketvault-registration.md`:** the commands of Task 2 in order, and the decisions the user must confirm (see the end of this plan).
- [ ] **Step 5: Check and commit.** `cargo test -j2 -p factory-core`, `cargo check -j2 -p factory-core`, and `cargo test -j2 -p factory-cli --test no_hardcoded_paths`. Commit: `test(core): check the RocketVault registration resolves`.

### Task 2: Register RocketVault in `~/.factory` and dry run with no agent

This task writes `~/.factory/projects/rocketvault/` and one disposable clone under `~/.factory/tmp/`. It starts no agent and spends no tokens.

- [ ] **Step 1: Register.** `target/debug/factory project add /home/numericlabs/data/rocket/rocketvault --preset go --name rocketvault --base v-4.0.0`. Replace `project.toml` with the fixture. Write `~/.factory/projects/rocketvault/commit-context.md` for RocketVault's commit style (short imperative subject, no type prefix, why-body, footer `Fixes #N` or `Relates to: #N`), using `~/data/rocket/Archive/rocket-tools-factory/factory` history or the `rocket` project's file as the template, minus the Conventional Commits table.
- [ ] **Step 2: Check the settings.** `factory project show rocketvault` shows no problems, base `v-4.0.0`, `tickets on merge: true`, and the agent files from the go preset. No `plan_rules` is set; if the user wants any, add them to the project file.
- [ ] **Step 3: Verify the check commands in a disposable clone, not in RocketVault.** `git clone --local /home/numericlabs/data/rocket/rocketvault ~/.factory/tmp/rocketvault-check`, then in it, one at a time and with the timings recorded: `go build ./...`, `go vet ./...`, `go test ./...` (note the runtime, any failure, and whether it needs a database or network), `bun install --cwd web --frozen-lockfile`, `bun run --cwd web typecheck`, `bun run --cwd web lint`, and the same three for `journeybook`. Any check that already fails on `v-4.0.0` must be reported and either fixed in the fixture (for example a package list) or flagged to the user, because a job would fail on it before it starts. Delete the clone when done.
- [ ] **Step 4: Dry run.** Check `git -C <rocketvault> status --short` before and after (it must be unchanged, 8 untracked plan docs). Create a plan-less job with `factory --project rocketvault new dryrun "Dry run"`, check `factory --project rocketvault status`, `factory --project rocketvault run <id>` parks it with `no plan.md in the job folder` and no tokens, then `cancel <id>` and `cleanup <id>`. `factoryd` shows project `rocketvault` in `GET /projects` and `branch --list 'factory/*'` in RocketVault has no new branch.
- [ ] **Step 5: Record** the timings, failures and the decisions in the plan ledger. Commit only changes in `rocket-factory` (docs). Nothing in RocketVault changes.

### Task 3: One real job in RocketVault (needs the user's go and a ticket)

Do not start this task on the strength of "go ahead with plan 10". Stop after Task 2 and ask.

- [ ] **Step 1: Choose the ticket.** Ask the user for a small RocketVault ticket (one package, no risk path, size S). Run `/factory <ticket link>` in a Claude session started in `~/data/rocket/rocketvault`, so the interview, the spec and the plans land in `rocketvault/docs/superpowers/` on `v-4.0.0` (the global command does this). The 8 untracked plan documents are not committed by it.
- [ ] **Step 2: Run.** With `factoryd` running the command queues the job. Expect about 160K to 300K weighted tokens. Check on merge: the job merged into `v-4.0.0` locally, `.claude/` untouched, no factory file in the diff, the issue commented and closed through `gh` (footer `Fixes #N`), `git status` of the main checkout unchanged apart from the merge.
- [ ] **Step 3: Index and memory.** Mark plan 10 done in `00-plan-index.md`, write the memory note, record tokens in `.superpowers/plan10-tokens.md`.

## Decisions the user must confirm

- `go test ./...` runs the whole repo with no `-race` (CI uses `-race`; it is slower and was left out by default). Choose after the timing in Task 2.
- No `golangci-lint` check, because CI has it switched off and it mixes in linters (`gosec`) that fail on old code.
- Frontend checks run only when `web/**` or `journeybook/**` changed, and install first, because a fresh worktree has no `node_modules`.
- Which ticket is the real job.

Next plan: none
