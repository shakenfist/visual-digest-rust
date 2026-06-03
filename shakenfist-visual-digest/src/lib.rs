#![cfg_attr(not(feature = "decode"), no_std)]
//! Visual on-screen digest format shared between
//! `shakenfist/uncalibrated-sextant` (encoder) and `shakenfist/ryll`
//! (future decoder). The wire-format specification lives at
//! `docs/visual-digest-format.md` in this repository.
//!
//! Default features = encoder only, `no_std`-safe. Enable the `decode`
//! feature to add the decoder (requires `alloc`). See `Cargo.toml`
//! for the full feature matrix.

pub mod encoder;
pub mod events;
pub mod format;
pub mod hashes;

#[cfg(feature = "decode")]
pub mod decoder;

#[cfg(feature = "qr")]
pub mod qr;

// Re-export the public API at the crate root for caller convenience.
// Sextant (step 1h) imports these names via
// `shakenfist_visual_digest::{encode, ChannelHashes, ...}`.
pub use encoder::encode;
pub use events::{BootloaderChoice, Event, Phase};
pub use format::{
    EncodeError, CRC32C, DIGEST_FIXED_OVERHEAD, DIGEST_HEADER_LEN, DIGEST_MAGIC,
    DIGEST_PAYLOAD_CAPACITY, DIGEST_SCHEMA_VERSION, DIGEST_TRAILER_LEN, MAX_RECORD_SIZE,
    NUM_HASH_CHANNELS, RECORD_HASH_SIZE,
};
pub use hashes::ChannelHashes;

// Decoder re-exports, gated on the `decode` feature.
#[cfg(feature = "decode")]
pub use decoder::{decode, DecodeError, Digest, Record, UnknownRecord};

// QR re-exports, gated on the `qr` feature.
#[cfg(feature = "qr")]
pub use qr::{decode_qr_png, decode_qr_rgba, QrError};
