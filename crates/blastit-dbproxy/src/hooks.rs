//! pregresql's two hooks, wired to blastit's policy
//! (`ARCHITECTURE.md` §2.2, §5.2, §6).
//!
//! * [`DbproxyAuth`] refuses a connection that did not arrive with the
//!   listener's own uid, refuses one when the proxy is at its connection cap,
//!   and otherwise hands pregresql the proxy's *real* upstream identity. The
//!   `user` and `database` a client sends in its startup packet are ignored:
//!   identity is the socket, never a claim.
//! * [`DbproxyQuery`] runs every statement through the listener's allowlist.
//!
//! Both hooks are per process and keep only an [`Arc`] to the shared state, so
//! a connection is never more than a few clones away from its policy.

use std::sync::Arc;

use async_trait::async_trait;
use blastit_events::{PgRefuseReason, SqlKind};
use blastit_sqlpolicy::{Decision, normalize};
use pregresql::protocol::TransactionStatus;
use pregresql::proxy::{
    AuthContext, AuthError, AuthHook, ClientAuthMethod, Endpoint, InboundConnection, QueryContext,
    QueryDecision, QueryHook, QueryKind, UpstreamTarget,
};

use crate::state::{Shared, connection_pid, credentials_match};

/// SQLSTATE a denied statement reports (`insufficient_privilege`).
pub(crate) const DENIED_SQLSTATE: &str = "42501";

/// SQLSTATE a refused connection reports when its credentials do not match the
/// listener (`invalid_authorization_specification`).
pub(crate) const CREDENTIALS_SQLSTATE: &str = "28000";

/// SQLSTATE a refused connection reports at the proxy's connection cap
/// (`too_many_connections`).
pub(crate) const TOO_MANY_SQLSTATE: &str = "53300";

/// SQLSTATE a refused connection reports when the upstream is unreachable
/// (`connection_failure`).
pub(crate) const UPSTREAM_SQLSTATE: &str = "08006";

/// What a view is told when its credentials do not match the listener.
const CREDENTIALS_MESSAGE: &str = "this connection may not use this database listener";

/// What a view is told when the proxy is at its connection cap.
const TOO_MANY_MESSAGE: &str =
    "too many connections to this database proxy: retry when a session ends";

/// What a view is told when the upstream database cannot be reached. It names
/// neither the host nor the database: a sandboxed view has no business knowing
/// where the real database lives.
const UPSTREAM_MESSAGE: &str = "the upstream database is unreachable";

/// What a view is told when the function-call sub-protocol is used. The
/// sub-protocol carries no SQL text, so there is nothing an allowlist could
/// match, and blastit denies it outright (`ARCHITECTURE.md` §6).
const FUNCTION_CALL_MESSAGE: &str =
    "the function-call protocol is not allowed through this database proxy";

/// The authentication hook.
pub(crate) struct DbproxyAuth {
    shared: Arc<Shared>,
}

