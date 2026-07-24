#!/usr/bin/env bash
#
# Publish shakenfist-visual-digest to crates.io.
#
# Modelled on ryll's tools/publish-crates.sh, but this workspace has a
# single publishable crate: shakenfist-visual-digest. The digest-decode
# helper bin stays publish = false and is never uploaded.
#
# Runs INSIDE the devcontainer (via `make publish-crates`), with
# CARGO_REGISTRY_TOKEN forwarded from the caller's environment.
# Publishing to crates.io is IRREVERSIBLE — a version number can never
# be reused, only yanked.

set -euo pipefail

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
    echo "ERROR: CARGO_REGISTRY_TOKEN is not set" >&2
    exit 1
fi

# Point cargo publish's verification build at an ephemeral,
# container-local target dir instead of the bind-mounted
# /workspace/target. That directory can be left owned by root by an
# earlier `docker run` without -u (e.g. a raw `cargo publish` one-off),
# which makes the verify build fail with EACCES on .cargo-artifact-lock
# when we run as the caller's UID. Building into a throwaway path under
# HOME (world-writable /build in the devcontainer) sidesteps the
# workspace target/ ownership entirely; the artifacts are discarded.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${HOME:-/tmp}/cargo-target-publish}"

echo "=== Publishing shakenfist-visual-digest ==="
echo "    (verification build target: $CARGO_TARGET_DIR)"
cargo publish -p shakenfist-visual-digest
