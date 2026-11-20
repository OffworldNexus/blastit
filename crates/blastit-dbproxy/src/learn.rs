//! Wire observation for `--learn` and `blastit check` (R24, R26, R31,
//! ADR-0003, ADR-0004).
//!
//! A learning listener never refuses a statement: it answers `Allow` to every
//! message, records what it saw, and writes one [`WireSqlObs`] per observation
//! as a JSON line to the internal `obs_fd`. Nothing here is a public event:
//! dbproxy's documented stdout vocabulary (`docs/reference/log-events.md`) gains
//! no kind, and no `sql.denied` is emitted in a learning mode.
//!
//! # Attribution
//!
//! A per-route listener (`listener.spec.route != "*"`, as in `blastit dev
//! --learn`) already names the route whose private socket accepted the
//! connection, so its statements are attributed directly to that route. A shared
//! wildcard listener (`"*"`, as in `blastit check`) is one socket per proxy, so
//! a route is not known from the connection: the control socket (`check_ctl_fd`)
//! carries a JSON line per operation — `push_routed`, `push_unrouted`, `pop`,
//! `set_db` — and the top of the resulting stack is the [`ObsTarget`] the next
//! statements belong to. Each line is bounded to [`CONTROL_MAX_LINE`] bytes and
//! every read and write is bounded by [`CONTROL_TIMEOUT`], so a wedged or
//! hostile peer can neither grow dbproxy's memory nor block it for ever.
//!
//! # Session setup
//!
//! A driver's connection setup (`SET`, `SHOW`, the fixed
//! [`KNOWN_STATIC_SETUP_SQL`](blastit_sqlpolicy::KNOWN_STATIC_SETUP_SQL)
//! statements, and read-only catalogue introspection) runs while no request is
//! in flight, so there is no target to attribute it to. Those statements are
//! recorded per session (at most [`MAX_SETUP_STATEMENTS`] distinct ones) and
//! replayed **once**, against the first target the session reports — routed or
//! unrouted, because either one is a request whose route needs the same grants
//! when it reuses the connection. Anything else under an empty stack is fixture
//! or migration SQL: it is not application traffic, so it is not observed at
//! all (zero `obs_fd` lines).
//!
//! # Redaction
//!
//! [`WireSqlObs::sql`] is always [`redact_literals`] of the normalised text, and
//! a refused statement's [`LearnVerdict::Refused`] carries the redacted text too.
//! No user-derived literal byte reaches `obs_fd` (S1, R24).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use blastit_events::SqlKind;
use blastit_sqlpolicy::{
    Allowlist, LearnVerdict, check_learnable, is_introspection_setup_sql, redact_literals,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::config::RoutePolicySpec;
use crate::policy::PolicyError;

/// How many distinct session-setup statements one session remembers.
///
/// A driver sends a handful (time zone, `client_encoding`, a version query);
/// eight is above every real one while keeping the buffer bounded.
pub(crate) const MAX_SETUP_STATEMENTS: usize = 8;

/// How deep the control channel's target stack may grow.
///
/// A routed request pushes one target and pops it; an unrouted one does the
/// same. Eight nested pushes mean the peer has lost its `pop`, and refusing the
/// push fails closed instead of growing without bound.
pub(crate) const MAX_TARGET_DEPTH: usize = 8;

/// The largest control line accepted, in bytes (excluding the newline).
pub(crate) const CONTROL_MAX_LINE: usize = 4096;

/// How long one control read or write may take before the channel is closed.
pub(crate) const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// What one observation belongs to.
///
/// Serialised as `{"scope":"routed","route":…}` or
/// `{"scope":"unrouted","method":…,"path":…}`, so a consumer tells the two
/// apart without a delimiter convention (ADR-0003, `T2-B3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ObsTarget {
    /// A request that matched a manifest route.
    Routed {
        /// Route name from the manifest.
        route: String,
    },
    /// A request that matched no route: `--learn` proposes one for it.
    Unrouted {
        /// Request method.
        method: String,
        /// Request path, query string already removed by the scheduler.
        path: String,
    },
}

