# visual-digest-rust

Rust crate implementing the visual on-screen digest format used by the
[shakenfist](https://github.com/shakenfist) project for UEFI boot-phase
telemetry.

## What this repo contains

- **`shakenfist-visual-digest`** — library crate providing the encoder,
  decoder, and optional QR locate-and-decode helper. Default features
  provide the encoder only in `no_std`-compatible form. The `decode`,
  `qr`, `serde`, and `cli` features unlock progressively more
  functionality (see `ARCHITECTURE.md` for the full feature matrix).
- **`digest-decode`** — CLI binary that takes a PNG screenshot, locates
  the QR code, decodes the digest payload, and prints JSON to stdout.
  Implemented in step 1g.

## Where it is consumed

- **[shakenfist/uncalibrated-sextant](https://github.com/shakenfist/uncalibrated-sextant)**
  — the UEFI firmware that encodes and renders the digest QR code.
  Uses the library with default features (encoder only, `no_std`).
- **[shakenfist/ryll](https://github.com/shakenfist/ryll)** — the host
  side test harness. Will consume the `qr` and `decode` features once
  phase 6 of the test-harness plan lands.

## Format specification

The wire format is documented in `docs/visual-digest-format.md`, which
lands in step 1b of the phase 1 plan.

## Building

```
cargo build --workspace
```

For the no_std smoke (verifies the library compiles without allocator):

```
cargo build --workspace --no-default-features
```

For the full feature set (decoder, QR helper, serde, CLI):

```
cargo build --workspace --all-features
```

## Development environment

Rust toolchain is owned by the Docker image in `.devcontainer/`.
This keeps the host clean when multiple projects use different toolchain
versions. To use it directly:

```
docker build -t visual-digest-rust-dev .devcontainer/
docker run --rm -v "$PWD":/workspace -w /workspace visual-digest-rust-dev \
    cargo build --workspace
```

Or use the wrapper script (also used by pre-commit and CI):

```
./scripts/check-rust.sh          # rustfmt --check + clippy
./scripts/check-rust.sh fix      # rustfmt --write + clippy --fix
```

## CI

GitHub Actions via `.github/workflows/ci.yml`. Runs on self-hosted
runners (`[self-hosted, vm, debian-12]`). Each job runs inside the
same Docker image as local dev.

## Releasing

Only `shakenfist-visual-digest` is published to
[crates.io](https://crates.io/crates/shakenfist-visual-digest). The
`digest-decode` helper bin is `publish = false` and keeps its own
version.

The `Makefile` targets model ryll's two-phase, PR-gated release flow,
and everything that touches Rust runs in the devcontainer (no native
toolchain needed):

```
make propose-release X.Y.Z   # branch off main, bump version, lint+test, push for PR
# ... open the release-X.Y.Z PR, review, merge ...
make tag-release X.Y.Z       # tag the merged commit on main
export CARGO_REGISTRY_TOKEN=...
make publish-crates          # upload to crates.io (IRREVERSIBLE)
```

Unlike ryll there is no tag-triggered release workflow: the `vX.Y.Z`
tag is just the canonical marker for the release commit, and
`make publish-crates` is the deliberate, manual upload step. crates.io
versions can never be reused, only yanked — `propose-release` refuses a
version that already exists on crates.io.

## Planning trail

This repo is part of the shakenfist test-harness project. Plans live in
`shakenfist/kerbside`:

- Master plan:
  `docs/plans/PLAN-test-harness.md`
- Phase 1 (this repo):
  `docs/plans/PLAN-test-harness-phase-01-digest-crate.md`
