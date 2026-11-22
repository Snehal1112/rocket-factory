# Signed factory commits

## Goal

Sign every commit the factory driver makes, the way `git commit` would, so GitHub shows the driver's commits as verified.

## Overview

On rocket-factory's main branch, every commit made with the git command line shows Verified on GitHub, and every commit the driver makes shows `unsigned`. Of the last 30 commits on `main`, the 19 unsigned ones are all driver commits: the `wip: task N` snapshots and the job's final merge, for example `ffb9880`, `e52ba9e` and `3939891`. The user's global git config (`~/.gitconfig`) has `commit.gpgsign = true` and `user.signingkey = 61D246B30285ED35` (an ed25519 OpenPGP key), so `git commit` signs. The driver makes its commits through git2, which never reads those settings and never signs.

## Current state

| Commit kind | Where | How |
|---|---|---|
| Snapshot (`wip: task N`, `wip: fix round R`, `wip: build the plan`) | `Repo::commit_staged` (`crates/factory-git/src/commits.rs:59-70`), called from `build.rs:548` | `inside.commit(Some("HEAD"), ..)` |
| Sync merge of main into the job branch | `Repo::merge_main_into` (`crates/factory-git/src/worktree.rs:225-253`), called from `sync.rs:208` | `commit_merge` |
| Final no-ff merge into main | `Repo::merge_no_ff` (`crates/factory-git/src/merge.rs:109-132`), called from `merge.rs:479` | `commit_merge` |

- `commit_merge` (`crates/factory-git/src/merge.rs:44-101`) creates the commit with `repo.commit(None, ..)`, checks its parents, checks out the tree and only then moves the branch ref.
- `signature` (`merge.rs:22-27`) is the committer identity: the repo's `user.name` and `user.email`, else `factory <factory@localhost>`.
- `MergeOutcome::Merged { merge_sha }` is matched in `crates/factory-engine/src/merge.rs:481`, `sync.rs:220` and the factory-git tests (`tests/merge.rs:139`, `tests/base_branch.rs:60`, `tests/worktree.rs:230`, `tests/commits.rs:98`). `commit_staged` is also called in `tests/commits.rs:62` and `:85` and `tests/reset.rs:91`.
- In factory-engine, the private `Step::Merged(String)` (`merge.rs:23`) carries the merge sha from `try_merge` to `merge_stage`, which calls `record_merge_pass`. `Step::Merged` is also built from `BaseState::Merged` (`merge.rs:386`, `:461`) for a merge that already exists.
- factory-git depends only on `git2` and `thiserror`. The project rule forbids shelling out to `git`; running another program is allowed.
- The `merge` gate's pass line is written by `record_merge_pass` (`crates/factory-engine/src/merge.rs:500`) with the short merge sha as its detail.
- A step after the merge reports a failure to the ledger through `write_step_ledger` (`merge.rs:232-240`), which calls `Ledger::ruling`.

Gaps: no driver commit is signed, and nothing tells the user when a commit could not be signed.

## Non-goals

- Re-signing commits that are already on `main` or on GitHub.
- `gpg.ssh.defaultKeyCommand`. With SSH signing and no `user.signingkey`, the commit is made unsigned with a warning.
- Signing tags, or verifying signatures.
- Any change to the dashboard UI. The only visible change is the `merge` gate's detail text.
- Parking a job because signing failed.

## Design

```
make a commit (snapshot, sync merge or final merge)
  commit.gpgsign is not true      -> plain commit, as today
  signer cannot be set up         -> plain commit, unsigned reason
  build the commit buffer, run the signing program (no prompt, 30 s cap)
    program fails or times out    -> plain commit, unsigned reason
    signature                     -> signed commit with a gpgsig header
caller gets the sha and an optional unsigned reason
  reason present -> ledger line, stderr warning, commit.unsigned event
                    (final merge also: merge gate detail "<sha7>, unsigned: <reason>")
```

### 1. When to sign

The driver signs when the git config value `commit.gpgsign` is true, read with git2 from the repository the commit is made in. git2 merges the system, global and repository config, as git does, so the user's `~/.gitconfig` counts. A job worktree reads the same config as the main checkout. A missing or false value means no signing, and the commit is made exactly as today.

### 2. The signer (`crates/factory-git/src/sign.rs`, new)

```rust
pub(crate) const SIGN_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum SignFormat { OpenPgp, X509, Ssh }

pub(crate) struct Signer { format: SignFormat, program: String, key: String, timeout: Duration }

/// None when commit.gpgsign is not true; Err with a reason when signing is on but cannot be set up.
pub(crate) fn signer_for(repo: &Repository) -> Option<Result<Signer, String>>;

impl Signer {
    /// The armored signature for `buffer`, or a one-line reason.
    pub(crate) fn sign(&self, buffer: &str) -> Result<String, String>;
}
```

