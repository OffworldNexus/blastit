//! The state every connection shares: one runtime per proxy, one per listener,
//! and the event sink.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use blastit_events::{Component, Event, EventKind, PgRefuseReason, PgRefused, SqlDenied};
use blastit_sqlpolicy::Allowlist;
use pregresql::proxy::cancel::CancelRegistry;
use pregresql::proxy::{InboundConnection, UpstreamTls};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;

use crate::Emit;
use crate::config::{DbproxyConfig, ListenerMode, ListenerSpec};
use crate::error::DbproxyError;
use crate::learn::{Learning, ObsTarget};
use crate::upstream::{CaBundles, SslPlan, upstream_parameters};

/// How long dbproxy waits for the upstream to accept a TCP connection before it
/// calls the database unreachable.
pub(crate) const UPSTREAM_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a probe that *succeeded* is trusted.
///
/// A dial that worked says nothing about the next minute, but it says a lot
/// about the next session; keeping it for a while is what stops dbproxy from
/// dialling the upstream twice per session.
const UPSTREAM_REACHABLE_TTL: Duration = Duration::from_secs(300);

/// How long a probe that *failed* is trusted.
///
/// A failed dial is retried quickly, so an upstream that comes back is picked
/// up within a second, and an upstream that is down is not dialled once per
/// connection.
const UPSTREAM_UNREACHABLE_TTL: Duration = Duration::from_secs(1);

/// How many connections one listener may have in the startup phase at once.
///
/// A client that connects and then stalls holds a task and a socket; the cap
/// keeps that bounded. Beyond it the accept loop simply stops accepting, so the
/// kernel queues the next client in the listener's backlog.
pub(crate) const MAX_STARTING_CONNECTIONS: usize = 16;

/// How many cancel forwards one proxy may have in flight at once.
///
/// A cancel is a short exchange (a dial and sixteen bytes), so it gets its own
/// small cap rather than competing with sessions for the connection cap.
pub(crate) const MAX_CANCEL_FORWARDS: usize = 4;

