# Releasing

A release is a GPG-signed git tag `vX.Y.Z`. Pushing the tag starts `.github/workflows/release.yml`, which builds the binaries, signs the archives and opens a draft GitHub Release. A maintainer reviews the draft and publishes it by hand.

The pattern comes from rocket-mem and the release playbook in the knowledge base (`playbooks/release-management.md`).

## Files

| File | What it does |
|---|---|
| `scripts/release.sh` | Runs the CI gate, bumps the version, commits, creates and verifies the signed tag, pushes. |
| `cliff.toml` | git-cliff config for the release notes. |
| `.github/workflows/ci.yml` | fmt, clippy `-D warnings` and tests for the driver crates and the dashboard. |
| `.github/workflows/release.yml` | Changelog, native builds, checksums, minisign signatures, draft release. |
| `RELEASE_SIGNING_KEY.pub` | The minisign public key. The maintainer adds it, see below. |

## Two layers of trust

- The git tag is GPG-signed with the maintainer's own key. It proves **who** cut the release.
- Each archive is minisign-signed in CI with a key used only by rocket-factory. It proves the archive is **what** that release's CI built.
- The `.sha256` files only catch a broken download. Anyone can compute a checksum for a swapped file, so they are no proof of origin.

## Cutting a release

Before tagging, run the live probe once for each harness:

```bash
factory probe --harness claude
factory probe --harness codex
```

A failing channel blocks the release unless the release notes say why. Paste the CLI versions and the tables into the release notes. The probe uses real credentials and costs one short turn per role, so `release.sh` does not run it.

```bash
./scripts/release.sh --dry-run patch   # every check, nothing changed
./scripts/release.sh patch             # or minor, major, or an explicit v1.2.3
```

The script does this, in order:

1. Refuses a dirty tree or a detached HEAD.
2. Runs the CI gate, the same commands as `ci.yml`. It names every crate with `-p`, because the repo rules forbid `--workspace` and `--all`.
3. Reads the current version from `[workspace.package]` in the root `Cargo.toml`. It never uses git tags for this, because the manifest is ahead of any tag.
4. Computes the new tag and refuses one that exists locally or on `origin`.
5. Asks `y/N`.
6. Checks the signing setup: `gpg.format` is GPG, `user.signingkey` is set, gpg has its secret key, and `user.email` is a UID on that key (a warning only).
7. Sets the new version in `Cargo.toml` and `dashboard/Cargo.toml`, refreshes both lockfiles and commits them as `chore: bump version to vX.Y.Z`. The script makes this commit itself, so it does not use the commit skill.
8. Runs `git tag -s`, then `git tag -v`. Without `Good signature` it deletes the tag and stops.
9. Pushes the branch and the tag and prints the Actions link.

The release workflow fails when the tag does not match both manifests, so a tag made by hand without the bump is refused.

The changelog skips `chore`, `test`, `ci`, `style` and `revert` commits and every subject that is not `type(scope): summary`. A factory job whose commit subject starts with `chore:` does not show up in the notes, so edit the draft when needed.

## The GitHub "Verified" badge

`git tag -v` passing is not enough. GitHub shows a tag as **Verified** only when all three hold:

1. The tagger email (`git config user.email` in this repo) is a UID on the GPG key.
2. The public key with that UID is uploaded to GitHub (Settings, SSH and GPG keys).
3. The same email is added and verified on the GitHub account (Settings, Emails).

To add a missing UID, then upload the key again:

```bash
gpg --quick-add-uid <fingerprint> "Name <email>"
gpg --armor --export <fingerprint>
```

`--quick-add-uid` only adds a UID. It does not remove one or change the primary UID.

Check the setup with a throwaway tag before the first real release. Its name does not match `v*.*.*`, so no workflow runs. Push it, look at it on GitHub, then delete it on both sides:

```bash
git tag -s signing-test -m test && git push origin signing-test
git push origin :refs/tags/signing-test && git tag -d signing-test
```

## Artifact signing key (one-time maintainer setup)

Use one keypair for rocket-factory only. Never reuse a key from another project. A leaked secret then affects only this project.

1. Generate the key locally. It asks for a password. Save the password in a password manager right away, because there is no recovery.

   ```bash
   cargo tauri signer generate -w ~/.minisign/rocket-factory.key
   ```

2. Add two repository secrets in the GitHub web UI (Settings, Secrets and variables, Actions, New repository secret):
   - `RELEASE_SIGNING_KEY`: the full contents of `~/.minisign/rocket-factory.key`.
   - `RELEASE_SIGNING_KEY_PASSWORD`: the password from step 1.

   Paste without a trailing newline. An extra newline breaks decryption in the same way as a wrong password.

3. Commit the public key as `RELEASE_SIGNING_KEY.pub` at the repo root. `cargo tauri signer` writes the `.pub` file base64-encoded, so decode it first. The result is the plain minisign format that `minisign` reads:

   ```bash
   base64 -d ~/.minisign/rocket-factory.key.pub > RELEASE_SIGNING_KEY.pub
   ```

The two secrets are a pair. A lost password means a new keypair, both secrets replaced, and a new `RELEASE_SIGNING_KEY.pub`.

When CI reports a wrong password, first test the pair locally:

```bash
TAURI_SIGNING_PRIVATE_KEY_PATH=~/.minisign/rocket-factory.key \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD='<password>' \
cargo tauri signer sign <any file>
```

If that works, the secret was pasted wrong.

## What a release contains

One archive per platform, built natively on its own runner:

- `rocket-factory-vX.Y.Z-linux-amd64.tar.gz`
- `rocket-factory-vX.Y.Z-darwin-arm64.tar.gz`

Each archive holds `factory`, `factoryd`, `factory-server` (the dashboard), `LICENSE` and `README.md` in one folder. They stay together because `factoryd` looks for `factory` next to its own file. Each archive has a `.sha256` and a `.sig` file. There is no Windows build, since `factoryd` does not support Windows.

## Verifying a download

Use the real `minisign` CLI (`brew install minisign` or `apt install minisign`). `cargo tauri` can only sign.

`cargo tauri signer sign` writes the `.sig` file base64-encoded, so decode it into the `.minisig` file that `minisign -V` looks for:

```bash
sha256sum -c rocket-factory-vX.Y.Z-linux-amd64.tar.gz.sha256
base64 -d rocket-factory-vX.Y.Z-linux-amd64.tar.gz.sig > rocket-factory-vX.Y.Z-linux-amd64.tar.gz.minisig
minisign -Vm rocket-factory-vX.Y.Z-linux-amd64.tar.gz -p RELEASE_SIGNING_KEY.pub
```

On macOS use `shasum -a 256 -c` for the first line, and `base64 -D` on older versions. To check the tag itself, run `git tag -v vX.Y.Z` in a clone.
