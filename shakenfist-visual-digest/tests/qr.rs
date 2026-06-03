//! Integration tests for the QR locate-and-decode helper.
//!
//! All tests are gated on `cfg(feature = "qr")`.
//!
//! ## Synthetic fixtures — rationale and deviation from phase plan
//!
//! The phase-plan brief (step 1f, PLAN-test-harness-phase-01-digest-crate.md)
//! mentions capturing fixture PNGs from Sextant under QEMU via
//! `scripts/screenshot.sh`.  Running QEMU + Docker from a sub-agent is
//! fragile and non-deterministic, so this step uses **synthetic** PNG
//! fixtures generated in-test using `qrcodegen`.  The synthetic approach:
//!
//! * Encodes known input bytes (the three golden fixtures from
//!   `tests/golden/`) into a QR using the same ECC level (Low) that
//!   Sextant uses.
//! * Renders the QR to RGBA at 4 px/module with a 4-module quiet zone,
//!   matching Sextant's `DIGEST_MODULE_PX` and `DIGEST_QR_BORDER`.
//! * Uses Sextant's exact phosphor-green foreground colour
//!   (`R=51, G=150, B=51`) on pure-black background, sourced from
//!   `uncalibrated-sextant/src/renderer/mod.rs` line ~71:
//!   `const FG: BltPixel = BltPixel::new(51, 150, 51)`.
//! * Gives precise expected values and runs fast in CI without QEMU.
//!
//! Byte-identity with Sextant's actual screenshot is validated end-to-end
//! in step 1h's `make digest-payload-smoke` against the migrated Sextant,
//! so the QR decoder's correctness against a real Sextant frame is covered
//! there.
//!
//! ## Adding a real-Sextant fixture
//!
//! To add a fixture from an actual Sextant QEMU session:
//!
//! 1. Run `make screenshot` inside the uncalibrated-sextant repo while QEMU
//!    is running a particular scene.  This produces a PNG of the full GOP
//!    framebuffer.
//! 2. Copy the resulting PNG to `tests/qr-fixtures/sextant-real-<scene>.png`.
//! 3. Add a test that loads it and asserts sensible-value bounds:
//!    - magic bytes present (the `decode` call succeeds)
//!    - schema version == 2
//!    - record count >= 8 (hash block always present)
//!    - frame_counter > 0
//! 4. The `decode_qr_png` call handles the full framebuffer; rqrr will
//!    locate the bottom-right QR automatically — no cropping needed.

#[cfg(feature = "qr")]
mod qr_tests {
    use std::path::PathBuf;

    use image::{ImageBuffer, Rgba};
    use qrcodegen::{QrCode, QrCodeEcc};
    use shakenfist_visual_digest::{decode, decode_qr_png, decode_qr_rgba, QrError};

    // =========================================================================
    // Rendering constants matching Sextant's digest renderer.
    // Source: uncalibrated-sextant/src/renderer/mod.rs
    // =========================================================================

    /// Pixels per QR module — matches `DIGEST_MODULE_PX = 4` in Sextant.
    const MODULE_PX: u32 = 4;

    /// Quiet-zone modules around the QR code — matches `DIGEST_QR_BORDER = 4`.
    const QUIET_MODULES: u32 = 4;

    /// Foreground colour: Sextant's phosphor-green, from
    /// `const FG: BltPixel = BltPixel::new(51, 150, 51)` in renderer/mod.rs.
    const FG_RGBA: [u8; 4] = [51, 150, 51, 255];

    /// Background colour: pure black, matching
    /// `const BG: BltPixel = BltPixel::new(0, 0, 0)`.
    const BG_RGBA: [u8; 4] = [0, 0, 0, 255];

    // =========================================================================
    // Helper functions
    // =========================================================================

    fn golden_dir() -> PathBuf {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo");
        PathBuf::from(manifest_dir).join("tests").join("golden")
    }