impl DbproxyAuth {
    /// Builds the hook over the shared state.
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl AuthHook for DbproxyAuth {
    /// The client proves nothing: its identity is the listener it reached,
    /// checked against the `SCM_CREDENTIALS` the kernel attached to its first
    /// message.
    fn client_auth_method(&self, _connection: &InboundConnection) -> ClientAuthMethod {
        ClientAuthMethod::Trust
    }

    /// Refuses strangers and sessions the proxy cannot take, and otherwise
    /// resolves the listener's proxy into the real upstream.
    async fn resolve_upstream(&self, ctx: AuthContext<'_>) -> Result<UpstreamTarget, AuthError> {
        let listener = match self.listener(ctx.connection) {
            Some(listener) => listener,
            None => {
                // Unreachable in practice: dbproxy tags every listener it
                // serves itself. Refuse loudly rather than pick a default.
                self.shared.emit_pg_refused(
                    None,
                    "unknown",
                    PgRefuseReason::Other("unknown_listener".to_string()),
                );
                return Err(AuthError::new(
                    CREDENTIALS_SQLSTATE,
                    "this connection did not arrive on a known database listener",
                ));
            }
        };
        let route = listener.spec.route.as_str();
        let proxy = Arc::clone(&listener.proxy);

        let credentials = ctx.connection.transport.message_credentials();
        if !credentials_match(listener.spec.uid, credentials) {
            self.shared.emit_pg_refused(
                Some(route),
                &proxy.name,
                PgRefuseReason::CredentialsMismatch,
            );
            return Err(AuthError::new(CREDENTIALS_SQLSTATE, CREDENTIALS_MESSAGE));
        }

        if proxy.needs_probe() {
            if !proxy.upstream_answers().await {
                proxy.mark_reachable(false);
                self.shared.emit_pg_refused(
                    Some(route),
                    &proxy.name,
                    PgRefuseReason::UpstreamUnreachable,
                );
                return Err(AuthError::new(UPSTREAM_SQLSTATE, UPSTREAM_MESSAGE));
            }
            proxy.mark_reachable(true);
        }

        if !proxy.acquire(ctx.connection.id) {
            self.shared.emit_pg_refused(
                Some(route),
                &proxy.name,
                PgRefuseReason::TooManyConnections,
            );
            return Err(AuthError::new(TOO_MANY_SQLSTATE, TOO_MANY_MESSAGE));
        }

        let (user, password) = proxy.credentials();
        Ok(UpstreamTarget {
            endpoint: Endpoint::tcp(proxy.host.clone(), proxy.port),
            tls: proxy.tls.clone(),
            user,
            password: Some(password),
            database: Some(proxy.database.clone()),
            params: proxy.parameters.clone(),
        })
    }
}

impl DbproxyAuth {
    /// The runtime of the listener a connection arrived on.
    ///
    /// The tag is dbproxy's own (the listener's position in the configuration),
    /// so this cannot be steered by the client.
    fn listener<'a>(
        &'a self,
        connection: &InboundConnection,
    ) -> Option<&'a crate::state::ListenerRuntime> {
        let index = connection.listener_tag()?.parse::<usize>().ok()?;
        self.shared.listener(index)
    }
}

/// The statement hook.
pub(crate) struct DbproxyQuery {
    shared: Arc<Shared>,
}

