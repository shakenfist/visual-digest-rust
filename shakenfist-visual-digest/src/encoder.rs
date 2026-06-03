//! TLV encoder for the visual on-screen digest.
//!
//! The encoder is a pure function over a caller-provided slice of
//! event references, a monotonic frame counter, and an injected
//! `framebuffer_hash`. It writes a fixed-size payload into a
//! caller-provided stack buffer. No I/O, no clock reads, no allocator.
//!
//! See `docs/visual-digest-format.md` for the full wire-format
//! specification, including header layout, TLV encoding, and the
//! rolling-hash channel algebra.

use crate::events::Event;
use crate::format::{choice_wire, phase_wire, size_of_record, EncodeError};
use crate::format::{
    DIGEST_FIXED_OVERHEAD, DIGEST_HEADER_LEN, DIGEST_MAGIC, DIGEST_PAYLOAD_CAPACITY,
    DIGEST_SCHEMA_VERSION, DIGEST_TRAILER_LEN, MAX_RECORD_SIZE, NUM_HASH_CHANNELS,
    RECORD_HASH_SIZE, TAG_BOOTLOADER_DECISION, TAG_BOOTLOADER_TIMEOUT,
    TAG_HASH_BOOTLOADER_DECISION, TAG_HASH_BOOTLOADER_TIMEOUT, TAG_HASH_KEYPRESS,
    TAG_HASH_LINE_RENDERED, TAG_HASH_MODE_CYCLE, TAG_HASH_MODE_SWITCH, TAG_HASH_PASTE_RECEIVED,
    TAG_HASH_SCENE_TRANSITION, TAG_KEYPRESS, TAG_LINE_RENDERED, TAG_MODE_CYCLE, TAG_MODE_SWITCH,
    TAG_PASTE_RECEIVED, TAG_SCENE_TRANSITION,
};
use crate::hashes::ChannelHashes;

