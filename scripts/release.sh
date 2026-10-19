#!/usr/bin/env bash
# Bumps the version, creates a GPG-signed release tag, and pushes both.
#
# Usage: ./scripts/release.sh [--dry-run] [major|minor|patch|<version>]
#
# The script itself makes the bump commit "chore: bump version to vX.Y.Z".
# It is a plain release commit made by this script, not a Claude commit, so it
# does not go through the 1-git-commit skill.
#
# --dry-run runs every check (clean tree, CI gate, version math, tag check,
# signing setup) and stops before the first step that changes anything.
#
# See docs/releasing.md for the full release process.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Workspace members, listed by name. The repo rules forbid --workspace and
# --all, so every cargo call names its crates. Keep in sync with Cargo.toml
# and .github/workflows/ci.yml.
CRATES=(
    factory-core
    factory-git
    factory-agent
    factory-checks
    factory-engine
    factory-cli
    factory-supervisor
    factory-telemetry
)
PKG_ARGS=()
for c in "${CRATES[@]}"; do
    PKG_ARGS+=(-p "$c")
done
DASH_MANIFEST="dashboard/Cargo.toml"

usage() {
    echo "Usage: $0 [--dry-run] [major|minor|patch|<version>]"
    echo ""
    echo "  major      v1.2.3 -> v2.0.0"
    echo "  minor      v1.2.3 -> v1.3.0"
    echo "  patch      v1.2.3 -> v1.2.4"
    echo "  <version>  explicit target, e.g. v4.0.0 or 4.0.0, for a jump that"
    echo "             is not a relative bump from the current version"
    echo "  --dry-run  run all checks, then stop before changing anything"
    exit 1
}

dry_run=0
bump=""
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry_run=1 ;;
        -h|--help) usage ;;
        -*) echo -e "${RED}Unknown option: ${arg}${NC}"; usage ;;
        *)
            if [[ -n "$bump" ]]; then
                echo -e "${RED}Only one bump type or version may be given.${NC}"
                usage
            fi
            bump="$arg"
            ;;
    esac
done

# The working tree must be clean. A signed release tag should capture exactly
# what is committed. The version bump below is the one commit made on top.
if [[ -n "$(git status --porcelain)" ]]; then
    echo -e "${RED}Working tree has uncommitted changes. Commit or stash before releasing.${NC}"
    git status --short
    exit 1
fi

# The bump commit and the push need a branch, not a detached HEAD.
if ! branch="$(git symbolic-ref --quiet --short HEAD)"; then
    echo -e "${RED}HEAD is detached. Check out the release branch first.${NC}"
    exit 1
fi

# CI gate. It mirrors .github/workflows/ci.yml, so a release never tags a
# commit that would fail CI. It runs first because it is the slowest step.
echo -e "${BLUE}Running CI gate (fmt, clippy, test, dashboard)...${NC}"
if ! cargo fmt "${PKG_ARGS[@]}" -- --check; then
    echo -e "${RED}cargo fmt found unformatted code. Run 'cargo fmt' and commit.${NC}"
    exit 1
fi
if ! cargo clippy -j4 "${PKG_ARGS[@]}" --all-targets -- -D warnings; then
    echo -e "${RED}cargo clippy found warnings. Fix them before releasing.${NC}"
    exit 1
fi
if ! cargo test -j4 "${PKG_ARGS[@]}"; then
    echo -e "${RED}Tests failed. Fix them before releasing.${NC}"
    exit 1
fi
if ! cargo fmt --manifest-path "$DASH_MANIFEST" -- --check; then
    echo -e "${RED}cargo fmt found unformatted dashboard code. Run 'cargo fmt --manifest-path ${DASH_MANIFEST}' and commit.${NC}"
    exit 1
fi
if ! cargo clippy -j4 --manifest-path "$DASH_MANIFEST" --all-targets -- -D warnings; then
    echo -e "${RED}cargo clippy found dashboard warnings. Fix them before releasing.${NC}"
    exit 1