    /// Encode `payload` as a QR (ECC Low, binary mode) and render it to an
    /// RGBA buffer at `MODULE_PX` px/module with a `QUIET_MODULES` quiet zone.
    ///
    /// The foreground (dark) modules use Sextant's phosphor-green palette
    /// (`FG_RGBA`); background (light) modules are pure black (`BG_RGBA`).
    ///
    /// Returns `(rgba_bytes, width, height)`.
    fn render_qr_to_rgba(payload: &[u8]) -> (Vec<u8>, u32, u32) {
        let qr = QrCode::encode_binary(payload, QrCodeEcc::Low)
            .expect("qrcodegen failed to encode payload");

        let qr_modules = qr.size() as u32;
        // Total side = (modules + 2 × quiet zone) × pixels per module.
        let side = (qr_modules + 2 * QUIET_MODULES) * MODULE_PX;

        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(side, side);

        for py in 0..side {
            for px in 0..side {
                // Map pixel coordinates to module indices (accounting for quiet zone).
                // Quiet zone is rendered as background colour.
                let mx = px / MODULE_PX;
                let my = py / MODULE_PX;

                let color = if mx < QUIET_MODULES
                    || my < QUIET_MODULES
                    || mx >= qr_modules + QUIET_MODULES
                    || my >= qr_modules + QUIET_MODULES
                {
                    BG_RGBA
                } else {
                    let qx = (mx - QUIET_MODULES) as i32;
                    let qy = (my - QUIET_MODULES) as i32;
                    if qr.get_module(qx, qy) {
                        FG_RGBA
                    } else {
                        BG_RGBA
                    }
                };

                img.put_pixel(px, py, Rgba(color));
            }
        }

        let width = img.width();
        let height = img.height();
        let raw = img.into_raw();
        (raw, width, height)
    }

    /// Render `payload` as a QR + PNG, write it to a named temp file, and
    /// return the `NamedTempFile` (kept alive so the file persists for the
    /// duration of the test).
    fn render_qr_to_png(payload: &[u8]) -> tempfile::NamedTempFile {
        let tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("could not create temp file");

        let (rgba, width, height) = render_qr_to_rgba(payload);

        // Re-create an ImageBuffer from raw bytes so we can save as PNG.
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width, height, rgba).expect("buffer dimensions mismatch");