/// Everything a connection needs that is built once, per process.
pub(crate) struct Shared {
    /// One entry per listener, indexed by its tag.
    listeners: Vec<ListenerRuntime>,
    emit: Emit,
    next_connection: AtomicU64,
    next_pid: AtomicI32,
    /// `/dev/urandom`, opened once; `None` when it could not be opened.
    urandom: Option<File>,
    /// Connections an authentication hook refused, so that the setup gate knows
    /// the `ErrorResponse` pregresql is about to write is dbproxy's own message
    /// and not an upstream failure.
    refusals: Mutex<HashSet<u64>>,
    /// What the listeners do with the statements they see.
    mode: ListenerMode,
    /// Whether a `uid == 0` listener is permitted (a root CI container).
    allow_root_listener: bool,
    /// The observation and attribution state (`--learn`/`check`).
    learning: Arc<Learning>,
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
        config.validate_proxies()?;
        config.validate_serving(listeners)?;
        config.validate_learning()?;

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
                    cancel_cap: Arc::new(Semaphore::new(MAX_CANCEL_FORWARDS)),
                    probe_lock: tokio::sync::Mutex::new(()),
                    probe_cache: Mutex::new(None),
                }),
            );
        }

        // The observation pipe (`--learn`/`check`) is handed over here and owned
        // by dbproxy from now on: init keeps the read end, nothing else refers
        // to this descriptor by number.
        //
        // SAFETY: the descriptor was created for dbproxy, is not referred to by
        // number anywhere else, and is closed when the `File` is dropped —
        // exactly the contract the inherited listener fds follow.
        let obs = config.obs_fd.map(|fd| unsafe { File::from_raw_fd(fd) });
        let learning = Arc::new(Learning::new(
            config.route_policies.iter().cloned(),
            proxies.keys().cloned(),
            obs,
        ));

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
                allowlist: if spec.unrestricted {
                    // `unrestricted = true` (§6): the route may run anything
                    // here, so the policy has nothing to compare.
                    Allowlist::unrestricted()
                } else {
                    Allowlist::new(&spec.queries, spec.transaction_control)
                        .with_tables(spec.tables.clone())
                },
                proxy: Arc::clone(proxy),
                // One registry per listener: a view can only cancel the sessions
                // that were opened on its own socket.
                cancels: CancelRegistry::new(),
                starting: Arc::new(Semaphore::new(MAX_STARTING_CONNECTIONS)),
            });
        }

        Ok(Arc::new(Self {
            listeners: runtimes,
            emit,
            next_connection: AtomicU64::new(1),
            next_pid: AtomicI32::new(1_000_000),
            urandom: File::open("/dev/urandom").ok(),
            refusals: Mutex::new(HashSet::new()),
            mode: config.mode,
            allow_root_listener: config.allow_root_listener,
            learning,
        }))
    }

    /// What the listeners do with the statements they see.
    pub(crate) fn mode(&self) -> ListenerMode {
        self.mode
    }

    /// Whether a listener may declare `uid == 0`.
    pub(crate) fn allow_root_listener(&self) -> bool {
        self.allow_root_listener
    }

    /// The observation and attribution state.
    pub(crate) fn learning(&self) -> &Arc<Learning> {
        &self.learning
    }

    /// The allowlist a statement on `listener` is evaluated against.
    ///
    /// A runtime listener carries its own route's grants (§6). A shared
    /// learning listener is one socket per proxy, so its grants come from the
    /// [`ObsTarget`] at the top of the control channel's stack, and an unknown
    /// route evaluates against an empty allowlist (never "anything allowed").
    pub(crate) fn allowlist_for<'a>(
        &'a self,
        listener: &'a ListenerRuntime,
        target: Option<&ObsTarget>,
    ) -> &'a Allowlist {
        if listener.spec.route != crate::config::WILDCARD_ROUTE {
            return &listener.allowlist;
        }
        match target {
            Some(ObsTarget::Routed { route }) => self.learning.policy(route, &listener.proxy.name),
            _ => self.learning.policy("", &listener.proxy.name),
        }
    }

    /// The listener with this tag, if there is one.
    pub(crate) fn listener(&self, index: usize) -> Option<&ListenerRuntime> {
        self.listeners.get(index)
    }

    /// A connection number, unique within this process.
    pub(crate) fn next_connection_id(&self) -> u64 {
        self.next_connection.fetch_add(1, Ordering::Relaxed)
    }

    /// The backend key dbproxy hands to one client.
    pub(crate) fn next_backend_key(&self) -> (i32, i32) {
        (
            self.next_pid.fetch_add(1, Ordering::Relaxed),
            self.random_secret(),
        )
    }

    /// A cancel secret: four bytes from `/dev/urandom`, or a mix of the clock
    /// and the pid if it cannot be read. The secret only has to be unguessable
    /// to another view, which never sees it.
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

    /// Remembers that a hook refused this connection, so the setup gate can
    /// tell dbproxy's own refusal message from an upstream failure.
    pub(crate) fn note_refusal(&self, connection: u64) {
        lock(&self.refusals).insert(connection);
    }

    /// Forgets a refusal, reporting whether there was one.
    pub(crate) fn take_refusal(&self, connection: u64) -> bool {
        lock(&self.refusals).remove(&connection)
    }

    /// Reports a statement the policy refused (`ARCHITECTURE.md` §8.1).
    ///
    /// The [`SqlDenied`] the caller builds carries the policy's own explanation
    /// — which table, column or function a table grant does not cover, or that no
    /// grant lists the text at all. It never carries a parameter value: values
    /// are not part of a policy check, and the caller redacts every quoted
    /// literal before it builds the event (R24, S1).
    pub(crate) fn emit_sql_denied(&self, route: &str, denied: SqlDenied) {
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

/// One listener's policy, its cancellation registry, and the proxy it reaches.
pub(crate) struct ListenerRuntime {
    /// What init declared for this listener.
    pub(crate) spec: ListenerSpec,
    /// The tag every connection on this listener carries, which is its index.
    pub(crate) tag: Arc<str>,
    /// The route's effective queries (`ARCHITECTURE.md` §6).
    pub(crate) allowlist: Allowlist,
    /// The upstream this listener's sessions run under.
    pub(crate) proxy: Arc<ProxyRuntime>,
    /// The cancel targets of the sessions that were opened on *this* listener.
    pub(crate) cancels: Arc<CancelRegistry>,
    /// Bounds connections that have not finished the startup phase.
    pub(crate) starting: Arc<Semaphore>,
}

/// One proxy's real upstream identity and its connection caps.
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
    /// A small, separate cap for cancel forwards.
    cancel_cap: Arc<Semaphore>,
    /// Serialises the reachability probe, so a cold proxy dials once.
    probe_lock: tokio::sync::Mutex<()>,
    /// The last probe's verdict and when it was taken.
    probe_cache: Mutex<Option<(Instant, bool)>>,
}

