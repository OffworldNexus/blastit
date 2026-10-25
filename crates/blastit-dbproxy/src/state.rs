//! The state every connection shares: one runtime per proxy, one per listener,
//! and the event sink.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use blastit_events::{Component, Event, EventKind, PgRefuseReason, PgRefused, SqlDenied, SqlKind};
use blastit_sqlpolicy::Allowlist;
use pregresql::proxy::cancel::CancelRegistry;
use pregresql::proxy::{InboundConnection, UpstreamTls};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

use crate::Emit;
use crate::config::{DbproxyConfig, ListenerSpec};
use crate::error::DbproxyError;
use crate::upstream::{CaBundles, SslPlan, upstream_parameters};

/// How long dbproxy waits for the upstream to accept a TCP connection before
/// it calls the database unreachable.
pub(crate) const UPSTREAM_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything a connection needs that is built once, per process.
pub(crate) struct Shared {
    /// One entry per listener, indexed by its tag.
    listeners: Vec<ListenerRuntime>,
    emit: Emit,
    /// pregresql's per-session cancel targets, keyed by the backend keys
    /// dbproxy hands to its clients.
    pub(crate) cancels: Arc<CancelRegistry>,
    next_connection: AtomicU64,
    next_pid: AtomicI32,
    /// `/dev/urandom`, opened once; `None` when it could not be opened.
    urandom: Option<File>,
}

impl Shared {
    /// Builds the runtime state for `listeners`, checking the configuration.
    ///
    /// `listeners` is the list the listeners are actually served from, in tag
    /// order; for [`run`](crate::run) that is `config.listeners`.
    pub(crate) fn build(
        config: &DbproxyConfig,
        listeners: &[ListenerSpec],
        emit: Emit,
    ) -> Result<Arc<Self>, DbproxyError> {
        config.validate()?;

        let mut bundles = CaBundles::new();
        let mut proxies = HashMap::new();
        for proxy in &config.proxies {
            let plan = SslPlan::parse(proxy.sslmode.as_deref())?;
            let ca = match (&proxy.ca_cert, plan.needs_ca()) {
                (Some(pem), true) => Some(bundles.write(&proxy.name, pem)?),
                _ => None,
            };
            proxies.insert(
                proxy.name.clone(),
                Arc::new(ProxyRuntime {
                    name: proxy.name.clone(),
                    host: proxy.host.clone(),
                    port: proxy.port,
                    user: proxy.user.clone(),
                    password: proxy.password.clone(),
                    database: proxy.database.clone(),
                    tls: plan.resolve(ca)?,
                    parameters: upstream_parameters(&proxy.options),
                    cap: Arc::new(Semaphore::new(proxy.max_connections as usize)),
                    permits: Mutex::new(HashMap::new()),
                    reachable: AtomicBool::new(false),
                }),
            );
        }

        let mut runtimes = Vec::with_capacity(listeners.len());
        for (index, spec) in listeners.iter().enumerate() {
            let proxy = proxies.get(&spec.proxy).ok_or_else(|| {
                DbproxyError::Config(format!(
                    "listener {index} (route {:?}) names the unknown proxy {:?}",
                    spec.route, spec.proxy
                ))
            })?;
            runtimes.push(ListenerRuntime {
                spec: spec.clone(),
                tag: Arc::from(index.to_string().as_str()),
                allowlist: Allowlist::new(&spec.queries, spec.transaction_control),
                proxy: Arc::clone(proxy),
            });
        }

        Ok(Arc::new(Self {
            listeners: runtimes,
            emit,
            cancels: CancelRegistry::new(),
            next_connection: AtomicU64::new(1),
            next_pid: AtomicI32::new(1_000_000),
            urandom: File::open("/dev/urandom").ok(),
        }))
    }

    /// The listener with this tag, if there is one.
    pub(crate) fn listener(&self, index: usize) -> Option<&ListenerRuntime> {
        self.listeners.get(index)
    }

    /// Retires one finished session: its connection slot goes back, and what
    /// the upstream attempt showed is recorded.
    ///
    /// A session that ended cleanly — or because the client went away, or
    /// because the upstream itself answered with an authentication or TLS
    /// failure — proves the upstream was reachable, so the next session does
    /// not have to check. Anything else (a dial that never connected) makes the
    /// next session check again, which is what turns "the database is down"
    /// into dbproxy's own refusal instead of pregresql's message naming the
    /// host.
    pub(crate) fn finish(
        &self,
        index: usize,
        connection: u64,
        outcome: &Result<(), pregresql::Error>,
    ) {
        let Some(listener) = self.listener(index) else {
            return;
        };
        listener.proxy.release(connection);
        let reachable = match outcome {
            Ok(()) => true,
            Err(error) if error.is_disconnect() => true,
            Err(pregresql::Error::Auth(_) | pregresql::Error::Tls(_)) => true,
            Err(_) => false,
        };
        listener.proxy.mark_reachable(reachable);
    }

