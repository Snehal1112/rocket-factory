# Signed factory commits Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Sign every commit the factory driver makes, the way `git commit` would, so GitHub shows the driver's commits as verified.

**Commit:** feat(git): sign the driver's commits

**Architecture:** A new `sign.rs` in factory-git reads git's signing settings and runs gpg, gpgsm or ssh-keygen with no prompt and a 30 s cap. One `write_commit` helper makes every driver commit signed when `commit.gpgsign` is on and falls back to unsigned with a reason. factory-engine reports an unsigned commit to the ledger, stderr, a `commit.unsigned` event and, for the final merge, the `merge` gate detail. Git is the concrete `factory_git::Repo`, called directly, behind no port.

**Tech Stack:** Rust, git2 0.19 (`commit_create_buffer`, `commit_signed`, `extract_signature`, `reference_matching`), `std::process`.

> **Spec:** docs/superpowers/specs/2026-11-22-signed-commits-design.md

## Global Constraints

- Never shell out to `git`. The signing programs run through `std::process::Command`.
- No new crate dependency and no `Cargo.toml` change.
- No prompt: OpenPGP and x509 get `--pinentry-mode error`; SSH gets `SSH_ASKPASS_REQUIRE=never`; stdin is the buffer or null, never a terminal.
- `SIGN_TIMEOUT` is 30 seconds.
- A failed signature never fails or parks a job; the commit is made unsigned.
- Reason texts, one line, at most 200 characters: "could not run <program>: <io error>", "<program> exited with <code>: <last stderr line>" (skipping `[GNUPG:]` lines), "<program> made no signature", "signing timed out after 30 s", "no user.signingkey for ssh signing", "unknown gpg.format <value>".
- No file under `dashboard/` changes. Board JSON and the gate line format do not change; no field is added to the board, ledger or events schema.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## File Structure

- `crates/factory-git/src/sign.rs` (new): `SIGN_TIMEOUT`, `SignFormat`, `Signer`, `signer_for`, `Signer::sign`, temp-file helper, unit tests.
- `crates/factory-git/src/commits.rs`: `write_commit`, `Committed`, `commit_staged` returns `Committed`.
- `crates/factory-git/src/merge.rs`: `commit_merge` uses `write_commit`; `MergeOutcome::Merged { merge_sha, unsigned }`.
- `crates/factory-git/src/lib.rs`: `mod sign;`, `pub use commits::Committed`.
- `crates/factory-git/tests/common/mod.rs` and every other repo-creating test helper: `commit.gpgsign = false` in the test repo's config.
- `crates/factory-git/tests/commits.rs`, `merge.rs`, `worktree.rs`, `base_branch.rs`, `reset.rs`: new and updated tests.
- `crates/factory-engine/src/build.rs`, `merge.rs`, `sync.rs`: report an unsigned commit.
- `crates/factory-engine/tests/signed_commits.rs` (new) and `tests/gate_record.rs`: engine tests.
- `docs/reference/software-factory.md`, `docs/guides/getting-started.md`.

## Review Focus

1. The user's `~/.gitconfig` turns signing on, so a test helper that misses `commit.gpgsign = false` makes that test run the real gpg. Expected: no test depends on the machine's gpg. Pinned by `no_gpgsign_commits_as_before`, and by the whole suite passing on this machine.
2. A signed snapshot must still refuse a branch that moved, as `repo.commit(Some("HEAD"), ..)` does today. Pinned by `a_signed_snapshot_refuses_a_moved_branch`.
3. A gpg that exits 0 without `SIG_CREATED` must not produce a signed commit with a junk signature. Pinned by `no_sig_created_line_is_no_signature`.
4. The final-merge reports must be written after the merge lock is released, and a merge found already on main (`BaseState::Merged`) reports nothing. Pinned by `an_unsigned_final_merge_writes_the_reason_in_the_merge_gate`.
5. A temp file for SSH signing must be removed even when ssh-keygen fails. Pinned by `ssh_temp_files_are_removed_on_failure`.

