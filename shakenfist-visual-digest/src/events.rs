//! Event vocabulary for the visual on-screen digest.
//!
//! `Event`, `Phase`, and `BootloaderChoice` are the on-wire types —
//! every `Event` variant maps 1:1 to a TLV tag, `Phase` and
//! `BootloaderChoice` have stable wire discriminants documented in
//! `docs/visual-digest-format.md`.
//!
//! Copied verbatim from `uncalibrated-sextant/src/event.rs` for the
//! three wire-format types. `RingBuffer` stays in Sextant — the
//! encoder API takes a `&[&Event]` slice that the caller materialises
//! from its container of choice.

/// Scene phases, ordered by progression through a single boot run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Phase {
    /// AWAITING OPERATOR — waiting for first keypress.
    Awaiting,
    /// Boot sequence playing out.
    Booting,
    /// Parked on SYSTEM ONLINE screen, waiting for final keypress.
    Parked,
}

impl Phase {
    /// Short lowercase tag used by the serial drain's one-line-per-event
    /// format. Kept stable so Ryll's future parser can match literally.
    pub fn tag(&self) -> &'static str {
        match self {
            Phase::Awaiting => "awaiting",
            Phase::Booting => "booting",
            Phase::Parked => "parked",
        }
    }
}

/// Operator's choice at the locked-bootloader R/I/A prompt.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum BootloaderChoice {
    /// Operator chose (R)etry — re-run the decryption attempt.
    Retry,
    /// Operator chose (I)gnore — proceed to the paste blob screen.
    Ignore,
    /// Operator chose (A)bort — cold-reset immediately.
    Abort,
}

impl BootloaderChoice {
    /// Short lowercase tag used by the serial drain's one-line-per-event
    /// format. Kept stable so Ryll's future parser can match literally.
    pub fn tag(&self) -> &'static str {
        match self {
            BootloaderChoice::Retry => "retry",
            BootloaderChoice::Ignore => "ignore",
            BootloaderChoice::Abort => "abort",
        }
    }
}

/// Events recorded during a run. Each variant maps to a TLV tag in
/// the digest wire format; see `format.rs` for tag constants and
/// `docs/visual-digest-format.md` for the full wire specification.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Event {
    /// A key was pressed by the operator.
    Keypress {
        unicode: char,
        scancode: u16,
        timestamp_ms: u64,
    },
    /// A text line was rendered to the screen.
    LineRendered { row: usize, timestamp_ms: u64 },
    /// The scene transitioned between phases.
    SceneTransition {
        from: Phase,
        to: Phase,
        timestamp_ms: u64,
    },
    /// Operator made a choice at the locked-bootloader R/I/A prompt.
    BootloaderDecision {
        choice: BootloaderChoice,
        /// 1-indexed count of times the prompt has been rendered so far.
        attempt: u32,
        timestamp_ms: u64,
    },
    /// A paste was received and validated at the awaiting-payload prompt.
    PasteReceived {
        /// Number of bytes in the paste (excluding any trailing CR/LF terminator).
        len: usize,
        /// Whether the paste matched the expected payload byte-exactly.
        correct: bool,
        timestamp_ms: u64,
    },
    /// The silent-wait timer elapsed; the visible countdown is about to begin.
    BootloaderTimeout { timestamp_ms: u64 },
    /// GOP mode switched (or attempted to switch) at the
    /// operator's request.
    ModeSwitch {
        requested_w: u32,
        requested_h: u32,
        applied_w: u32,
        applied_h: u32,
        timestamp_ms: u64,
    },
    /// Cycle-through-all-modes walk completed (or was
    /// interrupted). `count` is the number of mode switches
    /// performed during the cycle.
    ModeCycle {
        count: u32,
        interrupted: bool,
        timestamp_ms: u64,
    },
}
