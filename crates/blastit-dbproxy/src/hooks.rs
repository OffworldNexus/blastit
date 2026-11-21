//! pregresql's two hooks, wired to blastit's policy
//! (`ARCHITECTURE.md` §2.2, §5.2, §6).
//!
//! * [`DbproxyAuth`] refuses a connection that did not arrive with the
//!   listener's own uid, refuses one when the proxy is at its connection cap,
//!   and otherwise hands pregresql the proxy's *real* upstream identity. The
//!   `user` and `database` a client sends in its startup packet are ignored:
//!   identity is the socket, never a claim.
//! * [`DbproxyQuery`] runs every statement through the listener's allowlist —
//!   and, in [`ListenerMode::Learn`] / [`ListenerMode::DynamicCheck`], allows
//!   every statement and reports it instead (R26, ADR-0003).
//!
//! Both hooks are per process and keep only an [`Arc`] to the shared state, so
//! a connection is never more than a few clones away from its policy.

use std::sync::Arc;

use async_trait::async_trait;
use blastit_events::{PgRefuseReason, SqlDenied, SqlKind};
use blastit_sqlpolicy::{Decision, normalize, redact_literals};
use pregresql::protocol::TransactionStatus;
use pregresql::proxy::{
    AuthContext, AuthError, AuthHook, ClientAuthMethod, Endpoint, InboundConnection, QueryContext,
    QueryDecision, QueryHook, QueryKind, UpstreamTarget,
};

