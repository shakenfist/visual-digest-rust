//! QR-code locate and decode helper.
//!
//! Gated on the `qr` feature, which pulls in `rqrr` (QR detector/decoder)
//! and `image` (PNG loading and pixel-format conversion).  The `qr` feature
//! transitively enables `decode`, so [`crate::decode`] and friends are
//! available to callers who depend on this module.
//!
//! ## Entry points
//!
//! * [`decode_qr_rgba`] — take a raw RGBA framebuffer snapshot (Ryll's hot
//!   path against `SurfaceMirror`) and return the QR's byte-mode payload.
//! * [`decode_qr_png`] — open a PNG file on disk and return the same.
//!
//! ## Raw-bytes decode
//!
//! rqrr's [`rqrr::Grid::decode`] returns `(MetaData, String)`, which
//! would lose non-UTF-8 bytes from a 106-byte binary payload through lossy
//! conversion.  The correct path is [`rqrr::Grid::decode_to`], which writes
//! raw decoded bytes (after ECC correction and de-interleaving) to any
//! `std::io::Write` impl.  We use a `Vec<u8>` as the sink, giving us the
//! exact byte sequence that was encoded — no UTF-8 re-encoding involved.
//!
//! See the rqrr source at `src/lib.rs` (`Grid::decode_to`) and the internal
//! `src/decode.rs` (`decode` function) for confirmation that the writer
//! receives raw, mode-decoded bytes in the order they were encoded, including
//! length-indicator stripped per the QR spec, before any UTF-8 validation.

use std::path::Path;

use image::{ImageBuffer, Luma};
use rqrr::PreparedImage;

/// Errors that can occur when loading and decoding a QR from a PNG file.
#[derive(Debug, thiserror::Error)]
pub enum QrError {
    /// The file could not be opened or read.
    #[error("IO error reading QR PNG: {0}")]
    Io(#[from] std::io::Error),

    /// The `image` crate could not decode the file as a PNG (or other
    /// supported format).  A manual `From` impl is used rather than
    /// `#[from]` because `image::ImageError` does not unconditionally
    /// implement `std::error::Error` across all crate versions.
    #[error("image decode error: {0}")]
    Decode(image::ImageError),

    /// `rqrr` detected no decodable QR code in the image.
    #[error("no QR code found in image")]
    NoQrFound,
}

impl From<image::ImageError> for QrError {
    fn from(e: image::ImageError) -> Self {
        QrError::Decode(e)
    }
}

/// Locate and decode a QR code in a raw RGBA pixel buffer.
///
/// Used by Ryll's headless mode against framebuffer snapshots from
/// `SurfaceMirror`.  Returns the QR's byte-mode payload, or `None` if
/// no QR can be located or decoded.
///
/// The returned `Vec<u8>` is the raw decoded payload exactly as it was
/// encoded — suitable for feeding directly to [`crate::decode`].
///
/// # Parameters
///
/// * `rgba` — raw RGBA pixel data, row-major, 4 bytes per pixel.
/// * `width` — image width in pixels.
/// * `height` — image height in pixels.
///
/// # Returns
///
/// `Some(bytes)` if a QR was found and successfully decoded, `None`
/// if the buffer is malformed (wrong length) or contains no decodable QR.
pub fn decode_qr_rgba(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let expected_len = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected_len {
        return None;
    }

    // Convert RGBA to Luma<u8> using the ITU-R BT.601 luma coefficients.
    // These are the canonical weights for RGB→Y conversion that most
    // imaging pipelines use.  Alpha is ignored — rqrr only wants luma.
    //
    // Sextant renders QR modules as phosphor-green on a pure-black background.
    // In a standard QR code the data modules are "dark" (low luma) and the
    // background is "light" (high luma).  Sextant's scheme is the opposite:
    // modules are brighter than the background (green > black).  rqrr's
    // adaptive thresholding classifies pixels below the local average as
    // "Black" (= module), so we invert the luma (255 − L) to restore the
    // conventional polarity: green modules become dark, black background
    // becomes bright, and rqrr sees the QR in the expected orientation.
    let luma: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
        let base = ((y * width + x) * 4) as usize;
        let r = rgba[base] as u32;
        let g = rgba[base + 1] as u32;
        let b = rgba[base + 2] as u32;
        // Weights: R×299 + G×587 + B×114, divided by 1000.
        let luma_val = ((r * 299 + g * 587 + b * 114) / 1000) as u8;
        // Invert so that bright foreground modules appear dark to rqrr.
        Luma([255 - luma_val])
    });

    decode_from_luma(luma)
}

/// Locate and decode a QR code in a PNG file on disk.
///
/// Used by the `digest-decode` CLI binary (step 1g).
///
/// # Errors
///
/// Returns [`QrError::Io`] if the file cannot be opened,
/// [`QrError::Decode`] if the `image` crate cannot parse it, or
/// [`QrError::NoQrFound`] if `rqrr` finds no decodable QR.
pub fn decode_qr_png<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, QrError> {
    let img = image::open(path.as_ref())?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let raw = rgba.into_raw();
    decode_qr_rgba(&raw, width, height).ok_or(QrError::NoQrFound)
}

/// Internal: run rqrr detection + decoding on a Luma8 image buffer.
///
/// Returns the raw byte payload of the first successfully decoded QR grid,
/// or `None` if no grid decodes.
fn decode_from_luma(luma: ImageBuffer<Luma<u8>, Vec<u8>>) -> Option<Vec<u8>> {
    let mut prepared = PreparedImage::prepare(luma);
    let grids = prepared.detect_grids();

    for grid in grids {
        // `decode_to` writes the raw, ECC-corrected, mode-decoded bytes to
        // the supplied `Write` impl.  This is the correct raw-bytes path:
        // it bypasses `String::from_utf8` entirely, so non-UTF-8 binary
        // payloads (like our 106-byte digest) are returned intact.
        let mut payload: Vec<u8> = Vec::new();
        if grid.decode_to(&mut payload).is_ok() {
            return Some(payload);
        }
    }

    None
}