impl DbproxyQuery {
    /// Builds the hook over the shared state.
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl QueryHook for DbproxyQuery {
    /// Authorises one protocol message.
    ///
    /// `Query` and `Parse` carry SQL and are matched against the listener's
    /// allowlist. `Bind` and `Execute` are the second half of a statement
    /// pregresql already authorised, so they are forwarded — unless they refer
    /// to a statement dbproxy never saw, which cannot have been authorised.
    /// `FunctionCall` is refused outright.
    async fn authorize(&self, ctx: QueryContext<'_>) -> QueryDecision {
        let listener = match ctx
            .connection
            .listener_tag()
            .and_then(|tag| tag.parse::<usize>().ok())
            .and_then(|index| self.shared.listener(index))
        {
            Some(listener) => listener,
            None => {
                return QueryDecision::deny_with(
                    DENIED_SQLSTATE,
                    "this connection did not arrive on a known database listener",
                );
            }
        };
        let route = listener.spec.route.as_str();
        let proxy = listener.proxy.name.as_str();
        let pid = connection_pid(ctx.connection);
        let in_transaction = !matches!(ctx.transaction_status, TransactionStatus::Idle);

        match ctx.kind {
            QueryKind::Simple | QueryKind::Parse => {
                let kind = match ctx.kind {
                    QueryKind::Parse => SqlKind::Parse,
                    _ => SqlKind::Query,
                };
                match listener.allowlist.check(ctx.sql) {
                    Decision::Allowed => QueryDecision::Allow,
                    Decision::Denied { normalized } => {
                        self.shared.emit_sql_denied(
                            route,
                            pid,
                            proxy,
                            kind,
                            normalized,
                            in_transaction,
                        );
                        QueryDecision::deny_with(
                            DENIED_SQLSTATE,
                            format!("statement denied for route {route}"),
                        )
                    }
                    // `Decision` is non-exhaustive: a future decision kind is
                    // refused rather than allowed by accident.
                    _ => QueryDecision::deny_with(
                        DENIED_SQLSTATE,
                        format!("statement denied for route {route}"),
                    ),
                }
            }
            QueryKind::Bind | QueryKind::Execute => {
                if ctx.statement_known {
                    return QueryDecision::Allow;
                }
                let kind = match ctx.kind {
                    QueryKind::Execute => SqlKind::Other("execute".to_string()),
                    _ => SqlKind::Other("bind".to_string()),
                };
                self.shared.emit_sql_denied(
                    route,
                    pid,
                    proxy,
                    kind,
                    normalize(ctx.sql),
                    in_transaction,
                );
                QueryDecision::deny_with(
                    DENIED_SQLSTATE,
                    format!("statement denied for route {route}"),
                )
            }
            QueryKind::FunctionCall => {
                self.shared.emit_sql_denied(
                    route,
                    pid,
                    proxy,
                    SqlKind::FunctionCall,
                    String::new(),
                    in_transaction,
                );
                QueryDecision::deny_with(DENIED_SQLSTATE, FUNCTION_CALL_MESSAGE)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use blastit_events::{Event, EventKind};
    use pregresql::proxy::{
        AuthContext, InboundConnection, TlsInfo, Transport, UnixCredentials, UpstreamTarget,
    };

    use super::*;
    use crate::config::{DbproxyConfig, ListenerSpec, ProxySpec};

    /// The pid the fake client's `SCM_CREDENTIALS` carry.
    const CLIENT_PID: i32 = 4242;

    /// A proxy pointing at `port` on the loopback address.
    fn proxy(port: u16, max_connections: u32) -> ProxySpec {
        ProxySpec {
            name: "db".to_string(),
            host: "127.0.0.1".to_string(),
            port,
            user: "real-user".to_string(),
            password: "real-password".to_string().into(),
            database: "real-database".to_string(),
            sslmode: Some("disable".to_string()),
            ca_cert: None,
            options: vec![],
            max_connections,
        }
    }

    fn listener(uid: u32, queries: &[&str], transaction_control: bool) -> ListenerSpec {
        ListenerSpec {
            fd: -1,
            route: "product-detail".to_string(),
            proxy: "db".to_string(),
            uid,
            queries: queries.iter().map(|query| query.to_string()).collect(),
            transaction_control,
        }
    }

    fn state(
        proxy: ProxySpec,
        listeners: Vec<ListenerSpec>,
    ) -> (Arc<Shared>, Arc<Mutex<Vec<Event>>>) {
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let emit: crate::Emit = Arc::new(move |event| {
            sink.lock().unwrap().push(event);
        });
        let config = DbproxyConfig {
            proxies: vec![proxy],
            listeners: listeners.clone(),
        };
        let shared = Shared::build(&config, &listeners, emit).unwrap();
        (shared, events)
    }

    /// A connection with kernel-attached credentials, as gVisor reports them.
    fn connection(id: u64, tag: &str, uid: u32) -> InboundConnection {
        InboundConnection {
            id,
            transport: Transport::Unix {
                path: PathBuf::from("/run/blastit/views/21000/.s.PGSQL.5432"),
                // Never consulted: `SO_PEERCRED` lies under gVisor.
                peer_pid: None,
                peer_uid: None,
                peer_gid: None,
                message_credentials: Some(UnixCredentials {
                    pid: CLIENT_PID,
                    uid,
                    gid: uid,
                }),
            },
            tls: TlsInfo::default(),
            protocol_version: 196_608,
            params: vec![],
            listener_tag: Some(Arc::from(tag)),
        }
    }

    fn query_context<'a>(
        connection: &'a InboundConnection,
        kind: QueryKind,
        sql: &'a str,
        statement_known: bool,
    ) -> QueryContext<'a> {
        QueryContext {
            connection_id: connection.id,
            connection,
            user: "blastit",
            upstream_user: "real-user",
            transaction_status: TransactionStatus::Idle,
            kind,
            sql,
            statement_name: None,
            portal_name: None,
            statement_known,
            parameters: None,
        }
    }

    async fn resolve(
        shared: &Arc<Shared>,
        connection: &InboundConnection,
    ) -> Result<UpstreamTarget, AuthError> {
        DbproxyAuth::new(Arc::clone(shared))
            .resolve_upstream(AuthContext {
                connection,
                // Whatever the client claims must not matter.
                user: "claim-of-the-client",
                method: ClientAuthMethod::Trust,
                verified: false,
                password: None,
            })
            .await
    }

    /// A TCP port nothing accepts on, but on which connections *succeed*.
    fn reachable_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        let port = listener.local_addr().expect("an address").port();
        // The listener stays alive for the caller, but its backlog alone makes
        // a `connect` succeed.
        std::mem::forget(listener);
        port
    }

    /// A TCP port on which connections are refused.
    fn closed_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        listener.local_addr().expect("an address").port()
    }