    /// A connection number, unique within this process.
    pub(crate) fn next_connection_id(&self) -> u64 {
        self.next_connection.fetch_add(1, Ordering::Relaxed)
    }

    /// The backend key dbproxy hands to one client, together with pregresql's
    /// own connection id for the cancel registry.
    pub(crate) fn next_backend_key(&self) -> (i32, i32) {
        (
            self.next_pid.fetch_add(1, Ordering::Relaxed),
            self.random_secret(),
        )
    }

    /// A cancel secret: four bytes from `/dev/urandom`, or a mix of the clock
    /// and the pid if it cannot be read. The secret only has to be
    /// unguessable to another view, which never sees it.
    fn random_secret(&self) -> i32 {
        let mut bytes = [0u8; 4];
        let from_urandom = match &self.urandom {
            Some(file) => {
                let mut reader = file;
                reader.read_exact(&mut bytes).is_ok()
            }
            None => false,
        };
        if from_urandom {
            return i32::from_ne_bytes(bytes);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mixed = now ^ u64::from(std::process::id()) ^ self.next_connection_id();
        (mixed as i32) ^ ((mixed >> 32) as i32)
    }

    /// Reports a statement the policy refused (`ARCHITECTURE.md` §8.1).
    pub(crate) fn emit_sql_denied(
        &self,
        route: &str,
        pid: u32,
        proxy: &str,
        kind: SqlKind,
        sql: String,
        in_transaction: bool,
    ) {
        let denied = SqlDenied::new(
            pid,
            proxy,
            kind,
            sql,
            crate::hooks::DENIED_SQLSTATE,
            in_transaction,
        );
        self.emit_event(Some(route), EventKind::SqlDenied(denied));
    }

    /// Reports a connection dbproxy refused (`ARCHITECTURE.md` §8.1).
    pub(crate) fn emit_pg_refused(&self, route: Option<&str>, proxy: &str, reason: PgRefuseReason) {
        let refused = PgRefused::new(proxy, reason);
        self.emit_event(route, EventKind::PgRefused(refused));
    }

    /// Hands one event to init. Nothing else is written anywhere: dbproxy's
    /// stdout belongs to init (`ARCHITECTURE.md` §8.1).
    fn emit_event(&self, route: Option<&str>, kind: EventKind) {
        let mut event = Event::now(Component::Dbproxy, kind);
        if let Some(route) = route {
            event = event.with_route(route.to_string());
        }
        (self.emit)(event);
    }
}

/// One listener's policy, plus the proxy it reaches.
pub(crate) struct ListenerRuntime {
    /// What init declared for this listener.
    pub(crate) spec: ListenerSpec,
    /// The tag every connection on this listener carries, which is its index.
    pub(crate) tag: Arc<str>,
    /// The route's effective queries (`ARCHITECTURE.md` §6).
    pub(crate) allowlist: Allowlist,
    /// The upstream this listener's sessions run under.
    pub(crate) proxy: Arc<ProxyRuntime>,
}

/// One proxy's real upstream identity and its connection cap.
pub(crate) struct ProxyRuntime {
    /// Proxy name from the manifest.
    pub(crate) name: String,
    /// Upstream host.
    pub(crate) host: String,
    /// Upstream port.
    pub(crate) port: u16,
    /// Role dbproxy authenticates as.
    pub(crate) user: String,
    /// Password for that role.
    pub(crate) password: Zeroizing<String>,
    /// Database requested upstream.
    pub(crate) database: String,
    /// How to protect the upstream connection.
    pub(crate) tls: UpstreamTls,
    /// Extra startup parameters sent upstream.
    pub(crate) parameters: Vec<(String, String)>,
    /// One permit per concurrent upstream session.
    cap: Arc<Semaphore>,
    /// The permits in use, keyed by dbproxy's connection number, so the
    /// session's own task can give its permit back.
    permits: Mutex<HashMap<u64, OwnedSemaphorePermit>>,
    /// Whether the upstream was reachable the last time dbproxy looked.
    reachable: AtomicBool,
}

impl ProxyRuntime {
    /// Takes one of the proxy's connection slots, or reports that it is full.
    ///
    /// The permit is kept until [`ProxyRuntime::release`] is called with the
    /// same connection number, which the session's task does when it ends.
    pub(crate) fn acquire(&self, connection: u64) -> bool {
        let permit = match Arc::clone(&self.cap).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => return false,
        };
        lock(&self.permits).insert(connection, permit);
        true
    }

