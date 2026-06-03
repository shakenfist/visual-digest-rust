//! Golden-fixture regression tests for the visual-digest encoder.
//!
//! Each test constructs a deterministic event sequence, calls `encode()`,
//! and asserts byte-equality against a committed fixture file under
//! `tests/golden/`. The fixtures lock the on-wire format against silent
//! drift: any change to the encoder that alters the output bytes will
//! fail one or more of these tests loudly.
//!
//! ## Regenerating fixtures
//!
//! When an intentional wire-format change is made (schema-version bump,
//! new TLV tag, etc.), regenerate the fixtures by running the tests
//! with `CAPTURE_GOLDEN=1` set, then commit the updated `.bin` files
//! alongside the encoder change in the same commit. Without the env
//! variable, the tests assert; with it, they write.
//!
//! ```ignore
//! CAPTURE_GOLDEN=1 cargo test --test golden
//! ```
//!
//! The fixtures must be sourced from a trusted encoder state. The
//! initial seeding (step 1d of the test-harness plan) was sourced
//! from the encoder right after the verbatim extraction in step 1c.
//! Future regenerations should only happen alongside an intentional
//! wire-format change with the new bytes traced back to the spec at
//! `docs/visual-digest-format.md`.

use shakenfist_visual_digest::{
    encode, encoder::event_tlv_bytes, format::CRC32C, BootloaderChoice, ChannelHashes, Event,
    Phase, DIGEST_PAYLOAD_CAPACITY, MAX_RECORD_SIZE,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Return the path to the `tests/golden/` directory, located relative
/// to the crate root (the directory containing `Cargo.toml`).
fn golden_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is set by Cargo during test runs and points
    // at the crate root (where `Cargo.toml` lives).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by Cargo during test execution");
    PathBuf::from(manifest_dir).join("tests").join("golden")
}