/// [`LearnVerdict`] as `WireSqlObs.verdict` has always spelled it on the wire:
/// the tag `kind`, the field names `normalized` / `redacted` / `reason`, and a
/// one-line reason a consumer prints rather than matches on.
///
/// This module is the **one** adapter between the policy's verdict and that
/// record, and it is deliberately the whole of it: `check --json`, the audit and
/// the Python readers parse the record, so its bytes are a contract. The reason
/// is rendered with [`RefuseReason`]'s `Display` — already stripped of the token
/// the parser quoted — and read back as [`RefuseReason::Reported`], which prints
/// that text unchanged, so a refused record round-trips byte for byte.
mod verdict_wire {
    use blastit_sqlpolicy::{LearnVerdict, RefuseReason};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// The record's shape and only its shape: `LearnVerdict` converts to this
    /// and back, so the policy type itself needs no `serde`.
    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Wire {
        /// The statement is grantable, as the normalised text a grant holds.
        Query { normalized: String },
        /// The statement cannot be granted.
        Refused { redacted: String, reason: String },
    }

    /// Writes `verdict` as the record's JSON object.
    pub(super) fn serialize<S: Serializer>(
        verdict: &LearnVerdict,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let wire = match verdict {
            LearnVerdict::Query { normalized } => Wire::Query {
                normalized: normalized.clone(),
            },
            LearnVerdict::Refused { redacted, reason } => Wire::Refused {
                redacted: redacted.clone(),
                reason: reason.to_string(),
            },
            // `LearnVerdict` is non-exhaustive: a verdict this build does not
            // know is reported as a refusal, never as a grant.
            _ => Wire::Refused {
                redacted: "…".to_string(),
                reason: "the statement was refused by a newer policy verdict".to_string(),
            },
        };
        wire.serialize(serializer)
    }

    /// Reads the record's JSON object back into a [`LearnVerdict`].
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<LearnVerdict, D::Error> {
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Query { normalized } => LearnVerdict::Query { normalized },
            Wire::Refused { redacted, reason } => LearnVerdict::Refused {
                redacted,
                reason: RefuseReason::Reported(reason),
            },
        })
    }
}

/// One statement a learning listener saw, in its redacted wire form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSqlObs {
    /// The route or unrouted request the statement belongs to.
    pub target: ObsTarget,
    /// Proxy the statement ran against.
    pub proxy: String,
    /// Protocol kind, spelled exactly as [`SqlKind::as_str`] spells it.
    #[serde(with = "sql_kind")]
    pub kind: SqlKind,
    /// The normalised statement with every literal replaced by `…`.
    pub sql: String,
    /// Whether the statement is transaction control.
    pub is_tx: bool,
    /// Whether the route's grants would have allowed it in `Enforce`.
    pub allowed: bool,
    /// What `--learn` would propose for it.
    #[serde(with = "verdict_wire")]
    pub verdict: LearnVerdict,
}

/// [`SqlKind`] as the one string vocabulary the event stream already uses.
mod sql_kind {
    use blastit_events::SqlKind;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Writes the canonical spelling ([`SqlKind::as_str`]).
    pub(super) fn serialize<S: Serializer>(
        kind: &SqlKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(kind.as_str())
    }

    /// Reads a spelling back, keeping an unknown one as [`SqlKind::Other`].
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SqlKind, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(match text.as_str() {
            "query" => SqlKind::Query,
            "parse" => SqlKind::Parse,
            "function_call" => SqlKind::FunctionCall,
            "copy" => SqlKind::Copy,
            _ => SqlKind::Other(text),
        })
    }
}

/// One observation, before it is attributed to a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObsBody {
    /// Proxy the statement ran against.
    pub(crate) proxy: String,
    /// Protocol kind.
    pub(crate) kind: SqlKind,
    /// Normalised, redacted statement.
    pub(crate) sql: String,
    /// Whether it is transaction control.
    pub(crate) is_tx: bool,
    /// Whether the route's grants would have allowed it.
    pub(crate) allowed: bool,
    /// What `--learn` would propose for it.
    pub(crate) verdict: LearnVerdict,
}

impl ObsBody {
    /// The same observation, attributed to `target`.
    fn with_target(&self, target: &ObsTarget) -> WireSqlObs {
        WireSqlObs {
            target: target.clone(),
            proxy: self.proxy.clone(),
            kind: self.kind.clone(),
            sql: self.sql.clone(),
            is_tx: self.is_tx,
            allowed: self.allowed,
            verdict: self.verdict.clone(),
        }
    }
}

/// What one session still owes the observation stream.
#[derive(Debug, Default)]
struct SessionLearn {
    /// Setup statements recorded while no target was active.
    setup: Vec<ObsBody>,
    /// Whether they were already replayed against a routed target.
    replayed: bool,
}

