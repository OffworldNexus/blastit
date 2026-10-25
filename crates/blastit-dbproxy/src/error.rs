//! The error type dbproxy reports to its caller (init).
//!
//! Every message is dbproxy's own wording: upstream hosts, passwords and CA
//! material are never included, so an error can be logged or forwarded as is
//! (`ARCHITECTURE.md` §8.1: the log stream is the only output).

/// Everything `blastit-dbproxy` can fail with.
///
/// Not exhaustive: more variants may be added without a breaking change. No
/// variant ever carries a credential.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum DbproxyError {
    /// Reading or writing the configuration failed.
    #[error("dbproxy configuration i/o failed: {0}")]
    Io(#[from] std::io::Error),

    /// The configuration is well-formed JSON but not usable (missing proxy,
    /// unknown `sslmode`, a value out of range, …).
    #[error("dbproxy configuration is invalid: {0}")]
    Config(String),

    /// A listener handed over by init could not be adopted.
    #[error("dbproxy listener {index} could not be adopted: {source}")]
    Listener {
        /// Position of the listener in the configuration.
        index: usize,
        /// What went wrong (a bare `io::Error`; it names no secret).
        source: std::io::Error,
    },

    /// A listener's accept loop stopped for good, so dbproxy can no longer
    /// serve its route. init treats this as fatal (`ARCHITECTURE.md` §8).
    #[error("dbproxy listener {index} stopped accepting connections: {message}")]
    Accept {
        /// Position of the listener in the configuration.
        index: usize,
        /// Why the loop gave up.
        message: String,
    },
}
