//! The listener side: adopt the sockets init bound, take one connection at a
//! time, and drive pregresql's per-session state machine
//! (`ARCHITECTURE.md` §2.2, §3, §4).
//!
//! dbproxy drives pregresql's `Session` itself instead of using pregresql's
//! `Proxy`, for two things the hook API cannot express:
//!
//! * the connection cap needs one permit held for exactly one session's
//!   lifetime — `Proxy`'s own `max_connections` *waits* for a free slot
//!   instead of refusing with `53300`, and its hooks have no end-of-session
//!   callback to release a permit from;
//! * the refusals must reach the client with the SQLSTATEs `ARCHITECTURE.md` §8
//!   promises, and only the authentication hook can refuse while the client is
//!   still able to read a `FATAL`.
//!
//! Everything protocol-shaped still comes from pregresql: startup decoding, the
//! session state machine, prepared-statement bookkeeping, the cancellation
//! registry. dbproxy adds the identity check, the client-startup filter, the
//! cap, the policy, the events, and a gate that keeps pregresql's *upstream*
//! errors from reaching a view (see [`SetupGate`]).

use std::collections::HashSet;
use std::io;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use blastit_events::PgRefuseReason;
use pregresql::protocol::MAX_MESSAGE_LENGTH;
use pregresql::protocol::{StartupPacket, StartupParams, decode_startup_packet, fatal_response};
use pregresql::proxy::cancel::handle_cancel_request;
use pregresql::proxy::session::Session;
use pregresql::proxy::stream::{boxed, fill, read_buf_with_credentials};
use pregresql::proxy::{AuthHook, BoxedStream, QueryHook, TlsInfo, Transport, UnixCredentials};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{UnixListener as TokioListener, UnixStream};
use tokio::sync::OwnedSemaphorePermit;
use tokio::task::JoinSet;

use crate::Emit;
use crate::config::{DbproxyConfig, ListenerSpec};
use crate::error::DbproxyError;
use crate::hooks::{DbproxyAuth, DbproxyQuery, UPSTREAM_MESSAGE};
use crate::state::{ListenerRuntime, Shared, credentials_match};

/// How long a client has to finish the startup phase.
///
/// A client that connects and then says nothing (or sends half a startup
/// packet) holds a task and one of the listener's startup slots; the deadline
/// is what stops it from holding them for ever. Blastit's clients are views on
/// the same machine, so ten seconds is generous.
pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the accept loop waits after a transient `accept` failure, at first.
const ACCEPT_BACKOFF_MIN: Duration = Duration::from_millis(10);

/// How long it waits at most.
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_millis(1_000);

/// Largest amount of setup-phase output dbproxy holds back before it gives up
/// and releases the session announcement.
///
/// A successful setup writes a few hundred bytes (session parameters, the
/// backend key, `ReadyForQuery`).
const MAX_SETUP_BYTES: usize = 64 * 1024;

/// SQLSTATE a refused startup parameter reports
/// (`invalid_authorization_specification`).
const STARTUP_SQLSTATE: &str = "28000";

/// SQLSTATE an invalid startup packet reports (`protocol_violation`).
const PROTOCOL_SQLSTATE: &str = "08P01";

/// SQLSTATE an upstream failure reports (`connection_failure`).
const UPSTREAM_SQLSTATE: &str = "08006";

/// The startup parameters a client may send.
///
/// Everything else can reshape the upstream session (`options`, `search_path`,
/// `role`, `session_authorization`, `replication`,
/// `default_transaction_*`, …) or is a protocol extension blastit does not
/// implement, and none of it belongs to a view: a view's session is defined by
/// its listener, not by what it claims.
const ALLOWED_STARTUP_PARAMETERS: &[&str] = &["user", "database", "application_name"];

/// The only `client_encoding` a client may ask for.
const ALLOWED_CLIENT_ENCODING: &str = "utf8";

/// Serves the listeners `init` bound, until the process is stopped.
///
/// Each listener in `config` is adopted from its
/// [`ListenerSpec::fd`](crate::ListenerSpec::fd), which must be non-negative
/// and distinct across the configuration:
///
/// * **Safety contract.** The fds were created for dbproxy by init (which
///   `bind`s them in stage 0 and passes them across `execve`), they are unique
///   in this list, and dbproxy is the only process that refers to them by
///   number. Adopting one takes ownership: it is closed when dbproxy exits, and
///   nothing else may close it.
/// * Each listener is checked to have `SO_PASSCRED` set — and given it when it
///   does not, so the kernel attaches `SCM_CREDENTIALS` to every client's first
///   message — and switched to non-blocking mode.
///
/// The function only returns when a listener is unusable, so init sees a dead
/// child rather than a sandbox that quietly serves nothing.
pub async fn run(config: DbproxyConfig, emit: Emit) -> Result<(), DbproxyError> {
    run_with_ready(config, emit, || {}).await
}