---

### Task 1: The signer

**Files:**
- Create: `crates/factory-git/src/sign.rs`
- Modify: `crates/factory-git/src/lib.rs` (add `mod sign;`)
- Test: `crates/factory-git/src/sign.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `git2::Repository::config()`, `git2::Repository::signature()`.
- Produces, all `pub(crate)`:
  - `const SIGN_TIMEOUT: Duration = Duration::from_secs(30);`
  - `enum SignFormat { OpenPgp, X509, Ssh }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `struct Signer { format: SignFormat, program: String, key: String, timeout: Duration }`, with crate-visible fields so tests can build one.
  - `fn signer_for(repo: &Repository) -> Option<Result<Signer, String>>`
  - `fn Signer::sign(&self, buffer: &str) -> Result<String, String>`: the signature with one trailing newline removed.

Tests added: `signer_reads_openpgp_settings`, `signer_falls_back_to_gpg_program_then_gpg`, `signer_uses_the_committer_when_no_key_is_set`, `ssh_without_a_key_is_an_error`, `an_unknown_format_is_an_error`, `no_signer_without_gpgsign`, `a_hanging_program_times_out`, `no_sig_created_line_is_no_signature`, `the_reason_skips_gnupg_status_lines`, `ssh_temp_files_are_removed_on_failure`.

- [ ] **Step 1: Read** the spec sections Design 1 and 2 and the edge-case table, `crates/factory-git/src/merge.rs:15-30` (`signature`), `crates/factory-git/src/lib.rs`, and `crates/factory-git/tests/common/mod.rs` (`Fixture::on_branch`).

- [ ] **Step 2: Write the failing tests** in `sign.rs`. Each builds a repo with `git2::Repository::init` in a `tempfile::TempDir` (a dev-dependency already) and sets its own config with `repo.config().expect("config").set_str(..)` / `set_bool(..)`, always setting `commit.gpgsign` explicitly. A fake program is a `#!/bin/sh` script written into the temp dir and made executable with `std::os::unix::fs::PermissionsExt` (mode `0o755`). Full example:

```rust
#[test]
fn signer_reads_openpgp_settings() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("init");
    let mut cfg = repo.config().expect("config");
    cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
    cfg.set_str("gpg.openpgp.program", "/opt/gpg2").expect("program");
    cfg.set_str("gpg.program", "/usr/bin/other").expect("program");
    cfg.set_str("user.signingkey", "ABC").expect("key");
    let signer = signer_for(&repo).expect("signing on").expect("signer");
    assert_eq!(signer.format, SignFormat::OpenPgp);
    assert_eq!(signer.program, "/opt/gpg2");
    assert_eq!(signer.key, "ABC");
    assert_eq!(signer.timeout, SIGN_TIMEOUT);
}
```

The others share that shape:

| Test | Config or setup | Assertion |
|---|---|---|
| `signer_falls_back_to_gpg_program_then_gpg` | gpgsign true, key `ABC`, first `gpg.program = /usr/bin/g2`, then a second repo with no program set | program `/usr/bin/g2`, then `gpg` |
| `signer_uses_the_committer_when_no_key_is_set` | gpgsign true, `user.name = Ann`, `user.email = ann@x.io`, no key | key `Ann <ann@x.io>` |
| `ssh_without_a_key_is_an_error` | gpgsign true, `gpg.format = ssh`, no key | `Some(Err("no user.signingkey for ssh signing"))` |
| `an_unknown_format_is_an_error` | gpgsign true, `gpg.format = pgp2` | `Some(Err("unknown gpg.format pgp2"))` |
| `no_signer_without_gpgsign` | `commit.gpgsign = false` | `None` |
| `a_hanging_program_times_out` | `Signer { format: OpenPgp, program: <script "sleep 5">, key: "ABC", timeout: Duration::from_secs(1) }` | `sign("x")` is `Err("signing timed out after 1 s")`, returned in under 3 s |
| `no_sig_created_line_is_no_signature` | script that prints a signature to stdout and exits 0 with no status line | `Err` ending in `made no signature` |
| `the_reason_skips_gnupg_status_lines` | script: `echo "gpg: signing failed: No pinentry" >&2; echo "[GNUPG:] FAILURE sign 67108949" >&2; exit 2` | `Err` equal to `"<script path> exited with 2: gpg: signing failed: No pinentry"` |
| `ssh_temp_files_are_removed_on_failure` | `format: Ssh`, `key: "key::ssh-ed25519 AAAA x"`, a script that records all its arguments to a file (`echo "$@" > <dir>/args`) and exits 1 | `Err`; the file after `-f` and the last argument (the buffer file) no longer exist |

