#!/bin/bash
#
# Propose a release of shakenfist-visual-digest.
#
# Creates a `release-X.Y.Z` branch from main, bumps the
# shakenfist-visual-digest package version, refreshes Cargo.lock, runs
# the lint and test gates in the devcontainer, and (after
# confirmation) commits and pushes the branch. Does NOT open a PR and
# does NOT tag — both happen outside the script:
#
#   1. Run this script (or `make propose-release X.Y.Z`).
#   2. Open a PR from release-X.Y.Z to main, review, and merge it.
#   3. Run `make tag-release X.Y.Z` to tag the merge commit on main.
#   4. Run `make publish-crates` to publish to crates.io.
#
# Only shakenfist-visual-digest is versioned/published; the
# digest-decode helper bin is publish = false and keeps its own
# version.
#
# Modelled on ryll's tools/propose-release.sh, adapted for the single
# publishable crate and the `main` branch. The version bump is a
# targeted sed on the one [package] version line rather than
# cargo-release, so no extra host toolchain is required — everything
# that touches Rust runs in the devcontainer.
#
# Requirements on the host:
#   - git, curl (crates.io availability check)
#   - docker (lint/test gates run in the devcontainer)

set -euo pipefail

CRATE="shakenfist-visual-digest"
MANIFEST="$CRATE/Cargo.toml"

err() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*"; }

# --- arg parsing ---

[[ $# -eq 1 ]] || err "usage: $0 VERSION (e.g. 0.1.1)"
VERSION="$1"

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || err "version must be X.Y.Z, got: $VERSION"

TAG="v$VERSION"
RELEASE_BRANCH="release-$VERSION"

# --- tool availability ---

command -v curl >/dev/null || err "curl not installed"
command -v docker >/dev/null || err "docker not installed"

# --- working directory must be repo root ---

cd "$(dirname "$0")/.."
[[ -f Cargo.toml ]] || err "could not find repo root"
[[ -f "$MANIFEST" ]] || err "could not find $MANIFEST"

# --- git state checks ---

info "Checking git state"

BRANCH=$(git rev-parse --abbrev-ref HEAD)
[[ "$BRANCH" == "main" ]] \
    || err "must be on main, currently on: $BRANCH"

[[ -z "$(git status --porcelain)" ]] \
    || err "working tree is dirty; commit or stash first"

git fetch origin main --quiet
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main)
[[ "$LOCAL" == "$REMOTE" ]] \
    || err "local main is not in sync with origin/main"

if git rev-parse "$TAG" >/dev/null 2>&1; then
    err "tag $TAG already exists locally"
fi
if git ls-remote --tags --exit-code origin "$TAG" >/dev/null 2>&1; then
    err "tag $TAG already exists on origin"
fi

if git rev-parse --verify "$RELEASE_BRANCH" >/dev/null 2>&1; then
    err "branch $RELEASE_BRANCH already exists locally"
fi
if git ls-remote --heads --exit-code origin "$RELEASE_BRANCH" >/dev/null 2>&1; then
    err "branch $RELEASE_BRANCH already exists on origin"
fi

# --- crates.io version availability ---

info "Checking crates.io for existing $CRATE $VERSION"

# The versions endpoint returns 200 when the version exists and 404
# when it does not. Anything else is treated as "taken" to be safe.
url="https://crates.io/api/v1/crates/$CRATE/$VERSION"
code=$(curl -s -o /dev/null -w '%{http_code}' \
    -A 'visual-digest-release-script (mikal@stillhq.com)' "$url")
case "$code" in
    404) ;;  # good, version is free
    200) err "$CRATE $VERSION already published on crates.io" ;;
    *)   err "unexpected HTTP $code checking $CRATE $VERSION" ;;
esac

# --- create release branch ---

info "Creating branch $RELEASE_BRANCH from main"
git switch -c "$RELEASE_BRANCH"

# Clean up the branch if the script aborts before a successful push.
CLEANUP_BRANCH=1
cleanup() {
    if [[ "${CLEANUP_BRANCH:-0}" == "1" ]]; then
        info "Cleaning up: switching back to main and deleting $RELEASE_BRANCH"
        git checkout -- . 2>/dev/null || true
        git switch main 2>/dev/null || true
        git branch -D "$RELEASE_BRANCH" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# --- bump version ---
#
# Only the [package] version line in $MANIFEST starts with
# `version = "`; dependency versions are inline tables
# (`crc = { version = ... }`), so this anchored substitution touches
# exactly one line. We verify that afterwards.

info "Bumping $CRATE to $VERSION"
sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"$VERSION\"/" \
    "$MANIFEST"

grep -qxF "version = \"$VERSION\"" "$MANIFEST" \
    || err "failed to set version in $MANIFEST"

# --- lint + test gates (in the devcontainer) ---
#
# `make test` resolves the workspace and rewrites Cargo.lock with the
# new local version as a side effect, so the lockfile is committed in
# sync with the manifest.

info "Running lint (rustfmt + clippy) in devcontainer"
make lint

info "Running tests in devcontainer"
make test

# --- confirmation ---

echo
echo "About to propose release $VERSION on branch $RELEASE_BRANCH."
echo "Pending changes:"
git diff --stat
echo
read -rp "Commit and push $RELEASE_BRANCH? [y/N] " REPLY
[[ "$REPLY" =~ ^[Yy]$ ]] || {
    info "Aborted at confirmation."
    exit 1
}

# --- commit and push ---

info "Creating release proposal commit"
git add -u
git commit -m "Release ${VERSION}."

info "Pushing $RELEASE_BRANCH"
git push --set-upstream origin "$RELEASE_BRANCH"

# Successful push — leave the user on the release branch.
CLEANUP_BRANCH=0

echo
info "Release proposed on branch $RELEASE_BRANCH."
echo
echo "Next steps:"
echo "  1. Open a PR from $RELEASE_BRANCH into main:"
echo "     https://github.com/shakenfist/visual-digest-rust/pull/new/$RELEASE_BRANCH"
echo "  2. Get it reviewed and merged."
echo "  3. Run 'make tag-release $VERSION' to tag main."
echo "  4. Run 'make publish-crates' (with CARGO_REGISTRY_TOKEN set) to"
echo "     publish $CRATE $VERSION to crates.io."
