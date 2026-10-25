//! The one error type the front end reports to init.

use std::io;

use blastit_frame::FrameError;
use thiserror::Error;

/// Everything that ends [`serve`](crate::serve) early.
///
/// A failed *client* connection is not one of these: it is normal, it ends the
/// connection, and the server keeps running. Only the scheduler channel (and
/// the listener) can produce this.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HttpError {
    /// The scheduler end of `http.chan` went away (EOF) or was already gone.
    ///
    /// init treats this as fatal: without the scheduler there is nothing to
    /// route to.
    #[error("http.chan closed")]
    ChannelClosed,

    /// Reading from or writing to `http.chan` failed.
    #[error("http.chan i/o error: {0}")]
    ChannelIo(#[source] io::Error),

    /// A frame on `http.chan` could not be encoded or decoded.
    #[error("http.chan frame error: {0}")]
    Frame(#[from] FrameError),

    /// The listening socket could not accept a connection.
    #[error("accepting a client connection failed: {0}")]
    Accept(#[source] io::Error),
}
