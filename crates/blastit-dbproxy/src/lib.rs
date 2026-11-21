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
mod learn;
pub mod role;
mod server;
mod state;
mod upstream;

use std::sync::Arc;

use blastit_events::Event;

pub use config::{
    DbproxyConfig, ListenerMode, ListenerSpec, MAX_CONFIG_BYTES, ProxySpec, RoutePolicySpec,
    WILDCARD_ROUTE, read_config, write_config,
};
pub use error::DbproxyError;
pub use learn::{ObsTarget, WireSqlObs};
pub use policy::{ACK_REFUSED, ACK_SWAPPED, MAX_POLICY_DOCUMENT};
pub use server::{run, run_with_listeners, run_with_ready};

/// The dev-only policy channel: one document per save, one ack byte back
/// (`docs/design/dev-fanotify-reload.md` §4.6 step 2, §4.8, `ADR-0014` §4).
///
/// A `blastit dev` boot with live reload on creates one
/// `AF_UNIX`/`SOCK_STREAM` socketpair in stage 0. init's end travels to stage 1
/// (`--dev-policy-fd`) and is what the supervisor writes a document on; dbproxy's
/// end arrives in its configuration document (`policy_fd`, legal only with
/// `dev_policy`). After a `Blastfile` save the supervisor writes
/// `u32 length | JSON bytes` (both little-endian, at most
/// [`MAX_POLICY_DOCUMENT`]) naming the **new** manifest's whole route-policy set,
/// and dbproxy replaces its map in one step and answers with one byte:
/// [`ACK_SWAPPED`] or [`ACK_REFUSED`]. Nothing here is reachable on a production
/// or `check` boot, which never create the socketpair and never carry
/// `policy_fd`.
pub(crate) mod policy {
    use std::fmt;
    use std::time::Duration;

    use serde::Deserialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    use crate::config::{RoutePolicySpec, WILDCARD_ROUTE};

    /// The largest policy document accepted, in bytes.
    ///
    /// A manifest's route policies are its grants, one entry per `(route,
    /// proxy)`; 1 MiB is far above any real manifest and, unlike the boot's
    /// configuration, it is a per-save message, so the bound is also what keeps
    /// a bogus length prefix from allocating wildly.
    pub const MAX_POLICY_DOCUMENT: usize = 1024 * 1024;

    /// How long one document (and its ack) may take before the channel is
    /// treated as wedged.
    ///
    /// The supervisor's own deadline is 100 ms (§4.6 step 2); this is the
    /// reader's backstop for a peer that wrote a length and then stopped, so a
    /// half-written document cannot hold the task (or the descriptor) for ever.
    pub(crate) const POLICY_TIMEOUT: Duration = Duration::from_secs(5);

    /// The one-byte answer: the whole map was replaced.
    pub const ACK_SWAPPED: u8 = 0x00;

    /// The one-byte answer: the document was refused whole, the old map is in
    /// force.
    pub const ACK_REFUSED: u8 = 0x01;

    /// The only document version this build understands.
    const VERSION: u32 = 1;

    /// Why a document or the channel itself was refused.
    ///
    /// No message ever carries a statement, a literal or a credential: the
    /// document holds grants (already the manifest's public text) and the
    /// refusals name shapes and counts, never SQL.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct PolicyError(String);

    impl PolicyError {
        /// A refusal with a one-line reason.
        pub(crate) fn new(reason: impl Into<String>) -> Self {
            Self(reason.into())
        }
    }

