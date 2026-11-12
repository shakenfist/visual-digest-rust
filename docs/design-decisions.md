# Design decisions

This page records why the crate's API surface, feature flags and
distribution look the way they do. Read it before proposing a change to
any of them. Decisions about the bytes on the wire live in the format
specification, under
[Format ownership and stability](visual-digest-format.md#format-ownership-and-stability).

## Scope

The crate is the single source of truth for the visual on-screen digest:
shakenfist/uncalibrated-sextant encodes with it and shakenfist/ryll
decodes with it. It contains:

- an encoder, `no_std` and allocation-free, for the firmware;
- a decoder that turns a raw payload into a typed `Digest`;
- a QR locate-and-decode helper that turns a framebuffer or a PNG into
  a raw payload;
- the `digest-decode` CLI, which chains the QR helper and the decoder
  and prints JSON.

Nothing about how Sextant *renders* the digest lives here. The renderer
call site, the QR rasteriser and the framebuffer hash stay in Sextant;
the crate only produces the payload bytes.

## The crate owns Sextant's event vocabulary

`Event`, `Phase` and `BootloaderChoice` (`src/events.rs`) live in the
crate rather than in Sextant, alongside the format primitives. They are
the on-wire vocabulary: every `Event` variant maps 1:1 to a TLV tag, and
`Phase` and `BootloaderChoice` have stable wire discriminants. Splitting
them into a generic primitives layer would add an abstraction with no
second consumer to justify it.

The consequence is that the crate is specific to Sextant's events. A
different test guest either reuses this vocabulary or forks the crate.
If a second guest ever needs its own schema, the planned refactor is to
factor the primitives into their own module and layer guest-specific
event modules on top.

## The encoder takes a slice

```rust
pub fn encode(
    events: &[&Event],
    frame_counter: u32,
    framebuffer_hash: u32,
    channel_hashes: &ChannelHashes,
    out: &mut [u8; DIGEST_PAYLOAD_CAPACITY],
) -> Result<usize, EncodeError>
```

`encode` takes `&[&Event]`, not Sextant's `RingBuffer`. Sextant
materialises the slice from its ring at the call site, so the crate has
no opinion about the caller's event container and `RingBuffer` stays in
Sextant.

The encoder is a pure function: no IO, no clock, no allocator, no
renderer. It writes into a caller-provided fixed-size buffer. Every item
Sextant needs is `pub`, including `event_tlv_bytes` (the single source
of truth for the bytes one event produces) and `ChannelHashes`.

## The decoder returns typed records

`decode` returns a `Digest` whose `raw_records` are typed, so a test can
write

```rust
digest.raw_records.iter().any(|r| matches!(r, Record::PasteReceived { correct: true, .. }))
```

instead of picking bytes apart. `Record` is a type alias for `Event`:
in schema v2 a decoded record and a source event have identical shape
and semantics. If the two ever diverge, the alias becomes its own enum.

Records with a tag the decoder does not know are not an error. They are
collected in `unknown_records` as `UnknownRecord { tag, value }`, so a
decoder built against an older schema still reads a payload with new
tags in the reserved range, and the caller can see what it skipped.

Everything else is strict: a bad magic, an unsupported schema version, a
malformed hash block, a known tag with the wrong length, a value running
into the trailer, or stray bytes before the trailer each return a
`DecodeError`. The decoder reads untrusted screenshots, so it never
panics on malformed input.

## Feature flags

| Feature   | Adds                                         | Who uses it        |
|-----------|----------------------------------------------|--------------------|
| (default) | Encoder only, `no_std`                       | Sextant            |
| `decode`  | Decoder; turns on `std`                      | Ryll (via `qr`)    |
| `qr`      | QR helper via `rqrr` and `image`; `decode`   | Ryll, CLI          |
| `serde`   | `serde::Serialize` on the decoded types      | Ryll, CLI (JSON)   |
| `cli`     | `decode` + `qr` + `serde`                    | `digest-decode`    |

The default build is the encoder alone because the firmware is the
consumer that cannot afford anything else: it has no `std`, and every
dependency it pulls in must build for a UEFI target. Each feature above
the default adds exactly the dependencies its consumers need, so Sextant
never compiles `rqrr` or `image` and Ryll's production builds carry no
digest code unless its own `digest-decode` feature is on.

`ARCHITECTURE.md` holds the matrix of which features are `no_std`-safe;
`AGENTS.md` holds the rules that keep the default build `no_std`.

## QR decoding uses rqrr

`rqrr` is the most mature pure-Rust QR decoder, is maintained, and is
MIT/Apache licensed. Only the `qr` feature pulls it in; Sextant never
depends on it. The helper has two entry points: `decode_qr_rgba` for
Ryll's hot path, which takes a raw framebuffer snapshot, and
`decode_qr_png` for the CLI.

## Distribution

Only `shakenfist-visual-digest` is published to crates.io;
`digest-decode` is `publish = false`. The two consumers take the crate
differently:

- **Sextant** depends on it by `git` URL pinned to a `rev`, with default
  features. A change to the encoder's public API or wire output therefore
  lands here first and is picked up by a matching rev bump in Sextant.
- **Ryll** depends on the crates.io release. Its
  `shakenfist-spice-renderer` crate is itself published, and crates.io
  rejects a dependency that has no version, so the digest crate has to
  be on crates.io too.

Publication was originally deferred on the grounds that a git
dependency was enough; Ryll's need to publish its renderer is what
changed that. The release flow is described in the README.

## Naming and licence

The repository is `shakenfist/visual-digest-rust`, following the
`shakenfist/<thing>-<language>` convention for language-specific
repositories. The crate inside it is `shakenfist-visual-digest`, without
the `-rust` suffix, which would be noise on every `use` line. The
licence is Apache-2.0.

## Toolchain and CI

The Rust toolchain is owned by the Docker image in `.devcontainer/`, and
both local development and CI build inside it, following
shakenfist/ryll's pattern. The wrap is load-bearing: the maintainers run
several Rust projects at different toolchain versions and keep the host
free of a pinned Rust, and the self-hosted CI runners inherit that
constraint. CI checks rustfmt, clippy with `-D warnings`, the full test
suite with all features, and a `--no-default-features` build that proves
the encoder still compiles without `std`.

## History

These decisions were first made in the shakenfist/kerbside test-harness
plans, which have since been retired. This page, not those plans, is the
maintained record. The original text is kept for reference:

- [The crate's phase plan](https://github.com/shakenfist/kerbside/blob/dfaecbffbcae2605b5265b5e3e0e07203a1e37bb/docs/plans/PLAN-test-harness-phase-01-digest-crate.md)
- [The test-harness master plan](https://github.com/shakenfist/kerbside/blob/1da7b7b507a6dfafc235a2eaa8c2cf00519da24e/docs/plans/PLAN-test-harness.md)
