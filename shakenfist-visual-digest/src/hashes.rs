//! Per-channel rolling CRC32C accumulators for the visual digest.
//!
//! `ChannelHashes` is updated on every event push and embedded in
//! each digest payload so the decoder can verify per-event-type
//! coverage without replaying the full event stream. See
//! `docs/visual-digest-format.md` for the chaining algebra and
//! the on-wire encoding of hash records.

use crate::encoder::event_tlv_bytes;
use crate::events::Event;
use crate::format::{CRC32C, MAX_RECORD_SIZE};

/// Per-channel rolling CRC32C accumulators; one per event variant.
///
/// Each field accumulates CRC32C over every TLV-encoded event of that
/// variant since boot, in push order. The hash is updated on every
/// `push_event` call and stored as the _finalized_ CRC32C value
/// (i.e., the value `Digest::finalize()` returns). An empty channel
/// carries `0x00000000` — the CRC32C of zero bytes.
///
/// ## Chaining invariant
///
/// CRC_32_ISCSI (`refin=true`, `refout=true`, `xorout=0xFFFFFFFF`):
/// given a finalized value `f`, the internal pre-finalization state is
/// `raw = f ^ 0xFFFF_FFFF`. Because `init()` applies
/// `initial.reverse_bits()` (for `refin=true`), the correct argument
/// to `Crc::digest_with_initial` for resuming from `f` is
/// `(f ^ 0xFFFF_FFFF).reverse_bits()`. The `update_channel` method
/// encapsulates this so callers stay algorithm-agnostic.
///
/// ## Field naming
///
/// Named fields (one per `Event` variant) rather than an indexed
/// array. Named fields are self-documenting at each call site, let
/// the compiler enforce completeness in `update`, and make the
/// encoder read-path (`channel_hashes.keypress`, etc.) explicit.
/// The tag-indexed array alternative would require a safe mapping from
/// `TAG_*` constants to array indices; the named approach avoids that
/// indirection at the cost of eight field names instead of an indexing
/// expression.
///
/// ## Reachability
///
/// All eight fields are read by `ChannelHashes::update` (write path)
/// and consumed by the TLV encoder. No `#[allow(dead_code)]`
/// annotation is added; if a field appears dead before the encoder
/// lands, that is expected and benign — the compiler will not warn
/// because `update` writes every field on every matching push.
pub struct ChannelHashes {
    /// Running CRC32C over all `Event::Keypress` TLV records (tag 0x01).
    pub keypress: u32,
    /// Running CRC32C over all `Event::LineRendered` TLV records (tag 0x02).
    pub line_rendered: u32,
    /// Running CRC32C over all `Event::SceneTransition` TLV records (tag 0x03).
    pub scene_transition: u32,
    /// Running CRC32C over all `Event::BootloaderDecision` TLV records (tag 0x04).
    pub bootloader_decision: u32,
    /// Running CRC32C over all `Event::PasteReceived` TLV records (tag 0x05).
    pub paste_received: u32,
    /// Running CRC32C over all `Event::BootloaderTimeout` TLV records (tag 0x06).
    pub bootloader_timeout: u32,
    /// Running CRC32C over all `Event::ModeSwitch` TLV records (tag 0x07).
    pub mode_switch: u32,
    /// Running CRC32C over all `Event::ModeCycle` TLV records (tag 0x08).
    pub mode_cycle: u32,
}

impl Default for ChannelHashes {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelHashes {
    /// Initialise all eight accumulators to the CRC32C of zero bytes
    /// (`0x00000000` — the finalized value of an empty `Digest`).
    pub const fn new() -> Self {
        Self {
            keypress: 0,
            line_rendered: 0,
            scene_transition: 0,
            bootloader_decision: 0,
            paste_received: 0,
            bootloader_timeout: 0,
            mode_switch: 0,
            mode_cycle: 0,
        }
    }

    /// Compute the `digest_with_initial` argument required to resume a
    /// CRC_32_ISCSI (`refin=true`, `refout=true`, `xorout=0xFFFF_FFFF`)
    /// stream from a previously-finalized value.
    ///
    /// `finalize` applied `raw ^ xorout`, so `raw = f ^ xorout`.
    /// `init` applies `initial.reverse_bits()` (for `refin=true`), so
    /// the `digest_with_initial` argument is `raw.reverse_bits()`.
    #[inline]
    pub fn resume_initial(finalized: u32) -> u32 {
        (finalized ^ 0xFFFF_FFFF).reverse_bits()
    }

    /// Extend one channel's running CRC32C with the TLV bytes of
    /// `event` and return the new finalized value.
    #[inline]
    pub fn extend(current: u32, event: &Event) -> u32 {
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let len = event_tlv_bytes(event, &mut buf);
        let mut d = CRC32C.digest_with_initial(Self::resume_initial(current));
        d.update(&buf[..len]);
        d.finalize()
    }

    /// Update the accumulator for the channel matching `event`'s variant.
    ///
    /// Dispatches on the event variant and extends the corresponding
    /// field via `extend`. Every variant is covered so the compiler
    /// enforces completeness; if a new `Event` variant is added without
    /// updating this match, the code will not compile.
    pub fn update(&mut self, event: &Event) {
        match event {
            Event::Keypress { .. } => {
                self.keypress = Self::extend(self.keypress, event);
            }
            Event::LineRendered { .. } => {
                self.line_rendered = Self::extend(self.line_rendered, event);
            }
            Event::SceneTransition { .. } => {
                self.scene_transition = Self::extend(self.scene_transition, event);
            }
            Event::BootloaderDecision { .. } => {
                self.bootloader_decision = Self::extend(self.bootloader_decision, event);
            }
            Event::PasteReceived { .. } => {
                self.paste_received = Self::extend(self.paste_received, event);
            }
            Event::BootloaderTimeout { .. } => {
                self.bootloader_timeout = Self::extend(self.bootloader_timeout, event);
            }
            Event::ModeSwitch { .. } => {
                self.mode_switch = Self::extend(self.mode_switch, event);
            }
            Event::ModeCycle { .. } => {
                self.mode_cycle = Self::extend(self.mode_cycle, event);
            }
        }
    }
}