The settings, read the way git reads them:

| Setting | Values |
|---|---|
| Format | `gpg.format`: `openpgp` (also when unset), `x509`, `ssh`. Any other value is an error: "unknown gpg.format <value>". |
| OpenPGP program | `gpg.openpgp.program`, else `gpg.program`, else `gpg` |
| x509 program | `gpg.x509.program`, else `gpgsm` |
| SSH program | `gpg.ssh.program`, else `ssh-keygen` |
| Key | `user.signingkey`. When unset: OpenPGP and x509 use the committer as `Name <email>`; SSH is an error: "no user.signingkey for ssh signing". |

How each format signs. Every run has stdin from the buffer or null, never a terminal, and is killed after `timeout`:

- OpenPGP and x509: `<program> --status-fd=2 --pinentry-mode error -bsau <key>`, with the buffer on stdin. The signature is stdout. The run counts only when it exits 0 and stderr has a line starting `[GNUPG:] SIG_CREATED `, as git requires. `--pinentry-mode error` needs gpg 2.1 or later; an older gpg rejects it, and the commit falls back to unsigned with that reason.
- SSH: the buffer is written to a temporary file. A key starting `key::`, or starting `ssh-`, is a literal public key: it is written (without the `key::` prefix) to a temporary key file and `-U` is added so the agent signs. Any other key is a file path, with a leading `~/` expanded from `HOME`. The run is `<program> -Y sign -n git -f <key file> [-U] <buffer file>`, with `SSH_ASKPASS_REQUIRE=never` set so it cannot prompt. The signature is `<buffer file>.sig`. The temporary files are made with `tempfile`, which moves from factory-git's dev-dependencies to its dependencies (it is already a workspace dependency), and are removed afterwards. `-U` needs `SSH_AUTH_SOCK` in the driver's environment; without it ssh-keygen fails and the commit falls back to unsigned.

Every signature has one trailing newline removed before it is stored, so the `gpgsig` header matches what git writes byte for byte.

The reasons a sign returns are one line, at most 200 characters:

- "could not run <program>: <io error>"
- "<program> exited with <code>: <last stderr line>", where the last stderr line skips `[GNUPG:]` status lines
- "<program> made no signature" (exit 0 without `SIG_CREATED`, or no `.sig` file)
- "signing timed out after 30 s"

Example: with the key's passphrase not cached, gpg fails at once in error pinentry mode, and the reason reads like `gpg exited with 2: gpg: signing failed: No pinentry`.

### 3. Writing a commit

One private helper in factory-git does the writing for all three kinds:

```rust
/// Writes a commit, signed when the repository asks for it. Returns its id and the unsigned reason, if any.
pub(crate) fn write_commit(repo: &Repository, update_ref: Option<&str>, sig: &Signature<'_>, message: &str, tree: &Tree<'_>, parents: &[&Commit<'_>]) -> Result<(Oid, Option<String>)>;
```

- No signer: `repo.commit(update_ref, ..)`, reason `None`, as today.
- A signer: `repo.commit_create_buffer(..)`, then `Signer::sign`. On success, `repo.commit_signed(buffer, signature, Some("gpgsig"))`, which writes the object and moves no ref. Then, when `update_ref` is `Some("HEAD")`, the branch HEAD points to is moved with `reference_matching`, expecting the first parent's id and with the reflog message `commit: <subject>`. So a branch that moved meanwhile is refused, as `repo.commit(Some("HEAD"), ..)` refuses it today. The reason is `None`.
- A signer that cannot be set up, or a sign that fails: `repo.commit(update_ref, ..)` unsigned, with the reason.

`commit_staged` and `commit_merge` call it. `commit_merge` still passes `None` and moves the ref itself after its checks.

### 4. API changes

- `commit_staged` returns `Result<Committed>`, with `pub struct Committed { pub sha: String, pub unsigned: Option<String> }` exported from factory-git.
- `MergeOutcome::Merged` becomes `Merged { merge_sha: String, unsigned: Option<String> }`.

### 5. Reporting an unsigned commit

When a caller gets `unsigned: Some(reason)`, it reports in three places:

- **Ledger:** one line, `commit <sha7> is unsigned: <reason>`, through `Ledger::ruling` as `write_step_ledger` does.
- **stderr:** `warning: commit <sha7> is unsigned: <reason>`.
- **Event:** `tracing::warn!(event = "commit.unsigned", kind, sha, reason)`, with `kind` `snapshot`, `sync` or `merge`.

