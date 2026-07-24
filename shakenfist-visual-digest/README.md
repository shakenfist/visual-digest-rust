# shakenfist-visual-digest

Encoder and decoder for the visual on-screen digest format used by the
[shakenfist](https://github.com/shakenfist) project for UEFI boot-phase
telemetry: a compact, CRC-protected payload rendered on screen as a QR
code during boot and read back host-side to observe boot progress.

Default features provide the **encoder only**, in `no_std`-compatible
form, so it can run inside UEFI firmware. Optional features unlock
progressively more functionality:

| Feature   | Adds                                                        | `std` |
|-----------|-------------------------------------------------------------|-------|
| *(default)* | Encoder (`encode`, `ChannelHashes`, format constants)     | no    |
| `decode`  | Decoder (`decode`, `Digest`, `Record`)                      | yes   |
| `qr`      | QR locate-and-decode from PNG/RGBA (`decode_qr_png`, …); implies `decode` | yes |
| `serde`   | `serde` derives on the wire types                           | —     |
| `cli`     | Everything above (used by the `digest-decode` bin)          | yes   |

## Consumers

- **[shakenfist/uncalibrated-sextant](https://github.com/shakenfist/uncalibrated-sextant)**
  — UEFI firmware that encodes and renders the digest QR code. Uses the
  default features (encoder only, `no_std`).
- **[shakenfist/ryll](https://github.com/shakenfist/ryll)** — host-side
  test harness that consumes the `qr` and `decode` features to read the
  digest back.

## Wire format

The wire-format specification lives at
[`docs/visual-digest-format.md`](https://github.com/shakenfist/visual-digest-rust/blob/main/docs/visual-digest-format.md)
in the repository.

## License

Apache-2.0
