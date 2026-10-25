//! The one error type every fallible operation in this crate returns.

use blastit_frame::FrameError;

/// What can go wrong while the scheduler runs.
///
/// The scheduler only ever fails for reasons it cannot recover from: a broken
/// `ctl.chan`, or a message on it that the protocol does not allow. Everything
/// a client or a view can do — a bad request target, a view that dies, a
/// queue that overflows — is answered with an HTTP status, not with an error
/// (ARCHITECTURE.md §2.4, §3.2).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchedulerError {
    /// A channel's I/O failed.
    #[error("channel I/O: {0}")]
    Io(#[source] std::io::Error),
    /// A peer's outbound queue is over its byte cap, so the scheduler is the
    /// one falling behind: it stops rather than buffering without a bound
    /// (ARCHITECTURE.md §3.1).
    #[error("{peer} has {bytes} bytes queued, over its {cap} byte cap")]
    Overflow {
        /// The channel, for the log line.
        peer: &'static str,
        /// Bytes queued towards that peer.
        bytes: usize,
        /// The cap they went over.
        cap: usize,
    },
    /// A channel carried something the protocol does not allow.
    #[error("channel protocol: {0}")]
    Protocol(String),
    /// The `ctl.chan` peer closed its end, or is gone.
    #[error("ctl.chan closed")]
    Closed,
    /// A frame could not be encoded for a channel.
    #[error("frame: {0}")]
    Frame(#[source] FrameError),
}

impl SchedulerError {
    /// Wraps an I/O error.
    pub fn io(error: std::io::Error) -> Self {
        Self::Io(error)
    }

    /// A protocol violation, with a human-readable detail.
    pub fn protocol(detail: impl Into<String>) -> Self {
        Self::Protocol(detail.into())
    }

    /// The `ctl.chan` peer is gone.
    pub const fn closed() -> Self {
        Self::Closed
    }

    /// A peer's outbound queue went over its cap.
    pub const fn overflow(peer: &'static str, bytes: usize, cap: usize) -> Self {
        Self::Overflow { peer, bytes, cap }
    }

    /// Wraps a frame encoding error.
    pub fn frame(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<FrameError> for SchedulerError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<std::io::Error> for SchedulerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
