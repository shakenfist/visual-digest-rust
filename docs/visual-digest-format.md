# Visual digest wire format

The visual on-screen digest is a QR code rendered into the
bottom-right corner of the framebuffer. It encodes a fixed-size
TLV payload that an external decoder (e.g. ryll) can read by
screendumping the framebuffer and decoding the QR.

This document is the authoritative wire-format reference for
schema version 2. The sources of truth are:

- `shakenfist-visual-digest/src/encoder.rs` — encoder, all tag
  constants, and `event_tlv_bytes` (the single source of truth
  for what bytes a given event produces on the wire)
- `shakenfist-visual-digest/src/hashes.rs::ChannelHashes` —
  per-channel rolling-hash accumulators and the CRC chaining
  formula
- `shakenfist-visual-digest/src/events.rs` — event variant and
  mini-enum definitions
- `uncalibrated-sextant/src/renderer/mod.rs::Renderer::draw_digest`
  — renderer (Sextant-only; not part of this crate)
- `uncalibrated-sextant/scripts/digest-payload-smoke.sh` —
  host-side decoder reference implementation in Python

Drift between this document and any of the above is a bug.

## QR encoding parameters

- **Version**: 5 (37×37 modules of payload, plus a 4-module
  quiet zone = 45×45 grid, rendered as a 180×180 px region)
- **ECC level**: Low (per the QR Code 2005 spec Table 7,
  V5/L byte-mode capacity = 106 bytes)
- **Mode**: byte
- **Module pixel size**: 4×4 — each QR module is rendered as
  a 4×4 px tile via one `BltOp::BufferToVideo` call
  (Principle 6 of `DESIGN.md`)
- **Mask**: automatic (encoder picks)
- **Colours**: phosphor-green foreground, pure black background
  — matching the rest of the harness palette

## Payload layout

The QR encodes exactly `DIGEST_PAYLOAD_CAPACITY = 106` bytes of
byte-mode data, laid out as four regions in fixed order:

```
[10-byte header]
[48-byte hash block: 8 × 6-byte records, tags 0x11..=0x18]
[≤44-byte raw event records: newest-first selection]
[4-byte trailer: framebuffer CRC32C]
Total ≤ 106 bytes (DIGEST_PAYLOAD_CAPACITY, V5/L byte-mode capacity)
```

The hash block is fixed-size and always present. The raw event
region is variable-length (may be zero bytes when no events have
been recorded yet). The total never exceeds 106 bytes.

### Header (10 bytes, fixed)

| Offset | Length | Field              | Encoding                |
|--------|--------|--------------------|-------------------------|
| 0      | 4      | Magic              | ASCII `SXDG`            |
| 4      | 1      | Schema version     | `u8`, currently `0x02`  |
| 5      | 4      | Frame counter      | `u32` little-endian     |
| 9      | 1      | Record count       | `u8`                    |

**Magic** is `b"SXDG"` (Sextant DiGest). Four exact bytes at
offset 0 let a decoder detecting "this PNG contains a digest"
achieve very high confidence against random noise.

**Schema version** is `0x02`. Version 2 adds the 48-byte hash
block immediately after the header, keeping all v1 raw-event
tags (0x01..=0x08) unchanged and reserving the 0x10..=0x1F
range for new TLV types. Bump when a field shape changes or a
TLV type is repurposed; adding a new tag within the reserved
range does not require a bump.

**Frame counter** is monotonic per boot, starting at `1` for
the first refresh. Wraps at `u32::MAX` (136 years at 1 Hz —
not a real concern).

**Record count** is the total number of TLV records in this
payload: the eight hash records plus however many raw event
records fit. In a v2 payload the count is always ≥ 8. Caps at
255 by field width; in practice the capacity budget limits raw
records far below that.

### Hash block (48 bytes, fixed)

Eight per-channel rolling-hash records in tag-numeric order,
immediately after the header. The block is always 48 bytes
(`NUM_HASH_CHANNELS × RECORD_HASH_SIZE = 8 × 6`). Each record:

| Offset within record | Length | Field         | Encoding              |
|----------------------|--------|---------------|-----------------------|
| 0                    | 1      | Type tag      | `u8`, 0x11..=0x18     |
| 1                    | 1      | Value length  | `u8`, always `4`      |
| 2                    | 4      | CRC32C value  | `u32` little-endian   |

The eight records appear in this order:

| Tag  | Channel                 |
|------|-------------------------|
| 0x11 | `Keypress` rolling hash |
| 0x12 | `LineRendered` rolling hash |
| 0x13 | `SceneTransition` rolling hash |
| 0x14 | `BootloaderDecision` rolling hash |
| 0x15 | `PasteReceived` rolling hash |
| 0x16 | `BootloaderTimeout` rolling hash |
| 0x17 | `ModeSwitch` rolling hash |
| 0x18 | `ModeCycle` rolling hash |