        img.save(tmp.path()).expect("could not save QR PNG");
        tmp
    }

    // =========================================================================
    // Positive round-trip tests — one per golden fixture
    // =========================================================================

    /// Round-trip the `empty` fixture through QR encode → RGBA decode →
    /// PNG decode → digest decode.
    ///
    /// `empty.bin` encodes an empty ring: frame_counter=1, all hashes zero,
    /// no raw records, framebuffer_hash=0.
    #[test]
    fn round_trip_empty() {
        let fixture_bytes = std::fs::read(golden_dir().join("empty.bin"))
            .expect("cannot read empty.bin; run golden tests to seed it");

        // --- RGBA round-trip ---
        let (rgba, width, height) = render_qr_to_rgba(&fixture_bytes);
        let decoded_rgba = decode_qr_rgba(&rgba, width, height)
            .expect("decode_qr_rgba returned None for empty fixture");
        assert_eq!(
            decoded_rgba, fixture_bytes,
            "decode_qr_rgba: decoded bytes must equal fixture bytes (empty)"
        );

        // --- PNG round-trip ---
        let tmp = render_qr_to_png(&fixture_bytes);
        let decoded_png =
            decode_qr_png(tmp.path()).expect("decode_qr_png failed for empty fixture");
        assert_eq!(
            decoded_png, fixture_bytes,
            "decode_qr_png: decoded bytes must equal fixture bytes (empty)"
        );

        // --- Digest decode round-trip ---
        let digest = decode(&decoded_rgba).expect("decode() failed for empty fixture");
        assert_eq!(digest.frame_counter, 0x0000_0001, "frame_counter");
        assert!(digest.raw_records.is_empty(), "raw_records must be empty");
        assert!(
            digest.unknown_records.is_empty(),
            "unknown_records must be empty"
        );
        assert_eq!(digest.framebuffer_hash, 0x0000_0000, "framebuffer_hash");
    }

    /// Round-trip the `single_keypress` fixture through QR encode → RGBA
    /// decode → PNG decode → digest decode.
    ///
    /// `single_keypress.bin` encodes one keypress event:
    ///   frame_counter=0x12345678, framebuffer_hash=0xCAFEBABE.
    #[test]
    fn round_trip_single_keypress() {
        let fixture_bytes = std::fs::read(golden_dir().join("single_keypress.bin"))
            .expect("cannot read single_keypress.bin; run golden tests to seed it");

        // --- RGBA round-trip ---
        let (rgba, width, height) = render_qr_to_rgba(&fixture_bytes);
        let decoded_rgba = decode_qr_rgba(&rgba, width, height)
            .expect("decode_qr_rgba returned None for single_keypress fixture");
        assert_eq!(
            decoded_rgba, fixture_bytes,
            "decode_qr_rgba: decoded bytes must equal fixture bytes (single_keypress)"
        );

        // --- PNG round-trip ---
        let tmp = render_qr_to_png(&fixture_bytes);
        let decoded_png =
            decode_qr_png(tmp.path()).expect("decode_qr_png failed for single_keypress fixture");
        assert_eq!(
            decoded_png, fixture_bytes,
            "decode_qr_png: decoded bytes must equal fixture bytes (single_keypress)"
        );

        // --- Digest decode round-trip ---
        let digest = decode(&decoded_rgba).expect("decode() failed for single_keypress fixture");
        assert_eq!(digest.frame_counter, 0x1234_5678, "frame_counter");
        assert_eq!(digest.framebuffer_hash, 0xCAFE_BABE, "framebuffer_hash");
        assert_eq!(digest.raw_records.len(), 1, "raw_records length");
    }

    /// Round-trip the `mixed_all_variants` fixture through QR encode → RGBA
    /// decode → PNG decode → digest decode.
    ///
    /// `mixed_all_variants.bin` encodes 8 events; the 3 most-recent fit in
    /// the raw-record budget: frame_counter=0xDEADBEEF,
    /// framebuffer_hash=0x12345678.
    #[test]
    fn round_trip_mixed_all_variants() {
        let fixture_bytes = std::fs::read(golden_dir().join("mixed_all_variants.bin"))
            .expect("cannot read mixed_all_variants.bin; run golden tests to seed it");

        // --- RGBA round-trip ---
        let (rgba, width, height) = render_qr_to_rgba(&fixture_bytes);
        let decoded_rgba = decode_qr_rgba(&rgba, width, height)
            .expect("decode_qr_rgba returned None for mixed_all_variants fixture");
        assert_eq!(
            decoded_rgba, fixture_bytes,
            "decode_qr_rgba: decoded bytes must equal fixture bytes (mixed_all_variants)"
        );

        // --- PNG round-trip ---
        let tmp = render_qr_to_png(&fixture_bytes);
        let decoded_png =
            decode_qr_png(tmp.path()).expect("decode_qr_png failed for mixed_all_variants fixture");
        assert_eq!(
            decoded_png, fixture_bytes,
            "decode_qr_png: decoded bytes must equal fixture bytes (mixed_all_variants)"
        );

        // --- Digest decode round-trip ---
        let digest = decode(&decoded_rgba).expect("decode() failed for mixed_all_variants fixture");
        assert_eq!(digest.frame_counter, 0xDEAD_BEEF, "frame_counter");
        assert_eq!(digest.framebuffer_hash, 0x1234_5678, "framebuffer_hash");
        // The 3 most-recent events fit in 44 bytes: ModeSwitch(18) +
        // BootloaderTimeout(10) + ModeCycle(15) = 43 bytes.
        assert_eq!(digest.raw_records.len(), 3, "raw_records length");
    }

    // =========================================================================
    // Negative tests — no QR present
    // =========================================================================

    /// A blank black RGBA buffer (no QR at all) must return `None`.
    #[test]
    fn blank_rgba_returns_none() {
        let width: u32 = 180;
        let height: u32 = 180;
        let blank = vec![0u8; (width * height * 4) as usize];
        let result = decode_qr_rgba(&blank, width, height);
        assert!(
            result.is_none(),
            "decode_qr_rgba must return None for a blank buffer"
        );
    }

    /// A PNG of a blank black image (no QR) must return
    /// `Err(QrError::NoQrFound)`.
    #[test]
    fn blank_png_returns_no_qr_found() {
        let width: u32 = 180;
        let height: u32 = 180;
        let blank: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([0u8, 0u8, 0u8, 255u8]));

        let tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("could not create temp file");
        blank.save(tmp.path()).expect("could not save blank PNG");

        let result = decode_qr_png(tmp.path());
        assert!(
            matches!(result, Err(QrError::NoQrFound)),
            "decode_qr_png must return Err(QrError::NoQrFound) for blank PNG, got {result:?}"
        );
    }

    /// `decode_qr_rgba` must return `None` when the buffer length does not
    /// match the given dimensions.
    #[test]
    fn wrong_buffer_length_returns_none() {
        // Declare 10×10 but supply only 1 pixel worth of data.
        let result = decode_qr_rgba(&[0u8; 4], 10, 10);
        assert!(
            result.is_none(),
            "decode_qr_rgba must return None for wrong-length buffer"
        );
    }
}