    /// Gives a connection slot back.
    pub(crate) fn release(&self, connection: u64) {
        lock(&self.permits).remove(&connection);
    }

    /// Whether dbproxy should check that the upstream answers before it lets
    /// pregresql try.
    pub(crate) fn needs_probe(&self) -> bool {
        !self.reachable.load(Ordering::Relaxed)
    }

    /// Records what the last upstream attempt showed.
    pub(crate) fn mark_reachable(&self, reachable: bool) {
        self.reachable.store(reachable, Ordering::Relaxed);
    }

    /// Whether the upstream accepted a TCP connection.
    ///
    /// pregresql dials the upstream itself and, when that fails, answers the
    /// client with its own message — which names the upstream host — without
    /// calling a hook. dbproxy therefore checks reachability itself, so a
    /// refused connection carries dbproxy's wording and a `pg.refused` event.
    /// Closing the connection without writing anything costs PostgreSQL a
    /// backend that exits on EOF, which it does not log; the check runs only
    /// while the proxy is not known to be reachable, so a healthy proxy never
    /// probes.
    pub(crate) async fn upstream_answers(&self) -> bool {
        let connect = tokio::net::TcpStream::connect((self.host.as_str(), self.port));
        match tokio::time::timeout(UPSTREAM_PROBE_TIMEOUT, connect).await {
            Ok(Ok(stream)) => {
                drop(stream);
                true
            }
            Ok(Err(_)) | Err(_) => false,
        }
    }

    /// The upstream user a session runs as. Kept together with the password so
    /// the two are always used as a pair.
    pub(crate) fn credentials(&self) -> (String, String) {
        (self.user.clone(), self.password.to_string())
    }
}

/// Locks a mutex, treating poisoning as harmless: the maps this crate keeps
/// under a lock never hold a half-updated invariant.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Whether `credentials` are the view that may use a listener declaring
/// `spec_uid`.
///
/// The listener's file permissions already decide who *can* connect; this is
/// the second check `ARCHITECTURE.md` §2.2 asks for, against the credentials
/// the kernel attached to the client's first message (`SCM_CREDENTIALS`), which
/// gVisor fills in correctly and refuses to let a client forge.
///
/// `uid` 0 never passes: root is not a view, so a root client — or a forged
/// root credential, which the kernel only lets a privileged process create — is
/// refused like any other stranger.
pub(crate) fn credentials_match(
    spec_uid: u32,
    credentials: Option<pregresql::proxy::UnixCredentials>,
) -> bool {
    match credentials {
        Some(credentials) => credentials.uid != 0 && credentials.uid == spec_uid,
        None => false,
    }
}

/// The uid from the connection's kernel-attached credentials, when there are
/// any.
pub(crate) fn connection_pid(connection: &InboundConnection) -> u32 {
    connection
        .transport
        .message_credentials()
        .map(|credentials| credentials.pid.max(0) as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pregresql::proxy::UnixCredentials;

    fn credentials(pid: i32, uid: u32) -> UnixCredentials {
        UnixCredentials { pid, uid, gid: uid }
    }

    #[test]
    fn only_the_listeners_own_uid_is_accepted() {
        assert!(credentials_match(21_000, Some(credentials(7, 21_000))));
        assert!(!credentials_match(21_000, Some(credentials(7, 21_001))));
        assert!(!credentials_match(21_000, Some(credentials(7, 1000))));
    }

    #[test]
    fn root_is_never_accepted() {
        assert!(!credentials_match(0, Some(credentials(1, 0))));
        assert!(!credentials_match(21_000, Some(credentials(1, 0))));
    }

    #[test]
    fn a_connection_without_credentials_is_refused() {
        assert!(!credentials_match(21_000, None));
        assert!(!credentials_match(0, None));
    }

    #[test]
    fn the_connection_pid_comes_from_scm_credentials() {
        let connection = InboundConnection {
            id: 3,
            transport: pregresql::proxy::Transport::Unix {
                path: std::path::PathBuf::from("/run/blastit/views/21000/.s.PGSQL.5432"),
                peer_pid: None,
                peer_uid: None,
                peer_gid: None,
                message_credentials: Some(credentials(4242, 21_000)),
            },
            tls: pregresql::proxy::TlsInfo::default(),
            protocol_version: 196_608,
            params: vec![],
            listener_tag: Some(Arc::from("0")),
        };
        assert_eq!(connection_pid(&connection), 4242);
    }

    #[test]
    fn the_probe_timeout_is_bounded() {
        assert!(UPSTREAM_PROBE_TIMEOUT <= Duration::from_secs(30));
    }
}
