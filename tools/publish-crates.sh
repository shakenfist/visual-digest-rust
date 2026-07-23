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

echo "=== Publishing shakenfist-visual-digest ==="
cargo publish -p shakenfist-visual-digest
