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
//! registry. dbproxy adds the blacklist identity check, the cap, the policy and
//! the events.

use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pregresql::protocol::MAX_MESSAGE_LENGTH;
use pregresql::protocol::{StartupPacket, StartupParams, decode_startup_packet, fatal_response};
use pregresql::proxy::cancel::handle_cancel_request;
use pregresql::proxy::session::Session;
use pregresql::proxy::stream::{boxed, fill, read_buf_with_credentials};
use pregresql::proxy::{AuthHook, BoxedStream, QueryHook, TlsInfo, Transport};
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener as TokioListener, UnixStream};
use tokio::task::JoinSet;

use crate::Emit;
use crate::config::{DbproxyConfig, ListenerSpec};
use crate::error::DbproxyError;
use crate::hooks::{DbproxyAuth, DbproxyQuery};
use crate::state::Shared;

/// How long the accept loop waits after a failed `accept` before trying again.
///
/// A transient failure (`ECONNABORTED`, `EMFILE`, …) must not turn into a hot
/// loop over `accept`.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(10);

/// How many failed `accept` calls in a row make dbproxy give up on a listener.
///
/// With [`ACCEPT_BACKOFF`] between them this is about a second of a socket that
/// cannot accept anything, which `ARCHITECTURE.md` §8 treats as fatal: a
/// half-started sandbox is worse than a stack trace.
const MAX_ACCEPT_ERRORS: u32 = 100;

/// Serves the listeners `init` bound, until the process is stopped.
///
/// Each listener in `config` is adopted from its
/// [`ListenerSpec::fd`](crate::ListenerSpec::fd):
///
/// * **Safety contract.** The fds were created for dbproxy by init (which
///   `bind`s them in stage 0 and passes them across `execve`), they are
///   `O_CLOEXEC`-free only for dbproxy, they are unique in this list, and
///   dbproxy is the only process that refers to them by number. Adopting one
///   takes ownership: it is closed when dbproxy exits, and nothing else may
///   close it.
/// * Each listener is checked to have `SO_PASSCRED` set — and given it when it
///   does not, so the kernel attaches `SCM_CREDENTIALS` to every client's first
///   message — and switched to non-blocking mode.
///
/// The function only returns on a failure that makes the listener useless, so
/// init sees a dead child rather than a sandbox that quietly serves nothing.
pub async fn run(config: DbproxyConfig, emit: Emit) -> Result<(), DbproxyError> {
    config.validate()?;
    if config.listeners.is_empty() {
        return Err(DbproxyError::Config(
            "dbproxy was handed no listeners".to_string(),
        ));
    }
    let mut listeners = Vec::with_capacity(config.listeners.len());
    for spec in &config.listeners {
        // SAFETY: the fds belong to dbproxy, are unique in this list, and are
        // not referred to by number anywhere else (see the function docs).
        let listener = unsafe { UnixListener::from_raw_fd(spec.fd) };
        listeners.push((listener, spec.clone()));
    }
    run_with_listeners(config, listeners, emit).await
}

/// [`run`], with the listeners already in hand.
///
/// This is what `run` uses after adopting the inherited fds, and what the
/// tests use to serve sockets they bound themselves. Each tuple's
/// [`ListenerSpec`] describes that listener, and its `fd` field is ignored: the
/// listener is already owned. The tag of the listener at position *i* is `i`.
pub async fn run_with_listeners(
    config: DbproxyConfig,
    listeners: Vec<(UnixListener, ListenerSpec)>,
    emit: Emit,
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

    let mut tasks: JoinSet<(usize, Result<(), DbproxyError>)> = JoinSet::new();
    for (index, (listener, _spec)) in listeners.into_iter().enumerate() {
        let listener = prepare(listener, index)?;
        let path = listener_path(&listener);
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

/// Accepts connections until the listener can no longer do so.
async fn serve_listener(
    listener: TokioListener,
    index: usize,
    path: PathBuf,
    server: Arc<Server>,
) -> Result<(), DbproxyError> {
    let mut consecutive_errors = 0u32;
    loop {
        match listener.accept().await {
            Ok((stream, _peer)) => {
                consecutive_errors = 0;
                let path = path.clone();
                let server = Arc::clone(&server);
                tokio::spawn(async move { serve_connection(stream, index, path, server).await });
            }
            Err(error) => {
                consecutive_errors += 1;
                if consecutive_errors >= MAX_ACCEPT_ERRORS {
                    return Err(DbproxyError::Accept {
                        index,
                        message: format!(
                            "accept failed {consecutive_errors} times in a row: {error}"
                        ),
                    });
                }
                tokio::time::sleep(ACCEPT_BACKOFF).await;
            }
        }
    }
}

/// Serves one accepted connection from its first byte to its end.
async fn serve_connection(stream: UnixStream, index: usize, path: PathBuf, server: Arc<Server>) {
    // On Linux the flag is inherited from the listener; setting it again is
    // harmless and covers a listener whose flags could not be changed.
    let _ = rustix::net::sockopt::set_socket_passcred(&stream, true);

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
    let message_credentials = match read_buf_with_credentials(&stream, &mut buffer).await {
        Ok(credentials) => credentials,
        // The client went away before saying anything: nothing to refuse.
        Err(_) => return,
    };
    let mut client: BoxedStream = boxed(stream);

    let shared = Arc::clone(&server.shared);
    let Some(listener) = shared.listener(index) else {
        return;
    };
    let proxy = Arc::clone(&listener.proxy);
    let tag = Arc::clone(&listener.tag);

    // Startup negotiation. dbproxy serves a Unix socket with `sslmode=disable`
    // rendered into the app's environment, so there is no client-side TLS:
    // every request to encrypt is answered with `N`, exactly as a server
    // without TLS does, and the client continues in plaintext.
    let startup: StartupParams = loop {
        match decode_startup_packet(&mut buffer) {
            Ok(Some(StartupPacket::Startup(params))) => break params,
            Ok(Some(StartupPacket::SslRequest)) | Ok(Some(StartupPacket::GssEncRequest)) => {
                if client.write_all(b"N").await.is_err() {
                    return;
                }
            }
            Ok(Some(StartupPacket::CancelRequest { pid, secret })) => {
                // A cancel arrives on a connection of its own. pregresql
                // re-issues it against the upstream backend it registered; the
                // client reads no answer either way.
                if let Err(error) = handle_cancel_request(&shared.cancels, pid, secret).await {
                    shared.emit_pg_refused(
                        None,
                        &proxy.name,
                        blastit_events::PgRefuseReason::Other(format!("cancel_failed: {error}")),
                    );
                }
                return;
            }
            Ok(None) => match fill(&mut client, &mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            },
            Err(_) => {
                let frame = fatal_response("08P01", "invalid startup packet");
                let _ = client.write_all(&frame.encode()).await;
                return;
            }
        }
    };

    let transport = Transport::Unix {
        path,
        peer_pid: None,
        peer_uid: None,
        peer_gid: None,
        message_credentials,
    };
    let id = shared.next_connection_id();
    let (pid, secret) = shared.next_backend_key();
    let session = Session::new(
        id,
        client,
        buffer,
        startup,
        transport,
        Some(tag),
        TlsInfo::default(),
        Arc::clone(&server.auth),
        Arc::clone(&server.query),
        Arc::clone(&shared.cancels),
        MAX_MESSAGE_LENGTH,
        pid,
        secret,
    );
    let outcome = session.run().await;
    shared.finish(index, id, &outcome);
}
