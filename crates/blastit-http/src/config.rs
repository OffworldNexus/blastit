//! Configuration of the front end (ARCHITECTURE.md §8.2).
//!
//! Every knob here has a `BLASTIT_*` (or `PORT`) environment variable behind
//! it, but reading the environment is **not** this crate's job: init parses the
//! variables and hands the front end a filled-in [`HttpConfig`].

use std::time::Duration;

use blastit_frame::{INITIAL_WINDOW, Limits, MAX_FRAME_DEFAULT};

/// One mebibyte, spelled out so the defaults read like §8.2's table.
const KIB: usize = 1024;
const MIB: u64 = 1024 * 1024;

/// What the HTTP front end needs to know.
///
/// The [`Default`] is the §8.2 column: `BLASTIT_MAX_HEADER_BYTES=64 KiB`,
/// `BLASTIT_MAX_BODY_BYTES=100 MiB`, `BLASTIT_HEADER_TIMEOUT=10s`,
/// `BLASTIT_KEEPALIVE_TIMEOUT=75s`, `BLASTIT_TRUST_FORWARDED=1`,
/// `BLASTIT_FRAME_MAX=1 MiB` and `PORT=8080`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    /// Largest request head (request line plus headers), in bytes.
    ///
    /// A head that is still incomplete once this many bytes have arrived is
    /// refused with `431`.
    pub max_header_bytes: usize,
    /// Largest request body, in bytes. Applies to a declared `Content-Length`
    /// and, chunk by chunk, to a chunked body.
    pub max_body_bytes: u64,
    /// How long a request head may take to arrive.
    ///
    /// Measured from the first byte of the request, or — for the first request
    /// on a connection — from the moment it was accepted.
    pub header_timeout: Duration,
    /// How long a connection may sit idle without a single byte of the next
    /// request, before it is closed silently.
    ///
    /// The same idle bound applies to a request body that stops arriving.
    pub keepalive_timeout: Duration,
    /// Whether `X-Forwarded-For` and `X-Forwarded-Proto` are believed.
    ///
    /// They are only as trustworthy as the load balancer that writes them, so
    /// this must stay off when the front end is reachable directly.
    pub trust_forwarded: bool,
    /// Frame size limit of `http.chan`, from `BLASTIT_FRAME_MAX`.
    pub frame_limits: Limits,
    /// `SERVER_PORT` as the app sees it: the port the load balancer published,
    /// not the one this process is bound to.
    pub server_port: u16,
    /// Most response bytes one request may have queued for a client that is not
    /// reading them yet.
    ///
    /// Each request buffers on its own: a client that stops reading its
    /// response can fill this much and no more, and then only its own stream
    /// fails. The shared channel reader never waits for a client, so no stream
    /// can delay another.
    ///
    /// The default is `INITIAL_WINDOW + max_frame`: a well-behaved view never
    /// sends more than its credit (one window), and the extra frame is the one
    /// that can be in flight when the credit runs out. Anything past that is a
    /// peer sending beyond credit, and aborts the stream.
    pub stream_buffer_bytes: usize,
    /// Most client connections served at once.
    ///
    /// Above it, connections are accepted and answered with `503` straight
    /// away, so the load balancer gets a reply instead of a timeout.
    pub max_connections: usize,
    /// How long a request body may take in total, from the end of its head.
    ///
    /// App Platform closes uploads at 600 s, so waiting longer than that only
    /// ties up a connection for a request that can never complete.
    pub body_timeout: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            max_header_bytes: 64 * KIB,
            max_body_bytes: 100 * MIB,
            header_timeout: Duration::from_secs(10),
            keepalive_timeout: Duration::from_secs(75),
            trust_forwarded: true,
            frame_limits: Limits::default(),
            server_port: 8080,
            stream_buffer_bytes: INITIAL_WINDOW as usize + MAX_FRAME_DEFAULT,
            max_connections: 1024,
            body_timeout: Duration::from_secs(600),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_architecture_table() {
        let cfg = HttpConfig::default();
        assert_eq!(cfg.max_header_bytes, 64 * 1024);
        assert_eq!(cfg.max_body_bytes, 100 * 1024 * 1024);
        assert_eq!(cfg.header_timeout, Duration::from_secs(10));
        assert_eq!(cfg.keepalive_timeout, Duration::from_secs(75));
        assert_eq!(
            cfg.stream_buffer_bytes,
            INITIAL_WINDOW as usize + cfg.frame_limits.max_frame(),
            "the window plus the frame that may be in flight when it runs out"
        );
        assert_eq!(cfg.max_connections, 1024);
        assert_eq!(cfg.body_timeout, Duration::from_secs(600));
        assert!(cfg.trust_forwarded, "§8.2 defaults trust_forwarded to 1");
        assert_eq!(cfg.frame_limits, Limits::default());
        assert_eq!(cfg.server_port, 8080);
    }
}
