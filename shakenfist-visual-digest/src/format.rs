//! Wire-format constants, error types, and helper functions for the
//! visual on-screen digest. The full wire-format specification lives
//! at `docs/visual-digest-format.md` in this repository.
//!
//! Everything here is `no_std`-safe: no allocations, no I/O.

use crc::{Crc, CRC_32_ISCSI};

use crate::events::{BootloaderChoice, Event, Phase};

/// Magic identifier for a SeXtant DiGest payload. Four exact bytes at
/// offset 0 let a host-side decoder say "this PNG contains a digest"
/// with very high confidence against random noise.
pub const DIGEST_MAGIC: [u8; 4] = *b"SXDG";

/// Schema version of the wire format. Bumped from 1 to 2 in step 2c
/// to signal that per-channel rolling-hash records (tags 0x11..=0x18)
/// are now present in every payload. Existing raw event tags (0x01..=
/// 0x08) are unchanged; the bump is informational rather than a hard
/// break — a v1-only decoder that encounters v2 records sees unknown
/// tags in the 0x10–0x1F reserved range and should skip them.
pub const DIGEST_SCHEMA_VERSION: u8 = 0x02;

/// TLV type tag: `Event::Keypress`. Parallel to `serial::drain`'s
/// `type=keypress` discriminator.
pub const TAG_KEYPRESS: u8 = 0x01;
/// TLV type tag: `Event::LineRendered`. Parallel to
/// `serial::drain`'s `type=line` discriminator.
pub const TAG_LINE_RENDERED: u8 = 0x02;
/// TLV type tag: `Event::SceneTransition`. Parallel to
/// `serial::drain`'s `type=transition` discriminator.
pub const TAG_SCENE_TRANSITION: u8 = 0x03;
/// TLV type tag: `Event::BootloaderDecision`. Parallel to
/// `serial::drain`'s `type=bootloader_decision` discriminator.
pub const TAG_BOOTLOADER_DECISION: u8 = 0x04;
/// TLV type tag: `Event::PasteReceived`. Parallel to
/// `serial::drain`'s `type=paste` discriminator.
pub const TAG_PASTE_RECEIVED: u8 = 0x05;
/// TLV type tag: `Event::BootloaderTimeout`. Parallel to
/// `serial::drain`'s `type=bootloader_timeout` discriminator.
pub const TAG_BOOTLOADER_TIMEOUT: u8 = 0x06;
/// TLV type tag: `Event::ModeSwitch`. Parallel to `serial::drain`'s
/// `type=mode_switch` discriminator.
pub const TAG_MODE_SWITCH: u8 = 0x07;
/// TLV type tag: `Event::ModeCycle`. Parallel to `serial::drain`'s
/// `type=mode_cycle` discriminator.
pub const TAG_MODE_CYCLE: u8 = 0x08;

/// TLV type tag: per-channel rolling CRC32C hash for `Event::Keypress`.
/// Mirrors `TAG_KEYPRESS` in the 0x10–0x1F reserved range. The value
/// (4 bytes LE) is the CRC32C of every `Keypress` TLV record since boot.
pub const TAG_HASH_KEYPRESS: u8 = 0x11;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::LineRendered`.
/// Mirrors `TAG_LINE_RENDERED` in the 0x10–0x1F reserved range.
pub const TAG_HASH_LINE_RENDERED: u8 = 0x12;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::SceneTransition`.
/// Mirrors `TAG_SCENE_TRANSITION` in the 0x10–0x1F reserved range.
pub const TAG_HASH_SCENE_TRANSITION: u8 = 0x13;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::BootloaderDecision`.
/// Mirrors `TAG_BOOTLOADER_DECISION` in the 0x10–0x1F reserved range.
pub const TAG_HASH_BOOTLOADER_DECISION: u8 = 0x14;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::PasteReceived`.
/// Mirrors `TAG_PASTE_RECEIVED` in the 0x10–0x1F reserved range.
pub const TAG_HASH_PASTE_RECEIVED: u8 = 0x15;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::BootloaderTimeout`.
/// Mirrors `TAG_BOOTLOADER_TIMEOUT` in the 0x10–0x1F reserved range.
pub const TAG_HASH_BOOTLOADER_TIMEOUT: u8 = 0x16;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::ModeSwitch`.
/// Mirrors `TAG_MODE_SWITCH` in the 0x10–0x1F reserved range.
pub const TAG_HASH_MODE_SWITCH: u8 = 0x17;
/// TLV type tag: per-channel rolling CRC32C hash for `Event::ModeCycle`.
/// Mirrors `TAG_MODE_CYCLE` in the 0x10–0x1F reserved range.
pub const TAG_HASH_MODE_CYCLE: u8 = 0x18;