fi
if ! cargo test -j4 --manifest-path "$DASH_MANIFEST"; then
    echo -e "${RED}Dashboard tests failed. Fix them before releasing.${NC}"
    exit 1
fi
if ! node --test dashboard/tests/*.test.js; then
    echo -e "${RED}Dashboard JS tests failed. Fix them before releasing.${NC}"
    exit 1
fi
echo -e "${GREEN}CI gate passed.${NC}"
echo ""

# Resolve the bump type from the argument or a prompt.
if [[ -z "$bump" ]]; then
    echo -e "${BLUE}Bump type:${NC} major / minor / patch, or an explicit version (e.g. v4.0.0)"
    read -rp "> " bump
fi

# Prints the version line of one section of a Cargo.toml.
section_version() {
    local file="$1" section="$2"
    awk -v sec="[$section]" '
        $0 == sec { in_section = 1; next }
        /^\[/ { in_section = 0 }
        in_section && /^version = / { gsub(/^version = "|"$/, ""); print; exit }
    ' "$file"
}

# The current version comes from [workspace.package] in the root Cargo.toml,
# not from git tags. The manifest already says 0.1.0 with no tag cut, so
# tag-based math would go backwards.
current_version="$(section_version Cargo.toml workspace.package)"
dash_version="$(section_version "$DASH_MANIFEST" package)"
if [[ ! "$current_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo -e "${RED}Cannot read [workspace.package] version from Cargo.toml (got '${current_version}').${NC}"
    exit 1
fi
if [[ "$dash_version" != "$current_version" ]]; then
    echo -e "${YELLOW}Note: ${DASH_MANIFEST} is at ${dash_version}, the workspace at ${current_version}. Both get the new version.${NC}"
fi
latest="v${current_version}"

if [[ "$bump" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    # An explicit target version, for jumps no relative bump can reach.
    new_tag="v${bump#v}"
else
    case "$bump" in
        major|minor|patch) ;;
        *) echo -e "${RED}Invalid bump type or version: ${bump}${NC}"; usage ;;
    esac

    major="${current_version%%.*}"; rest="${current_version#*.}"
    minor="${rest%%.*}"; patch="${rest#*.}"

    case "$bump" in
        major) major=$((major + 1)); minor=0; patch=0 ;;
        minor) minor=$((minor + 1)); patch=0 ;;
        patch) patch=$((patch + 1)) ;;
    esac

    new_tag="v${major}.${minor}.${patch}"
fi
new_version="${new_tag#v}"

if git rev-parse -q --verify "refs/tags/${new_tag}" >/dev/null; then
    echo -e "${RED}Tag ${new_tag} already exists locally.${NC}"
    exit 1
fi
if git ls-remote --exit-code --tags origin "refs/tags/${new_tag}" >/dev/null 2>&1; then
    echo -e "${RED}Tag ${new_tag} already exists on origin.${NC}"
    exit 1
fi

# Checks the GPG signing setup without changing anything. It runs before the
# version bump commit, so a failure leaves nothing to unwind.
check_signing() {
    local fmt signing_key email
    fmt="$(git config --get gpg.format 2>/dev/null || true)"
    if [[ -n "$fmt" && "$fmt" != "openpgp" ]]; then
        echo -e "${RED}gpg.format is '${fmt}'. Release tags must be GPG (openpgp) signed.${NC}"
        return 1
    fi
    signing_key="$(git config --get user.signingkey 2>/dev/null || true)"
    if [[ -z "$signing_key" ]]; then
        echo -e "${RED}No user.signingkey configured. Run:${NC}"
        echo "  git config user.signingkey <KEY_ID>"
        return 1
    fi
    if ! gpg --list-secret-keys "$signing_key" >/dev/null 2>&1; then
        echo -e "${RED}user.signingkey is '${signing_key}', but gpg has no secret key for it.${NC}"
        echo "  Check with: git config --show-origin --get-all user.signingkey"
        return 1
    fi
    # GitHub shows "Verified" only when the tagger email is a UID on the key
    # and a verified email on the GitHub account. Only the first part can be
    # checked here.
    email="$(git config --get user.email 2>/dev/null || true)"
    if [[ -z "$email" ]]; then
        echo -e "${RED}No user.email configured. The tag needs a tagger email.${NC}"
        return 1
    fi
    if ! gpg --list-keys --with-colons "$signing_key" 2>/dev/null \
            | awk -F: '$1 == "uid" { print $10 }' | grep -qiF "<${email}>"; then
        echo -e "${YELLOW}Warning: user.email '${email}' is not a UID on key ${signing_key}.${NC}"
        echo -e "${YELLOW}The tag will verify locally but GitHub will show it as Unverified. See docs/releasing.md.${NC}"
    fi
    echo -e "${GREEN}Signing key ${signing_key} found for ${email}.${NC}"
}

echo ""
echo -e "  Branch              : ${YELLOW}${branch}${NC}"
echo -e "  Current version     : ${YELLOW}${latest}${NC}"
echo -e "  New tag             : ${GREEN}${new_tag}${NC}"
echo -e "  Cargo.toml          : ${YELLOW}${current_version}${NC} -> ${GREEN}${new_version}${NC}"
echo -e "  ${DASH_MANIFEST} : ${YELLOW}${dash_version}${NC} -> ${GREEN}${new_version}${NC}"
echo ""

if [[ "$dry_run" -eq 1 ]]; then
    check_signing || exit 1
    echo ""
    echo -e "${GREEN}Dry run passed. Nothing was changed.${NC}"
    echo "A real run would bump both manifests and lockfiles, commit"
    echo "\"chore: bump version to ${new_tag}\", create the signed tag ${new_tag},"
    echo "verify it, and push ${branch} and ${new_tag} to origin."
    exit 0
fi

read -rp "Bump version, create and push signed tag ${new_tag}? [y/N] " confirm
if [[ "${confirm,,}" != "y" ]]; then
    echo "Aborted."
    exit 0
fi

check_signing || exit 1

# Sets the version line inside one section of a Cargo.toml. Scoped to that
# section, so other "version = " lines stay untouched.
set_section_version() {
    local file="$1" section="$2" new="$3"
    awk -v sec="[$section]" -v new="$new" '
        $0 == sec { in_section = 1; print; next }
        /^\[/ { in_section = 0 }
        in_section && /^version = / { print "version = \"" new "\""; in_section = 0; next }
        { print }
    ' "$file" > "${file}.tmp" && mv "${file}.tmp" "$file"
}

set_section_version Cargo.toml workspace.package "$new_version"
set_section_version "$DASH_MANIFEST" package "$new_version"

# Refresh both lockfiles. The members inherit version.workspace = true, so a
# check rewrites their recorded versions.
cargo check -j4 --quiet
cargo check -j4 --quiet --manifest-path "$DASH_MANIFEST"

git add Cargo.toml Cargo.lock "$DASH_MANIFEST" dashboard/Cargo.lock
git commit -m "chore: bump version to ${new_tag}"

# Create the signed tag on the version bump commit.
git tag -s "$new_tag" -m "Release ${new_tag}"

# Verify before pushing. Never push an unverified tag.
if ! git tag -v "$new_tag" 2>&1 | grep -q "Good signature"; then
    echo -e "${RED}Tag signature verification failed. Not pushing.${NC}"
    git tag -d "$new_tag"
    echo -e "${YELLOW}The version bump commit is still local. 'git reset --hard HEAD~1' undoes it.${NC}"
    exit 1
fi

git push origin "$branch" "$new_tag"

repo_slug="$(git remote get-url origin | sed -E 's#^.*github\.com[:/]##; s#\.git$##')"
echo ""
echo -e "${GREEN}Bumped, tagged and pushed: ${new_tag}${NC}"
echo "GitHub Actions builds the release binaries and opens a draft release."
echo "Monitor: https://github.com/${repo_slug}/actions"
