//! dbproxy: one PostgreSQL listener per (route, proxy), identity from the
//! listener and `SCM_CREDENTIALS`, per-route SQL allowlists
//! (`ARCHITECTURE.md` §2.2, §3, §4, §5.2, §6, §8).
//!
//! init `fork`s and `execve`s dbproxy with an **empty environment**: everything
//! it knows arrives over `secrets.pipe` ([`read_config`]) together with the
//! listening sockets it bound in stage 0 ([`run`] adopts them by fd). Views then
//! connect over a Unix socket of their own — `/proc/self/cwd/.s.PGSQL.<port>`
//! — and dbproxy answers with pregresql, blastit's two hooks, and the real
//! upstream credentials the view must never see.
//!
//! # How a connection is decided
//!
//! * **Identity is the socket, checked twice.** The listener's file permissions
//!   decide who *can* connect; the `SCM_CREDENTIALS` the kernel attaches to the
//!   client's first message decide who *did*. dbproxy requires that uid to
//!   equal the uid init declared for that listener, and never consults
//!   `SO_PEERCRED`, which under gVisor reports the listening process (§2.2).
//!   Root is not accepted: a view is never root. A cancel request is checked the
//!   same way, and against the listener's *own* session table, so a view can
//!   only cancel its own work.
//! * **The client authenticates with nothing** (`trust`), because the socket
//!   already proved who it is, and its startup packet is filtered: only `user`,
//!   `database`, `application_name` and a UTF-8 `client_encoding` are accepted,
//!   and only `application_name` is passed upstream. `options`, `search_path`,
//!   `role`, `session_authorization`, `replication` and the
//!   `default_transaction_*` settings are refused with `28000` and a
//!   `pg.refused` event: a view's session is defined by its listener, not by
//!   what it claims. The `user` and `database` values themselves are ignored;
//!   the session runs as the proxy's real role against the proxy's real
//!   database.
//! * **Every statement is checked** against the listener's route: its effective
//!   queries (§6) are matched exactly after normalisation. A denial is answered
//!   with `42501` before it can reach the server, and logged as `sql.denied` —
//!   with `kind: "copy"` when the refused statement is a `COPY`.
//! * **Each proxy has a connection cap**; the connection that would exceed it is
//!   refused with `53300` and logged as `pg.refused`. A connection that never
//!   finishes its startup phase holds no slot, is bounded per listener, and is
//!   dropped after ten seconds without a startup packet.
//! * **Nothing is logged anywhere but [`Emit`]**: dbproxy's stdout belongs to
//!   init, and pregresql's `tracing` output is never collected, so dbproxy
//!   installs no subscriber.
//!
//! # What a view may learn about the upstream
//!
//! Nothing that identifies it. dbproxy dials the upstream itself before it
//! hands a session over (see *An upstream without a name*), names no host, port
//! or role in any message to a view, and holds back everything pregresql writes
//! while a session is being set up until it knows the setup succeeded. When it
//! did not, the client receives dbproxy's own fixed message rather than
//! pregresql's — which would name the host — or PostgreSQL's, which would name
//! the role.
//!
//! # Relation to pregresql
//!
//! dbproxy is built on pregresql's hooks and its per-session state machine
//! (`Session`), rather than on pregresql's `Proxy`. `Proxy` cannot hold one
//! connection permit for exactly one session's lifetime — its own
//! `max_connections` waits for a free slot instead of refusing with `53300`,
//! and its hooks have no end-of-session callback — and it owns the accept loop,
//! which is where the `SO_PASSCRED` assertion, the listener tags, the startup
//! filter and the setup gate have to happen. The protocol work itself is
//! pregresql's: startup decoding, the session state machine, prepared statements
//! and portals, cancellation.
//!
//! # An upstream without a name
//!
//! pregresql dials the upstream and, when that dial fails, answers the client
//! itself — naming the host — without calling a hook, and its account of an
//! authentication failure carries the server's words. dbproxy therefore checks
//! that the upstream answers before it hands a session over, and answers the
//! client itself whenever the setup fails. The reachability verdict comes from
//! dbproxy's own dial — kept for a few minutes when it worked and for a second
//! when it did not, and invalidated by any failed session — so a healthy proxy
//! does not dial twice per session and a broken one is re-checked promptly.
//!
//! # The one dependency dbproxy does not name
//!
//! pregresql's low-level session API carries `bytes::BytesMut` (the connection
//! buffer). `bytes` is not one of this crate's declared dependencies — adding
//! one would change `Cargo.lock`, which other work in flight also edits — so
//! the buffer is obtained by type inference (`Default::default()`, resolved by
//! the calls it is passed to) instead of by naming the type. Nothing else from
//! `bytes` is used.
//!
//! # Platform
//!
//! Linux only, like blastit itself: `SO_PASSCRED` and `SCM_CREDENTIALS` are how
//! identity survives gVisor (§2.2, §9.1).

mod config;
mod error;
mod hooks;
pub mod role;
mod server;
mod state;
mod upstream;

use std::sync::Arc;

use blastit_events::Event;

pub use config::{
    DbproxyConfig, ListenerSpec, MAX_CONFIG_BYTES, ProxySpec, read_config, write_config,
};
pub use error::DbproxyError;
pub use server::{run, run_with_listeners, run_with_ready};

/// Where dbproxy sends its events.
///
/// init owns stdout and stamps each event's component from the pipe it arrived
/// on, so dbproxy only ever calls this closure; it never writes a log line
/// anywhere else, and installs no `tracing` subscriber.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