/// [`run`], calling `ready` once every listener is adopted and checked, before
/// the first connection is accepted.
///
/// init uses this to tell the platform that the sandbox is up (`BLASTIT_READY`
/// in `ARCHITECTURE.md` §4): when `ready` runs, dbproxy is serving — the
/// listeners exist, `SO_PASSCRED` is set on each of them, and a client that
/// connects now will be accepted.
pub async fn run_with_ready(
    config: DbproxyConfig,
    emit: Emit,
    ready: impl FnOnce() + Send,
) -> Result<(), DbproxyError> {
    config.validate()?;
    if config.listeners.is_empty() {
        return Err(DbproxyError::Config(
            "dbproxy was handed no listeners".to_string(),
        ));
    }

    let mut seen: HashSet<i32> = HashSet::with_capacity(config.listeners.len());
    for (index, spec) in config.listeners.iter().enumerate() {
        if spec.fd < 0 {
            return Err(DbproxyError::Listener {
                index,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("listener fd {} is negative", spec.fd),
                ),
            });
        }
        if !seen.insert(spec.fd) {
            return Err(DbproxyError::Listener {
                index,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("listener fd {} is used twice", spec.fd),
                ),
            });
        }
    }

    let mut listeners = Vec::with_capacity(config.listeners.len());
    for spec in &config.listeners {
        // SAFETY: the fds belong to dbproxy, are non-negative and unique in
        // this list, and are not referred to by number anywhere else (see the
        // function docs).
        let listener = unsafe { UnixListener::from_raw_fd(spec.fd) };
        listeners.push((listener, spec.clone()));
    }
    serve(config, listeners, emit, ready).await
}

/// [`run`], with the listeners already in hand.
///
/// This is what `run` uses after adopting the inherited fds, and what the tests
/// use to serve sockets they bound themselves. Each tuple's [`ListenerSpec`]
/// describes that listener, and its **`fd` field is ignored**: the listener is
/// already owned, which is why `cfg.listeners` may be empty here. The tag of
/// the listener at position *i* is `i`.
pub async fn run_with_listeners(
    config: DbproxyConfig,
    listeners: Vec<(UnixListener, ListenerSpec)>,
    emit: Emit,
) -> Result<(), DbproxyError> {
    serve(config, listeners, emit, || {}).await
}

/// Builds the state, prepares the listeners, calls `ready`, and serves.
async fn serve(
    config: DbproxyConfig,
    listeners: Vec<(UnixListener, ListenerSpec)>,
    emit: Emit,
    ready: impl FnOnce(),
) -> Result<(), DbproxyError> {
    if listeners.is_empty() {
        return Err(DbproxyError::Config(
            "dbproxy was handed no listeners".to_string(),
        ));
    }
    let specs: Vec<ListenerSpec> = listeners.iter().map(|(_, spec)| spec.clone()).collect();
    let shared = Shared::build(&config, &specs, emit)?;
    let server = Arc::new(Server {
        auth: Arc::new(DbproxyAuth::new(Arc::clone(&shared))),
        query: Arc::new(DbproxyQuery::new(Arc::clone(&shared))),
        shared,
    });

    let mut prepared = Vec::with_capacity(listeners.len());
    for (index, (listener, _spec)) in listeners.into_iter().enumerate() {
        let listener = prepare(listener, index)?;
        let path = listener_path(&listener);
        prepared.push((index, listener, path));
    }

    // Every listener is adopted, has `SO_PASSCRED`, and is ready to accept.
    ready();

    let mut tasks: JoinSet<(usize, Result<(), DbproxyError>)> = JoinSet::new();
    for (index, listener, path) in prepared {
        let server = Arc::clone(&server);
        tasks.spawn(async move { (index, serve_listener(listener, index, path, server).await) });
    }

    // A listener only comes back when it can no longer accept, and that is
    // fatal: stop the others (dropping the `JoinSet` aborts them) and let init
    // see the failure rather than a sandbox that quietly serves nothing.
    match tasks.join_next().await {
        Some(Ok((index, Ok(())))) => Err(DbproxyError::Accept {
            index,
            message: "the accept loop stopped without an error".to_string(),
        }),
        Some(Ok((_, Err(error)))) => Err(error),
        Some(Err(error)) => Err(DbproxyError::Accept {
            index: 0,
            message: format!("a listener task stopped unexpectedly: {error}"),
        }),
        None => Err(DbproxyError::Accept {
            index: 0,
            message: "every listener stopped".to_string(),
        }),
    }
}

