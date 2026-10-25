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
//!   simply the start of the next pipelined request.
//! * **Enforce.** `431` for a head over `max_header_bytes`, `408` for a head
//!   that does not finish inside `header_timeout`, a silent close for an idle
//!   keep-alive connection, `400`/`413`/`417`/`501`/`505` for the framing and
//!   protocol refusals listed in §2.3. Every refusal writes an
//!   `http.rejected` event (§8.1) and closes the connection.
//! * **Normalise.** Hop-by-hop fields never cross a hop in either direction;
//!   the client address and scheme come from `X-Forwarded-For` /
//!   `X-Forwarded-Proto` only when [`HttpConfig::trust_forwarded`] is set;
//!   `server_name` is `Host` without its port.
//! * **Encode.** Status line, validated headers and `Content-Length`,
//!   chunked or close-delimited framing, chosen here and not by the view.
//! * **Cancel.** A client that goes away mid-request, or a response that
//!   cannot be finished, turns into `CANCEL` on that stream; later frames for
//!   it are dropped.
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

/// Where the front end sends its structured events (ARCHITECTURE.md §8.1).
///
/// init collects them over the events pipe and is the only process that writes
/// to stdout. The closure is called from the connection tasks, so it must be
/// cheap and must not block; it may be called concurrently.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
