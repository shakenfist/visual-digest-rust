//! Integration tests for the visual-digest decoder.
//!
//! All tests are gated on `cfg(feature = "decode")`.
//!
//! ## Test categories
//!
//! 1. Round-trip tests: encode the same three golden sequences used in
//!    `golden.rs`, then decode and verify structural equality.
//! 2. Forward-compatibility test: a hand-built payload with an unknown tag
//!    (0x09) must decode without error and surface the tag in `unknown_records`.
//! 3. Malformed-input tests: various byte corruptions of `single_keypress.bin`
//!    must return the expected `DecodeError` variant without panicking.

#[cfg(feature = "decode")]
mod decode_tests {
    use shakenfist_visual_digest::{
        decode, BootloaderChoice, ChannelHashes, DecodeError, Event, Phase, UnknownRecord,
    };
    use std::{fs, path::PathBuf};

    fn golden_dir() -> PathBuf {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR must be set by Cargo during test execution");
        PathBuf::from(manifest_dir).join("tests").join("golden")
    }

    // =========================================================================
    // Round-trip tests
    // =========================================================================

    /// Decode the `empty` golden fixture and verify all fields.
    ///
    /// Fixture properties:
    ///   frame_counter    = 0x00000001
    ///   channel_hashes   = all 0 (empty channels)
    ///   raw_records      = []
    ///   unknown_records  = []
    ///   framebuffer_hash = 0x00000000
    #[test]
    fn round_trip_empty() {
        let fixture = fs::read(golden_dir().join("empty.bin"))
            .expect("cannot read empty.bin; run golden tests to seed it");
        let digest = decode(&fixture).expect("decode failed for empty fixture");

        assert_eq!(digest.frame_counter, 0x0000_0001, "frame_counter");
        assert_eq!(
            digest.channel_hashes,
            ChannelHashes::new(),
            "channel_hashes"
        );
        assert!(digest.raw_records.is_empty(), "raw_records must be empty");
        assert!(
            digest.unknown_records.is_empty(),
            "unknown_records must be empty"
        );
        assert_eq!(digest.framebuffer_hash, 0x0000_0000, "framebuffer_hash");
    }

    /// Decode the `single_keypress` golden fixture and verify all fields.
    ///
    /// Fixture properties:
    ///   frame_counter    = 0x12345678
    ///   keypress event   = Keypress { unicode: 'A', scancode: 0x1234,
    ///                        timestamp_ms: 0x0102030405060708 }
    ///   raw_records      = [Keypress { ... }]
    ///   unknown_records  = []
    ///   framebuffer_hash = 0xCAFEBABE
    #[test]
    fn round_trip_single_keypress() {
        let fixture = fs::read(golden_dir().join("single_keypress.bin"))
            .expect("cannot read single_keypress.bin; run golden tests to seed it");
        let digest = decode(&fixture).expect("decode failed for single_keypress fixture");

        assert_eq!(digest.frame_counter, 0x1234_5678, "frame_counter");
        assert_eq!(digest.framebuffer_hash, 0xCAFE_BABE, "framebuffer_hash");
        assert!(
            digest.unknown_records.is_empty(),
            "unknown_records must be empty"
        );

        // Verify channel hashes: keypress = CRC32C of that keypress TLV.
        let keypress = Event::Keypress {
            unicode: 'A',
            scancode: 0x1234,
            timestamp_ms: 0x0102_0304_0506_0708,
        };
        let mut expected_hashes = ChannelHashes::new();
        expected_hashes.update(&keypress);
        assert_eq!(digest.channel_hashes, expected_hashes, "channel_hashes");

        // raw_records: exactly one Keypress.
        assert_eq!(digest.raw_records.len(), 1, "raw_records length");
        assert_eq!(
            digest.raw_records[0],
            Event::Keypress {
                unicode: 'A',
                scancode: 0x1234,
                timestamp_ms: 0x0102_0304_0506_0708,
            },
            "raw_records[0]"
        );
    }