/// Encode a digest payload into `out`. Returns the number of bytes written.
///
/// Callers must provide `events` as a slice of event references in
/// **chronological (forward) order** — oldest event first, newest last.
/// Sextant calls this with the contents of its `RingBuffer<256>`
/// materialised into a stack array of `&Event` using `ring.iter()`.
///
/// Algorithm:
///
/// 1. Walk `events` newest-to-oldest, summing per-record sizes, until
///    adding one more would exceed the raw-event record budget (44 bytes
///    after subtracting the 48-byte rolling-hash block). Remember the
///    oldest included index.
/// 2. Write the 10-byte header: magic, version, frame counter (u32
///    LE), total record count (u8, hash records + raw records).
/// 3. Write the 8 per-channel rolling-hash records in tag-numeric
///    order (0x11..=0x18), immediately after the header. Each record
///    is 6 bytes: tag (1) + len=4 (1) + CRC32C value (4, LE).
/// 4. Write the included raw-event records in chronological (forward)
///    order.
/// 5. Write the 4-byte trailer: `framebuffer_hash` as u32 LE.
///
/// Wire layout (v2):
///   [10-byte header]
///   [8 × 6-byte hash records   = 48 bytes]
///   [raw event records          ≤ 44 bytes]
///   [4-byte trailer]
///   Total ≤ 106 bytes (DIGEST_PAYLOAD_CAPACITY, V5/L).
///
/// Pure function: no IO, no clock reads, no `&mut Renderer`.
pub fn encode(
    events: &[&Event],
    frame_counter: u32,
    framebuffer_hash: u32,
    channel_hashes: &ChannelHashes,
    out: &mut [u8; DIGEST_PAYLOAD_CAPACITY],
) -> Result<usize, EncodeError> {
    // Walk newest-to-oldest, selecting the most-recent run of events
    // that fits in the raw-event record budget. The hash block (8 ×
    // RECORD_HASH_SIZE = 48 bytes) is deducted from the budget so
    // raw records never displace hash records.
    const HASH_BLOCK_BYTES: usize = NUM_HASH_CHANNELS * RECORD_HASH_SIZE; // 48
    const RECORD_BUDGET: usize = DIGEST_PAYLOAD_CAPACITY - DIGEST_FIXED_OVERHEAD - HASH_BLOCK_BYTES; // 44
    let mut record_bytes: usize = 0;
    let mut included: usize = 0;
    for event in events.iter().rev() {
        let sz = size_of_record(event);
        if record_bytes + sz > RECORD_BUDGET {
            break;
        }
        record_bytes += sz;
        included += 1;
        if included == 255 {
            // Record count is a u8; cap at 255 - NUM_HASH_CHANNELS to
            // leave space for the hash records in the count field.
            break;
        }
    }

    let first_idx = events.len() - included;
    // Total record count for the header includes both hash records and
    // raw event records.
    let total_records = NUM_HASH_CHANNELS + included;
    let total_len = DIGEST_HEADER_LEN + HASH_BLOCK_BYTES + record_bytes + DIGEST_TRAILER_LEN;
    if total_len > out.len() {
        return Err(EncodeError::InternalOverflow);
    }

    // Header.
    let mut pos: usize = 0;
    out[pos..pos + 4].copy_from_slice(&DIGEST_MAGIC);
    pos += 4;
    out[pos] = DIGEST_SCHEMA_VERSION;
    pos += 1;
    out[pos..pos + 4].copy_from_slice(&frame_counter.to_le_bytes());
    pos += 4;
    // Record count covers hash records + raw event records.
    out[pos] = total_records as u8;
    pos += 1;

    // Per-channel rolling-hash records in tag-numeric order
    // (0x11..=0x18). Each record: tag (1) + len=4 (1) + hash (4 LE).
    // These appear before raw event records so they survive any
    // capacity constraint; the raw-event budget is already reduced
    // by HASH_BLOCK_BYTES above.
    let hash_channels: [(u8, u32); NUM_HASH_CHANNELS] = [
        (TAG_HASH_KEYPRESS, channel_hashes.keypress),
        (TAG_HASH_LINE_RENDERED, channel_hashes.line_rendered),
        (TAG_HASH_SCENE_TRANSITION, channel_hashes.scene_transition),
        (
            TAG_HASH_BOOTLOADER_DECISION,
            channel_hashes.bootloader_decision,
        ),
        (TAG_HASH_PASTE_RECEIVED, channel_hashes.paste_received),
        (
            TAG_HASH_BOOTLOADER_TIMEOUT,
            channel_hashes.bootloader_timeout,
        ),
        (TAG_HASH_MODE_SWITCH, channel_hashes.mode_switch),
        (TAG_HASH_MODE_CYCLE, channel_hashes.mode_cycle),
    ];
    for (tag, hash) in &hash_channels {
        if pos + RECORD_HASH_SIZE > out.len() {
            return Err(EncodeError::InternalOverflow);
        }
        out[pos] = *tag;
        out[pos + 1] = 4; // length of value field
        out[pos + 2..pos + 6].copy_from_slice(&hash.to_le_bytes());
        pos += RECORD_HASH_SIZE;
    }

    // Raw event records, in chronological (forward) order.
    for event in &events[first_idx..] {
        pos = write_record(event, out, pos)?;
    }

    // Trailer: framebuffer hash, u32 LE.
    if pos + DIGEST_TRAILER_LEN > out.len() {
        return Err(EncodeError::InternalOverflow);
    }
    out[pos..pos + 4].copy_from_slice(&framebuffer_hash.to_le_bytes());
    pos += 4;

    Ok(pos)
}