/// The observation and attribution state one process shares.
///
/// `Enforce` builds one too, empty: the hooks never look at it there (the mode
/// decides), and keeping the field total avoids a second `Shared` shape.
pub(crate) struct Learning {
    /// The effective grants of every route, replaced whole by a dev policy
    /// document (§4.6 step 2).
    ///
    /// `Arc` values, not bare [`Allowlist`]s: a swap replaces the map in one
    /// assignment, and a reader that already cloned its route's `Arc` keeps
    /// evaluating against the policy it started with, so no statement can ever
    /// see a half-swapped table.
    route_policies: Mutex<HashMap<(String, String), Arc<Allowlist>>>,
    /// The declared proxy names, so `set_db` cannot invent one.
    proxies: HashSet<String>,
    /// The allowlist an unknown route evaluates against.
    empty: Arc<Allowlist>,
    /// The `obs_fd` write end; `None` once it is closed or was never given.
    obs: Mutex<Option<File>>,
    /// The active target stack, top last.
    targets: Mutex<Vec<ObsTarget>>,
    /// The per-session setup recorder, by connection number.
    sessions: Mutex<HashMap<u64, SessionLearn>>,
    /// The per-proxy database `set_db` selected.
    databases: Mutex<HashMap<String, String>>,
}

impl Learning {
    /// Builds the state from the routes known before any request ran.
    pub(crate) fn new(
        policies: impl IntoIterator<Item = RoutePolicySpec>,
        proxies: impl IntoIterator<Item = String>,
        obs: Option<File>,
    ) -> Self {
        let route_policies = policies
            .into_iter()
            .map(|policy| {
                (
                    (policy.route.clone(), policy.proxy.clone()),
                    Arc::new(policy.allowlist()),
                )
            })
            .collect();
        Self {
            route_policies: Mutex::new(route_policies),
            proxies: proxies.into_iter().collect(),
            empty: Arc::new(Allowlist::new(std::iter::empty::<&str>(), false)),
            obs: Mutex::new(obs),
            targets: Mutex::new(Vec::new()),
            sessions: Mutex::new(HashMap::new()),
            databases: Mutex::new(HashMap::new()),
        }
    }

    /// Replaces **every** route's grants with the document a `Blastfile` save
    /// produced (§4.6 step 2, §4.8).
    ///
    /// The new map is built and validated in full before anything is swapped, so
    /// a refusal leaves the old policy untouched; the swap itself is one
    /// assignment under the lock, so a reader sees either the old map or the new
    /// one and never a mixture. It is **authoritative**: a pair the document does
    /// not name evaluates against the empty allowlist afterwards, which is how a
    /// route that dropped its `postgres {}` block stops being allowed to do
    /// anything — a removal can never fail open.
    ///
    /// # Errors
    ///
    /// [`PolicyError`] naming the entry that is not a route policy. Callers
    /// answer the document with [`crate::policy::ACK_REFUSED`].
    pub(crate) fn replace_route_policies(
        &self,
        policies: impl IntoIterator<Item = RoutePolicySpec>,
    ) -> Result<(), PolicyError> {
        let mut next: HashMap<(String, String), Arc<Allowlist>> = HashMap::new();
        for (index, policy) in policies.into_iter().enumerate() {
            if policy.route.is_empty() || policy.route == crate::config::WILDCARD_ROUTE {
                return Err(PolicyError::new(format!(
                    "route policy {index} names {:?}, which is not a route name",
                    policy.route
                )));
            }
            if policy.proxy.is_empty() {
                return Err(PolicyError::new(format!(
                    "route policy {index} names no proxy"
                )));
            }
            let key = (policy.route.clone(), policy.proxy.clone());
            if next.contains_key(&key) {
                return Err(PolicyError::new(format!(
                    "two route policies cover route {:?} on proxy {:?}",
                    key.0, key.1
                )));
            }
            next.insert(key, Arc::new(policy.allowlist()));
        }
        *lock(&self.route_policies) = next;
        Ok(())
    }

