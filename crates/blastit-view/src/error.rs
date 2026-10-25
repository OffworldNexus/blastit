//! The one error type [`crate::serve`] returns.

use thiserror::Error;

/// Everything that can make [`crate::serve`] return early.
///
/// Errors are *channel* errors: the scheduler end of `view.chan` could not be
/// read, the bytes on it are not a frame stream, or the scheduler sent a frame
/// a view may not receive. Whatever an application does — raising, returning
/// junk, being cancelled — is handled inside its own request thread and never
/// reaches the caller of [`crate::serve`]; it shows up as the `DONE` frame's
/// outcome and, for a broken channel, as a write error inside that thread.
///
/// When `serve` returns an error it has already waited for every request thread
/// it started, so the process can exit without leaking work.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ViewError {
    /// `view.chan` could not be read, or the initial `READY` could not be sent.
    #[error("view channel I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The scheduler sent bytes that are not a valid frame stream (a bad type
    /// code, a payload that does not match its frame kind, an oversized frame).
    #[error("view channel framing error: {0}")]
    Frame(#[from] blastit_frame::FrameError),

    /// The scheduler sent something that is not part of §3.1's towards-the-app
    /// vocabulary, or a second `REQ_HEAD` for a request that is still running.
    #[error("view protocol error: {0}")]
    Protocol(String),
}

impl From<crate::wire::WriteError> for ViewError {
    fn from(err: crate::wire::WriteError) -> Self {
        match err {
            crate::wire::WriteError::Io(err) => Self::Io(err),
            crate::wire::WriteError::Frame(err) => Self::Frame(err),
        }
    }
}
