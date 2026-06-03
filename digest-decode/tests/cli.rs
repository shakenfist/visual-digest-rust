//! Integration tests for the `digest-decode` CLI binary.
//!
//! ## Synthetic PNG fixtures
//!
//! We follow the same synthetic-fixture approach used in
//! `shakenfist-visual-digest/tests/qr.rs`: payloads are encoded into a QR
//! using `qrcodegen` and rendered to PNG using the same colour palette and
//! module size that Sextant uses, so `decode_qr_png` can locate them.
//!
//! Rendering constants are taken from Sextant's renderer:
//!   - `DIGEST_MODULE_PX = 4` → `MODULE_PX = 4` px/module
//!   - `DIGEST_QR_BORDER  = 4` → `QUIET_MODULES = 4` quiet-zone modules
//!   - Foreground: `BltPixel::new(51, 150, 51)` (phosphor green) + alpha 255
//!   - Background: `BltPixel::new(0, 0, 0)` (pure black) + alpha 255

use assert_cmd::Command;
use image::{ImageBuffer, Rgba};
use predicates::prelude::PredicateBooleanExt;
use qrcodegen::{QrCode, QrCodeEcc};

// =========================================================================
// Rendering constants — mirror of shakenfist-visual-digest/tests/qr.rs
// =========================================================================

/// Pixels per QR module.
const MODULE_PX: u32 = 4;

/// Quiet-zone width in modules.
const QUIET_MODULES: u32 = 4;

/// Foreground: Sextant phosphor green.
const FG_RGBA: [u8; 4] = [51, 150, 51, 255];

/// Background: pure black.
const BG_RGBA: [u8; 4] = [0, 0, 0, 255];

// =========================================================================
// Rendering helpers
// =========================================================================

/// Encode `payload` as a QR (ECC Low, binary mode) and render it to a
/// temporary PNG file.  Returns the `NamedTempFile` — the caller must keep
/// it alive for the duration of the test.
fn render_to_temp_png(payload: &[u8]) -> tempfile::NamedTempFile {
    let qr =
        QrCode::encode_binary(payload, QrCodeEcc::Low).expect("qrcodegen failed to encode payload");

    let qr_modules = qr.size() as u32;
    let side = (qr_modules + 2 * QUIET_MODULES) * MODULE_PX;

    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(side, side);

    for py in 0..side {
        for px in 0..side {
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

    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("could not create temp file");
    img.save(tmp.path()).expect("could not save QR PNG");
    tmp
}

// =========================================================================
// Tests
// =========================================================================

/// Happy path: decode a synthetic PNG for the `single_keypress` golden
/// fixture, assert exit 0, parse the JSON, and check key fields.
///
/// `single_keypress.bin` encodes:
///   frame_counter    = 0x12345678 (305_419_896 decimal)
///   framebuffer_hash = 0xCAFEBABE (3_405_691_582 decimal)
///   one Keypress record
#[test]
fn happy_path() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../shakenfist-visual-digest/tests/golden/single_keypress.bin");
    let bytes = std::fs::read(&fixture).expect("load golden fixture");

    let tmp = render_to_temp_png(&bytes);

    let output = Command::cargo_bin("digest-decode")
        .expect("digest-decode binary not found")
        .arg(tmp.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout is not UTF-8");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is not valid JSON");

    // frame_counter: 0x12345678 = 305_419_896 decimal
    assert_eq!(
        json["frame_counter"]
            .as_u64()
            .expect("frame_counter missing or not a number"),
        0x1234_5678_u64,
        "frame_counter must be 0x12345678"
    );

    // framebuffer_hash: 0xCAFEBABE = 3_405_691_582 decimal
    assert_eq!(
        json["framebuffer_hash"]
            .as_u64()
            .expect("framebuffer_hash missing or not a number"),
        0xCAFE_BABE_u64,
        "framebuffer_hash must be 0xCAFEBABE"
    );

    // raw_records: exactly one entry
    let records = json["raw_records"]
        .as_array()
        .expect("raw_records missing or not an array");
    assert_eq!(records.len(), 1, "expected exactly one raw record");

    // The record must have a "Keypress" key (externally-tagged serde default).
    let record = &records[0];
    assert!(
        record.get("Keypress").is_some(),
        "expected raw_records[0] to have key 'Keypress', got: {record}"
    );
}

/// Usage error: invoking with no arguments must exit code 1 and print
/// something containing "usage:" to stderr.
#[test]
fn usage_no_args() {
    Command::cargo_bin("digest-decode")
        .expect("digest-decode binary not found")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("usage:"));
}

/// I/O error: a path that does not exist must exit code 2 with non-empty
/// stderr.
#[test]
fn file_not_found() {
    Command::cargo_bin("digest-decode")
        .expect("digest-decode binary not found")
        .arg("/tmp/this-file-does-not-exist-92347.png")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::is_empty().not());
}

/// No QR: a fully-black 100×100 PNG must exit code 3 with stderr
/// containing a substring that identifies the "no QR" failure.
#[test]
fn blank_png_no_qr() {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgba([0u8, 0u8, 0u8, 255u8]));
    let tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("could not create temp file");
    img.save(tmp.path()).expect("could not save blank PNG");

    Command::cargo_bin("digest-decode")
        .expect("digest-decode binary not found")
        .arg(tmp.path())
        .assert()
        .failure()
        .code(3)
        .stderr(predicates::str::contains("no QR"));
}

/// Malformed QR payload: a QR containing bytes that are not a valid digest
/// must exit code 4.
#[test]
fn malformed_qr_payload() {
    // Any bytes that successfully decode as a QR but fail the digest parser.
    // The magic check fires immediately on "NOTASEXTANTDIGEST!".
    let bad_payload = b"NOTASEXTANTDIGEST!";
    let tmp = render_to_temp_png(bad_payload);

    Command::cargo_bin("digest-decode")
        .expect("digest-decode binary not found")
        .arg(tmp.path())
        .assert()
        .failure()
        .code(4)
        .stderr(predicates::str::is_empty().not());
}