    /// The allowlist one route's statements are evaluated against.
    ///
    /// A route the configuration does not know evaluates against an empty
    /// allowlist, so it reports `allowed: false` rather than silently widening
    /// to everything. The returned handle is the policy in force when it was
    /// asked for: a swap after this point is invisible to a statement already
    /// being checked against it.
    pub(crate) fn policy(&self, route: &str, proxy: &str) -> Arc<Allowlist> {
        lock(&self.route_policies)
            .get(&(route.to_string(), proxy.to_string()))
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.empty))
    }

    /// The target new statements belong to, if a request is in flight.
    pub(crate) fn current_target(&self) -> Option<ObsTarget> {
        lock(&self.targets).last().cloned()
    }

    /// Pushes one target onto the stack.
    pub(crate) fn push(&self, target: ObsTarget) -> Result<(), &'static str> {
        let mut targets = lock(&self.targets);
        if targets.len() >= MAX_TARGET_DEPTH {
            return Err("the route stack is full");
        }
        targets.push(target);
        Ok(())
    }

    /// Pops the innermost target.
    pub(crate) fn pop(&self) -> Result<(), &'static str> {
        match lock(&self.targets).pop() {
            Some(_) => Ok(()),
            None => Err("the route stack is empty"),
        }
    }

    /// Selects the upstream database a proxy's next sessions request.
    pub(crate) fn set_database(&self, proxy: &str, database: &str) -> Result<(), &'static str> {
        if !self.proxies.contains(proxy) {
            return Err("unknown proxy");
        }
        if database.is_empty() {
            return Err("the database name is empty");
        }
        lock(&self.databases).insert(proxy.to_string(), database.to_string());
        Ok(())
    }

    /// The database `set_db` selected for a proxy, if any.
    pub(crate) fn database(&self, proxy: &str) -> Option<String> {
        lock(&self.databases).get(proxy).cloned()
    }

    /// Records a setup statement seen while no target was active.
    ///
    /// Only a session that has not replayed yet records anything: after the
    /// one-time replay the connection is attributed to routes, and a setup
    /// statement that arrives with an empty stack again is fixture work.
    pub(crate) fn record_setup(&self, connection: u64, body: ObsBody) {
        let mut sessions = lock(&self.sessions);
        let session = sessions.entry(connection).or_default();
        if session.replayed || session.setup.len() >= MAX_SETUP_STATEMENTS {
            return;
        }
        if session
            .setup
            .iter()
            .any(|seen| seen.sql == body.sql && seen.kind == body.kind)
        {
            return;
        }
        session.setup.push(body);
    }

    /// Records a statement attributed to `target`.
    ///
    /// The session's setup statements are replayed first, exactly once, so the
    /// first request that reuses the connection learns the grants every route
    /// on it needs.
    pub(crate) fn record_statement(&self, connection: u64, target: &ObsTarget, body: ObsBody) {
        self.replay_setup(connection, target);
        self.write(&body.with_target(target));
    }

    /// Forgets a finished session's setup recorder.
    pub(crate) fn forget(&self, connection: u64) {
        lock(&self.sessions).remove(&connection);
    }

    /// Replays the session's held setup statements against `target`, once.
    fn replay_setup(&self, connection: u64, target: &ObsTarget) {
        let pending = {
            let mut sessions = lock(&self.sessions);
            let session = sessions.entry(connection).or_default();
            if session.replayed {
                return;
            }
            session.replayed = true;
            std::mem::take(&mut session.setup)
        };
        for body in pending {
            self.write(&body.with_target(target));
        }
    }

    /// Writes one observation as a JSON line.
    ///
    /// Best effort by construction: a closed `obs_fd` means the orchestrator
    /// that reads it is gone, so the sink is dropped and later statements are
    /// not even serialised. Nothing is written anywhere else — stdout belongs
    /// to init (`ARCHITECTURE.md` §8.1).
    fn write(&self, obs: &WireSqlObs) {
        let Ok(mut line) = serde_json::to_string(obs) else {
            return;
        };
        line.push('\n');
        let mut sink = lock(&self.obs);
        let Some(file) = sink.as_mut() else {
            return;
        };
        if file.write_all(line.as_bytes()).is_err() {
            *sink = None;
        }
    }
}

/// One command of the JSON-line control protocol (§4.2 D.2).
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ControlCommand {
    /// A request matched `route`; the statements that follow belong to it.
    PushRouted { route: String },
    /// A request matched no route; `--learn` proposes one for it.
    PushUnrouted { method: String, path: String },
    /// The request that pushed a target is over.
    Pop,
    /// The upstream database `proxy`'s next sessions request.
    SetDb { proxy: String, database: String },
}

/// The reply to one control command.
#[derive(Debug, Serialize)]
struct ControlReply<'a> {
    /// Whether the command was applied.
    ok: bool,
    /// Why it was not, when it was not.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

