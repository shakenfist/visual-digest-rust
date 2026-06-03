# ARCHITECTURE.md — visual-digest-rust

## Crate layout (workspace)

This is a flat-at-root workspace (Ryll convention): workspace members
sit directly beside the workspace `Cargo.toml`, with no `crates/`
subdirectory.

```
visual-digest-rust/
├── Cargo.toml                       # workspace manifest only
├── shakenfist-visual-digest/        # library crate
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
└── digest-decode/                   # CLI binary crate
    ├── Cargo.toml
    └── src/
        └── main.rs
```

As the phase 1 steps land, the library will grow additional source
modules:

```
shakenfist-visual-digest/src/
├── lib.rs          (feature gates, re-exports)
├── format.rs       (constants, wire types)
├── events.rs       (Event, Phase, BootloaderChoice)
├── encoder.rs      (encode, event_tlv_bytes)
├── hashes.rs       (ChannelHashes)
├── decoder.rs      (decode, Digest, Record, DecodeError)  [decode feature]
└── qr.rs           (QR locate helpers)                    [qr feature, step 1f]
```

## Feature flag matrix

| Feature   | Adds to the build                       | no_std-safe? |
|-----------|-----------------------------------------|--------------|
| (default) | Encoder (`encoder.rs`, `format.rs`, …) | Yes          |
| `decode`  | Decoder (`decoder.rs`), `thiserror`     | No           |
| `qr`      | QR helpers (`qr.rs`), `rqrr`, `image`  | No           |
| `serde`   | `serde::Serialize` on decoded types     | Yes*         |
| `cli`     | Alias for `decode + qr + serde`         | No           |

*`serde` itself can be `no_std`-compatible, but `serde` is only useful
here alongside `decode` (the types being serialised require `decode`).

The `#![cfg_attr(not(feature = "decode"), no_std)]` directive in
`lib.rs` enforces the no_std constraint at compiler level whenever the
`decode` feature is absent.

## Relationship to Sextant

`shakenfist/uncalibrated-sextant` is the UEFI firmware that encodes
and renders the visual digest. After step 1h it will consume this crate
with default features (encoder only, `no_std`). The dependency is
declared via `git = "https://github.com/shakenfist/visual-digest-rust"`
rather than crates.io (publication deliberately deferred; see plan
decisions).

The encoder API takes `events: &[&Event]` (not a `RingBuffer`) —
Sextant materialises the slice from its ring buffer at the call site.

## Relationship to Ryll

`shakenfist/ryll` is the host-side test harness. It will consume the
`qr` and `decode` features in phase 6. There is no dependency today.

## Decoder public API

The decoder (behind the `decode` feature) exposes:

- `decode(bytes: &[u8]) -> Result<Digest, DecodeError>` — entry point.
- `Digest` — fully decoded payload: `frame_counter`, `channel_hashes`,
  `raw_records: Vec<Record>`, `unknown_records: Vec<UnknownRecord>`,
  `framebuffer_hash`.
- `Record` — type alias for `Event`; record and event have identical shape.
- `UnknownRecord { tag: u8, value: Vec<u8> }` — forward-compat capture
  for tags the decoder doesn't recognise.
- `DecodeError` — `thiserror`-derived enum covering `Short`, `BadMagic`,
  `UnsupportedSchemaVersion`, `RecordCountTooSmall`, `MalformedHashBlock`,
  `MalformedRawRecord`, `TruncatedValue`, `Trailing`.

## QR helper public API

The QR locate-and-decode helper (behind the `qr` feature) exposes:

- `decode_qr_rgba(rgba, width, height) -> Option<Vec<u8>>` — Ryll's hot
  path. Takes a raw RGBA framebuffer snapshot (e.g. from `SurfaceMirror`),
  converts to luma (inverted so bright phosphor-green foreground appears dark
  to rqrr's adaptive thresholding), detects QR grids, returns the first
  decoded byte payload or `None`.
- `decode_qr_png(path) -> Result<Vec<u8>, QrError>` — CLI entry point.
  Opens a PNG via `image::open`, converts to RGBA, delegates to `decode_qr_rgba`.
- `QrError` — `thiserror` enum: `Io`, `Decode(image::ImageError)`, `NoQrFound`.

Raw-bytes decode: `rqrr::Grid::decode_to(&mut Vec<u8>)` writes ECC-corrected,
mode-decoded bytes to the writer, bypassing UTF-8 validation. A 106-byte
binary digest payload survives the round-trip intact.

Sextant renders QR modules as phosphor-green (R=51, G=150, B=51) on black.
This is the inverse of conventional QR polarity (dark modules on white), so
the luma conversion inverts (255 − L) before passing to rqrr.

## Format specification

The wire format spec lives in `docs/visual-digest-format.md`.