    #[tokio::test]
    async fn a_stranger_is_refused_and_logged() {
        let uid = 21_000;
        let (shared, events) = state(proxy(reachable_port(), 4), vec![listener(uid, &[], false)]);
        // The connection carries another uid.
        let connection = connection(1, "0", uid + 1);
        let error = resolve(&shared, &connection).await.expect_err("refused");
        assert_eq!(error.sqlstate, CREDENTIALS_SQLSTATE);
        let events = events.lock().unwrap();
        let refused = events
            .iter()
            .find_map(|event| match event.kind() {
                EventKind::PgRefused(refused) => Some(refused),
                _ => None,
            })
            .expect("a pg.refused event");
        assert_eq!(refused.reason(), &PgRefuseReason::CredentialsMismatch);
        assert_eq!(refused.proxy(), "db");
    }

    #[tokio::test]
    async fn root_is_refused_even_on_a_listener_that_declares_uid_zero() {
        let (shared, _events) = state(proxy(reachable_port(), 4), vec![listener(0, &[], false)]);
        let error = resolve(&shared, &connection(1, "0", 0))
            .await
            .expect_err("root is not a view");
        assert_eq!(error.sqlstate, CREDENTIALS_SQLSTATE);
    }

    #[tokio::test]
    async fn the_cap_refuses_the_extra_session_and_releases_its_slot() {
        let uid = 21_000;
        let (shared, events) = state(proxy(reachable_port(), 1), vec![listener(uid, &[], false)]);

        let accepted = resolve(&shared, &connection(1, "0", uid)).await;
        assert!(accepted.is_ok(), "{accepted:?}");

        let error = resolve(&shared, &connection(2, "0", uid))
            .await
            .expect_err("the cap refuses the second session");
        assert_eq!(error.sqlstate, TOO_MANY_SQLSTATE);
        assert!(
            events.lock().unwrap().iter().any(|event| matches!(
                event.kind(),
                EventKind::PgRefused(refused) if refused.reason() == &PgRefuseReason::TooManyConnections
            )),
            "the refusal must be logged"
        );

        // The first session ends: its slot comes back.
        shared.finish(0, 1, &Ok(()));
        let third = resolve(&shared, &connection(3, "0", uid)).await;
        assert!(third.is_ok(), "{third:?}");

        // A failed session that never reached the upstream does not leak a
        // slot either.
        shared.finish(
            0,
            3,
            &Err(pregresql::Error::other("cannot connect to upstream")),
        );
        let fourth = resolve(&shared, &connection(4, "0", uid)).await;
        assert!(fourth.is_ok(), "{fourth:?}");
    }

