# Registering Rocket in the factory home

The steps below repeat plan 9, Task 2, on another machine. They touch `~/.factory` and
`~/.claude/commands/factory.md`, never Rocket's repo. Nothing here starts an agent.
`ROCKET` is the Rocket checkout, `FACTORY` is this repo.

```bash
ROCKET=/home/numericlabs/data/rocket/rocket
FACTORY=/home/numericlabs/data/rocket/rocket-factory
cd "$FACTORY"
```

1. Build the binaries.

   ```bash
   cargo build -j4 -p factory-cli -p factory-supervisor
   # target/debug/factory and target/debug/factoryd
   ```

2. Register the project. This seeds the presets and writes a generated `project.toml`.

   ```bash
   target/debug/factory project add "$ROCKET" --preset rust --name rocket --base main
   ```

3. Replace the generated file with the fixture. The parity test
   `crates/factory-core/tests/rocket_parity.rs` guards its content. If you change the repo path, edit the
   `repo` line.

   ```bash
   cp crates/factory-core/tests/fixtures/rocket-project.toml ~/.factory/projects/rocket/project.toml
   ```

4. Copy Rocket's agent files and committer context into the project folder.

   ```bash
   mkdir -p ~/.factory/projects/rocket/agents ~/.factory/projects/rocket/state
   cp "$ROCKET"/.claude/agents/factory-*.md ~/.factory/projects/rocket/agents/
   cp "$ROCKET"/.claude/factory/commit-context.md ~/.factory/projects/rocket/commit-context.md
   ```

5. Copy the past job state (jobs 001 to 015).

   ```bash
   cp "$ROCKET"/.superpowers/factory/{board.json,costs.json,events.jsonl} ~/.factory/projects/rocket/state/
   ```

6. Install the global `/factory` command. Add `--force` only when it reports a differing file and you
   want it replaced.

   ```bash
   target/debug/factory install
   ```

7. Check the settings. There must be no fail-closed warning, `tickets on merge: true`, the values of the
   fixture and the agent files from the project folder.

   ```bash
   target/debug/factory project show rocket
   ```

8. Dry run.

   ```bash
   git -C "$ROCKET" status --short                      # must be empty
   target/debug/factory --project rocket status          # jobs 001 to 015 with the old costs
   target/debug/factory --project rocket new dryrun "Dry run of the standalone factory"
   git -C "$ROCKET" status --short                      # still empty
   ```

   Start the supervisor in the background and check it, then call the API with the token (it listens on
   `127.0.0.1:8766` by default and wants `Authorization: Bearer <token>`).

   ```bash
   target/debug/factoryd run &
   target/debug/factoryd is-running                      # exits 0
   TOKEN=$(cat ~/.factory/api.token)
   curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8766/projects
   curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8766/usage
   cargo run -j4 --manifest-path dashboard/Cargo.toml -- --home ~/.factory --api 127.0.0.1:8766 --port 8765 &
   curl -s http://127.0.0.1:8765/api/projects             # the dashboard adds the token itself
   kill %2                                               # stop the dashboard
   target/debug/factoryd stop
   ```

9. Clean up the dry-run job and check Rocket is untouched.

   ```bash
   target/debug/factory --project rocket cancel <id>
   target/debug/factory --project rocket cleanup <id>
   git -C "$ROCKET" branch --list 'factory/*'            # no new branch
   gh auth status
   git -C "$ROCKET" remote get-url origin                # a GitHub URL, the ticket step needs it
   ```
