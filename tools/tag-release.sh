#!/bin/bash
#
# Tag a release of shakenfist-visual-digest.
#
# Run after the release-X.Y.Z PR from tools/propose-release.sh has been
# reviewed and merged into main. Fetches origin/main, verifies its tip
# carries the expected shakenfist-visual-digest version, and (after
# confirmation) creates an annotated tag vX.Y.Z pointing at that commit
# and pushes it.
#
# Unlike ryll, this repo has no tag-triggered release workflow: the tag
# is the canonical marker for "this commit is release X.Y.Z", and the
# actual crates.io upload is the separate, deliberate `make
# publish-crates` step (IRREVERSIBLE). Run that after tagging.
#
# Usage:
#   tools/tag-release.sh VERSION
#   make tag-release VERSION
#
# Requirements on the host:
#   - git

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

# --- working directory must be repo root ---

cd "$(dirname "$0")/.."
[[ -f Cargo.toml ]] || err "could not find repo root"

# --- fetch latest main ---

info "Fetching origin"
git fetch origin main --tags --quiet

# --- tag must not already exist ---

if git rev-parse "$TAG" >/dev/null 2>&1; then
    err "tag $TAG already exists locally"
fi
if git ls-remote --tags --exit-code origin "$TAG" >/dev/null 2>&1; then
    err "tag $TAG already exists on origin"
fi

# --- verify crate version on origin/main ---

info "Verifying $CRATE version on origin/main"
CRATE_TOML=$(git show "origin/main:$MANIFEST")
ACTUAL=$(printf '%s\n' "$CRATE_TOML" | awk '
    /^\[package\]/ { in_pkg=1; next }
    /^\[/ { in_pkg=0 }
    in_pkg && /^version *= */ {
        gsub(/version *= *"|"/, "")
        print
        exit
    }
')

[[ -n "$ACTUAL" ]] \
    || err "could not read [package].version from origin/main:$MANIFEST"
[[ "$ACTUAL" == "$VERSION" ]] \
    || err "origin/main $CRATE version is $ACTUAL, expected $VERSION. Has the release-$VERSION PR been merged?"

TARGET_SHA=$(git rev-parse origin/main)
TARGET_SUBJECT=$(git log -1 --format=%s origin/main)

# --- confirmation ---

echo
echo "About to tag $TAG at $TARGET_SHA on origin/main:"
echo "  $TARGET_SUBJECT"
echo
read -rp "Create and push tag $TAG? [y/N] " REPLY
[[ "$REPLY" =~ ^[Yy]$ ]] || {
    info "Aborted."
    exit 1
}

# --- tag and push ---

info "Creating annotated tag $TAG"
git tag -a "$TAG" -m "Release $VERSION" "$TARGET_SHA"

info "Pushing tag $TAG"
git push origin "$TAG"

echo
info "Tagged $TAG."
echo
echo "Next step:"
echo "  Run 'make publish-crates' (with CARGO_REGISTRY_TOKEN set) to"
echo "  publish $CRATE $VERSION to crates.io. This is IRREVERSIBLE."
