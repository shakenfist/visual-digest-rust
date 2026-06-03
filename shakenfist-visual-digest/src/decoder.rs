//! TLV decoder for the visual on-screen digest.
//!
//! Gated on the `decode` feature, which pulls in `alloc` (and `thiserror`
//! for ergonomic error types). The encoder remains `no_std`-safe without
//! this feature.
//!
//! See `docs/visual-digest-format.md` for the full wire-format
//! specification, including header layout, TLV encoding, hash block, and
//! the forward-compatibility rules for unknown tags.

extern crate alloc;
use alloc::vec::Vec;

use crate::events::{BootloaderChoice, Event, Phase};
use crate::format::{
    CHOICE_ANYWAY, CHOICE_IGNORE, CHOICE_RECOVER, DIGEST_HEADER_LEN, DIGEST_MAGIC,
    DIGEST_SCHEMA_VERSION, DIGEST_TRAILER_LEN, NUM_HASH_CHANNELS, PHASE_AWAITING, PHASE_BOOTING,
    PHASE_PARKED, RECORD_HASH_SIZE, TAG_BOOTLOADER_DECISION, TAG_BOOTLOADER_TIMEOUT,
    TAG_HASH_BOOTLOADER_DECISION, TAG_HASH_BOOTLOADER_TIMEOUT, TAG_HASH_KEYPRESS,
    TAG_HASH_LINE_RENDERED, TAG_HASH_MODE_CYCLE, TAG_HASH_MODE_SWITCH, TAG_HASH_PASTE_RECEIVED,
    TAG_HASH_SCENE_TRANSITION, TAG_KEYPRESS, TAG_LINE_RENDERED, TAG_MODE_CYCLE, TAG_MODE_SWITCH,
    TAG_PASTE_RECEIVED, TAG_SCENE_TRANSITION,
};
use crate::hashes::ChannelHashes;

/// Minimum valid payload length: 10 header + 48 hash block + 4 trailer = 62 bytes.
pub const MIN_PAYLOAD: usize =
    DIGEST_HEADER_LEN + (NUM_HASH_CHANNELS * RECORD_HASH_SIZE) + DIGEST_TRAILER_LEN;

// The hash block is always exactly 48 bytes immediately after the header.
const HASH_BLOCK_BYTES: usize = NUM_HASH_CHANNELS * RECORD_HASH_SIZE;

// Offset at which the raw record region begins (after header + hash block).
const RAW_RECORDS_START: usize = DIGEST_HEADER_LEN + HASH_BLOCK_BYTES;

/// A decoded record. This is a type alias for `Event` because the decoded
/// record and source event have identical shape and identical wire semantics
/// in v2. If the format ever diverges between encoder-side and decoder-side
/// representations, this alias can be split into a separate enum.
pub type Record = Event;

/// A TLV record with an unknown tag encountered during decoding.
///
/// When the decoder encounters a tag it doesn't recognise in the raw-record
/// region, it captures the tag byte and value bytes here rather than erroring.
/// This provides forward compatibility: future schema extensions that add new
/// tags in the reserved range are consumed safely by older decoders.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct UnknownRecord {
    /// The unrecognised TLV tag byte.
    pub tag: u8,
    /// The raw value bytes for this record (not including tag or length bytes).
    pub value: Vec<u8>,
}

/// A fully decoded visual digest payload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Digest {
    /// Monotonic frame counter from the header.
    pub frame_counter: u32,
    /// Per-channel rolling CRC32C hash accumulators decoded from the hash block.
    pub channel_hashes: ChannelHashes,
    /// Decoded raw event records from the raw-record region.
    pub raw_records: Vec<Record>,
    /// Unknown TLV records encountered in the raw-record region.
    /// These are captured rather than causing an error, providing forward
    /// compatibility for future tag additions.
    pub unknown_records: Vec<UnknownRecord>,
    /// Framebuffer CRC32C from the trailer (last 4 bytes of the payload).
    pub framebuffer_hash: u32,
}