/// On-wire byte size of a single per-channel rolling-hash record:
/// one tag byte + one length byte (always 4) + four CRC32C bytes.
pub const RECORD_HASH_SIZE: usize = 6;

/// Number of per-channel rolling-hash records emitted per payload.
/// One record per `Event` variant; tags 0x11..=0x18 in numeric order.
pub const NUM_HASH_CHANNELS: usize = 8;

/// Wire discriminant for `Phase::Awaiting`.
pub const PHASE_AWAITING: u8 = 0x00;
/// Wire discriminant for `Phase::Booting`.
pub const PHASE_BOOTING: u8 = 0x01;
/// Wire discriminant for `Phase::Parked`.
pub const PHASE_PARKED: u8 = 0x02;
/// Wire discriminant for `Phase::StreamExercise` — fixed-duration
/// scene that runs between `Awaiting` and `Booting`, used to
/// deterministically trigger spice-server's stream-creation
/// heuristic for the upstream SIGSYS reproducer.
pub const PHASE_STREAM_EXERCISE: u8 = 0x03;

/// Wire discriminant for `BootloaderChoice::Retry`.
pub const CHOICE_RECOVER: u8 = 0x00;
/// Wire discriminant for `BootloaderChoice::Ignore`.
pub const CHOICE_IGNORE: u8 = 0x01;
/// Wire discriminant for `BootloaderChoice::Abort`.
pub const CHOICE_ANYWAY: u8 = 0x02;

/// Fixed header length: magic (4) + version (1) + frame counter (4) +
/// record count (1).
pub const DIGEST_HEADER_LEN: usize = 10;
/// Fixed trailer length: framebuffer hash (4).
pub const DIGEST_TRAILER_LEN: usize = 4;
/// Header + trailer overhead = 14 bytes.
pub const DIGEST_FIXED_OVERHEAD: usize = DIGEST_HEADER_LEN + DIGEST_TRAILER_LEN;

/// Maximum QR Version 5 / ECC Low byte-mode capacity, in bytes.
/// Derived from QR Code 2005 spec, Table 7: V5 byte-mode capacity by
/// ECC level is L=106, M=84, Q=60, H=46. The renderer's
/// `draw_digest` configures the encoder for V5/Low to match; if you
/// change one, you must change the other.
/// `uncalibrated-sextant/src/renderer/mod.rs::draw_digest`'s doc
/// comment carries the wider rationale (ECC trade-off, mismatch
/// hazard, panic-on-oversize behaviour).
pub const DIGEST_PAYLOAD_CAPACITY: usize = 106;

// Pin the capacity to the V5/Low spec figure. `QrCodeEcc::Low` in
// `uncalibrated-sextant/src/renderer/mod.rs::draw_digest` is the
// other half of the pairing and is not const-evaluable, so the
// cross-file invariant cannot be enforced in one assertion. This half
// catches the common drift mode: someone raises the capacity (e.g. to
// fit more records) without realising they need to bump the ECC level
// too. Raising past 106 forces a re-read of the table above.
const _: () = assert!(
    DIGEST_PAYLOAD_CAPACITY <= 106,
    "DIGEST_PAYLOAD_CAPACITY exceeds QR Version 5 / ECC Low byte-mode \
     capacity (106 bytes). Either drop the capacity back to 106, or \
     bump the encoder in uncalibrated-sextant/src/renderer/mod.rs::draw_digest \
     to a version/ECC combination that supports the new value (see QR Code \
     2005 spec, Table 7).",
);