    /// Decode the `mixed_all_variants` golden fixture and verify all fields.
    ///
    /// The encoder includes only the 3 most-recent events that fit in the
    /// 44-byte raw budget: ModeSwitch(18) + BootloaderTimeout(10) + ModeCycle(15)
    /// = 43 bytes. The channel hashes cover all 8 events since boot.
    ///
    /// Fixture properties:
    ///   frame_counter    = 0xDEADBEEF
    ///   raw_records      = [ModeSwitch, BootloaderTimeout, ModeCycle]
    ///   unknown_records  = []
    ///   framebuffer_hash = 0x12345678
    #[test]
    fn round_trip_mixed_all_variants() {
        let fixture = fs::read(golden_dir().join("mixed_all_variants.bin"))
            .expect("cannot read mixed_all_variants.bin; run golden tests to seed it");
        let digest = decode(&fixture).expect("decode failed for mixed_all_variants fixture");

        assert_eq!(digest.frame_counter, 0xDEAD_BEEF, "frame_counter");
        assert_eq!(digest.framebuffer_hash, 0x1234_5678, "framebuffer_hash");
        assert!(
            digest.unknown_records.is_empty(),
            "unknown_records must be empty"
        );

        // Build all 8 events as used in golden.rs.
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
        let all_events = [&e1, &e2, &e3, &e4, &e5, &e6, &e7, &e8];

        // Verify channel hashes match what the encoder would have computed.
        let mut expected_hashes = ChannelHashes::new();
        for event in &all_events {
            expected_hashes.update(event);
        }
        assert_eq!(digest.channel_hashes, expected_hashes, "channel_hashes");

        // raw_records: the 3 most-recent events that fit in 44 bytes.
        // Encoder selects newest-to-oldest: e8(ModeCycle,15) + e7(ModeSwitch,18) +
        // e6(BootloaderTimeout,10) = 43 bytes. Emitted in chronological order.
        assert_eq!(digest.raw_records.len(), 3, "raw_records length");
        assert_eq!(
            digest.raw_records[0], e6,
            "raw_records[0] = BootloaderTimeout"
        );
        assert_eq!(digest.raw_records[1], e7, "raw_records[1] = ModeSwitch");
        assert_eq!(digest.raw_records[2], e8, "raw_records[2] = ModeCycle");
    }

    /// Wire round-trip for `Phase::StreamExercise`. The variant was added
    /// alongside uncalibrated-sextant's `run_stream_exercise` scene
    /// (reproducer for the upstream spice-server SIGSYS crash); this test
    /// confirms the wire discriminant survives encode → decode in both
    /// `from` and `to` positions of a `SceneTransition` record. Catches
    /// regressions where someone reorders the `Phase` enum or forgets to
    /// extend `decode_phase` after adding a fourth variant.
    #[test]
    fn round_trip_phase_stream_exercise() {
        use shakenfist_visual_digest::encode;

        // Two transitions: Awaiting → StreamExercise (the scene starts)
        // and StreamExercise → Booting (the scene completes and hands
        // off). Together they exercise StreamExercise as both `to` and
        // `from`.
        let e1 = Event::SceneTransition {
            from: Phase::Awaiting,
            to: Phase::StreamExercise,
            timestamp_ms: 0x0000_0000_2000_0001,
        };
        let e2 = Event::SceneTransition {
            from: Phase::StreamExercise,
            to: Phase::Booting,
            timestamp_ms: 0x0000_0000_2000_0002,
        };

        let mut channel_hashes = ChannelHashes::new();
        channel_hashes.update(&e1);
        channel_hashes.update(&e2);

        let mut buf = [0u8; shakenfist_visual_digest::DIGEST_PAYLOAD_CAPACITY];
        let len = encode(
            &[&e1, &e2],
            0xAAAA_AAAA,
            0xBBBB_BBBB,
            &channel_hashes,
            &mut buf,
        )
        .expect("encode failed");

        let digest = decode(&buf[..len]).expect("decode failed");

        assert_eq!(digest.frame_counter, 0xAAAA_AAAA, "frame_counter");
        assert_eq!(digest.framebuffer_hash, 0xBBBB_BBBB, "framebuffer_hash");
        assert!(
            digest.unknown_records.is_empty(),
            "unknown_records must be empty"
        );
        assert_eq!(digest.channel_hashes, channel_hashes, "channel_hashes");

        // The encoder selects the most-recent N events that fit; the
        // decoder yields them in chronological (oldest-first) order.
        // Both transitions fit the budget, so we get both back in
        // input order.
        assert_eq!(digest.raw_records.len(), 2, "raw_records length");
        match &digest.raw_records[0] {
            Event::SceneTransition { from, to, .. } => {
                assert_eq!(*from, Phase::Awaiting);
                assert_eq!(*to, Phase::StreamExercise);
            }
            other => panic!(
                "raw_records[0] not the expected SceneTransition: {:?}",
                other
            ),
        }
        match &digest.raw_records[1] {
            Event::SceneTransition { from, to, .. } => {
                assert_eq!(*from, Phase::StreamExercise);
                assert_eq!(*to, Phase::Booting);
            }
            other => panic!(
                "raw_records[1] not the expected SceneTransition: {:?}",
                other
            ),
        }
    }