/// Encode a single event as a TLV record into `buf`. Returns the
/// number of bytes written. The buffer must be at least
/// `MAX_RECORD_SIZE` bytes (18); the caller provides it as a fixed
/// `[u8; MAX_RECORD_SIZE]` so the size invariant is enforced at
/// compile time.
///
/// This is the single source of truth for "what bytes does a given
/// event produce on the wire". Both `write_record` (the encoder) and
/// `hashes::ChannelHashes::update` (the rolling-hash updater) call
/// through here so the bytes they see are identical by construction —
/// drift between "what was encoded" and "what was hashed" is prevented
/// at the API boundary rather than by convention.
pub fn event_tlv_bytes(event: &Event, buf: &mut [u8; MAX_RECORD_SIZE]) -> usize {
    let total = size_of_record(event);
    let value_len = (total - 2) as u8;
    match event {
        Event::Keypress {
            unicode,
            scancode,
            timestamp_ms,
        } => {
            buf[0] = TAG_KEYPRESS;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            // `char as u32` then truncate to u16. Production input is
            // ASCII; supplementary-plane code points would be lossy but
            // this codebase never emits them.
            let unicode_u16 = (*unicode as u32) as u16;
            buf[10..12].copy_from_slice(&unicode_u16.to_le_bytes());
            buf[12..14].copy_from_slice(&scancode.to_le_bytes());
            14
        }
        Event::LineRendered { row, timestamp_ms } => {
            buf[0] = TAG_LINE_RENDERED;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            let row_u16 = *row as u16;
            buf[10..12].copy_from_slice(&row_u16.to_le_bytes());
            12
        }
        Event::SceneTransition {
            from,
            to,
            timestamp_ms,
        } => {
            buf[0] = TAG_SCENE_TRANSITION;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            buf[10] = phase_wire(*from);
            buf[11] = phase_wire(*to);
            12
        }
        Event::BootloaderDecision {
            choice,
            attempt,
            timestamp_ms,
        } => {
            buf[0] = TAG_BOOTLOADER_DECISION;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            buf[10] = choice_wire(*choice);
            buf[11..15].copy_from_slice(&attempt.to_le_bytes());
            15
        }
        Event::PasteReceived {
            len,
            correct,
            timestamp_ms,
        } => {
            buf[0] = TAG_PASTE_RECEIVED;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            let len_u16 = *len as u16;
            buf[10..12].copy_from_slice(&len_u16.to_le_bytes());
            buf[12] = u8::from(*correct);
            13
        }
        Event::BootloaderTimeout { timestamp_ms } => {
            buf[0] = TAG_BOOTLOADER_TIMEOUT;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            10
        }
        Event::ModeSwitch {
            requested_w,
            requested_h,
            applied_w,
            applied_h,
            timestamp_ms,
        } => {
            buf[0] = TAG_MODE_SWITCH;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            // OVMF modes are well under u16::MAX; truncation is safe.
            let rw = *requested_w as u16;
            let rh = *requested_h as u16;
            let aw = *applied_w as u16;
            let ah = *applied_h as u16;
            buf[10..12].copy_from_slice(&rw.to_le_bytes());
            buf[12..14].copy_from_slice(&rh.to_le_bytes());
            buf[14..16].copy_from_slice(&aw.to_le_bytes());
            buf[16..18].copy_from_slice(&ah.to_le_bytes());
            18
        }
        Event::ModeCycle {
            count,
            interrupted,
            timestamp_ms,
        } => {
            buf[0] = TAG_MODE_CYCLE;
            buf[1] = value_len;
            buf[2..10].copy_from_slice(&timestamp_ms.to_le_bytes());
            buf[10..14].copy_from_slice(&count.to_le_bytes());
            buf[14] = u8::from(*interrupted);
            15
        }
    }
}

/// Write a single TLV record into `out` starting at `pos`. Returns
/// the new write position. Errors with `InternalOverflow` if the
/// caller's bookkeeping was off (should never happen — `encode`
/// validates the total length up front).
fn write_record(
    event: &Event,
    out: &mut [u8; DIGEST_PAYLOAD_CAPACITY],
    pos: usize,
) -> Result<usize, EncodeError> {
    let total = size_of_record(event);
    if pos + total > out.len() {
        return Err(EncodeError::InternalOverflow);
    }
    let mut buf = [0u8; MAX_RECORD_SIZE];
    let written = event_tlv_bytes(event, &mut buf);
    out[pos..pos + written].copy_from_slice(&buf[..written]);
    Ok(pos + written)
}