/// Serves the JSON-line control channel until the peer closes it or misbehaves.
///
/// A malformed or oversized line is answered with an error and closes the
/// channel: a peer that cannot speak the protocol cannot be trusted to attribute
/// what follows, and a half-parsed line must never hang the run. A peer that
/// stops mid-line is cut off after [`CONTROL_TIMEOUT`], which is what keeps a
/// wedged scheduler from blocking dbproxy's control path for ever.
pub(crate) async fn serve_control(mut stream: UnixStream, learning: Arc<Learning>) {
    let mut line = Vec::new();
    loop {
        match tokio::time::timeout(CONTROL_TIMEOUT, read_line(&mut stream, &mut line)).await {
            // The peer closed the channel: the run is over.
            Ok(Ok(false)) => return,
            Ok(Ok(true)) => {}
            Ok(Err(error)) => {
                let message = if error.kind() == io::ErrorKind::InvalidData {
                    "the control line is longer than 4096 bytes"
                } else {
                    "the control channel could not be read"
                };
                let _ = reply(&mut stream, Err(message)).await;
                return;
            }
            // No complete line within the deadline: cut the peer off rather
            // than hold the channel open for ever.
            Err(_) => return,
        }
        let trimmed = match std::str::from_utf8(&line) {
            Ok(text) => text.trim_end_matches('\r'),
            Err(_) => {
                let _ = reply(&mut stream, Err("the control line is not UTF-8")).await;
                return;
            }
        };
        let Ok(command) = serde_json::from_str::<ControlCommand>(trimmed) else {
            let _ = reply(&mut stream, Err("malformed control command")).await;
            return;
        };
        let outcome = match command {
            ControlCommand::PushRouted { route } => {
                if route.is_empty() || route == crate::config::WILDCARD_ROUTE {
                    Err("the route name is not a route")
                } else {
                    learning.push(ObsTarget::Routed { route })
                }
            }
            ControlCommand::PushUnrouted { method, path } => {
                if method.is_empty() || path.is_empty() {
                    Err("an unrouted target needs a method and a path")
                } else {
                    learning.push(ObsTarget::Unrouted { method, path })
                }
            }
            ControlCommand::Pop => learning.pop(),
            ControlCommand::SetDb { proxy, database } => learning.set_database(&proxy, &database),
        };
        if reply(&mut stream, outcome).await.is_err() {
            return;
        }
    }
}

/// Reads one newline-terminated line, bounded in bytes.
///
/// Returns `false` on end of stream. A line that grows past
/// [`CONTROL_MAX_LINE`] is refused with [`io::ErrorKind::InvalidData`] before
/// another byte is buffered.
async fn read_line(stream: &mut UnixStream, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    let mut chunk = [0u8; 256];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(false);
        }
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                return Ok(true);
            }
            line.push(byte);
            if line.len() > CONTROL_MAX_LINE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the control line is longer than 4096 bytes",
                ));
            }
        }
    }
}

/// Writes one reply line, under the same deadline as a read.
async fn reply(stream: &mut UnixStream, outcome: Result<(), &'static str>) -> io::Result<()> {
    let reply = ControlReply {
        ok: outcome.is_ok(),
        error: outcome.err(),
    };
    let mut line = serde_json::to_string(&reply).map_err(io::Error::other)?;
    line.push('\n');
    tokio::time::timeout(CONTROL_TIMEOUT, async {
        stream.write_all(line.as_bytes()).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the control reply timed out"))?
}

/// Locks a mutex, treating poisoning as harmless: what these maps hold is
/// observation state, never a half-updated invariant a reader could trip on.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Whether `normalized` is a single transaction-control statement.
///
/// Asked of the policy grammar itself — an allowlist with no queries that
/// grants transaction control allows exactly those statements and nothing else
/// (§6) — so the observation stream cannot drift from what `Enforce` decides.
pub(crate) fn is_transaction_control(normalized: &str) -> bool {
    Allowlist::new(std::iter::empty::<&str>(), true)
        .check(normalized)
        .is_allowed()
}

/// Whether a statement seen under an empty target stack is session setup.
///
/// Exact [`KNOWN_STATIC_SETUP_SQL`] matches, `SET`/`SHOW`, and read-only
/// catalogue introspection: the statements a driver sends before it runs a
/// query, which every route reusing the connection needs granted.
pub(crate) fn is_setup_statement(normalized: &str) -> bool {
    is_introspection_setup_sql(normalized)
}

/// Builds the observation a hook reports for one statement.
pub(crate) fn observe(
    proxy: &str,
    kind: SqlKind,
    sql: &str,
    normalized: &str,
    allowed: bool,
) -> ObsBody {
    ObsBody {
        proxy: proxy.to_string(),
        kind,
        sql: redact_literals(normalized),
        is_tx: is_transaction_control(normalized),
        allowed,
        verdict: check_learnable(sql),
    }
}