The timeout reason uses the signer's own timeout, so the 30 s default reads "signing timed out after 30 s".

- [ ] **Step 3: Run** `cargo test -j4 -p factory-git --lib sign`. Expected: does not compile, with `cannot find function 'signer_for' in this scope`.

- [ ] **Step 4: Implement** `sign.rs` from the Interfaces and the spec's Design 2:
  - `signer_for` reads `commit.gpgsign` with `get_bool` (missing is `None`), then the format, program and key by the spec's settings table.
  - `sign` spawns the program with piped stdout and stderr, then polls `try_wait` every 50 ms until `timeout`, killing the child on expiry. OpenPGP and x509 write the buffer to stdin; SSH uses null stdin.
  - SSH writes the buffer, and a literal key, to unique files in `std::env::temp_dir()` named `factory-sign-<pid>-<counter>-<nanos>`, opened with `create_new`. It removes them on every path.
  - Every reason is cut to 200 characters.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-git --lib sign`. Expected: 10 passed, 0 failed.

### Task 2: Signed commits in factory-git

**Files:**
- Modify: `crates/factory-git/src/commits.rs:59-70`, `crates/factory-git/src/merge.rs:44-101` and the `MergeOutcome` enum, `crates/factory-git/src/lib.rs`
- Modify: `crates/factory-engine/src/build.rs:548`, `crates/factory-engine/src/merge.rs:481`, `crates/factory-engine/src/sync.rs:220` (compile only, see Step 6)
- Modify: every test helper that creates a repository: `crates/factory-git/tests/common/mod.rs` (`Fixture::on_branch`), `crates/factory-engine/tests/common/mod.rs` (`init_repo`), `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/support/mod.rs`, `crates/factory-supervisor/src/api/testutil.rs`, the test modules in `crates/factory-engine/src/{park,drafts,runloop,cargo_risk}.rs`, and each test file that calls `Repository::init` itself (find them with `grep -rln "Repository::init" crates/*/tests crates/*/src`)
- Test: `crates/factory-git/tests/commits.rs`, `tests/merge.rs`, `tests/worktree.rs`; update `tests/base_branch.rs:60`, `tests/merge.rs:139`, `tests/worktree.rs:230`, `tests/reset.rs:91`, `tests/commits.rs:62` and `:85`

**Interfaces:**
- Consumes: `signer_for`, `Signer::sign` (from the signer task).
- Produces:
  - `pub struct Committed { pub sha: String, pub unsigned: Option<String> }` (`Debug, Clone, PartialEq, Eq`), exported from factory-git.
  - `pub fn Repo::commit_staged(&self, wt: &Worktree, message: &str) -> Result<Committed>`
  - `MergeOutcome::Merged { merge_sha: String, unsigned: Option<String> }`
  - `pub(crate) fn write_commit(repo: &Repository, update_ref: Option<&str>, sig: &Signature<'_>, message: &str, tree: &Tree<'_>, parents: &[&Commit<'_>]) -> Result<(Oid, Option<String>)>`

Tests added: `a_snapshot_is_signed_when_gpgsign_is_on`, `the_openpgp_program_gets_no_prompt_flags`, `a_failing_signer_commits_unsigned_with_a_reason`, `a_signed_snapshot_moves_the_branch`, `a_signed_snapshot_refuses_a_moved_branch`, `no_gpgsign_commits_as_before`, `ssh_signing_passes_the_key_file`, `ssh_literal_key_uses_the_agent`, `x509_uses_gpgsm_settings`, `a_signed_merge_into_main_has_a_gpgsig`, `an_unsigned_merge_reports_the_reason`, `a_signed_sync_merge_has_a_gpgsig`.

- [ ] **Step 1: Read** the spec's Design 3 and 4, `crates/factory-git/src/commits.rs`, `merge.rs`, `worktree.rs:220-255`, the factory-git tests listed above, and git2 0.19's `commit_create_buffer`, `commit_signed`, `extract_signature` and `reference_matching` in `~/.cargo/registry/src/*/git2-0.19*/src/repo.rs`.

- [ ] **Step 2: Isolate the tests.** In every repo-creating test helper listed under Files, set `commit.gpgsign = false` in the new repo's own config right after `init`, with a one-sentence comment: the user's global config may turn signing on.

- [ ] **Step 3: Write the failing tests.** Add a helper `fake_gpg(dir: &Path) -> PathBuf` in `crates/factory-git/tests/common/mod.rs` that writes this executable script and returns its path:

```sh
#!/bin/sh
echo "$@" > "$(dirname "$0")/args"
cat > /dev/null
echo "[GNUPG:] SIG_CREATED D 22 8 00 1 ABC" >&2
printf -- '-----BEGIN PGP SIGNATURE-----\nfake\n-----END PGP SIGNATURE-----\n'
```

Full example, in `tests/commits.rs`, building the worktree and staging a file as the existing `commit_staged` tests do:

```rust
#[test]
fn a_snapshot_is_signed_when_gpgsign_is_on() {
    let fx = Fixture::new();
    let mut cfg = fx.git().config().expect("config");
    cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
    cfg.set_str("gpg.program", fake_gpg(fx.dir.path()).to_str().expect("utf8")).expect("program");
    cfg.set_str("user.signingkey", "ABC").expect("key");
    // Create the job worktree and stage one file as `commit_staged_*` tests do.
    let done = repo.commit_staged(&wt, "wip: task 1").expect("commit");
    assert_eq!(done.unsigned, None);
    let oid = git2::Oid::from_str(&done.sha).expect("oid");
    let (sig, data) = fx.git().extract_signature(&oid, None).expect("signature");
    assert_eq!(sig.as_str().expect("utf8"), "-----BEGIN PGP SIGNATURE-----\nfake\n-----END PGP SIGNATURE-----");
    assert!(data.as_str().expect("utf8").contains("wip: task 1"));
}
```

The others, same shape:

| Test | Setup | Assertion |
|---|---|---|
| `the_openpgp_program_gets_no_prompt_flags` | as above | the `args` file reads `--status-fd=2 --pinentry-mode error -bsau ABC` |
| `a_failing_signer_commits_unsigned_with_a_reason` | `gpg.program` = script `exit 2` | `unsigned` is `Some` and contains `exited with 2`; `extract_signature` is an error; the branch tip is the new sha |
| `a_signed_snapshot_moves_the_branch` | as the full example | the job branch tip equals `done.sha` |
| `a_signed_snapshot_refuses_a_moved_branch` | a `#[cfg(test)]` unit test in `crates/factory-git/src/commits.rs`: signing on with the fake gpg, then call `write_commit(repo, Some("HEAD"), ..)` with a parent that is no longer the branch tip (move the branch to a second commit first) | the call returns an error, and the branch still points at the second commit |
| `no_gpgsign_commits_as_before` | `commit.gpgsign = false` | `unsigned` is `None`; `extract_signature` is an error |
| `ssh_signing_passes_the_key_file` | `gpg.format = ssh`, `user.signingkey = /k/id`, `gpg.ssh.program` = a script that records `$@` and then runs `for last; do :; done; printf 'SSHSIG\n' > "$last.sig"` | args start `-Y sign -n git -f /k/id` with no `-U`; signed |
| `ssh_literal_key_uses_the_agent` | as above with `user.signingkey = key::ssh-ed25519 AAAA x` | args contain `-U` before the buffer file |
| `x509_uses_gpgsm_settings` | `gpg.format = x509`, `gpg.x509.program` = fake gpg | the x509 script ran (its `args` file exists); signed |
| `a_signed_merge_into_main_has_a_gpgsig` (`tests/merge.rs`) | fake gpg on, then `merge_no_ff` as the existing merge test does | `Merged { unsigned: None, .. }`; the merge commit has a signature |
| `an_unsigned_merge_reports_the_reason` (`tests/merge.rs`) | `gpg.program` = script `exit 2` | `Merged { unsigned: Some(r), .. }` with `r` containing `exited with 2`; main moved |
| `a_signed_sync_merge_has_a_gpgsig` (`tests/worktree.rs`) | fake gpg on, then `merge_main_into` as the existing sync test does | `Merged { unsigned: None, .. }`; signed |

- [ ] **Step 4: Run** `cargo test -j4 -p factory-git`. Expected: does not compile, with `no field 'unsigned'` or `cannot find type 'Committed'`.

- [ ] **Step 5: Implement** `write_commit` and the API changes as the spec's Design 3 and 4 say:
  - When a signature is made and `update_ref` is `Some("HEAD")`, move the branch HEAD points to with `reference_matching(<branch>, new_id, true, <first parent id>, "commit: <subject>")`.
  - `commit_staged` and `commit_merge` call `write_commit`. `commit_merge` passes `None` as today and puts the reason into `MergeOutcome::Merged`.
  - Update the existing factory-git tests to the new shapes.

- [ ] **Step 6: Keep the engine compiling.**
  - `build.rs:548` takes `.sha` from the returned `Committed`.
  - `merge.rs:481` and `sync.rs:220` match `MergeOutcome::Merged { merge_sha, .. }`.
  - The reporting task replaces these with the reports.

- [ ] **Step 7: Run** `cargo test -j4 -p factory-git`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-supervisor`. Expected: all pass. The 12 new factory-git tests pass, and no test runs the machine's gpg.

### Task 3: Reporting an unsigned commit, and docs

**Files:**
- Modify: `crates/factory-engine/src/build.rs:540-560`, `crates/factory-engine/src/merge.rs` (`Step::Merged` at `:23`, `try_merge` around `:479`, `merge_stage` around `:249`, `record_merge_pass` at `:500`), `crates/factory-engine/src/sync.rs:205-225`
- Modify: `docs/reference/software-factory.md` (`## Commits`), `docs/guides/getting-started.md`
- Test: `crates/factory-engine/tests/signed_commits.rs` (new), `crates/factory-engine/tests/gate_record.rs`

**Interfaces:**
- Consumes: `Committed { sha, unsigned }` and `MergeOutcome::Merged { merge_sha, unsigned }` (from the factory-git task).
- Produces: `fn report_unsigned(driver: &Driver, id: &str, kind: &str, sha: &str, reason: &str)` in `crates/factory-engine/src/merge.rs`, `pub(crate)`. `Step::Merged(String, Option<String>)` is private. The public `MergeResult` keeps its shape.

Tests added: `an_unsigned_snapshot_writes_a_ledger_line`, `an_unsigned_sync_merge_writes_a_ledger_line`, `a_signed_snapshot_writes_no_unsigned_line`, `an_unsigned_final_merge_writes_the_reason_in_the_merge_gate`.

- [ ] **Step 1: Read** the spec's Design 5 and 6, `crates/factory-engine/src/merge.rs:15-30`, `:190-260`, `:370-520`, `sync.rs:190-230`, `build.rs:530-560`, `crates/factory-engine/tests/common/mod.rs` (`Fixture`, `init_repo`, `MergeFixture`), and `tests/gate_record.rs` (`recording_merge_fixture`, `an_auto_merge_writes_only_passes_notes_and_skips`, `merge_lines`).

- [ ] **Step 2: Write the failing tests.** In `tests/signed_commits.rs`, set signing on the fixture's main repo after setup:

```rust
fn failing_signer(main: &std::path::Path, dir: &std::path::Path) {
    let script = dir.join("gpg-fails");
    std::fs::write(&script, "#!/bin/sh\necho 'gpg: signing failed: No pinentry' >&2\nexit 2\n").expect("script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let repo = git2::Repository::open(main).expect("open");
    let mut cfg = repo.config().expect("config");
    cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
    cfg.set_str("gpg.program", script.to_str().expect("utf8")).expect("program");
    cfg.set_str("user.signingkey", "ABC").expect("key");
}

#[test]
fn an_unsigned_snapshot_writes_a_ledger_line() {
    // Build a Fixture and a scripted runner with one write effect, as gate_record's `a_build_writes_guard_builder_and_cap_passes` does.
    failing_signer(&fx.main, fx.dir.path());
    fx.engine(&runner).build_stage().expect("build");
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    let line = ledger.lines().find(|l| l.contains("is unsigned:")).expect("unsigned line");
    assert!(line.contains("gpg: signing failed: No pinentry"), "{line}");
}
```

| Test | Setup | Assertion |
|---|---|---|
| `an_unsigned_sync_merge_writes_a_ledger_line` | a job branch behind main, as the existing sync tests set it up, plus `failing_signer` | the ledger has one `is unsigned:` line naming the sync merge's short sha |
| `a_signed_snapshot_writes_no_unsigned_line` | a fake gpg that succeeds (the script from the factory-git task) | no ledger line contains `is unsigned:` |
| `an_unsigned_final_merge_writes_the_reason_in_the_merge_gate` (`tests/gate_record.rs`) | `recording_merge_fixture(common::approving_runner(3))`, `failing_signer` on its main repo, then the auto-merge steps of `an_auto_merge_writes_only_passes_notes_and_skips` | the `merge` line is `pass` and its detail starts with the short sha and contains `, unsigned: ` and `exited with 2` |

Fill the setup comments from the named tests; do not invent new fixture types.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test signed_commits --test gate_record`. Expected: the four new tests fail on their assertions, for example with `unsigned line` not found.

- [ ] **Step 4: Implement** the reporting:
  - `report_unsigned` writes the ledger line `commit <sha7> is unsigned: <reason>` through `driver.ledger(&job).ruling(..)` the way `write_step_ledger` does. It prints `warning: commit <sha7> is unsigned: <reason>` with `eprintln!` and logs `tracing::warn!(event = "commit.unsigned", kind, sha, reason)`.
  - Call it from `build.rs` (kind `snapshot`) and from `sync.rs` (kind `sync`).
  - `Step::Merged` carries the reason. `BaseState::Merged` paths pass `None`.
  - `merge_stage` calls `report_unsigned` (kind `merge`) after `try_merge` returns, and passes the reason to `record_merge_pass`. With a reason, the detail becomes `<sha7>, unsigned: <reason>`.

- [ ] **Step 5: Update the docs.**
  - Add a bullet to the `## Commits` section of `docs/reference/software-factory.md`. It covers signing when `commit.gpgsign` is on, the three formats and their programs, no prompt, the 30 s cap, and the unsigned fallback with its ledger, stderr, event and merge-gate reports.
  - Add a short note to `docs/guides/getting-started.md`: keep the GPG agent's passphrase cached, or use an SSH agent, so background runs can sign.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-git -p factory-engine --all-targets -- -D warnings` and `cargo fmt --check`. Expected: all pass, with the four new tests, no warnings and no formatting changes.

Next plan: none