#[cfg(test)]
mod tests {
    //! Host-side unit tests for the pure pieces of the digest module.
    //!
    //! These cover the encoder regression net for `event_tlv_bytes`
    //! (one per `Event` variant, asserting exact wire bytes) and the
    //! CRC32C chaining math used by `ChannelHashes::extend` /
    //! `ChannelHashes::resume_initial`. The QEMU digest-payload smoke
    //! still gates UEFI-side behaviour; these tests localise failures
    //! in the pure functions so a regression there doesn't masquerade
    //! as a renderer or scene bug.
    use super::*;
    use crate::events::{BootloaderChoice, Event, Phase};
    use crate::format::{CHOICE_IGNORE, CRC32C, PHASE_AWAITING, PHASE_BOOTING};
    use crate::hashes::ChannelHashes;

    /// `Keypress` TLV: tag 0x01, len 0x0c, timestamp_ms LE (8),
    /// unicode u16 LE (2), scancode u16 LE (2). Total 14 bytes.
    #[test]
    fn keypress_encodes_to_expected_bytes() {
        let event = Event::Keypress {
            unicode: 'A',
            scancode: 0x1234,
            timestamp_ms: 0x0102_0304_0506_0708,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 14);
        assert_eq!(
            &buf[..14],
            &[
                TAG_KEYPRESS,
                0x0c, // value length: total (14) - tag/len overhead (2)
                0x08,
                0x07,
                0x06,
                0x05,
                0x04,
                0x03,
                0x02,
                0x01, // timestamp LE
                0x41,
                0x00, // 'A' as u16 LE
                0x34,
                0x12, // scancode 0x1234 LE
            ]
        );
    }

    /// `LineRendered` TLV: tag 0x02, len 0x0a, timestamp_ms LE (8),
    /// row u16 LE (2). Total 12 bytes.
    #[test]
    fn line_rendered_encodes_to_expected_bytes() {
        let event = Event::LineRendered {
            row: 0x00ab,
            timestamp_ms: 0x1122_3344_5566_7788,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 12);
        assert_eq!(
            &buf[..12],
            &[
                TAG_LINE_RENDERED,
                0x0a,
                0x88,
                0x77,
                0x66,
                0x55,
                0x44,
                0x33,
                0x22,
                0x11, // timestamp LE
                0xab,
                0x00, // row LE
            ]
        );
    }

    /// `SceneTransition` TLV: tag 0x03, len 0x0a, timestamp_ms LE (8),
    /// from u8, to u8. Total 12 bytes. Phase discriminants are matched
    /// by `phase_wire` (Awaiting=0, Booting=1, Parked=2).
    #[test]
    fn scene_transition_encodes_to_expected_bytes() {
        let event = Event::SceneTransition {
            from: Phase::Awaiting,
            to: Phase::Booting,
            timestamp_ms: 0x0000_0000_0000_002a,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 12);
        assert_eq!(
            &buf[..12],
            &[
                TAG_SCENE_TRANSITION,
                0x0a,
                0x2a,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00, // timestamp LE
                PHASE_AWAITING,
                PHASE_BOOTING,
            ]
        );
    }

    /// `BootloaderDecision` TLV: tag 0x04, len 0x0d, timestamp_ms LE
    /// (8), choice u8, attempt u32 LE. Total 15 bytes.
    #[test]
    fn bootloader_decision_encodes_to_expected_bytes() {
        let event = Event::BootloaderDecision {
            choice: BootloaderChoice::Ignore,
            attempt: 0xdead_beef,
            timestamp_ms: 0x0000_0000_0000_0001,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 15);
        assert_eq!(
            &buf[..15],
            &[
                TAG_BOOTLOADER_DECISION,
                0x0d,
                0x01,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00, // timestamp LE
                CHOICE_IGNORE,
                0xef,
                0xbe,
                0xad,
                0xde, // attempt LE
            ]
        );
    }