    #[tokio::test]
    async fn an_unreachable_upstream_is_refused_and_logged() {
        let uid = 21_000;
        let (shared, events) = state(proxy(closed_port(), 4), vec![listener(uid, &[], false)]);
        let error = resolve(&shared, &connection(1, "0", uid))
            .await
            .expect_err("refused");
        assert_eq!(error.sqlstate, UPSTREAM_SQLSTATE);
        assert!(
            !error.message.contains("real-user") && !error.message.contains("127.0.0.1"),
            "the refusal must not name the upstream: {}",
            error.message
        );
        assert!(
            !error.message.contains("real-password"),
            "the refusal must not carry the password: {}",
            error.message
        );
        assert!(events.lock().unwrap().iter().any(|event| matches!(
            event.kind(),
            EventKind::PgRefused(refused) if refused.reason() == &PgRefuseReason::UpstreamUnreachable
        )));
    }

    #[tokio::test]
    async fn the_hook_uses_the_proxys_identity_and_never_the_clients() {
        let uid = 21_000;
        let port = reachable_port();
        let (shared, _events) = state(proxy(port, 4), vec![listener(uid, &["SELECT 1"], true)]);
        let target = resolve(&shared, &connection(1, "0", uid))
            .await
            .expect("resolved");
        assert_eq!(target.user, "real-user");
        assert_eq!(target.password.as_deref(), Some("real-password"));
        assert_eq!(target.database.as_deref(), Some("real-database"));
        assert_eq!(target.endpoint, Endpoint::tcp("127.0.0.1", port));
        // `standard_conforming_strings` is forced on the upstream session.
        assert_eq!(
            target.params,
            [(
                "options".to_string(),
                crate::upstream::FORCE_STANDARD_CONFORMING_STRINGS.to_string()
            )]
        );
    }

    #[tokio::test]
    async fn a_granted_statement_is_allowed_and_an_ungranted_one_is_denied() {
        let uid = 21_000;
        let (shared, events) = state(
            proxy(reachable_port(), 4),
            vec![listener(uid, &["SELECT 1"], true)],
        );
        let connection = connection(1, "0", uid);
        let hook = DbproxyQuery::new(Arc::clone(&shared));

        // The simple and the extended protocols go through the same check, and
        // the grant's extra whitespace does not matter.
        for (kind, sql) in [
            (QueryKind::Simple, "SELECT 1"),
            (QueryKind::Parse, "  SELECT   1 ; "),
        ] {
            assert_eq!(
                hook.authorize(query_context(&connection, kind, sql, true))
                    .await,
                QueryDecision::Allow,
                "{kind:?} {sql:?}"
            );
        }
        // ... and this listener grants transaction control.
        assert_eq!(
            hook.authorize(query_context(&connection, QueryKind::Simple, "BEGIN", true))
                .await,
            QueryDecision::Allow
        );

        let decision = hook
            .authorize(query_context(
                &connection,
                QueryKind::Parse,
                "  SELECT   2  ",
                true,
            ))
            .await;
        assert_eq!(
            decision,
            QueryDecision::deny_with(DENIED_SQLSTATE, "statement denied for route product-detail")
        );

        let events = events.lock().unwrap();
        let denied = events
            .iter()
            .find_map(|event| match event.kind() {
                EventKind::SqlDenied(denied) => Some(denied),
                _ => None,
            })
            .expect("a sql.denied event");
        assert_eq!(denied.sql(), "SELECT 2", "the event carries normalised SQL");
        assert_eq!(denied.kind(), &SqlKind::Parse);
        assert_eq!(denied.proxy(), "db");
        assert_eq!(denied.sqlstate(), "42501");
        assert_eq!(denied.pid(), CLIENT_PID as u32);
        assert!(!denied.in_transaction());
        assert_eq!(events[0].route(), Some("product-detail"));
    }