/// Errors that can occur during digest decoding.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    /// The input is shorter than the minimum valid payload (62 bytes:
    /// 10 header + 48 hash block + 4 trailer).
    #[error("input is {found} bytes; minimum valid payload is {min} bytes",
        found = .found, min = MIN_PAYLOAD)]
    Short { found: usize },

    /// The magic bytes at offset 0 are not `SXDG`.
    #[error("bad magic: expected SXDG, found {found:02x?}")]
    BadMagic { found: [u8; 4] },

    /// The schema version byte (offset 4) is not `0x02`.
    #[error("unsupported schema version {0:#04x}; only 0x02 is supported")]
    UnsupportedSchemaVersion(u8),

    /// The record count (offset 9) is less than 8. In v2 the hash block
    /// always contributes exactly 8 records, so any count < 8 is malformed.
    #[error("record count {found} is less than the minimum 8 (hash block always present in v2)")]
    RecordCountTooSmall { found: u8 },

    /// A hash-block record has the wrong tag, wrong length byte, or runs
    /// off the end of the input.
    #[error("malformed hash block at offset {offset}: {reason}")]
    MalformedHashBlock { offset: usize, reason: &'static str },

    /// A known raw-record tag was found but the value length or value bytes
    /// do not match the specification.
    #[error("malformed raw record tag {tag:#04x} at offset {offset}: {reason}")]
    MalformedRawRecord {
        tag: u8,
        offset: usize,
        reason: &'static str,
    },

    /// A record's declared value length runs past the trailer boundary.
    #[error("record tag {tag:#04x} at offset {offset}: value length runs past the trailer")]
    TruncatedValue { tag: u8, offset: usize },

    /// Bytes remain between the last decoded record and the trailer.
    /// The decoder operates in strict mode and rejects trailing garbage.
    #[error("{extra_bytes} unexpected bytes between last record and trailer")]
    Trailing { extra_bytes: usize },
}