    // =========================================================================
    // Forward-compatibility test
    // =========================================================================

    /// A payload with an unknown tag (0x09) in the raw-record region must
    /// decode without error and surface the tag in `unknown_records`.
    ///
    /// ## Hand-built payload layout
    ///
    /// This payload is constructed manually by following the spec. It uses
    /// the minimal structure: header + hash block + one unknown record + trailer.
    ///
    /// Bytes (annotated):
    ///
    /// Header (10 bytes):
    ///   [00] 0x53 'S'
    ///   [01] 0x58 'X'
    ///   [02] 0x44 'D'
    ///   [03] 0x47 'G'
    ///   [04] 0x02  schema version = 2
    ///   [05..08] 0x01 0x00 0x00 0x00  frame_counter = 1 (LE u32)
    ///   [09] 0x09  record_count = 9 (8 hash + 1 unknown)
    ///
    /// Hash block (48 bytes, all zeros for all channels):
    ///   [10] 0x11 0x04 0x00 0x00 0x00 0x00  keypress hash = 0
    ///   [16] 0x12 0x04 0x00 0x00 0x00 0x00  line_rendered hash = 0
    ///   [22] 0x13 0x04 0x00 0x00 0x00 0x00  scene_transition hash = 0
    ///   [28] 0x14 0x04 0x00 0x00 0x00 0x00  bootloader_decision hash = 0
    ///   [34] 0x15 0x04 0x00 0x00 0x00 0x00  paste_received hash = 0
    ///   [40] 0x16 0x04 0x00 0x00 0x00 0x00  bootloader_timeout hash = 0
    ///   [46] 0x17 0x04 0x00 0x00 0x00 0x00  mode_switch hash = 0
    ///   [52] 0x18 0x04 0x00 0x00 0x00 0x00  mode_cycle hash = 0
    ///
    /// Unknown record (4 bytes):
    ///   [58] 0x09  tag = 0x09 (reserved, not assigned in v2)
    ///   [59] 0x02  value_len = 2
    ///   [60] 0xAB  value byte 0
    ///   [61] 0xCD  value byte 1
    ///
    /// Trailer (4 bytes):
    ///   [62..65] 0x78 0x56 0x34 0x12  framebuffer_hash = 0x12345678 (LE u32)
    ///
    /// Total: 66 bytes.
    #[test]
    fn forward_compat_unknown_tag_captured() {
        #[rustfmt::skip]
        let payload: &[u8] = &[
            // Header
            0x53, 0x58, 0x44, 0x47,  // magic "SXDG"
            0x02,                    // schema version 2
            0x01, 0x00, 0x00, 0x00,  // frame_counter = 1 (LE)
            0x09,                    // record_count = 9 (8 hash + 1 unknown)
            // Hash block: 8 × [tag, 0x04, 0x00, 0x00, 0x00, 0x00]
            0x11, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x12, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x13, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x14, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x15, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x16, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x17, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x18, 0x04, 0x00, 0x00, 0x00, 0x00,
            // Unknown record: tag=0x09 (reserved), value=[0xAB, 0xCD]
            0x09, 0x02, 0xAB, 0xCD,
            // Trailer: framebuffer_hash = 0x12345678 (LE)
            0x78, 0x56, 0x34, 0x12,
        ];
        assert_eq!(payload.len(), 66, "hand-built payload must be 66 bytes");

        let digest = decode(payload).expect("decode must succeed for forward-compat payload");

        assert_eq!(digest.frame_counter, 1, "frame_counter");
        assert_eq!(
            digest.channel_hashes,
            ChannelHashes::new(),
            "all channel hashes zero"
        );
        assert!(digest.raw_records.is_empty(), "no known raw records");
        assert_eq!(digest.framebuffer_hash, 0x1234_5678, "framebuffer_hash");

        // The unknown record must be captured.
        assert_eq!(
            digest.unknown_records.len(),
            1,
            "exactly one unknown record"
        );
        assert_eq!(
            digest.unknown_records[0],
            UnknownRecord {
                tag: 0x09,
                value: vec![0xAB, 0xCD],
            },
            "unknown record content"
        );
    }