**Hash semantics.** Each value is the rolling CRC32C of every
TLV-encoded event of that variant since boot. The bytes hashed
are exactly what `event_tlv_bytes` emits for each event (tag +
length + value). An unpopulated channel — no events of that
variant have been recorded since boot — carries `0x00000000`,
which is the finalized CRC32C of zero bytes.

#### CRC chaining semantics

The rolling hash uses `CRC_32_ISCSI` (Castagnoli, `init =
0xFFFFFFFF`, `xorout = 0xFFFFFFFF`, `refin = true`, `refout =
true`). To extend a previously-finalized value `f` with new
bytes, the internal pre-finalization state must be recovered
first: `raw = f ^ 0xFFFF_FFFF`. Because `init()` applies
`initial.reverse_bits()` for `refin = true`, the correct
argument to `Crc::digest_with_initial` is
`(f ^ 0xFFFF_FFFF).reverse_bits()`. Update with the new event
bytes, finalize, and store the result. This formula is
implemented as `ChannelHashes::resume_initial` in
`shakenfist-visual-digest/src/hashes.rs` and its correctness is
verified by the chaining assertions in
`uncalibrated-sextant/scripts/digest-payload-smoke.sh` (an
unpopulated channel must hash to zero; a populated channel
must not).

### Raw event records (≤44 bytes, variable)

TLV records, one per event, selected newest-first from the ring
buffer until adding the next would exceed the 44-byte budget.
The encoder emits the selected records in chronological
(forward) order. Each record:

| Offset | Length | Field           | Encoding                  |
|--------|--------|-----------------|---------------------------|
| 0      | 1      | Type tag        | `u8` — see table below    |
| 1      | 1      | Length of value | `u8`                      |
| 2      | N      | Value           | type-specific, see below  |

## Tag table

### Raw event tags (0x01..=0x08)

All integers little-endian. Value length is total record size
minus 2 (the tag and length bytes).

| Tag  | Variant              | Value fields                                                  | Value len | Record size |
|------|----------------------|---------------------------------------------------------------|-----------|-------------|
| 0x01 | `Keypress`           | `u64 timestamp_ms`, `u16 unicode`, `u16 scancode`            | 12        | 14          |
| 0x02 | `LineRendered`       | `u64 timestamp_ms`, `u16 row`                                 | 10        | 12          |
| 0x03 | `SceneTransition`    | `u64 timestamp_ms`, `u8 from_phase`, `u8 to_phase`            | 10        | 12          |
| 0x04 | `BootloaderDecision` | `u64 timestamp_ms`, `u8 choice`, `u32 attempt`                | 13        | 15          |
| 0x05 | `PasteReceived`      | `u64 timestamp_ms`, `u16 len`, `u8 correct`                   | 11        | 13          |
| 0x06 | `BootloaderTimeout`  | `u64 timestamp_ms`                                            | 8         | 10          |
| 0x07 | `ModeSwitch`         | `u64 timestamp_ms`, `u16 req_w`, `u16 req_h`, `u16 app_w`, `u16 app_h` | 16 | 18     |
| 0x08 | `ModeCycle`          | `u64 timestamp_ms`, `u32 count`, `u8 interrupted`             | 13        | 15          |

### Per-channel hash tags (0x11..=0x18)

Value is always a 4-byte CRC32C LE; record size is always 6.

| Tag  | Channel                      | Value len | Record size |
|------|------------------------------|-----------|-------------|
| 0x11 | `Keypress` rolling hash      | 4         | 6           |
| 0x12 | `LineRendered` rolling hash  | 4         | 6           |
| 0x13 | `SceneTransition` rolling hash | 4       | 6           |
| 0x14 | `BootloaderDecision` rolling hash | 4    | 6           |
| 0x15 | `PasteReceived` rolling hash | 4         | 6           |
| 0x16 | `BootloaderTimeout` rolling hash | 4      | 6           |
| 0x17 | `ModeSwitch` rolling hash    | 4         | 6           |
| 0x18 | `ModeCycle` rolling hash     | 4         | 6           |

Tags 0x09..=0x10 and 0x19..=0xFF are reserved.

## Phase and BootloaderChoice wire discriminants

`phase` and `choice` fields use stable `u8` discriminants
chosen by an explicit `match` in the encoder, **not** Rust's
default repr discriminants. The wire values are:

- `PHASE_AWAITING = 0x00`
- `PHASE_BOOTING = 0x01`
- `PHASE_PARKED = 0x02`
- `CHOICE_RECOVER = 0x00`
- `CHOICE_IGNORE = 0x01`
- `CHOICE_ANYWAY = 0x02`

These wire numbers are stable across reorderings of the Rust
enum variants. Reorder the source freely; the wire stays put.

### Trailer (4 bytes, fixed)

| Offset | Length | Field   | Encoding                  |
|--------|--------|---------|---------------------------|
| 0      | 4      | CRC32C  | Castagnoli, little-endian |