The job goes on as if the commit were signed.

For the final merge, `Step::Merged` carries the sha and the unsigned reason, and `BaseState::Merged` gives `None`. The ledger line, the stderr warning and the event are written in `merge_stage` after `try_merge` returns, never under the merge lock. The public `MergeResult` keeps its shape. `record_merge_pass` also writes the `merge: pass` detail as `<sha7>, unsigned: <reason>` instead of `<sha7>`. The line is still cut to 300 characters.

### 6. Docs

- The `## Commits` section of `docs/reference/software-factory.md` gets a bullet on signing: when it happens, the three formats, no prompt, the 30 s cap and the unsigned fallback with its three reports.
- `docs/guides/getting-started.md` gets a short note: keep the GPG agent's passphrase cached (or use an SSH agent) so background runs can sign.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `commit.gpgsign` unset or false | Unsigned commit, no warning, as today. |
| Signing on, passphrase not cached | gpg fails at once (no prompt). The commit is unsigned and reported. |
| Signing program missing | "could not run <program>: ..."; unsigned and reported. |
| Program hangs | Killed after 30 s; "signing timed out after 30 s"; unsigned and reported. |
| gpg exits 0 without `SIG_CREATED` | "<program> made no signature"; unsigned and reported. |
| `gpg.format = ssh` with no `user.signingkey` | "no user.signingkey for ssh signing"; unsigned and reported. |
| Unknown `gpg.format` | "unknown gpg.format <value>"; unsigned and reported. |
| Signed final merge whose checkout then fails | The ref is not moved, as today; the signed object is left unreferenced. |
| Merge or sync that conflicts | No commit is made, so nothing is signed; unchanged. |
| Signed commit | Carries a `gpgsig` header; `git log --show-signature` and GitHub verify it when the key is registered there. |

## Testing

Test isolation: the user's global config turns signing on, and CI has none. Every test helper that creates a repository (the `common` and `support` modules, and each test file that calls `Repository::init` itself) writes `commit.gpgsign = false` into that repo's own config, which overrides the global value. A test that checks signing sets it to `true` and points the program setting at a fake script. Without this, existing tests on this machine would run the real gpg with the real key.

factory-git, with fake signing programs: small `#!/bin/sh` scripts written into a temp dir and named in the test repo's config. Each script records its arguments to a file.

- `sign.rs` unit tests:
  - `signer_reads_openpgp_settings`
  - `signer_falls_back_to_gpg_program_then_gpg`
  - `signer_uses_the_committer_when_no_key_is_set`
  - `ssh_without_a_key_is_an_error`
  - `an_unknown_format_is_an_error`
  - `no_signer_without_gpgsign`
  - `a_hanging_program_times_out` (built with a 1 s timeout)
- `tests/commits.rs`:
  - `a_snapshot_is_signed_when_gpgsign_is_on`: the commit's signature (`repo.extract_signature`) equals the fake output without its trailing newline, and the signed data equals the commit buffer.
  - `a_signed_snapshot_refuses_a_moved_branch`: when the branch moved after the buffer was built, the ref is not moved and an error is returned.
  - `the_reason_skips_gnupg_status_lines`: a fake gpg that prints a `gpg:` line and then a `[GNUPG:]` line and exits 2 gives a reason ending in the `gpg:` line.
  - `the_openpgp_program_gets_no_prompt_flags`: the recorded args are `--status-fd=2 --pinentry-mode error -bsau ABC`.
  - `a_failing_signer_commits_unsigned_with_a_reason`: `unsigned` names the exit code; there is no `gpgsig` header; HEAD moved.
  - `a_signed_snapshot_moves_the_branch`: the branch tip is the new sha.
  - `no_gpgsign_commits_as_before`: `unsigned` is `None` and there is no `gpgsig`.
  - `ssh_signing_passes_the_key_file`: args `-Y sign -n git -f <path> <file>`.
  - `ssh_literal_key_uses_the_agent`: a `key::` value adds `-U`.
  - `x509_uses_gpgsm_settings`: `gpg.format = x509` runs `gpg.x509.program`.
- `tests/merge.rs`: `a_signed_merge_into_main_has_a_gpgsig`, and `an_unsigned_merge_reports_the_reason` in `MergeOutcome::Merged`.
- `tests/worktree.rs`: `a_signed_sync_merge_has_a_gpgsig`.
- factory-engine: `an_unsigned_snapshot_writes_a_ledger_line` and `an_unsigned_final_merge_writes_the_reason_in_the_merge_gate`, on fixtures whose repo config points `gpg.program` at a failing script.

