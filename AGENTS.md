# AGENTS.md — guidance for AI agents working in visual-digest-rust

## Build / test / lint commands

All Rust work runs inside Docker to keep the host toolchain-free.
The wrapper script handles image build-on-demand, cache directories,
and UID mapping.

```bash
# Format check + clippy (used by CI and pre-commit)
./scripts/check-rust.sh check

# Auto-fix formatting and clippy --fix
./scripts/check-rust.sh fix

# Full test suite
docker run --rm -v "$PWD":/workspace -w /workspace \
    -v "$PWD/.cargo-cache/registry":/build/.cargo/registry \
    -v "$PWD/.cargo-cache/git":/build/.cargo/git \
    -u "$(id -u):$(id -g)" -e HOME=/build \
    visual-digest-rust-dev \
    cargo test --workspace --all-features

# no_std smoke (library must compile without std)
docker run --rm -v "$PWD":/workspace -w /workspace \
    -v "$PWD/.cargo-cache/registry":/build/.cargo/registry \
    -v "$PWD/.cargo-cache/git":/build/.cargo/git \
    -u "$(id -u):$(id -g)" -e HOME=/build \
    visual-digest-rust-dev \
    cargo build --workspace --no-default-features
```

Build the Docker image first if it does not exist:

```bash
docker build -t visual-digest-rust-dev .devcontainer/
```

The `Makefile` wraps these in convenience targets (`make build`,
`make test`, `make lint`, `make lint-fix`) that use the same Docker
image and cache mounts.

## Releasing

Only `shakenfist-visual-digest` is published to crates.io;
`digest-decode` is `publish = false`. The release flow is two-phase and
PR-gated, modelled on ryll:

```bash
make propose-release X.Y.Z   # branch off main, bump version, lint+test, push
make tag-release X.Y.Z       # after the PR merges: tag main
CARGO_REGISTRY_TOKEN=... make publish-crates   # upload (IRREVERSIBLE)
```

There is no tag-triggered release workflow — `make publish-crates` is
the deliberate manual upload. See README.md "Releasing" for detail.

## no_std discipline — critical invariant

The encoder (`shakenfist-visual-digest` with default features) MUST
remain `no_std`-compatible. This is enforced by the `#![cfg_attr]`
at the top of `lib.rs` and by the CI `--no-default-features` build step.

Rules:
- Code in `lib.rs` and any module compiled without the `decode` feature
  must not use `std`. Use `core::` equivalents.
- The `decode` feature unlocks `std` (and `alloc`). Code behind
  `#[cfg(feature = "decode")]` may use `Vec`, `String`, `Box`, etc.
- The `crc` dependency is already declared with `default-features =
  false` to satisfy the `no_std` constraint.
- Do not add `std`-requiring crates to default dependencies.

## Code conventions

- Formatting is managed by `rustfmt`. Do not fight it; run
  `./scripts/check-rust.sh fix` to apply it.
- Clippy is run with `-D warnings`. All warnings are errors.
- String literals: use the Rust idiom (double-quoted). The project's
  Python convention of preferring single quotes does not apply here.
- Line wrapping: `rustfmt` handles Rust source. For shell scripts and
  documentation, wrap at 80 characters.

## Feature flag matrix

| Feature   | Enables                                | Requires `std`? |
|-----------|----------------------------------------|-----------------|
| (default) | Encoder only                           | No (`no_std`)   |
| `decode`  | Decoder + `thiserror`                  | Yes             |
| `qr`      | QR locate helper + `rqrr` + `image`    | Yes             |
| `serde`   | `serde::Serialize` on decoded types    | No (via `serde`)  |
| `cli`     | All of `decode` + `qr` + `serde`       | Yes             |

## Cross-repo relationships

- **shakenfist/uncalibrated-sextant** — UEFI firmware; consumes this
  crate with default features (encoder, `no_std`). It depends via
  `git = "..."` rather than crates.io (see plan decisions). Any change
  to the encoder's public API or wire output must be coordinated with a
  corresponding change in Sextant (step 1h of the phase 1 plan).
- **shakenfist/ryll** — Host-side test harness; will consume the `qr`
  and `decode` features in phase 6. No dependency yet.

## Planning trail

Phase plan (authoritative):
`shakenfist/kerbside/docs/plans/PLAN-test-harness-phase-01-digest-crate.md`

Master plan:
`shakenfist/kerbside/docs/plans/PLAN-test-harness.md`

The "Decisions baked into this plan" section of the phase plan is the
primary reference for design decisions. Read it before proposing any
change to the crate's API surface, feature flags, or encoder behaviour.

## Wire format

The visual-digest wire format spec will live at
`docs/visual-digest-format.md` once step 1b lands. Until then, the
authoritative reference is
`shakenfist/uncalibrated-sextant/docs/visual-digest-format.md`.

**Do not change the wire format** without updating the spec doc and the
golden test fixtures in `tests/golden/` (added in step 1d).