impl ProxyRuntime {
    /// Takes one of the proxy's connection slots, or reports that it is full.
    ///
    /// The caller holds the permit while it decides, and hands it to
    /// [`ProxyRuntime::store_slot`] once the session is committed, so a probe
    /// and a session can never exceed the cap together.
    pub(crate) fn take_slot(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.cap).try_acquire_owned().ok()
    }

    /// Keeps a connection's slot until its session ends.
    pub(crate) fn store_slot(&self, connection: u64, permit: OwnedSemaphorePermit) {
        lock(&self.permits).insert(connection, permit);
    }

    /// Gives a connection slot back.
    pub(crate) fn release_slot(&self, connection: u64) {
        lock(&self.permits).remove(&connection);
    }

    /// Takes one of the proxy's cancel-forward slots.
    pub(crate) fn take_cancel_slot(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.cancel_cap).try_acquire_owned().ok()
    }

    /// Whether the upstream answers a TCP connection.
    ///
    /// The verdict is cached — for a few minutes when the dial worked, for a
    /// second when it did not — and only one connection dials at a time, so a
    /// cold proxy pays for one probe and a healthy one pays for none. A session
    /// that fails upstream-side invalidates the cache
    /// ([`ProxyRuntime::invalidate_probe`]), so the next connection dials
    /// again.
    ///
    /// pregresql dials the upstream itself and, when that fails, answers the
    /// client with its own message, which names the upstream host, without
    /// calling a hook. dbproxy therefore establishes reachability itself, so a
    /// refused connection carries dbproxy's wording and a `pg.refused` event.
    /// A probe connection that closes without writing anything is quiet: on
    /// PostgreSQL 16.15 a bare connect-and-close logs nothing, while a client
    /// that sends a *partial* startup packet gets `incomplete startup packet`
    /// or `invalid length of startup packet` — and with the cache, probes are
    /// rare either way.
    pub(crate) async fn upstream_answers(&self) -> bool {
        let _single_flight = self.probe_lock.lock().await;
        if let Some((taken, verdict)) = *lock(&self.probe_cache) {
            let ttl = if verdict {
                UPSTREAM_REACHABLE_TTL
            } else {
                UPSTREAM_UNREACHABLE_TTL
            };
            if taken.elapsed() < ttl {
                return verdict;
            }
        }
        let verdict = self.dial().await;
        *lock(&self.probe_cache) = Some((Instant::now(), verdict));
        verdict
    }

    /// Dials the upstream, once.
    async fn dial(&self) -> bool {
        let connect = tokio::net::TcpStream::connect((self.host.as_str(), self.port));
        match tokio::time::timeout(UPSTREAM_PROBE_TIMEOUT, connect).await {
            Ok(Ok(stream)) => {
                drop(stream);
                true
            }
            Ok(Err(_)) | Err(_) => false,
        }
    }

    /// Forgets the cached verdict, so the next connection dials again. Used
    /// when a session fails before it is set up: whatever the error says, the
    /// dial is the thing to re-check.
    pub(crate) fn invalidate_probe(&self) {
        *lock(&self.probe_cache) = None;
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
/// `uid` 0 never passes unless the configuration opted in with
/// `allow_root_listener` *and* the listener itself declares `uid == 0`: root is
/// not a view (`ARCHITECTURE.md` §2.2), and the only reason the opt-in exists is
/// a root CI container where the whole tree runs as root (ADR-0001).
pub(crate) fn credentials_match(
    spec_uid: u32,
    credentials: Option<pregresql::proxy::UnixCredentials>,
    allow_root: bool,
) -> bool {
    match credentials {
        Some(credentials) => {
            let root_allowed = credentials.uid != 0 || (allow_root && spec_uid == 0);
            root_allowed && credentials.uid == spec_uid
        }
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
    use blastit_sqlpolicy::TableGrants;
    use pregresql::proxy::UnixCredentials;

    fn credentials(pid: i32, uid: u32) -> UnixCredentials {
        UnixCredentials { pid, uid, gid: uid }
    }

    #[test]
    fn only_the_listeners_own_uid_is_accepted() {
        assert!(credentials_match(
            21_000,
            Some(credentials(7, 21_000)),
            false
        ));
        assert!(!credentials_match(
            21_000,
            Some(credentials(7, 21_001)),
            false
        ));
        assert!(!credentials_match(
            21_000,
            Some(credentials(7, 1000)),
            false
        ));
    }

    #[test]
    fn root_is_never_accepted_without_the_opt_in() {
        assert!(!credentials_match(0, Some(credentials(1, 0)), false));
        assert!(!credentials_match(21_000, Some(credentials(1, 0)), false));
        // A listener that declares another uid does not accept root either,
        // even when the container opted in.
        assert!(!credentials_match(21_000, Some(credentials(1, 0)), true));
    }

    #[test]
    fn a_root_ci_container_may_opt_into_a_root_listener() {
        // ADR-0001: the opt-in exists for a container where every process is
        // root, and it only ever admits uid 0 on a listener that declares 0.
        assert!(credentials_match(0, Some(credentials(1, 0)), true));
        assert!(!credentials_match(0, Some(credentials(1, 1)), true));
        assert!(credentials_match(
            21_000,
            Some(credentials(1, 21_000)),
            true
        ));
        assert!(!credentials_match(
            21_000,
            Some(credentials(1, 21_001)),
            true
        ));
    }

    #[test]
    fn a_connection_without_credentials_is_refused() {
        assert!(!credentials_match(21_000, None, false));
        assert!(!credentials_match(0, None, true));
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
        assert!(UPSTREAM_UNREACHABLE_TTL <= UPSTREAM_REACHABLE_TTL);
    }

    #[test]
    fn every_listener_has_its_own_cap_registry_and_cancel_registry() {
        use crate::config::{DbproxyConfig, ProxySpec};
        use blastit_events::Event;

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
        let listener = |route: &str| ListenerSpec {
            fd: -1,
            route: route.to_string(),
            proxy: "db".to_string(),
            uid: 21_000,
            queries: vec![],
            transaction_control: false,
            unrestricted: false,
            tables: TableGrants::default(),
        };
        let listeners = vec![listener("a"), listener("b")];
        let config = DbproxyConfig {
            proxies: vec![proxy],
            listeners: listeners.clone(),
            ..DbproxyConfig::default()
        };
        let emit: crate::Emit = Arc::new(|_: Event| {});
        let shared = Shared::build(&config, &listeners, emit).expect("state");

        let first = shared.listener(0).expect("first");
        let second = shared.listener(1).expect("second");
        assert!(
            !Arc::ptr_eq(&first.cancels, &second.cancels),
            "a view may only cancel its own listener's sessions"
        );
        assert!(!Arc::ptr_eq(&first.starting, &second.starting));
        assert!(
            Arc::ptr_eq(&first.proxy, &second.proxy),
            "both routes reach the same upstream"
        );
        assert_eq!(
            first.starting.available_permits(),
            MAX_STARTING_CONNECTIONS,
            "each listener bounds its own startup phase"
        );
    }

    #[test]
    fn the_cancel_cap_is_small_and_separate_from_the_session_cap() {
        use crate::config::{DbproxyConfig, ProxySpec};
        use blastit_events::Event;

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
            max_connections: 2,
        };
        let listener = ListenerSpec {
            fd: -1,
            route: "a".to_string(),
            proxy: "db".to_string(),
            uid: 21_000,
            queries: vec![],
            transaction_control: false,
            unrestricted: false,
            tables: TableGrants::default(),
        };
        let config = DbproxyConfig {
            proxies: vec![proxy],
            listeners: vec![listener.clone()],
            ..DbproxyConfig::default()
        };
        let emit: crate::Emit = Arc::new(|_: Event| {});
        let shared = Shared::build(&config, &[listener], emit).expect("state");
        let proxy = &shared.listener(0).expect("listener").proxy;

        // Two session slots, and a small number of cancel forwards that do not
        // compete with them.
        let first = proxy.take_slot().expect("the first slot");
        let second = proxy.take_slot().expect("the second slot");
        assert!(proxy.take_slot().is_none(), "the session cap is two");
        let _sessions = (first, second);

        let mut cancels = Vec::new();
        while let Some(permit) = proxy.take_cancel_slot() {
            cancels.push(permit);
        }
        assert_eq!(cancels.len(), MAX_CANCEL_FORWARDS);
        assert!(
            proxy.take_cancel_slot().is_none(),
            "the cancel cap is bounded"
        );
        // ... and sessions are still refused, unaffected by the cancel cap.
        assert!(proxy.take_slot().is_none());
    }
}