    impl fmt::Display for PolicyError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    /// The wire document, exactly as the design freezes it.
    ///
    /// `deny_unknown_fields` is what makes an unknown member — a `fd`, a stray
    /// key a future version invented — a refusal instead of something silently
    /// dropped, and [`RoutePolicySpec`] carries the same rule for its own
    /// members.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        /// Must be [`VERSION`].
        version: u32,
        /// One entry per `(route, proxy)` the new manifest declares or grants.
        route_policies: Vec<RoutePolicySpec>,
    }

    /// What one read produced.
    #[derive(Debug)]
    pub(crate) enum DocumentRead {
        /// A well-formed document, to be applied whole.
        Apply(Vec<RoutePolicySpec>),
        /// A document that is refused whole; the old policy stays and the
        /// channel remains usable for the next save.
        Refused(PolicyError),
    }

    /// Reads one length-prefixed document.
    ///
    /// `Ok(None)` is a clean end-of-file before a length: the peer is gone, and
    /// the task ends. `Err` is a framing or transport failure — an oversize
    /// length, a body that stopped arriving, a deadline — after which the stream
    /// cannot be resynchronised, so the caller refuses and closes the channel.
    /// `Ok(Some(DocumentRead::Refused))` is a document whose *framing* was
    /// intact but whose shape or version this build refuses.
    ///
    /// The channel is **idle** between saves, so waiting for the first header
    /// byte is unbounded: a dev boot may serve for hours before its first
    /// `Blastfile` save, and a deadline there would close a channel nobody has
    /// used yet. [`POLICY_TIMEOUT`] bounds a document that has **started** and
    /// then stalled, which is the only shape a wedged peer can produce.
    ///
    /// # Errors
    ///
    /// A one-line reason naming the length, the limit or the deadline — never a
    /// byte of the document.
    pub(crate) async fn read_document(
        stream: &mut UnixStream,
    ) -> Result<Option<DocumentRead>, PolicyError> {
        read_document_within(stream, POLICY_TIMEOUT).await
    }

    /// [`read_document`] with an injectable deadline, so the stall rule is
    /// asserted in milliseconds instead of seconds.
    pub(crate) async fn read_document_within(
        stream: &mut UnixStream,
        deadline: Duration,
    ) -> Result<Option<DocumentRead>, PolicyError> {
        let mut header = [0u8; 4];
        let mut filled = 0;
        while filled < 4 {
            let read = if filled == 0 {
                stream.read(&mut header[filled..]).await
            } else {
                match tokio::time::timeout(deadline, stream.read(&mut header[filled..])).await {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        return Err(PolicyError::new(format!(
                            "a half-written policy header stalled for more than {} ms",
                            deadline.as_millis()
                        )));
                    }
                }
            };
            match read {
                Ok(0) if filled == 0 => return Ok(None),
                Ok(0) => {
                    return Err(PolicyError::new(
                        "the policy header was cut short before its fourth byte",
                    ));
                }
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(PolicyError::new(format!(
                        "reading the policy header: {error}"
                    )));
                }
            }
        }
        let length = u32::from_le_bytes(header) as usize;
        if length == 0 {
            return Err(PolicyError::new("the policy document is empty"));
        }
        if length > MAX_POLICY_DOCUMENT {
            return Err(PolicyError::new(format!(
                "the policy document is {length} bytes, more than the \
                 {MAX_POLICY_DOCUMENT}-byte limit"
            )));
        }
        let mut body = vec![0u8; length];
        match tokio::time::timeout(deadline, stream.read_exact(&mut body)).await {
            Err(_elapsed) => {
                return Err(PolicyError::new(format!(
                    "the policy document stopped arriving for more than {} ms",
                    deadline.as_millis()
                )));
            }
            Ok(Err(error)) => {
                return Err(PolicyError::new(format!(
                    "reading the policy document: {error}"
                )));
            }
            Ok(Ok(_read)) => {}
        }
        let document: Document = match serde_json::from_slice(&body) {
            Ok(document) => document,
            Err(error) => {
                return Ok(Some(DocumentRead::Refused(PolicyError::new(format!(
                    "the policy document is not the documented shape: {error}"
                )))));
            }
        };
        if document.version != VERSION {
            return Ok(Some(DocumentRead::Refused(PolicyError::new(format!(
                "the policy document says version {}, not {VERSION}",
                document.version
            )))));
        }
        for (index, policy) in document.route_policies.iter().enumerate() {
            if policy.route.is_empty() || policy.route == WILDCARD_ROUTE {
                return Ok(Some(DocumentRead::Refused(PolicyError::new(format!(
                    "route policy {index} names {:?}, which is not a route name",
                    policy.route
                )))));
            }
            if policy.proxy.is_empty() {
                return Ok(Some(DocumentRead::Refused(PolicyError::new(format!(
                    "route policy {index} names no proxy"
                )))));
            }
            if document.route_policies[..index]
                .iter()
                .any(|other| other.route == policy.route && other.proxy == policy.proxy)
            {
                return Ok(Some(DocumentRead::Refused(PolicyError::new(format!(
                    "two route policies cover route {:?} on proxy {:?}",
                    policy.route, policy.proxy
                )))));
            }
        }
        Ok(Some(DocumentRead::Apply(document.route_policies)))
    }

    /// Writes the one-byte ack.
    ///
    /// # Errors
    ///
    /// The transport error, when the peer is gone: an ack nobody waits for any
    /// more is not a reason to keep the channel.
    pub(crate) async fn write_ack(stream: &mut UnixStream, byte: u8) -> std::io::Result<()> {
        stream.write_all(&[byte]).await
    }
}

/// Where dbproxy sends its events.
///
/// init owns stdout and stamps each event's component from the pipe it arrived
/// on, so dbproxy only ever calls this closure; it never writes a log line
/// anywhere else, and installs no `tracing` subscriber.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