/// The hooks and the state one process serves with.
struct Server {
    shared: Arc<Shared>,
    auth: Arc<dyn AuthHook>,
    query: Arc<dyn QueryHook>,
}

/// Checks `SO_PASSCRED`, sets it when it is missing, and switches the listener
/// to non-blocking mode.
fn prepare(listener: UnixListener, index: usize) -> Result<TokioListener, DbproxyError> {
    match rustix::net::sockopt::socket_passcred(&listener) {
        Ok(true) => {}
        Ok(false) => {
            rustix::net::sockopt::set_socket_passcred(&listener, true).map_err(|error| {
                DbproxyError::Listener {
                    index,
                    source: error.into(),
                }
            })?;
        }
        Err(error) => {
            return Err(DbproxyError::Listener {
                index,
                source: error.into(),
            });
        }
    }
    listener
        .set_nonblocking(true)
        .map_err(|source| DbproxyError::Listener { index, source })?;
    TokioListener::from_std(listener).map_err(|source| DbproxyError::Listener { index, source })
}

/// The path a listener is bound to, for the `Transport` the hooks see. It is
/// informational only: dbproxy never dials it.
fn listener_path(listener: &TokioListener) -> PathBuf {
    listener
        .local_addr()
        .ok()
        .and_then(|address| address.as_pathname().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// Whether an `accept` failure means the listener itself is unusable.
///
/// `EBADF`/`EINVAL`/`ENOTSOCK` mean the fd cannot accept anything, ever.
/// Everything else — `EMFILE`, `ENFILE`, `ECONNABORTED`, `EINTR`, `EAGAIN`,
/// `ENOBUFS` — is transient: the accept loop backs off and keeps serving, so a
/// moment of fd exhaustion never takes dbproxy down.
fn is_fatal_accept_error(error: &io::Error) -> bool {
    let Some(errno) = error.raw_os_error() else {
        return false;
    };
    [
        rustix::io::Errno::BADF,
        rustix::io::Errno::INVAL,
        rustix::io::Errno::NOTSOCK,
    ]
    .iter()
    .any(|fatal| fatal.raw_os_error() == errno)
}

/// Accepts connections until the listener can no longer do so.
async fn serve_listener(
    listener: TokioListener,
    index: usize,
    path: PathBuf,
    server: Arc<Server>,
) -> Result<(), DbproxyError> {
    let mut backoff = ACCEPT_BACKOFF_MIN;
    loop {
        // Back-pressure on clients that have not finished their startup: once
        // the listener has `MAX_STARTING_CONNECTIONS` of them, stop accepting.
        // The kernel queues the next client in the listener's backlog, so a
        // view that stalls cannot grow dbproxy without bound.
        let Some(runtime) = server.shared.listener(index) else {
            return Err(DbproxyError::Accept {
                index,
                message: "the listener is not in dbproxy's own table".to_string(),
            });
        };
        let permit = match Arc::clone(&runtime.starting).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                // The semaphore is never closed; if it were, refusing loudly is
                // better than serving with no bound at all.
                return Err(DbproxyError::Accept {
                    index,
                    message: "the startup counter is closed".to_string(),
                });
            }
        };

        match listener.accept().await {
            Ok((stream, _peer)) => {
                backoff = ACCEPT_BACKOFF_MIN;
                let server = Arc::clone(&server);
                let path = path.clone();
                tokio::spawn(async move {
                    serve_connection(stream, index, path, server, permit).await;
                });
            }
            Err(error) => {
                if is_fatal_accept_error(&error) {
                    return Err(DbproxyError::Accept {
                        index,
                        message: format!("the listener cannot accept connections: {error}"),
                    });
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(ACCEPT_BACKOFF_MAX);
            }
        }
    }
}

/// Serves one accepted connection from its first byte to its end.
async fn serve_connection(
    stream: UnixStream,
    index: usize,
    path: PathBuf,
    server: Arc<Server>,
    starting: OwnedSemaphorePermit,
) {
    // On Linux the flag is inherited from the listener; setting it again is
    // harmless and covers a listener whose flags could not be changed.
    let _ = rustix::net::sockopt::set_socket_passcred(&stream, true);

    let shared = Arc::clone(&server.shared);
    let Some(listener) = shared.listener(index) else {
        return;
    };
    let route = listener.spec.route.clone();
    let proxy = Arc::clone(&listener.proxy);
    let tag = Arc::clone(&listener.tag);

    // The first read is a `recvmsg`, so the kernel hands over the
    // `SCM_CREDENTIALS` of whoever wrote these bytes, in the same buffer the
    // startup negotiation reads from. Nothing is lost or reordered.
    //
    // `SO_PEERCRED` is deliberately never consulted: under gVisor it reports
    // the *listening* process for every connection (`ARCHITECTURE.md` §2.2).
    //
    // The buffer's type (`bytes::BytesMut`, pregresql's low-level wire buffer)
    // is inferred from the calls it is passed to; see the crate documentation
    // for why dbproxy does not name it.
    let mut buffer = Default::default();
    let first_read = tokio::time::timeout(
        STARTUP_TIMEOUT,
        read_buf_with_credentials(&stream, &mut buffer),
    )
    .await;
    let message_credentials = match first_read {
        Ok(Ok(credentials)) => credentials,
        // The client went away before saying anything: nothing to refuse.
        Ok(Err(_)) => return,
        Err(_) => {
            shared.emit_pg_refused(
                Some(&route),
                &proxy.name,
                PgRefuseReason::Other("startup_timeout".to_string()),
            );
            return;
        }
    };

    // A copy of the client socket for the one message dbproxy may have to send
    // itself after the session is over (see [`SetupGate`]).
    let mut failure_writer = clone_client(&stream);
    let mut client: BoxedStream = boxed(stream);

    // Startup negotiation: no client-side TLS (the rendered `sslmode` for the
    // local leg is `disable`), so every request to encrypt is answered with
    // `N`, and a `CancelRequest` is a connection of its own.
    let negotiation = tokio::time::timeout(STARTUP_TIMEOUT, async {
        loop {
            match decode_startup_packet(&mut buffer) {
                Ok(Some(StartupPacket::Startup(params))) => break Some(params),
                Ok(Some(StartupPacket::SslRequest)) | Ok(Some(StartupPacket::GssEncRequest)) => {
                    if client.write_all(b"N").await.is_err() {
                        break None;
                    }
                }
                Ok(Some(StartupPacket::CancelRequest { pid, secret })) => {
                    handle_cancel(&shared, listener, message_credentials, pid, secret).await;
                    break None;
                }
                Ok(None) => match fill(&mut client, &mut buffer).await {
                    // 0 is end of stream; anything else means more to decode.
                    Ok(0) | Err(_) => break None,
                    Ok(_) => {}
                },
                Err(_) => {
                    let frame = fatal_response(PROTOCOL_SQLSTATE, "invalid startup packet");
                    let _ = client.write_all(&frame.encode()).await;
                    break None;
                }
            }
        }
    })
    .await;

    let startup = match negotiation {
        Ok(Some(startup)) => startup,
        Ok(None) => return,
        Err(_) => {
            shared.emit_pg_refused(
                Some(&route),
                &proxy.name,
                PgRefuseReason::Other("startup_timeout".to_string()),
            );
            return;
        }
    };

    // Only dbproxy's own parameters go upstream; everything else a client sends
    // is refused, because a view does not get to reshape its session.
    let startup = match check_startup_parameters(&startup) {
        Ok(startup) => startup,
        Err(message) => {
            shared.emit_pg_refused(
                Some(&route),
                &proxy.name,
                PgRefuseReason::Other("startup_parameter".to_string()),
            );
            let frame = fatal_response(STARTUP_SQLSTATE, message);
            let _ = client.write_all(&frame.encode()).await;
            return;
        }
    };

    // The startup phase is over: the client gets its slot back for other
    // connections.
    drop(starting);

    let transport = Transport::Unix {
        path,
        peer_pid: None,
        peer_uid: None,
        peer_gid: None,
        message_credentials,
    };
    let id = shared.next_connection_id();
    let (pid, secret) = shared.next_backend_key();
    let gate = SetupGate::new(client, Arc::clone(&shared), id);
    let gate_state = Arc::clone(&gate.state);
    let session = Session::new(
        id,
        Box::new(gate),
        buffer,
        startup,
        transport,
        Some(tag),
        TlsInfo::default(),
        Arc::clone(&server.auth),
        Arc::clone(&server.query),
        Arc::clone(&listener.cancels),
        MAX_MESSAGE_LENGTH,
        pid,
        secret,
    );
    let outcome = session.run().await;

    proxy.release_slot(id);
    let _ = shared.take_refusal(id);

    if gate_state.upstream_failure() {
        // pregresql's own account of the failure was swallowed: the client
        // gets dbproxy's fixed message, and the event says what dbproxy saw.
        proxy.invalidate_probe();
        shared.emit_pg_refused(Some(&route), &proxy.name, upstream_reason(&outcome));
        if let Some(writer) = failure_writer.as_mut() {
            let frame = fatal_response(UPSTREAM_SQLSTATE, UPSTREAM_MESSAGE);
            let _ = writer.write_all(&frame.encode()).await;
        }
    }
}

/// A second handle on an accepted client socket.
///
/// The session owns (and eventually drops) the stream it was given; dbproxy
/// keeps this copy so it can still answer a client whose setup failed — the
/// client sees no end of stream until both handles are closed.
fn clone_client(stream: &UnixStream) -> Option<UnixStream> {
    use std::os::fd::AsFd;

    let fd = stream.as_fd().try_clone_to_owned().ok()?;
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream.set_nonblocking(true).ok()?;
    UnixStream::from_std(stream).ok()
}

/// Handles a client's `CancelRequest`.
///
/// Identity comes first, exactly as for a session: only the uid a listener
/// belongs to may cancel, and the registry is the listener's own, so a view can
/// only ever cancel the sessions that were opened on its own socket.
async fn handle_cancel(
    shared: &Shared,
    listener: &ListenerRuntime,
    credentials: Option<UnixCredentials>,
    pid: i32,
    secret: i32,
) {
    let route = listener.spec.route.as_str();
    let proxy = &listener.proxy;
    if !credentials_match(listener.spec.uid, credentials) {
        shared.emit_pg_refused(
            Some(route),
            &proxy.name,
            PgRefuseReason::CredentialsMismatch,
        );
        return;
    }
    let _permit = match proxy.take_cancel_slot() {
        Some(permit) => permit,
        None => {
            shared.emit_pg_refused(
                Some(route),
                &proxy.name,
                PgRefuseReason::Other("too_many_cancels".to_string()),
            );
            return;
        }
    };
    match handle_cancel_request(&listener.cancels, pid, secret).await {
        // Forwarded: a cancel is a normal part of life, not an event.
        Ok(true) => {}
        Ok(false) => {
            // No session with this key on *this* listener: another route's, or
            // long gone — either way it is refused.
            shared.emit_pg_refused(
                Some(route),
                &proxy.name,
                PgRefuseReason::Other("cancel_unknown_key".to_string()),
            );
        }
        Err(_) => {
            // Never the upstream's wording: it can name the host.
            shared.emit_pg_refused(
                Some(route),
                &proxy.name,
                PgRefuseReason::Other("cancel_failed".to_string()),
            );
        }
    }
}

/// What a failed setup means, for the `pg.refused` event.
///
/// The client always gets the same fixed message ([`UPSTREAM_MESSAGE`]); only
/// the event says which side of the upstream connection failed, and never with
/// pregresql's or the server's words.
fn upstream_reason(outcome: &Result<(), pregresql::Error>) -> PgRefuseReason {
    match outcome {
        Err(pregresql::Error::Auth(_)) => {
            PgRefuseReason::Other("upstream_authentication_failed".to_string())
        }
        Err(pregresql::Error::Tls(_)) => PgRefuseReason::Other("upstream_tls_failed".to_string()),
        Err(pregresql::Error::Protocol(_)) => {
            PgRefuseReason::Other("upstream_protocol_failed".to_string())
        }
        // The dial itself, or a server that went away while being talked to.
        _ => PgRefuseReason::UpstreamUnreachable,
    }
}

/// Filters a client's startup packet.
///
/// Only [`ALLOWED_STARTUP_PARAMETERS`] plus a UTF-8 `client_encoding` are
/// accepted. `user` and `database` are kept for pregresql's bookkeeping but
/// replaced by the proxy's identity upstream, `application_name` is passed on,
/// and `client_encoding` is accepted but dropped (the upstream reports its own
/// encoding back to the client).
///
/// The returned error names the parameter, never its value.
fn check_startup_parameters(params: &StartupParams) -> Result<StartupParams, String> {
    let mut kept = Vec::with_capacity(params.params.len());
    for (name, value) in &params.params {
        let allowed = if ALLOWED_STARTUP_PARAMETERS.contains(&name.as_str()) {
            true
        } else if name == "client_encoding" {
            value.to_ascii_lowercase() == ALLOWED_CLIENT_ENCODING
        } else {
            false
        };
        if !allowed {
            return Err(format!(
                "startup parameter {name:?} is not allowed on this listener"
            ));
        }
        // `client_encoding` is accepted and not forwarded.
        if name != "client_encoding" {
            kept.push((name.clone(), value.clone()));
        }
    }
    Ok(StartupParams {
        version: params.version,
        params: kept,
    })
}

/// A client stream that holds back everything pregresql writes while it sets a
/// session up.
///
/// A session is announced by its `ReadyForQuery`, and only then does the client
/// receive anything. If the setup fails, pregresql writes an `ErrorResponse`,
/// and *dbproxy* decides what the client sees:
///
/// * when dbproxy itself refused the connection (an authentication-hook
///   refusal), that `ErrorResponse` is dbproxy's own message: it is released
///   unchanged;
/// * otherwise the setup failed on the upstream side (a dial, TLS or
///   authentication failure, or an unexpected server message): the bytes are
///   swallowed, and the connection's task answers with dbproxy's fixed message
///   and a `pg.refused` event. pregresql's account of an upstream failure names
///   the host, and PostgreSQL's own names the role — neither is a view's
///   business.
struct SetupGate {
    inner: BoxedStream,
    state: Arc<GateState>,
}

/// The part of a [`SetupGate`] the connection's task can look at, after the
/// session has taken the stream.
struct GateState {
    shared: Arc<Shared>,
    connection: u64,
    inner: Mutex<GateInner>,
}

#[derive(Default)]
struct GateInner {
    /// Bytes written before the session was announced.
    buffered: Vec<u8>,
    /// How many of them have been handed to the socket.
    sent: usize,
    /// The session's startup phase finished: writes go straight through.
    released: bool,
    /// The setup failed upstream-side: dbproxy answers the client itself.
    upstream_failure: bool,
}

impl SetupGate {
    /// Wraps an accepted client stream.
    fn new(inner: BoxedStream, shared: Arc<Shared>, connection: u64) -> Self {
        Self {
            inner,
            state: Arc::new(GateState {
                shared,
                connection,
                inner: Mutex::new(GateInner::default()),
            }),
        }
    }
}

impl GateState {
    /// Whether the session announced itself. The connection's task uses this to
    /// tell a completed setup from a failed one.
    fn upstream_failure(&self) -> bool {
        lock(&self.inner).upstream_failure
    }

    /// Looks at the buffered setup output: does the session start, or did the
    /// setup fail?
    fn classify(&self, state: &mut GateInner) {
        if state.released || state.upstream_failure {
            return;
        }
        if ends_with_ready_for_query(&state.buffered) {
            state.released = true;
            return;
        }
        if has_error_response(&state.buffered) {
            if self.shared.take_refusal(self.connection) {
                // dbproxy's own refusal: deliver it.
                state.released = true;
            } else {
                // An upstream failure: swallow pregresql's words.
                state.upstream_failure = true;
                state.buffered.clear();
                state.sent = 0;
            }
        }
    }
}

impl AsyncRead for SetupGate {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for SetupGate {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let (pending, buffered) = {
            let mut state = lock(&this.state.inner);
            if state.upstream_failure {
                // The client is told by dbproxy; nothing pregresql writes now
                // is delivered.
                return Poll::Ready(Ok(buf.len()));
            }
            if !state.released {
                state.buffered.extend_from_slice(buf);
                if state.buffered.len() > MAX_SETUP_BYTES {
                    // Nothing a setup writes is this big; fail open rather than
                    // grow without bound.
                    state.released = true;
                }
                this.state.classify(&mut state);
                if !state.released {
                    return Poll::Ready(Ok(buf.len()));
                }
                // This write released the session; the bytes are already held.
                (true, true)
            } else {
                (state.buffered.len() > state.sent, false)
            }
        };
        if pending {
            // The announcement is still on its way out: keep the order, and let
            // `drain` (here, or on the next flush) finish it.
            if !buffered {
                lock(&this.state.inner).buffered.extend_from_slice(buf);
            }
            return match this.drain(cx) {
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                // The rest is drained by the next write or flush; the caller
                // may believe all of `buf` was taken, because it was.
                _ => Poll::Ready(Ok(buf.len())),
            };
        }
        // A session that is running, with nothing held back, writes straight
        // through: the gate costs nothing in the request path.
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        {
            let state = lock(&this.state.inner);
            if state.upstream_failure {
                return Poll::Ready(Ok(()));
            }
            if !state.released {
                // Still setting the session up: nothing leaves this gate until
                // the session is announced (or dbproxy answers instead).
                return Poll::Ready(Ok(()));
            }
        }
        match this.drain(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_flush(cx),
            other => other,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

impl SetupGate {
    /// Writes as much of the held-back output as the socket takes.
    ///
    /// Nothing is copied: the bytes go from the buffer to the socket under the
    /// lock, which is released again before anything else touches it.
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            let outcome = {
                let mut state = lock(&self.state.inner);
                if state.sent >= state.buffered.len() {
                    state.buffered.clear();
                    state.sent = 0;
                    return Poll::Ready(Ok(()));
                }
                let start = state.sent;
                let chunk = &state.buffered[start..];
                Pin::new(&mut self.inner).poll_write(cx, chunk)
            };
            match outcome {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "the client socket took no bytes",
                    )));
                }
                Poll::Ready(Ok(written)) => {
                    lock(&self.state.inner).sent += written;
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Whether the buffer ends with a complete `ReadyForQuery`, which is how a
/// backend announces a session.
fn ends_with_ready_for_query(buffer: &[u8]) -> bool {
    buffer.len() >= 6
        && buffer[buffer.len() - 6] == b'Z'
        && buffer[buffer.len() - 5..buffer.len() - 1] == [0, 0, 0, 5]
}

/// Whether the buffer holds a complete `ErrorResponse` frame.
fn has_error_response(buffer: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset + 5 <= buffer.len() {
        let tag = buffer[offset];
        let length = i32::from_be_bytes([
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
            buffer[offset + 4],
        ]);
        if length < 4 {
            return false;
        }
        let end = offset + 1 + length as usize;
        if end > buffer.len() {
            // An incomplete frame: not a decision yet.
            return false;
        }
        if tag == b'E' {
            return true;
        }
        offset = end;
    }
    false
}

/// Locks a mutex, treating poisoning as harmless: what this gate keeps under
/// its lock is a plain byte buffer.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn startup(params: &[(&str, &str)]) -> StartupParams {
        StartupParams::new(
            params
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        )
    }

    /// A backend `ErrorResponse` frame, as pregresql would relay it.
    fn error_frame(fields: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (code, value) in fields {
            body.push(code.as_bytes()[0]);
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
        body.push(0);
        let mut frame = vec![b'E'];
        frame.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    /// A backend `ReadyForQuery` frame.
    fn ready_for_query() -> Vec<u8> {
        vec![b'Z', 0, 0, 0, 5, b'I']
    }

    /// A parameter status frame, which is not an announcement.
    fn parameter_status() -> Vec<u8> {
        let mut body = Vec::new();
        for value in ["client_encoding", "UTF8"] {
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
        let mut frame = vec![b'S'];
        frame.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    #[test]
    fn only_the_documented_startup_parameters_are_accepted() {
        let accepted = check_startup_parameters(&startup(&[
            ("user", "blastit"),
            ("database", "app"),
            ("application_name", "django"),
            ("client_encoding", "UTF8"),
        ]))
        .expect("accepted");
        // `client_encoding` is accepted and *not* forwarded.
        assert_eq!(
            accepted.params,
            [
                ("user".to_string(), "blastit".to_string()),
                ("database".to_string(), "app".to_string()),
                ("application_name".to_string(), "django".to_string()),
            ]
        );
        assert!(check_startup_parameters(&startup(&[("client_encoding", "utf8")])).is_ok());
        assert!(check_startup_parameters(&startup(&[("client_encoding", "UtF8")])).is_ok());
    }

    #[test]
    fn every_other_startup_parameter_is_refused_and_named() {
        for (name, value) in [
            ("options", "-c search_path=public"),
            ("search_path", "public"),
            ("role", "postgres"),
            ("session_authorization", "postgres"),
            ("replication", "true"),
            ("default_transaction_isolation", "serializable"),
            ("default_transaction_read_only", "off"),
            ("client_encoding", "LATIN1"),
            ("_pq_.something", "1"),
            ("extra_float_digits", "3"),
        ] {
            let error =
                check_startup_parameters(&startup(&[(name, value)])).expect_err("must be refused");
            assert!(error.contains(name), "{error}");
            assert!(
                !error.contains(value),
                "the refusal must not quote a value: {error}"
            );
        }
    }

    #[test]
    fn the_setup_gate_announces_a_session_only_after_ready_for_query() {
        let shared = state();
        let state = Arc::new(GateState {
            shared,
            connection: 1,
            inner: Mutex::new(GateInner::default()),
        });
        let mut inner = lock(&state.inner);
        inner.buffered.extend_from_slice(&parameter_status());
        state.classify(&mut inner);
        assert!(!inner.released, "a parameter is not an announcement");
        inner.buffered.extend_from_slice(&ready_for_query());
        state.classify(&mut inner);
        assert!(inner.released);
        assert!(!inner.upstream_failure);
    }

    #[test]
    fn the_setup_gate_delivers_a_refusal_of_dbroxys_own() {
        let shared = state();
        shared.note_refusal(7);
        let state = Arc::new(GateState {
            shared,
            connection: 7,
            inner: Mutex::new(GateInner::default()),
        });
        let mut inner = lock(&state.inner);
        inner
            .buffered
            .extend_from_slice(&error_frame(&[("C", "28000"), ("M", "refused")]));
        state.classify(&mut inner);
        assert!(inner.released, "dbproxy's own refusal reaches the client");
        assert!(!inner.upstream_failure);
        assert!(!inner.buffered.is_empty(), "and it is still there to send");
    }

    #[test]
    fn the_setup_gate_swallows_an_upstream_failure() {
        let shared = state();
        let state = Arc::new(GateState {
            shared,
            connection: 8,
            inner: Mutex::new(GateInner::default()),
        });
        let mut inner = lock(&state.inner);
        inner.buffered.extend_from_slice(&error_frame(&[
            ("S", "FATAL"),
            ("C", "08006"),
            ("M", "dial to db.internal:5432 failed"),
        ]));
        state.classify(&mut inner);
        assert!(!inner.released);
        assert!(inner.upstream_failure);
        assert!(inner.buffered.is_empty(), "nothing of it is kept");
    }

    #[test]
    fn incomplete_frames_are_not_a_decision() {
        assert!(!has_error_response(b"E\0\0\0\x30C08006"));
        assert!(!has_error_response(b"S\0\0\0\x0cfoo\0bar\0"));
        assert!(has_error_response(
            b"S\0\0\0\x0cfoo\0bar\0E\0\0\0\x08C42501"
        ));
        assert!(!ends_with_ready_for_query(b"Z\0\0\0\x05"));
        assert!(ends_with_ready_for_query(b"Z\0\0\0\x05I"));
    }

    #[test]
    fn only_an_invalid_listener_is_a_fatal_accept_error() {
        assert!(is_fatal_accept_error(&io::Error::from_raw_os_error(
            rustix::io::Errno::BADF.raw_os_error()
        )));
        assert!(is_fatal_accept_error(&io::Error::from_raw_os_error(
            rustix::io::Errno::NOTSOCK.raw_os_error()
        )));
        for transient in [
            rustix::io::Errno::MFILE,
            rustix::io::Errno::NFILE,
            rustix::io::Errno::CONNABORTED,
            rustix::io::Errno::INTR,
            rustix::io::Errno::AGAIN,
        ] {
            assert!(
                !is_fatal_accept_error(&io::Error::from_raw_os_error(transient.raw_os_error())),
                "{transient:?} must not be fatal"
            );
        }
    }

    #[tokio::test]
    async fn the_adopted_listener_fds_are_checked_before_they_are_used() {
        use crate::config::{DbproxyConfig, ListenerSpec, ProxySpec};
        use blastit_sqlpolicy::TableGrants;

        let proxy = ProxySpec {
            name: "db".to_string(),
            host: "127.0.0.1".to_string(),
            port: 1,
            user: "u".to_string(),
            password: "p".to_string().into(),
            database: "d".to_string(),
            sslmode: Some("disable".to_string()),
            ca_cert: None,
            options: vec![],
            max_connections: 4,
        };
        let listener = |fd: i32, route: &str| ListenerSpec {
            fd,
            route: route.to_string(),
            proxy: "db".to_string(),
            uid: 21_000,
            queries: vec![],
            transaction_control: false,
            unrestricted: false,
            tables: TableGrants::default(),
        };
        let emit: crate::Emit = Arc::new(|_: blastit_events::Event| {});

        // A negative fd is refused: it is never passed to `from_raw_fd`.
        let config = DbproxyConfig {
            proxies: vec![proxy.clone()],
            listeners: vec![listener(-1, "a")],
        };
        let error = run(config, Arc::clone(&emit))
            .await
            .expect_err("a negative fd");
        assert!(error.to_string().contains("negative"), "{error}");

        // Two listeners cannot share one fd.
        let config = DbproxyConfig {
            proxies: vec![proxy],
            listeners: vec![listener(3, "a"), listener(3, "b")],
        };
        let error = run(config, emit).await.expect_err("a repeated fd");
        assert!(error.to_string().contains("twice"), "{error}");
    }

    /// The shared state of one accepted listener, for the gate's tests.
    fn state() -> Arc<Shared> {
        use crate::config::{DbproxyConfig, ListenerSpec, ProxySpec};
        use blastit_events::Event;
        use blastit_sqlpolicy::TableGrants;

        let listener = ListenerSpec {
            fd: -1,
            route: "product-detail".to_string(),
            proxy: "db".to_string(),
            uid: 21_000,
            queries: vec![],
            transaction_control: false,
            unrestricted: false,
            tables: TableGrants::default(),
        };
        let proxy = ProxySpec {
            name: "db".to_string(),
            host: "127.0.0.1".to_string(),
            port: 1,
            user: "u".to_string(),
            password: "p".to_string().into(),
            database: "d".to_string(),
            sslmode: Some("disable".to_string()),
            ca_cert: None,
            options: vec![],
            max_connections: 4,
        };
        let config = DbproxyConfig {
            proxies: vec![proxy],
            listeners: vec![listener.clone()],
        };
        let emit: crate::Emit = Arc::new(|_: Event| {});
        Shared::build(&config, &[listener], emit).expect("state")
    }
}