use crate::learn::{is_setup_statement, observe};
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
/// where the real database lives, and the same fixed message is what a view
/// gets for *every* upstream-side setup failure — dial, TLS or authentication.
pub(crate) const UPSTREAM_MESSAGE: &str = "the upstream database could not be reached";

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
                self.shared.note_refusal(ctx.connection.id);
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
        let connection = ctx.connection.id;

        let credentials = ctx.connection.transport.message_credentials();
        if !credentials_match(
            listener.spec.uid,
            credentials,
            self.shared.allow_root_listener(),
        ) {
            self.refuse(
                connection,
                Some(route),
                &proxy.name,
                PgRefuseReason::CredentialsMismatch,
            );
            return Err(AuthError::new(CREDENTIALS_SQLSTATE, CREDENTIALS_MESSAGE));
        }

        // The connection slot comes first: a probe is a connection to the
        // upstream and counts against the proxy's cap, and the permit is kept
        // for the session's lifetime once the session is committed.
        let slot = match proxy.take_slot() {
            Some(slot) => slot,
            None => {
                self.refuse(
                    connection,
                    Some(route),
                    &proxy.name,
                    PgRefuseReason::TooManyConnections,
                );
                return Err(AuthError::new(TOO_MANY_SQLSTATE, TOO_MANY_MESSAGE));
            }
        };

        if !proxy.upstream_answers().await {
            self.refuse(
                connection,
                Some(route),
                &proxy.name,
                PgRefuseReason::UpstreamUnreachable,
            );
            // `slot` is dropped here: a refused connection costs no slot.
            return Err(AuthError::new(UPSTREAM_SQLSTATE, UPSTREAM_MESSAGE));
        }

        proxy.store_slot(connection, slot);
        let (user, password) = proxy.credentials();
        // `set_db` (a `blastit check` run's own database) steers this proxy's
        // next sessions; an already-open session keeps the database it opened.
        let database = self
            .shared
            .learning()
            .database(&proxy.name)
            .unwrap_or_else(|| proxy.database.clone());
        Ok(UpstreamTarget {
            endpoint: Endpoint::tcp(proxy.host.clone(), proxy.port),
            tls: proxy.tls.clone(),
            user,
            password: Some(password),
            database: Some(database),
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

    /// Logs a refusal and marks the connection, so the setup gate delivers the
    /// `FATAL` pregresql is about to write — dbproxy's own message — instead of
    /// treating every setup-phase error as an upstream failure.
    fn refuse(&self, connection: u64, route: Option<&str>, proxy: &str, reason: PgRefuseReason) {
        self.shared.note_refusal(connection);
        self.shared.emit_pg_refused(route, proxy, reason);
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

        // A learning listener never refuses: `--learn` has to run every
        // statement of every request in one pass. It answers `Allow` and reports
        // what it saw over `obs_fd`; no `sql.denied` is emitted (R26, ADR-0003).
        if !self.shared.mode().enforces() {
            if matches!(ctx.kind, QueryKind::Simple | QueryKind::Parse) {
                self.observe(listener, &ctx);
            }
            return QueryDecision::Allow;
        }

        match ctx.kind {
            QueryKind::Simple | QueryKind::Parse => {
                let kind = match ctx.kind {
                    QueryKind::Parse => SqlKind::Parse,
                    _ => SqlKind::Query,
                };
                match listener.allowlist.check(ctx.sql) {
                    Decision::Allowed => QueryDecision::Allow,
                    Decision::Denied { normalized, reason } => {
                        let kind = if is_copy(&normalized) {
                            SqlKind::Copy
                        } else {
                            kind
                        };
                        self.shared.emit_sql_denied(
                            route,
                            denial(pid, proxy, kind, &normalized, in_transaction, reason),
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
            // `Parse` and `Query` are the gates, because they are the only
            // messages that carry SQL: a statement is authorised when it is
            // parsed or run. `Bind` is the safety net for the case where
            // pregresql is asked about a statement it never saw, which cannot
            // have been authorised.
            QueryKind::Bind => {
                if ctx.statement_known {
                    return QueryDecision::Allow;
                }
                let sql = normalize(ctx.sql);
                let kind = if is_copy(&sql) {
                    SqlKind::Copy
                } else {
                    SqlKind::Other("bind".to_string())
                };
                self.shared.emit_sql_denied(
                    route,
                    denial(
                        pid,
                        proxy,
                        kind,
                        &sql,
                        in_transaction,
                        "the statement was never authorised".to_string(),
                    ),
                );
                QueryDecision::deny_with(
                    DENIED_SQLSTATE,
                    format!("statement denied for route {route}"),
                )
            }
            // pregresql never asks about an `Execute` (it relays the message,
            // and the `Bind` before it was already authorised), but a hook must
            // answer something: an `Execute` runs a statement that was
            // authorised when it was parsed.
            QueryKind::Execute => QueryDecision::Allow,
            QueryKind::FunctionCall => {
                self.shared.emit_sql_denied(
                    route,
                    denial(
                        pid,
                        proxy,
                        SqlKind::FunctionCall,
                        "",
                        in_transaction,
                        FUNCTION_CALL_MESSAGE.to_string(),
                    ),
                );
                QueryDecision::deny_with(DENIED_SQLSTATE, FUNCTION_CALL_MESSAGE)
            }
        }
    }
}

impl DbproxyQuery {
    /// Reports one statement to the observation stream.
    ///
    /// A per-route listener (`listener.spec.route != "*"`) already identifies
    /// the route whose private socket accepted the connection, so every
    /// statement on it belongs to that route regardless of view spawn order
    /// (`R26`, `R27`). On a shared wildcard listener (`"*"`), the control
    /// channel's stack decides attribution: a request in flight names the route
    /// (or the unrouted request); an empty stack means the statement is
    /// connection setup, which every route reusing the session needs, or fixture
    /// SQL, which no route needs — and which is therefore not observed at all
    /// (ADR-0004).
    fn observe(&self, listener: &crate::state::ListenerRuntime, ctx: &QueryContext<'_>) {
        let target = if listener.spec.route == crate::config::WILDCARD_ROUTE {
            self.shared.learning().current_target()
        } else {
            Some(crate::learn::ObsTarget::Routed {
                route: listener.spec.route.clone(),
            })
        };
        let allowed = self
            .shared
            .allowlist_for(listener, target.as_ref())
            .check(ctx.sql)
            .is_allowed();
        let normalized = normalize(ctx.sql);
        let base = match ctx.kind {
            QueryKind::Parse => SqlKind::Parse,
            _ => SqlKind::Query,
        };
        let kind = if is_copy(&normalized) {
            SqlKind::Copy
        } else {
            base
        };
        let body = observe(&listener.proxy.name, kind, ctx.sql, &normalized, allowed);
        match target {
            Some(target) => {
                self.shared
                    .learning()
                    .record_statement(ctx.connection_id, &target, body)
            }
            None => {
                if is_setup_statement(&normalized) {
                    self.shared.learning().record_setup(ctx.connection_id, body);
                }
            }
        }
    }
}

/// A denial record, with every quoted literal redacted (R24, S1).
///
/// A statement that inlines a value — `WHERE email = 'alice@example.com'` — must
/// never put that value in an event that gets pasted into a ticket or shipped to
/// a log collector, so the normalised text is redacted before it is stored.
fn denial(
    pid: u32,
    proxy: &str,
    kind: SqlKind,
    normalized: &str,
    in_transaction: bool,
    reason: String,
) -> SqlDenied {
    SqlDenied::new(
        pid,
        proxy,
        kind,
        redact_literals(normalized),
        DENIED_SQLSTATE,
        in_transaction,
    )
    .with_reason(reason)
}

/// Whether a normalised statement is a `COPY`.
///
/// This is a keyword check, not a parser: blastit deliberately does not parse
/// SQL, and the only thing this decides is which `kind` a *denied* statement
/// reports. Only the text's first word is looked at, and only when it is
/// separated from the rest by a delimiter, so `COPYING` is not a `COPY`.
fn is_copy(normalized_sql: &str) -> bool {
    let mut chars = normalized_sql.char_indices();
    let mut end = 0;
    for (index, character) in chars.by_ref() {
        if character.is_ascii_alphabetic() || character == '_' {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return false;
    }
    let (word, rest) = normalized_sql.split_at(end);
    if !word.eq_ignore_ascii_case("copy") {
        return false;
    }
    match rest.chars().next() {
        None => true,
        Some(character) => character.is_whitespace() || character == '(' || character == ';',
    }
}