    /// `PasteReceived` TLV: tag 0x05, len 0x0b, timestamp_ms LE (8),
    /// len u16 LE, correct u8. Total 13 bytes.
    #[test]
    fn paste_received_encodes_to_expected_bytes() {
        let event = Event::PasteReceived {
            len: 0x0102,
            correct: true,
            timestamp_ms: 0x0000_0000_0000_0009,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 13);
        assert_eq!(
            &buf[..13],
            &[
                TAG_PASTE_RECEIVED,
                0x0b,
                0x09,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00, // timestamp LE
                0x02,
                0x01, // len LE
                0x01, // correct
            ]
        );
    }

    /// `BootloaderTimeout` TLV: tag 0x06, len 0x08, timestamp_ms LE
    /// (8). Total 10 bytes.
    #[test]
    fn bootloader_timeout_encodes_to_expected_bytes() {
        let event = Event::BootloaderTimeout {
            timestamp_ms: 0xffff_ffff_ffff_ffff,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 10);
        assert_eq!(
            &buf[..10],
            &[
                TAG_BOOTLOADER_TIMEOUT,
                0x08,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff, // timestamp LE
            ]
        );
    }

    /// `ModeSwitch` TLV: tag 0x07, len 0x10, timestamp_ms LE (8),
    /// requested_w/h u16 LE, applied_w/h u16 LE. Total 18 bytes.
    #[test]
    fn mode_switch_encodes_to_expected_bytes() {
        let event = Event::ModeSwitch {
            requested_w: 1024,
            requested_h: 768,
            applied_w: 800,
            applied_h: 600,
            timestamp_ms: 0x0000_0000_0000_0003,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 18);
        assert_eq!(
            &buf[..18],
            &[
                TAG_MODE_SWITCH,
                0x10,
                0x03,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00, // timestamp LE
                0x00,
                0x04, // requested_w 1024 LE
                0x00,
                0x03, // requested_h 768 LE
                0x20,
                0x03, // applied_w 800 LE
                0x58,
                0x02, // applied_h 600 LE
            ]
        );
    }