    // =========================================================================
    // Malformed-input tests
    // All use `single_keypress.bin` as the base; each corrupts one aspect.
    // =========================================================================

    fn load_single_keypress() -> Vec<u8> {
        fs::read(
            PathBuf::from(
                std::env::var("CARGO_MANIFEST_DIR")
                    .expect("CARGO_MANIFEST_DIR must be set by Cargo"),
            )
            .join("tests")
            .join("golden")
            .join("single_keypress.bin"),
        )
        .expect("cannot read single_keypress.bin; run golden tests to seed it")
    }

    /// Truncate input to 30 bytes -> Short.
    #[test]
    fn malformed_truncate_to_30_bytes() {
        let base = load_single_keypress();
        let truncated = &base[..30];
        let err = decode(truncated).expect_err("expected Short error");
        assert!(
            matches!(err, DecodeError::Short { found: 30 }),
            "expected Short {{ found: 30 }}, got {err:?}"
        );
    }

    /// Flip byte 0 (the 'S' of 'SXDG') -> BadMagic.
    #[test]
    fn malformed_bad_magic() {
        let mut payload = load_single_keypress();
        payload[0] ^= 0xFF; // corrupt first byte of magic
        let err = decode(&payload).expect_err("expected BadMagic error");
        assert!(
            matches!(err, DecodeError::BadMagic { .. }),
            "expected BadMagic, got {err:?}"
        );
    }

    /// Set byte 4 (schema version) to 0x03 -> UnsupportedSchemaVersion(3).
    #[test]
    fn malformed_unsupported_schema_version() {
        let mut payload = load_single_keypress();
        payload[4] = 0x03;
        let err = decode(&payload).expect_err("expected UnsupportedSchemaVersion error");
        assert!(
            matches!(err, DecodeError::UnsupportedSchemaVersion(3)),
            "expected UnsupportedSchemaVersion(3), got {err:?}"
        );
    }

    /// Set byte 9 (record count) to 0x07 -> RecordCountTooSmall { found: 7 }.
    #[test]
    fn malformed_record_count_too_small() {
        let mut payload = load_single_keypress();
        payload[9] = 0x07;
        let err = decode(&payload).expect_err("expected RecordCountTooSmall error");
        assert!(
            matches!(err, DecodeError::RecordCountTooSmall { found: 7 }),
            "expected RecordCountTooSmall {{ found: 7 }}, got {err:?}"
        );
    }

    /// Corrupt hash-record tag at byte 10 (0x11 -> 0x77) -> MalformedHashBlock.
    #[test]
    fn malformed_hash_block_bad_tag() {
        let mut payload = load_single_keypress();
        payload[10] = 0x77; // first hash record's tag byte
        let err = decode(&payload).expect_err("expected MalformedHashBlock error");
        assert!(
            matches!(err, DecodeError::MalformedHashBlock { .. }),
            "expected MalformedHashBlock, got {err:?}"
        );
    }

    /// Corrupt hash-record length byte at byte 11 (0x04 -> 0x05) -> MalformedHashBlock.
    #[test]
    fn malformed_hash_block_bad_length() {
        let mut payload = load_single_keypress();
        payload[11] = 0x05; // first hash record's length byte (must be 4)
        let err = decode(&payload).expect_err("expected MalformedHashBlock error");
        assert!(
            matches!(err, DecodeError::MalformedHashBlock { .. }),
            "expected MalformedHashBlock, got {err:?}"
        );
    }

