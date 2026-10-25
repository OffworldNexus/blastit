//! HTTP/1.1 front end: client connections to frames on `http.chan`
//! (ARCHITECTURE.md §2.3).
//!
//! This is the internet-facing parser. It holds no Python, no secrets and no
//! file access beyond what it inherited; its only sockets are the listener init
//! bound and its end of the `http.chan` socketpair. It parses HTTP/1.1,
//! enforces the limits from §8.2, normalises what it forwards, and encodes what
//! comes back — a view never writes raw bytes to a client.
//!
//! ```
//! use std::sync::Arc;
//!
//! use blastit_events::Event;
//! use blastit_http::{HttpConfig, Emit, HttpError, serve};
//!
//! # async fn run(
//! #     listener: tokio::net::TcpListener,
//! #     chan: tokio::net::UnixStream,
//! # ) -> Result<(), HttpError> {
//! // init passes the listener it bound, its end of `http.chan`, the
//! // configuration it read from the environment, and the events pipe.
//! let emit: Emit = Arc::new(|_event: Event| eprintln!("one structured event"));
//! serve(listener, chan, HttpConfig::default(), emit).await
//! # }
//! ```
//!
//! # What it does
//!
//! * **Accept and parse.** [`serve`] takes the inherited listening socket and
//!   the inherited channel; [`serve_with`] and [`Acceptor`] are the same loop
//!   with both left generic. Each connection is handled by its own task, one
//!   request at a time, strictly in order, so leftover bytes after a head are
//!   simply the start of the next pipelined request. Above
//!   [`HttpConfig::max_connections`], an accepted connection is answered with
//!   `503` at once, so a load balancer gets a reply instead of a timeout.
//! * **Enforce.** `431` for a head over `max_header_bytes`, `408` for a head
//!   that does not finish inside `header_timeout`, a silent close for an idle
//!   keep-alive connection, `408` again for a body that cannot finish inside
//!   `body_timeout`, and the framing and protocol refusals of §2.3: a malformed
//!   request, a bare LF where a CRLF belongs, a target that is not origin form,
//!   `CONNECT`, a missing or repeated `Host` on HTTP/1.1, `Content-Length`
//!   together with `Transfer-Encoding`, any `Transfer-Encoding` on HTTP/1.0 or
//!   anything but exactly `chunked`, `Upgrade`, any other `Expect`, a version
//!   that is not HTTP/1.0 or 1.1, and a body over `max_body_bytes`. Every
//!   refusal writes an `http.rejected` event (§8.1) and closes the connection.
//! * **Normalise.** Hop-by-hop fields never cross a hop in either direction;
//!   the client address and scheme come from `X-Forwarded-For` /
//!   `X-Forwarded-Proto` only when [`HttpConfig::trust_forwarded`] is set;
//!   `server_name` is `Host` without its port.
//! * **Encode.** Status line, validated headers and `Content-Length`,
//!   chunked or close-delimited framing, chosen here and not by the view — and
//!   never a length on a 1xx or 204, which cannot have a body at all.
//! * **Cancel.** A client that goes away mid-request, or a response that
//!   cannot be finished, turns into `CANCEL` on that stream; later frames for
//!   it are dropped.
//!
//! # One slow client cannot hold up the rest
//!
//! Every request shares one `http.chan` reader, so the reader never waits for a
//! connection: frames go into a per-stream queue that does not block, and
//! `CANCEL` takes a queue of its own that the writer always drains first.
//!
//! What bounds those buffers is credit, not blocking (§3.1). Each body, in each
//! direction, starts with [`INITIAL_WINDOW`] bytes of it:
//!
//! * **Request bodies.** The front end sends at most the credit it has. When it
//!   runs out it stops reading that client's socket — and only that client's —
//!   until the scheduler forwards a `WINDOW` for the stream. A slow app slows
//!   its own client down, over TCP; `body_timeout` still applies while the
//!   upload waits, so a stalled app cannot hold a connection forever.
//! * **Response bodies.** The front end grants credit back as it writes to the
//!   client socket: batched in 64 KiB worth of writes, and once more whenever
//!   the response goes quiet, because a view that is *waiting* for credit has
//!   nothing left to send. A slow client slows its own request thread down.
//!
//! The per-stream buffer cap ([`HttpConfig::stream_buffer_bytes`], the window
//! plus one frame by default) is therefore the *protocol-violation* guard: a
//! peer that sends past its credit has broken the protocol, and that one stream
//! is cancelled, its connection aborted and everything still arriving for it
//! dropped. Every other connection carries on, because the reader was never
//! waiting for it.
//!
//! # What it never logs
//!
//! §8.1 forbids logging query strings, header values and bodies, and the only
//! event this crate emits carries an address, a status and a fixed label. That
//! is visible in the signature of [`Emit`]: a caller cannot hand it anything
//! but a [`blastit_events::Event`], and no `Event` field can hold a header
//! value.
//!
//! # Layout
//!
//! * [`HttpConfig`] — the §8.2 knobs, filled in by init.
//! * [`Server`], [`serve`], [`serve_with`], [`Acceptor`], [`TcpAcceptor`] —
//!   accepting and running connections.
//! * [`Chan`], [`Stream`] — the client side of `http.chan`, shared by every
//!   connection.
//! * [`HttpError`] — the only failure that stops the server.
//! * [`canonical_reason`] — the reason phrases used when a view sends none.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::sync::Arc;

use blastit_events::Event;

mod chan;
mod chunked;
mod config;
mod conn;
mod error;
mod head;
mod headers;
mod reason;
mod response;
mod server;

pub use crate::chan::{Chan, Stream};
pub use crate::config::HttpConfig;
pub use crate::error::HttpError;
pub use crate::head::{BodyFraming, Reject, RequestHead, Version};
pub use crate::reason::canonical_reason;
#[cfg(unix)]
pub use crate::server::serve;
pub use crate::server::{Acceptor, Server, TcpAcceptor, serve_with};
pub use blastit_frame::INITIAL_WINDOW;

/// Where the front end sends its structured events (ARCHITECTURE.md §8.1).
///
/// init collects them over the events pipe and is the only process that writes
/// to stdout. The closure is called from the connection tasks, so it must be
/// cheap and must not block; it may be called concurrently.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