    /// `ModeCycle` TLV: tag 0x08, len 0x0d, timestamp_ms LE (8),
    /// count u32 LE, interrupted u8. Total 15 bytes.
    #[test]
    fn mode_cycle_encodes_to_expected_bytes() {
        let event = Event::ModeCycle {
            count: 0x0000_0007,
            interrupted: false,
            timestamp_ms: 0x0000_0000_0000_0005,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let written = event_tlv_bytes(&event, &mut buf);
        assert_eq!(written, 15);
        assert_eq!(
            &buf[..15],
            &[
                TAG_MODE_CYCLE,
                0x0d,
                0x05,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00, // timestamp LE
                0x07,
                0x00,
                0x00,
                0x00, // count LE
                0x00, // interrupted = false
            ]
        );
    }

    /// `resume_initial` must round-trip: a `Digest` resumed from a
    /// finalized value `f` and fed zero bytes must re-finalize to
    /// `f`. This is the algebraic identity that proves the formula
    /// `(f ^ 0xFFFF_FFFF).reverse_bits()` correct against the
    /// `CRC_32_ISCSI` parameters (`refin=true`, `refout=true`,
    /// `xorout=0xFFFF_FFFF`).
    #[test]
    fn resume_initial_round_trips_finalized_value() {
        // A spread of representative values: empty-stream sentinel,
        // a small one, a typical 32-bit value, and the all-ones edge.
        for f in [0x0000_0000_u32, 0x0000_0001, 0xdead_beef, 0xffff_ffff] {
            let mut d = CRC32C.digest_with_initial(ChannelHashes::resume_initial(f));
            d.update(&[]);
            assert_eq!(d.finalize(), f, "round-trip failed for f=0x{:08x}", f);
        }
    }

    /// CRC32C chaining must agree with a single-pass CRC32C over the
    /// concatenated bytes. Split a known byte string at every
    /// internal boundary and verify that resuming from the first
    /// half's finalized value, then feeding the second half, yields
    /// the same result as a single-pass digest of the whole string.
    /// This guards `ChannelHashes::extend`'s "resume + feed" pattern
    /// against any future drift in the `resume_initial` formula.
    #[test]
    fn chained_crc32c_matches_single_pass() {
        // "123456789" is the canonical CRC test vector; CRC32C
        // (CRC_32_ISCSI) of it is 0xe3069283. We don't hard-code that
        // here — `Crc::checksum` of the whole string is the oracle.
        let message: &[u8] = b"123456789";
        let one_shot = CRC32C.checksum(message);

        for split in 0..=message.len() {
            let (left, right) = message.split_at(split);
            let left_finalized = {
                let mut d = CRC32C.digest();
                d.update(left);
                d.finalize()
            };
            let chained = {
                let mut d =
                    CRC32C.digest_with_initial(ChannelHashes::resume_initial(left_finalized));
                d.update(right);
                d.finalize()
            };
            assert_eq!(
                chained, one_shot,
                "chained digest mismatch at split={}: chained=0x{:08x} expected=0x{:08x}",
                split, chained, one_shot,
            );
        }
    }

    /// `ChannelHashes::extend` over a single event must equal a
    /// fresh CRC32C of that event's TLV bytes. This pins the
    /// "extend == CRC32C over TLV bytes" contract that the on-wire
    /// per-channel hash records depend on.
    #[test]
    fn extend_single_event_matches_one_shot_crc() {
        let event = Event::Keypress {
            unicode: 'k',
            scancode: 0x0042,
            timestamp_ms: 0x0000_0000_1234_5678,
        };
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let len = event_tlv_bytes(&event, &mut buf);

        let expected = CRC32C.checksum(&buf[..len]);
        let actual = ChannelHashes::extend(0, &event);
        assert_eq!(actual, expected);
    }

    /// `ChannelHashes::extend` over two events must equal a fresh
    /// CRC32C over the concatenated TLV bytes. This is the chaining
    /// property in production form: two `extend` calls compose into
    /// one running hash over the per-channel byte stream.
    #[test]
    fn extend_two_events_matches_concatenated_one_shot_crc() {
        let first = Event::Keypress {
            unicode: 'a',
            scancode: 0x0001,
            timestamp_ms: 0x0000_0000_0000_0010,
        };
        let second = Event::Keypress {
            unicode: 'b',
            scancode: 0x0002,
            timestamp_ms: 0x0000_0000_0000_0020,
        };

        let mut buf1 = [0u8; MAX_RECORD_SIZE];
        let len1 = event_tlv_bytes(&first, &mut buf1);
        let mut buf2 = [0u8; MAX_RECORD_SIZE];
        let len2 = event_tlv_bytes(&second, &mut buf2);

        let mut concat = [0u8; MAX_RECORD_SIZE * 2];
        concat[..len1].copy_from_slice(&buf1[..len1]);
        concat[len1..len1 + len2].copy_from_slice(&buf2[..len2]);
        let expected = CRC32C.checksum(&concat[..len1 + len2]);

        let after_first = ChannelHashes::extend(0, &first);
        let after_second = ChannelHashes::extend(after_first, &second);

        assert_eq!(after_second, expected);
    }

    /// Sanity check against a precomputed CRC32C reference value.
    /// "123456789" -> 0xe3069283 (per the CRC_32_ISCSI / CRC-32C
    /// reference in the CRC catalogue). If the underlying `crc`
    /// crate ever silently swapped algorithms, this test fails
    /// loudly rather than letting downstream chaining tests pass
    /// against an off-spec checksum.
    #[test]
    fn crc32c_known_vector() {
        assert_eq!(CRC32C.checksum(b"123456789"), 0xe306_9283);
    }
}