    /// Insert 4 orphan bytes between the last decoded record and the trailer
    /// -> Trailing { extra_bytes: 4 }.
    ///
    /// The `Trailing` error fires when `pos != trailer_start` after the
    /// raw-record loop — i.e., when there are bytes after the last decoded
    /// record that are too short to form a new complete TLV record header (tag +
    /// len + value), but the loop already exited because those bytes start with
    /// a zero-length unknown record that brings pos to within 4 of
    /// `trailer_start`, and then there are 4 remaining orphan bytes.
    ///
    /// Simplest construction: build a payload where after decoding all records
    /// there are 4 orphan bytes before the trailer. We use four 0x00/0x00
    /// zero-length records... but those would all be consumed. Instead, insert
    /// bytes that can't form a valid record at the boundary.
    ///
    /// Cleanest approach: insert a raw-record that claims a value length of 0
    /// (an empty unknown record: tag=0x09, len=0x00 = 2 bytes consumed) four
    /// times, but then insert 1 extra single orphan byte at the end. That way:
    /// - 4 zero-len unknown records consume 8 bytes
    /// - 1 orphan byte at pos = trailer_start - 1 fires Trailing { extra_bytes: 1 }
    ///
    /// Actual construction used here: insert one empty unknown record + 4 orphan
    /// bytes before the trailer. After the empty record, there are 4 bytes that
    /// cannot form a complete record header (actually they can since len=0). So
    /// instead use 3 extra zero-len records (6 bytes) + 1 orphan byte = Trailing
    /// with extra_bytes=1.
    ///
    /// For simplicity and clarity, we hand-build a minimal payload:
    ///
    /// Header (10 bytes) + hash block (48 bytes) + 4 zero-length unknown records
    /// (8 bytes) + 4 orphan bytes + trailer (4 bytes) = 74 bytes.
    ///
    /// The 4 orphan bytes cannot be parsed because pos + 2 <= trailer_start is
    /// true (4 bytes available), so the decoder tries tag + len.
    /// Instead, we choose a single 1-byte orphan so Trailing fires cleanly.
    ///
    /// Simplest: header + hash block + no raw records + 4 padding bytes + trailer.
    /// payload[58..62] = [0x09, 0x00, 0x09, 0x00]  two empty unknown records
    ///                 = pos advances to 62
    /// payload[62..66] = [0x09, 0x00, 0x09, 0x00]  two more empty unknown records
    ///                 = pos advances to 66
    /// Total = 10 + 48 + 8 + 4 (trailer) = 70 bytes. trailer_start = 66.
    ///
    /// After 4 zero-len records (8 bytes), pos = 66 = trailer_start. No Trailing.
    ///
    /// To trigger Trailing we need bytes the decoder cannot fully consume.
    /// Use a record that declares a value length matching up to within 3 bytes
    /// of trailer_start — impossible to finish that way.
    ///
    /// Correct minimal construction:
    ///   payload[58] = 0x09  tag = unknown
    ///   payload[59] = 0x00  value_len = 0
    ///   payload[60] = 0xFF  orphan byte (cannot be consumed as full record
    ///                        since pos+2=62 <= trailer_start=61? No, 62 > 61)
    ///
    /// Header(10) + hash_block(48) + 1 unknown record(2) + 1 orphan byte + trailer(4)
    /// = 65 bytes. trailer_start = 61.
    /// After unknown record: pos = 60 < 61. Loop continues: pos+2=62 > 61 → Trailing(1).
    ///
    /// Final construction: 65-byte payload with 1 orphan byte → Trailing { extra_bytes: 1 }.
    #[test]
    fn malformed_trailing_bytes() {
        // Hand-built 65-byte payload:
        //   header (10) + hash block (48) + [0x09, 0x00] unknown record (2)
        //   + [0xFF] orphan byte (1) + trailer (4)
        // trailer_start = 65 - 4 = 61
        // After decoding unknown record at pos=58: pos=60 < 61.
        // Loop: pos+2=62 > trailer_start=61 → Trailing { extra_bytes: 1 }.
        #[rustfmt::skip]
        let payload: &[u8] = &[
            // Header
            0x53, 0x58, 0x44, 0x47,  // magic "SXDG"
            0x02,                    // schema version 2
            0x01, 0x00, 0x00, 0x00,  // frame_counter = 1 (LE)
            0x0A,                    // record_count = 10 (8 hash + 1 unknown + 1 orphan)
            // Hash block: 8 × [tag, 0x04, 0x00, 0x00, 0x00, 0x00]
            0x11, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x12, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x13, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x14, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x15, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x16, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x17, 0x04, 0x00, 0x00, 0x00, 0x00,
            0x18, 0x04, 0x00, 0x00, 0x00, 0x00,
            // Unknown record at pos=58: tag=0x09, len=0 (2 bytes, no value)
            0x09, 0x00,
            // Orphan byte at pos=60 (1 byte, cannot form a record since
            // trailer_start=61 and pos+2=62 > 61)
            0xFF,
            // Trailer: framebuffer_hash = 0x00000000 (LE)
            0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(payload.len(), 65, "hand-built payload must be 65 bytes");

        let err = decode(payload).expect_err("expected Trailing error");
        assert!(
            matches!(err, DecodeError::Trailing { extra_bytes: 1 }),
            "expected Trailing {{ extra_bytes: 1 }}, got {err:?}"
        );
    }
}