/// CRC32C algorithm (Castagnoli polynomial, as used in iSCSI, SCTP,
/// and Btrfs). The `crc` crate computes this with a const table at
/// zero runtime cost beyond the per-byte XOR. Consumed by
/// `uncalibrated-sextant/src/renderer/mod.rs::crc32c_framebuffer_excluding_digest`
/// and by `hashes::ChannelHashes`.
pub const CRC32C: Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);

/// Outcome of an `encode` call.
#[derive(Debug)]
pub enum EncodeError {
    /// Caller supplied a buffer smaller than `DIGEST_PAYLOAD_CAPACITY`.
    /// Currently unreachable because the buffer type carries the
    /// capacity invariant; retained for future flexibility.
    #[allow(dead_code)]
    BufferTooSmall,
    /// Encoder bookkeeping error — should not happen if the per-variant
    /// record sizes are correct. Reported instead of panicking so a
    /// runtime accounting bug degrades to "no digest this frame"
    /// rather than a UEFI crash.
    InternalOverflow,
}

/// Map a `Phase` to its wire discriminant. The match is deliberate —
/// the wire values are stable across format-version bumps and are
/// **not** tied to Rust's default enum discriminants (which the
/// compiler may renumber if variants are reordered, added, or
/// removed). Reorder `Phase` freely; the wire still works.
pub fn phase_wire(phase: Phase) -> u8 {
    match phase {
        Phase::Awaiting => PHASE_AWAITING,
        Phase::StreamExercise => PHASE_STREAM_EXERCISE,
        Phase::Booting => PHASE_BOOTING,
        Phase::Parked => PHASE_PARKED,
    }
}

/// Map a `BootloaderChoice` to its wire discriminant. Same stability
/// rationale as `phase_wire`: the match is the contract, not Rust's
/// default reprs.
pub fn choice_wire(choice: BootloaderChoice) -> u8 {
    match choice {
        BootloaderChoice::Retry => CHOICE_RECOVER,
        BootloaderChoice::Ignore => CHOICE_IGNORE,
        BootloaderChoice::Abort => CHOICE_ANYWAY,
    }
}

/// Total on-wire byte size of a single record (including its 2-byte
/// type-tag + length-of-value overhead). Per the format spec table:
///
/// | Variant              | Bytes |
/// |----------------------|-------|
/// | `Keypress`           | 14    |
/// | `LineRendered`       | 12    |
/// | `SceneTransition`    | 12    |
/// | `BootloaderDecision` | 15    |
/// | `PasteReceived`      | 13    |
/// | `BootloaderTimeout`  | 10    |
/// | `ModeSwitch`         | 18    |
/// | `ModeCycle`          | 15    |
pub fn size_of_record(event: &Event) -> usize {
    match event {
        Event::Keypress { .. } => 14,
        Event::LineRendered { .. } => 12,
        Event::SceneTransition { .. } => 12,
        Event::BootloaderDecision { .. } => 15,
        Event::PasteReceived { .. } => 13,
        Event::BootloaderTimeout { .. } => 10,
        Event::ModeSwitch { .. } => 18,
        Event::ModeCycle { .. } => 15,
    }
}

/// Maximum on-wire size of a single TLV record across all event
/// variants. Used as the stack-buffer size in `event_tlv_bytes`.
/// `ModeSwitch` is the largest at 18 bytes.
pub const MAX_RECORD_SIZE: usize = 18;

// Compile-time guard: keep `MAX_RECORD_SIZE` in step with the
// largest arm of `size_of_record`. `event_tlv_bytes` writes
// per-variant byte offsets directly into `[u8; MAX_RECORD_SIZE]`;
// if a new variant exceeds this and `MAX_RECORD_SIZE` is not
// raised in step, the indexed writes panic on slice bounds. The
// failure mode is a panic-on-bounds (not memory corruption — the
// typed array length is the bound), but a compile-time check
// catches the drift before it ships. If you grow a variant or
// add one, raise the literal here AND in `MAX_RECORD_SIZE` above.
const _: () = assert!(
    MAX_RECORD_SIZE >= 18,
    "MAX_RECORD_SIZE must accommodate the largest size_of_record \
     arm. ModeSwitch is currently the largest at 18 bytes; update \
     both this assertion and MAX_RECORD_SIZE when adding or growing \
     an Event variant.",
);