Commands:

- `cargo test -j4 -p factory-git`
- `cargo test -j4 -p factory-engine`
- `cargo test -j4 -p factory-cli`
- `cargo clippy -j4 -p factory-git -p factory-engine --all-targets -- -D warnings`
- `cargo fmt --check`

## Files changed

- `crates/factory-git/src/sign.rs` (new), `lib.rs`, `commits.rs`, `merge.rs`, `worktree.rs`
- `crates/factory-git/Cargo.toml` (`tempfile` to dependencies)
- `crates/factory-git/tests/commits.rs`, `tests/merge.rs`, `tests/worktree.rs`, `tests/base_branch.rs`, `tests/reset.rs`, and `tests/common/mod.rs`
- The repository-creating test helpers in factory-engine, factory-cli and factory-supervisor tests (`commit.gpgsign = false`)
- `crates/factory-engine/src/build.rs`, `merge.rs`, `sync.rs` and their tests
- `docs/reference/software-factory.md`, `docs/guides/getting-started.md`

## Constraints

- Never shell out to `git`. The signing programs are run with `std::process::Command`.
- No new crate dependency. `tempfile`, already a workspace dependency, becomes a normal dependency of factory-git.
- No prompt: OpenPGP and x509 get `--pinentry-mode error`, SSH gets `SSH_ASKPASS_REQUIRE=never`, and stdin is never a terminal.
- `SIGN_TIMEOUT` is 30 seconds.
- A failed signature never fails or parks a job.
- No file under `dashboard/` changes. Board JSON and the gate line format do not change.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## Acceptance criteria

- With `commit.gpgsign = true`, snapshot, sync and final merge commits carry a `gpgsig` header made by the configured program.
- With it unset or false, commits are made exactly as before, with no warning.
- OpenPGP, x509 and SSH signing each run the program and arguments in the Design.
- The signing program is never allowed to prompt, and is killed after 30 seconds.
- When signing fails, the commit is still made unsigned and the job continues. The ledger, stderr and a `commit.unsigned` event each name the sha and the reason.
- An unsigned final merge shows `<sha7>, unsigned: <reason>` in the `merge` gate detail.
- `commit_staged` returns `Committed { sha, unsigned }`, and `MergeOutcome::Merged` carries `unsigned`.
- The docs describe signing, the fallback and the cached-passphrase advice.
- Every test in Testing passes, along with all existing tests, clippy and fmt.

## Out of scope

- Re-signing existing commits.
- `gpg.ssh.defaultKeyCommand`.
- A project setting that overrides git's `commit.gpgsign`.

## Decisions

- When should the driver sign its commits? Options: follow git's `commit.gpgsign`, a new project setting, only the final merge. Chosen: follow `commit.gpgsign` with `user.signingkey`, for all three commit kinds. Decided by: user
- What happens when signing fails? Options: park the job, commit unsigned and warn, park only at the final merge. Chosen: commit unsigned and warn. Decided by: user
- Which key types are supported? Options: OpenPGP only, OpenPGP and SSH, all three (OpenPGP, SSH, x509). Chosen: all three. Decided by: user
- May the signing program prompt for a passphrase? Options: never prompt, prompt with a time limit, behave exactly like git. Chosen: never prompt, and cap each signing call at 30 seconds. Decided by: user
- Where is an unsigned commit reported? Options: ledger, stderr and an event; also in the merge gate detail; only the ledger. Chosen: ledger, stderr and an event, plus the merge gate detail for the final merge. Decided by: user
- The signer lives in factory-git next to the commit code, and runs the programs with `std::process` and a polling wait, not through factory-procgroup. A signing run lasts seconds. Decided by: call
- SSH with no `user.signingkey` is unsigned with a warning; `gpg.ssh.defaultKeyCommand` is not read. Decided by: call
- The key for OpenPGP and x509 falls back to the committer identity, as git does. Decided by: call
- The docs add a cached-passphrase note to the getting-started guide. Decided by: call
- Test helpers write `commit.gpgsign = false` into each test repo, so the user's global config never makes tests sign. Decided by: call
- One trailing newline is removed from every signature, to match git's header byte for byte. Decided by: call
- The final-merge reports are written in `merge_stage` after `try_merge`, carried by `Step::Merged`; the public `MergeResult` keeps its shape. Decided by: call
- A signed snapshot moves its branch with a compare against the first parent, keeping today's refusal of a moved branch. Decided by: call
- `tempfile` becomes a normal dependency of factory-git for the SSH temporary files. Decided by: call
- A failure reason skips `[GNUPG:]` status lines when it picks the last stderr line. Decided by: call