/// Assert or capture a golden fixture.
///
/// - Without `CAPTURE_GOLDEN` set: reads the fixture at `path` and
///   `assert_eq!`s byte-for-byte against `actual`.
/// - With `CAPTURE_GOLDEN=1`: writes `actual` to `path` (creating
///   parent directories as needed).
fn assert_or_capture(path: &Path, actual: &[u8]) {
    if std::env::var("CAPTURE_GOLDEN").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("Failed to create dir {}: {}", parent.display(), e));
        }
        fs::write(path, actual)
            .unwrap_or_else(|e| panic!("Failed to write fixture {}: {}", path.display(), e));
        println!("Captured {} bytes to {}", actual.len(), path.display());
    } else {
        let expected = fs::read(path).unwrap_or_else(|e| {
            panic!(
                "Cannot read fixture {}: {}. Run with CAPTURE_GOLDEN=1 to seed it.",
                path.display(),
                e
            )
        });
        if actual != expected.as_slice() {
            let act_len = actual.len();
            let exp_len = expected.len();
            // Find first differing byte.
            let first_diff = actual
                .iter()
                .zip(expected.iter())
                .enumerate()
                .find(|(_, (a, b))| a != b)
                .map(|(i, _)| i)
                .unwrap_or(act_len.min(exp_len));
            // Neighbourhood dump: up to 8 bytes around the first diff.
            let lo = first_diff.saturating_sub(4);
            let hi = (first_diff + 4).min(act_len.min(exp_len));
            let act_slice = &actual[lo..hi];
            let exp_slice = &expected[lo..hi];
            panic!(
                "Golden fixture mismatch for {}:\n  actual   len={}\n  expected len={}\n  \
                 first diff at byte offset {}\n  actual   [{lo}..{hi}]: {:02x?}\n  \
                 expected [{lo}..{hi}]: {:02x?}",
                path.display(),
                act_len,
                exp_len,
                first_diff,
                act_slice,
                exp_slice,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: compute CRC32C of one event's TLV bytes (used to build
// expected channel-hash values for the single_keypress and
// mixed_all_variants sequences).
// ---------------------------------------------------------------------------
fn crc32c_of_event(event: &Event) -> u32 {
    let mut buf = [0u8; MAX_RECORD_SIZE];
    let len = event_tlv_bytes(event, &mut buf);
    CRC32C.checksum(&buf[..len])
}

// ---------------------------------------------------------------------------
// Test 1: empty -- no events at all.
// ---------------------------------------------------------------------------
// Frame counter  : 0x00000001
// Framebuffer hash: 0x00000000
// Channel hashes : all 0x00000000 (CRC32C of zero bytes)
// Raw records    : none
// Expected size  : 10 (header) + 48 (hash block) + 0 (raw) + 4 (trailer) = 62 bytes
// ---------------------------------------------------------------------------
#[test]
fn empty() {
    let events: &[&Event] = &[];
    let channel_hashes = ChannelHashes::new();
    let frame_counter: u32 = 0x0000_0001;
    let framebuffer_hash: u32 = 0x0000_0000;

    let mut out = [0u8; DIGEST_PAYLOAD_CAPACITY];
    let written = encode(
        events,
        frame_counter,
        framebuffer_hash,
        &channel_hashes,
        &mut out,
    )
    .expect("encode failed for empty sequence");

    assert_eq!(written, 62, "empty: expected 62 bytes, got {}", written);

    let path = golden_dir().join("empty.bin");
    assert_or_capture(&path, &out[..written]);
}

// ---------------------------------------------------------------------------
// Test 2: single_keypress -- one Keypress event.
// ---------------------------------------------------------------------------
// Event         : Keypress { unicode: 'A', scancode: 0x1234, timestamp_ms: 0x0102030405060708 }
// Frame counter : 0x12345678
// Framebuffer hash: 0xCAFEBABE
// Channel hashes: keypress = CRC32C of that keypress TLV bytes; others = 0
// Expected size : 10 + 48 + 14 + 4 = 76 bytes
// ---------------------------------------------------------------------------
#[test]
fn single_keypress() {
    let keypress = Event::Keypress {
        unicode: 'A',
        scancode: 0x1234,
        timestamp_ms: 0x0102_0304_0506_0708,
    };
    let events: &[&Event] = &[&keypress];

    let mut channel_hashes = ChannelHashes::new();
    channel_hashes.update(&keypress);

    let frame_counter: u32 = 0x1234_5678;
    let framebuffer_hash: u32 = 0xCAFE_BABE;

    // Verify channel hash matches what we compute directly.
    let expected_keypress_hash = crc32c_of_event(&keypress);
    assert_eq!(
        channel_hashes.keypress, expected_keypress_hash,
        "keypress channel hash mismatch"
    );

    let mut out = [0u8; DIGEST_PAYLOAD_CAPACITY];
    let written = encode(
        events,
        frame_counter,
        framebuffer_hash,
        &channel_hashes,
        &mut out,
    )
    .expect("encode failed for single_keypress sequence");

    assert_eq!(
        written, 76,
        "single_keypress: expected 76 bytes, got {}",
        written
    );

    let path = golden_dir().join("single_keypress.bin");
    assert_or_capture(&path, &out[..written]);
}

// ---------------------------------------------------------------------------
// Test 3: mixed_all_variants -- one of every Event variant.
// ---------------------------------------------------------------------------
// Event order (canonical, chronological, oldest first):
//   1. Keypress          { unicode: 'k', scancode: 0x0042, timestamp_ms: 0x0000_0000_1000_0001 }
//   2. LineRendered      { row: 0x0007,  timestamp_ms: 0x0000_0000_1000_0002 }
//   3. SceneTransition   { from: Awaiting, to: Booting, timestamp_ms: 0x0000_0000_1000_0003 }
//   4. BootloaderDecision{ choice: Ignore, attempt: 3, timestamp_ms: 0x0000_0000_1000_0004 }
//   5. PasteReceived     { len: 0x001c, correct: true, timestamp_ms: 0x0000_0000_1000_0005 }
//   6. BootloaderTimeout { timestamp_ms: 0x0000_0000_1000_0006 }
//   7. ModeSwitch        { requested_w: 1024, requested_h: 768, applied_w: 800, applied_h: 600,
//                          timestamp_ms: 0x0000_0000_1000_0007 }
//   8. ModeCycle         { count: 0x000000ff, interrupted: false, timestamp_ms: 0x0000_0000_1000_0008 }
//
// Record sizes: 14+12+12+15+13+10+18+15 = 109 bytes. Exceeds the 44-byte raw budget.
//
// Budget walk (newest-to-oldest):
//   ModeCycle (15)         -> cumulative 15  <= 44 -> include
//   ModeSwitch (18)        -> cumulative 33  <= 44 -> include
//   BootloaderTimeout (10) -> cumulative 43  <= 44 -> include
//   PasteReceived (13)     -> cumulative 56  >  44 -> stop
//
// Included raw records (chronological): ModeSwitch(18) + BootloaderTimeout(10) + ModeCycle(15) = 43 bytes
//
// Frame counter   : 0xDEADBEEF
// Framebuffer hash: 0x12345678
// Expected size   : 10 + 48 + 43 + 4 = 105 bytes
// ---------------------------------------------------------------------------
#[test]
fn mixed_all_variants() {
    let e1 = Event::Keypress {
        unicode: 'k',
        scancode: 0x0042,
        timestamp_ms: 0x0000_0000_1000_0001,
    };
    let e2 = Event::LineRendered {
        row: 0x0007,
        timestamp_ms: 0x0000_0000_1000_0002,
    };
    let e3 = Event::SceneTransition {
        from: Phase::Awaiting,
        to: Phase::Booting,
        timestamp_ms: 0x0000_0000_1000_0003,
    };
    let e4 = Event::BootloaderDecision {
        choice: BootloaderChoice::Ignore,
        attempt: 3,
        timestamp_ms: 0x0000_0000_1000_0004,
    };
    let e5 = Event::PasteReceived {
        len: 0x001c,
        correct: true,
        timestamp_ms: 0x0000_0000_1000_0005,
    };
    let e6 = Event::BootloaderTimeout {
        timestamp_ms: 0x0000_0000_1000_0006,
    };
    let e7 = Event::ModeSwitch {
        requested_w: 1024,
        requested_h: 768,
        applied_w: 800,
        applied_h: 600,
        timestamp_ms: 0x0000_0000_1000_0007,
    };
    let e8 = Event::ModeCycle {
        count: 0x0000_00ff,
        interrupted: false,
        timestamp_ms: 0x0000_0000_1000_0008,
    };

    let events: &[&Event] = &[&e1, &e2, &e3, &e4, &e5, &e6, &e7, &e8];

    // Build channel hashes by updating once per event in order.
    let mut channel_hashes = ChannelHashes::new();
    for event in events {
        channel_hashes.update(event);
    }

    // Verify each channel hash equals CRC32C of that event's TLV bytes
    // (since there is exactly one event per channel).
    assert_eq!(
        channel_hashes.keypress,
        crc32c_of_event(&e1),
        "keypress hash"
    );
    assert_eq!(
        channel_hashes.line_rendered,
        crc32c_of_event(&e2),
        "line_rendered hash"
    );
    assert_eq!(
        channel_hashes.scene_transition,
        crc32c_of_event(&e3),
        "scene_transition hash"
    );
    assert_eq!(
        channel_hashes.bootloader_decision,
        crc32c_of_event(&e4),
        "bootloader_decision hash"
    );
    assert_eq!(
        channel_hashes.paste_received,
        crc32c_of_event(&e5),
        "paste_received hash"
    );
    assert_eq!(
        channel_hashes.bootloader_timeout,
        crc32c_of_event(&e6),
        "bootloader_timeout hash"
    );
    assert_eq!(
        channel_hashes.mode_switch,
        crc32c_of_event(&e7),
        "mode_switch hash"
    );
    assert_eq!(
        channel_hashes.mode_cycle,
        crc32c_of_event(&e8),
        "mode_cycle hash"
    );

    let frame_counter: u32 = 0xDEAD_BEEF;
    let framebuffer_hash: u32 = 0x1234_5678;

    let mut out = [0u8; DIGEST_PAYLOAD_CAPACITY];
    let written = encode(
        events,
        frame_counter,
        framebuffer_hash,
        &channel_hashes,
        &mut out,
    )
    .expect("encode failed for mixed_all_variants sequence");

    // The encoder includes the 3 newest events that fit in 44 bytes:
    // ModeSwitch(18) + BootloaderTimeout(10) + ModeCycle(15) = 43 bytes raw.
    // Total: 10 + 48 + 43 + 4 = 105 bytes.
    assert_eq!(
        written, 105,
        "mixed_all_variants: expected 105 bytes, got {}",
        written
    );

    let path = golden_dir().join("mixed_all_variants.bin");
    assert_or_capture(&path, &out[..written]);
}