The CRC32C is computed over every framebuffer pixel byte
*outside* the digest region — the QR encodes a hash of
everything-on-screen-except-itself (path A). The excluded
rectangle is the runtime right-anchored 180×180 px region at
`(origin_x, origin_y)`, where `origin_x` and `origin_y` derive
from the current GOP mode's width and height (see
`uncalibrated-sextant/src/renderer/mod.rs::Renderer::draw_digest`).

`BltPixel` is 4 bytes (BGRA-ish in uefi-rs 0.37), so a
1024×768 framebuffer is ~3 MB of pixel data to hash. The
read-back goes one scanline at a time via
`BltOp::VideoToBltBuffer` to avoid any framebuffer-sized
allocation; measured cost is ~7 ms per call under OVMF+QXL at
1024×768 (~21.5M cycles at 3 GHz).

## Capacity budget

QR Version 5 / ECC Low byte-mode capacity is **106 bytes**.

| Region            | Size      | Notes                          |
|-------------------|-----------|--------------------------------|
| Header            | 10 bytes  | Fixed                          |
| Hash block        | 48 bytes  | 8 × 6 bytes, always present    |
| Raw event records | ≤44 bytes | Newest-first, variable count   |
| Trailer           | 4 bytes   | Fixed                          |
| **Total**         | **≤106**  | = `DIGEST_PAYLOAD_CAPACITY`    |

The 44-byte raw event budget accommodates roughly 2–4 records
at the 10–18 byte per-record range:

- Worst case (all `ModeSwitch`, 18 B each): **2 records**
- Best case (all `BootloaderTimeout`, 10 B each): **4 records**
- Typical (mixed events, ~12–14 B average): **~3 records**

The encoder takes the *most-recent-N* events that fit, walking
the ring buffer in reverse, then emits the included records in
chronological (forward) order in the QR. The per-channel
rolling hashes carry the complete since-boot summary for each
variant; the raw records provide the most-recent context for
human and tool debugging.

If more raw-record capacity is needed, the documented next step
is QR Version 7 (45×45 modules, 154 B byte-mode capacity at
ECC Medium → 140 bytes after header/trailer/hash-block → ~76
bytes for raw records). Version 7 at 4-pixel modules with
quiet zone runs to 212 px square, which overflows the current
180 px region — switching would require either dropping module
scale to 3 px or enlarging the region. Both are larger changes
than the current scope covers; tracked under *Future work* in
[uncalibrated-sextant/docs/plans/PLAN-visual-digest.md](https://github.com/shakenfist/uncalibrated-sextant/blob/main/docs/plans/PLAN-visual-digest.md).

## Choice of ECC level

The harness uses ECC Low to maximise payload capacity. Low
tolerates ~7% of QR modules being unreadable before the
decoder gives up; Medium tolerates ~15% but caps V5 byte mode
at 84 bytes. The 106-byte capacity is load-bearing — the
encoder relies on it to fit the fixed header, the 48-byte hash
block, several raw event records, and the CRC trailer — so the
ECC trade was worth the lower damage budget.

Practical implication: a future CRT-scruff overlay (see
DESIGN.md) must keep its damage budget conservative in the
digest region, or the QR will stop decoding. The QR sits in
its own non-scruff layer, so the issue is bounded.

## Schema version compatibility

Schema v2 adds the 48-byte hash block immediately after the
10-byte header, keeping all v1 raw-event tags (0x01..=0x08)
unchanged. The reserved 0x10..=0x1F tag range was chosen so
that the eight new hash records (0x11..=0x18) fall where a
v1-only decoder sees unknown tags. A well-behaved v1 decoder
that skips unknown TLV entries will consume the hash block
records without failure and then find the raw event records in
the expected position. A v1 decoder that fails on unknown tags
will reject v2 payloads entirely. See the step-3e finding in
`uncalibrated-sextant/docs/plans/PLAN-continuous-digest-phase-03-closeout.md`
for verification of ryll's specific behaviour.

## Provenance

- Encoder: `shakenfist-visual-digest/src/encoder.rs::encode`
- Event TLV bytes (single source of truth):
  `shakenfist-visual-digest/src/encoder.rs::event_tlv_bytes`
- Event variants: `shakenfist-visual-digest/src/events.rs`
- Per-channel rolling hashes:
  `shakenfist-visual-digest/src/hashes.rs::ChannelHashes`
- Renderer:
  `uncalibrated-sextant/src/renderer/mod.rs::Renderer::draw_digest`
- Framebuffer hash:
  `uncalibrated-sextant/src/renderer/mod.rs::Renderer::crc32c_framebuffer_excluding_digest`
- Smoke harness:
  `uncalibrated-sextant/scripts/digest-payload-smoke.sh`
  (decoder reference implementation in Python)

Update this doc and the source it documents together. Drift
between this document and any source listed above is a bug.