/// Decode a raw digest payload into a typed [`Digest`].
///
/// The decoder is strict: it validates the magic, schema version, hash-block
/// structure, and known-tag value lengths. Unknown tags in the raw-record
/// region are captured in [`Digest::unknown_records`] rather than causing an
/// error (forward compatibility).
///
/// # Errors
///
/// Returns [`DecodeError`] on any structural violation. The decoder never
/// panics on adversarial input.
pub fn decode(bytes: &[u8]) -> Result<Digest, DecodeError> {
    // -------------------------------------------------------------------------
    // 1. Minimum length check.
    // -------------------------------------------------------------------------
    if bytes.len() < MIN_PAYLOAD {
        return Err(DecodeError::Short { found: bytes.len() });
    }

    // -------------------------------------------------------------------------
    // 2. Magic verification.
    // -------------------------------------------------------------------------
    let magic: [u8; 4] = bytes[0..4].try_into().unwrap();
    if magic != DIGEST_MAGIC {
        return Err(DecodeError::BadMagic { found: magic });
    }

    // -------------------------------------------------------------------------
    // 3. Schema version check (offset 4).
    // -------------------------------------------------------------------------
    let schema_version = bytes[4];
    if schema_version != DIGEST_SCHEMA_VERSION {
        return Err(DecodeError::UnsupportedSchemaVersion(schema_version));
    }

    // -------------------------------------------------------------------------
    // 4. Frame counter (LE u32, offsets 5..9).
    // -------------------------------------------------------------------------
    let frame_counter = u32::from_le_bytes(bytes[5..9].try_into().unwrap());

    // -------------------------------------------------------------------------
    // 5. Record count (offset 9).
    // -------------------------------------------------------------------------
    let record_count = bytes[9];
    if record_count < NUM_HASH_CHANNELS as u8 {
        return Err(DecodeError::RecordCountTooSmall {
            found: record_count,
        });
    }

    // -------------------------------------------------------------------------
    // 6. Parse the eight hash records at offsets 10..58.
    //    Each record: [tag u8, 0x04 u8, b0, b1, b2, b3] (6 bytes).
    //    Tags must be 0x11..=0x18 in numeric order.
    // -------------------------------------------------------------------------
    let expected_hash_tags = [
        TAG_HASH_KEYPRESS,
        TAG_HASH_LINE_RENDERED,
        TAG_HASH_SCENE_TRANSITION,
        TAG_HASH_BOOTLOADER_DECISION,
        TAG_HASH_PASTE_RECEIVED,
        TAG_HASH_BOOTLOADER_TIMEOUT,
        TAG_HASH_MODE_SWITCH,
        TAG_HASH_MODE_CYCLE,
    ];

    let mut hash_values = [0u32; NUM_HASH_CHANNELS];
    for (i, &expected_tag) in expected_hash_tags.iter().enumerate() {
        let offset = DIGEST_HEADER_LEN + i * RECORD_HASH_SIZE;
        // Bounds: guaranteed by MIN_PAYLOAD check above. For i < 8,
        // offset + 6 <= 10 + 8*6 = 58 <= MIN_PAYLOAD = 62 <= bytes.len().
        let tag = bytes[offset];
        if tag != expected_tag {
            return Err(DecodeError::MalformedHashBlock {
                offset,
                reason: "hash record has unexpected tag",
            });
        }
        let len = bytes[offset + 1];
        if len != 4 {
            return Err(DecodeError::MalformedHashBlock {
                offset,
                reason: "hash record length byte is not 4",
            });
        }
        hash_values[i] = u32::from_le_bytes(bytes[offset + 2..offset + 6].try_into().unwrap());
    }

    let channel_hashes = ChannelHashes {
        keypress: hash_values[0],
        line_rendered: hash_values[1],
        scene_transition: hash_values[2],
        bootloader_decision: hash_values[3],
        paste_received: hash_values[4],
        bootloader_timeout: hash_values[5],
        mode_switch: hash_values[6],
        mode_cycle: hash_values[7],
    };

    // -------------------------------------------------------------------------
    // 7. Parse raw records starting at offset 58 until 4 bytes before end.
    //    Trailer occupies the last DIGEST_TRAILER_LEN = 4 bytes.
    // -------------------------------------------------------------------------
    let trailer_start = bytes.len().checked_sub(DIGEST_TRAILER_LEN).unwrap();
    let mut pos = RAW_RECORDS_START; // 58
    let mut raw_records: Vec<Record> = Vec::new();
    let mut unknown_records: Vec<UnknownRecord> = Vec::new();

    while pos < trailer_start {
        // Need at least 2 bytes for tag + length.
        if pos + 2 > trailer_start {
            // Single orphan byte between records and trailer — treat as trailing.
            return Err(DecodeError::Trailing {
                extra_bytes: trailer_start - pos,
            });
        }

        let tag = bytes[pos];
        let value_len = bytes[pos + 1] as usize;

        // Check that the declared value bytes don't run past the trailer.
        let value_start = pos + 2;
        let value_end = value_start
            .checked_add(value_len)
            .ok_or(DecodeError::TruncatedValue { tag, offset: pos })?;
        if value_end > trailer_start {
            return Err(DecodeError::TruncatedValue { tag, offset: pos });
        }

        let value_bytes = &bytes[value_start..value_end];

        match tag {
            TAG_KEYPRESS => {
                // Expected value length: 12 (timestamp u64 + unicode u16 + scancode u16).
                if value_len != 12 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "Keypress value length must be 12",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let unicode_u16 = u16::from_le_bytes(value_bytes[8..10].try_into().unwrap());
                let scancode = u16::from_le_bytes(value_bytes[10..12].try_into().unwrap());
                // Convert u16 back to char; use REPLACEMENT CHARACTER for invalid code points.
                // The encoder narrows char to u16 (see encoder.rs), so out-of-range values
                // would only appear from adversarial or corrupted payloads.
                let unicode =
                    char::from_u32(unicode_u16 as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
                raw_records.push(Event::Keypress {
                    unicode,
                    scancode,
                    timestamp_ms,
                });
            }
            TAG_LINE_RENDERED => {
                // Expected value length: 10 (timestamp u64 + row u16).
                if value_len != 10 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "LineRendered value length must be 10",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let row = u16::from_le_bytes(value_bytes[8..10].try_into().unwrap()) as usize;
                raw_records.push(Event::LineRendered { row, timestamp_ms });
            }
            TAG_SCENE_TRANSITION => {
                // Expected value length: 10 (timestamp u64 + from u8 + to u8).
                if value_len != 10 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "SceneTransition value length must be 10",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let from = decode_phase(value_bytes[8]).ok_or(DecodeError::MalformedRawRecord {
                    tag,
                    offset: pos,
                    reason: "SceneTransition: unknown from_phase discriminant",
                })?;
                let to = decode_phase(value_bytes[9]).ok_or(DecodeError::MalformedRawRecord {
                    tag,
                    offset: pos,
                    reason: "SceneTransition: unknown to_phase discriminant",
                })?;
                raw_records.push(Event::SceneTransition {
                    from,
                    to,
                    timestamp_ms,
                });
            }
            TAG_BOOTLOADER_DECISION => {
                // Expected value length: 13 (timestamp u64 + choice u8 + attempt u32).
                if value_len != 13 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "BootloaderDecision value length must be 13",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let choice = decode_bootloader_choice(value_bytes[8]).ok_or(
                    DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "BootloaderDecision: unknown choice discriminant",
                    },
                )?;
                let attempt = u32::from_le_bytes(value_bytes[9..13].try_into().unwrap());
                raw_records.push(Event::BootloaderDecision {
                    choice,
                    attempt,
                    timestamp_ms,
                });
            }
            TAG_PASTE_RECEIVED => {
                // Expected value length: 11 (timestamp u64 + len u16 + correct u8).
                if value_len != 11 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "PasteReceived value length must be 11",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let len = u16::from_le_bytes(value_bytes[8..10].try_into().unwrap()) as usize;
                let correct = value_bytes[10] != 0;
                raw_records.push(Event::PasteReceived {
                    len,
                    correct,
                    timestamp_ms,
                });
            }
            TAG_BOOTLOADER_TIMEOUT => {
                // Expected value length: 8 (timestamp u64 only).
                if value_len != 8 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "BootloaderTimeout value length must be 8",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                raw_records.push(Event::BootloaderTimeout { timestamp_ms });
            }
            TAG_MODE_SWITCH => {
                // Expected value length: 16 (timestamp u64 + req_w/h u16 + app_w/h u16).
                if value_len != 16 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "ModeSwitch value length must be 16",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let requested_w = u16::from_le_bytes(value_bytes[8..10].try_into().unwrap()) as u32;
                let requested_h =
                    u16::from_le_bytes(value_bytes[10..12].try_into().unwrap()) as u32;
                let applied_w = u16::from_le_bytes(value_bytes[12..14].try_into().unwrap()) as u32;
                let applied_h = u16::from_le_bytes(value_bytes[14..16].try_into().unwrap()) as u32;
                raw_records.push(Event::ModeSwitch {
                    requested_w,
                    requested_h,
                    applied_w,
                    applied_h,
                    timestamp_ms,
                });
            }
            TAG_MODE_CYCLE => {
                // Expected value length: 13 (timestamp u64 + count u32 + interrupted u8).
                if value_len != 13 {
                    return Err(DecodeError::MalformedRawRecord {
                        tag,
                        offset: pos,
                        reason: "ModeCycle value length must be 13",
                    });
                }
                let timestamp_ms = u64::from_le_bytes(value_bytes[0..8].try_into().unwrap());
                let count = u32::from_le_bytes(value_bytes[8..12].try_into().unwrap());
                let interrupted = value_bytes[12] != 0;
                raw_records.push(Event::ModeCycle {
                    count,
                    interrupted,
                    timestamp_ms,
                });
            }
            // Any other tag is unknown — capture it for forward compatibility.
            // Per the spec, tags 0x09..=0x10 and 0x19..=0xFF are reserved.
            _ => {
                unknown_records.push(UnknownRecord {
                    tag,
                    value: value_bytes.to_vec(),
                });
            }
        }

        // Advance past this record (2 overhead bytes + value bytes).
        pos = value_end;
    }

    // -------------------------------------------------------------------------
    // 8. Strict trailing-bytes check: pos must be exactly at trailer_start.
    // -------------------------------------------------------------------------
    if pos != trailer_start {
        return Err(DecodeError::Trailing {
            extra_bytes: trailer_start - pos,
        });
    }

    // -------------------------------------------------------------------------
    // 9. Trailer: last 4 bytes = framebuffer_hash (LE u32).
    // -------------------------------------------------------------------------
    let framebuffer_hash =
        u32::from_le_bytes(bytes[trailer_start..trailer_start + 4].try_into().unwrap());

    Ok(Digest {
        frame_counter,
        channel_hashes,
        raw_records,
        unknown_records,
        framebuffer_hash,
    })
}

/// Map a wire `u8` discriminant to a [`Phase`] variant.
#[inline]
fn decode_phase(byte: u8) -> Option<Phase> {
    match byte {
        PHASE_AWAITING => Some(Phase::Awaiting),
        PHASE_BOOTING => Some(Phase::Booting),
        PHASE_PARKED => Some(Phase::Parked),
        _ => None,
    }
}

/// Map a wire `u8` discriminant to a [`BootloaderChoice`] variant.
#[inline]
fn decode_bootloader_choice(byte: u8) -> Option<BootloaderChoice> {
    match byte {
        CHOICE_RECOVER => Some(BootloaderChoice::Retry),
        CHOICE_IGNORE => Some(BootloaderChoice::Ignore),
        CHOICE_ANYWAY => Some(BootloaderChoice::Abort),
        _ => None,
    }
}