    #[tokio::test]
    async fn transaction_control_is_only_granted_when_the_listener_grants_it() {
        let uid = 21_000;
        let (shared, events) = state(proxy(reachable_port(), 4), vec![listener(uid, &[], false)]);
        let connection = connection(1, "0", uid);
        let hook = DbproxyQuery::new(Arc::clone(&shared));

        assert_eq!(
            hook.authorize(query_context(&connection, QueryKind::Simple, "BEGIN", true))
                .await,
            QueryDecision::deny_with(DENIED_SQLSTATE, "statement denied for route product-detail")
        );
        // A query that is not granted either.
        assert_eq!(
            hook.authorize(query_context(
                &connection,
                QueryKind::Simple,
                "SELECT 1",
                true
            ))
            .await,
            QueryDecision::deny_with(DENIED_SQLSTATE, "statement denied for route product-detail")
        );
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_function_call_protocol_is_always_denied() {
        let uid = 21_000;
        // Even a listener that grants everything a query could be.
        let (shared, events) = state(
            proxy(reachable_port(), 4),
            vec![listener(uid, &["SELECT 1"], true)],
        );
        let connection = connection(1, "0", uid);
        let hook = DbproxyQuery::new(Arc::clone(&shared));

        assert_eq!(
            hook.authorize(query_context(
                &connection,
                QueryKind::FunctionCall,
                "",
                false
            ))
            .await,
            QueryDecision::deny_with(DENIED_SQLSTATE, FUNCTION_CALL_MESSAGE)
        );
        let events = events.lock().unwrap();
        let denied = events
            .iter()
            .find_map(|event| match event.kind() {
                EventKind::SqlDenied(denied) => Some(denied),
                _ => None,
            })
            .expect("a sql.denied event");
        assert_eq!(denied.kind(), &SqlKind::FunctionCall);
        assert_eq!(denied.sql(), "", "the sub-protocol carries no SQL text");
    }

    #[tokio::test]
    async fn a_bind_of_a_statement_the_proxy_never_authorised_is_denied() {
        let uid = 21_000;
        let (shared, events) = state(
            proxy(reachable_port(), 4),
            vec![listener(uid, &["SELECT 1"], false)],
        );
        let connection = connection(1, "0", uid);
        let hook = DbproxyQuery::new(Arc::clone(&shared));

        // The `Parse` before it was authorised, so pregresql remembers it.
        assert_eq!(
            hook.authorize(query_context(
                &connection,
                QueryKind::Bind,
                "SELECT 1",
                true
            ))
            .await,
            QueryDecision::Allow
        );
        // A `Bind` for a statement dbproxy never saw cannot have been
        // authorised.
        assert_eq!(
            hook.authorize(query_context(
                &connection,
                QueryKind::Bind,
                "SELECT secret FROM somewhere",
                false
            ))
            .await,
            QueryDecision::deny_with(DENIED_SQLSTATE, "statement denied for route product-detail")
        );
        let events = events.lock().unwrap();
        let denied = events
            .iter()
            .find_map(|event| match event.kind() {
                EventKind::SqlDenied(denied) => Some(denied),
                _ => None,
            })
            .expect("a sql.denied event");
        assert_eq!(denied.kind(), &SqlKind::Other("bind".to_string()));
        assert_eq!(denied.sql(), "SELECT secret FROM somewhere");
    }

    #[tokio::test]
    async fn a_connection_without_credentials_is_refused() {
        let uid = 21_000;
        let (shared, _events) = state(proxy(reachable_port(), 4), vec![listener(uid, &[], false)]);
        let mut connection = connection(1, "0", uid);
        connection.transport = Transport::Unix {
            path: PathBuf::from("/run/blastit/views/21000/.s.PGSQL.5432"),
            peer_pid: None,
            peer_uid: None,
            peer_gid: None,
            // `SO_PASSCRED` could not be enabled, so there is nothing to check.
            message_credentials: None,
        };
        let error = resolve(&shared, &connection).await.expect_err("refused");
        assert_eq!(error.sqlstate, CREDENTIALS_SQLSTATE);
    }

    #[tokio::test]
    async fn an_unknown_listener_tag_is_refused() {
        let uid = 21_000;
        let (shared, _events) = state(proxy(reachable_port(), 4), vec![listener(uid, &[], false)]);
        let error = resolve(&shared, &connection(1, "7", uid))
            .await
            .expect_err("refused");
        assert_eq!(error.sqlstate, CREDENTIALS_SQLSTATE);
    }
}
