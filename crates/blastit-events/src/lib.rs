//! Structured log events and their encoders (ARCHITECTURE.md §8.1).
//!
//! blastit's stdout is an event stream: one event per line, written by init
//! alone. This crate owns the event model and the two wire formats:
//!
//! * [`encode_json`] — one JSON object per line, for the server
//!   (`BLASTIT_LOG_FORMAT=json`, the default);
//! * [`encode_pretty`] — the same event as one coloured, human-readable line
//!   (`BLASTIT_LOG_FORMAT=pretty`).
//!
//! Two properties hold by construction, not by convention:
//!
//! * **Privacy.** §8.1 never logs query strings, header values, request or
//!   response bodies, or SQL parameter values, and the model has no field for
//!   any of them, so no caller can log them. [`RequestInfo::from_target`]
//!   strips the query string from the request target; SQL events carry the
//!   normalised statement with its parameters left as placeholders.
//! * **Size.** Whatever the input, an encoded JSON event is at most
//!   [`MAX_EVENT_BYTES`]: long strings are cut on a character boundary and
//!   named in `"truncated"`, and [`split_app_output`] turns one over-long app
//!   line into several `app.output` events that fit.
//!
//! ```
//! use blastit_events::{encode_json, Component, Event, EventKind, RequestInfo};
//!
//! let info = RequestInfo::from_target("GET", "/products/42/?utm_source=mail")
//!     .with_status(200)
//!     .with_timings(0.1, 18.4)
//!     .with_bytes(0, 5120)
//!     .with_remote_addr("203.0.113.7");
//! let event = Event::now(Component::Scheduler, EventKind::Request(info))
//!     .with_request_id("01JB9")
//!     .with_route("product-detail");
//!
//! let line = encode_json(&event);
//! assert!(line.contains(r#""path":"/products/42/""#));
//! assert!(!line.contains("utm_source"));
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};

#[cfg(test)]
mod fixture;
mod json;
mod pretty;
mod split;

pub use json::encode_json;
pub use pretty::encode_pretty;
pub use split::{split_app_output, split_app_output_at};

/// Largest encoded event, in bytes.
///
/// DigitalOcean truncates forwarded lines at 2048 bytes and a truncated JSON
/// line cannot be parsed, so §8.1 keeps every event at or below this. Both
/// encoders honour it for any input.
pub const MAX_EVENT_BYTES: usize = 2048;

/// Severity of an event.
///
/// Ordered from least to most severe, so that applying `BLASTIT_LOG_LEVEL` is
/// a simple comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Verbose detail, off by default.
    Debug,
    /// Normal operation.
    Info,
    /// Something an operator should look at.
    Warn,
    /// A failure.
    Error,
}

impl Level {
    /// The name used in the JSON stream and in `BLASTIT_LOG_LEVEL`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned by [`Level::from_str`] for anything but `debug`, `info`, `warn`,
/// `warning` or `error`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown log level `{0}`")]
pub struct UnknownLevel(String);

impl FromStr for Level {
    type Err = UnknownLevel;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(UnknownLevel(value.to_owned())),
        }
    }
}

/// Which process emitted an event.
///
/// init stamps this from the pipe an event arrived on, so a view cannot forge
/// another component's events (§8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    /// PID 1: startup, preload, process supervision, log collection.
    Init,
    /// The HTTP/1.1 front end.
    Http,
    /// Routing, admission control and worker budgets.
    Scheduler,
    /// The database proxy.
    Dbproxy,
    /// A sandboxed view process.
    View,
}

impl Component {
    /// The name used in the JSON stream.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Http => "http",
            Self::Scheduler => "scheduler",
            Self::Dbproxy => "dbproxy",
            Self::View => "view",
        }
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which stream of a child process a line of app output came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    /// Standard output, logged at [`Level::Info`].
    Stdout,
    /// Standard error, logged at [`Level::Warn`].
    Stderr,
}

impl Stream {
    /// The name used in the JSON stream.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// How a request ended, for `request` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The view answered.
    Ok,
    /// No route matched, `404`.
    Unmatched,
    /// The admission queue was full, `503`.
    QueueFull,
    /// The queue wait exceeded `BLASTIT_QUEUE_TIMEOUT`, `503`.
    QueueTimeout,
    /// `BLASTIT_REQUEST_TIMEOUT` expired first, `504`.
    Timeout,
    /// The view process died mid-request, `502`.
    ViewDied,
    /// The client went away before the response was complete.
    ClientGone,
}

impl Outcome {
    /// The name used in the JSON stream.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unmatched => "unmatched",
            Self::QueueFull => "queue_full",
            Self::QueueTimeout => "queue_timeout",
            Self::Timeout => "timeout",
            Self::ViewDied => "view_died",
            Self::ClientGone => "client_gone",
        }
    }
}

/// What kind of statement the database proxy refused.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SqlKind {
    /// A query (`SELECT`, `INSERT`, …).
    Query,
    /// A statement that could not be parsed at all.
    Parse,
    /// A function call.
    FunctionCall,
    /// A `COPY` statement.
    Copy,
    /// Anything else, by its protocol-reported name.
    Other(String),
}

impl SqlKind {
    /// The name used in the JSON stream.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Query => "query",
            Self::Parse => "parse",
            Self::FunctionCall => "function_call",
            Self::Copy => "copy",
            Self::Other(name) => name,
        }
    }
}

/// Why the database proxy refused a connection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PgRefuseReason {
    /// The uid that connected is not allowed to use the proxy.
    CredentialsMismatch,
    /// `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` is reached.
    TooManyConnections,
    /// The upstream database could not be reached.
    UpstreamUnreachable,
    /// Anything else.
    Other(String),
}

impl PgRefuseReason {
    /// The name used in the JSON stream.
    pub fn as_str(&self) -> &str {
        match self {
            Self::CredentialsMismatch => "credentials_mismatch",
            Self::TooManyConnections => "too_many_connections",
            Self::UpstreamUnreachable => "upstream_unreachable",
            Self::Other(reason) => reason,
        }
    }
}

/// Why http refused a request before the scheduler saw it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RejectReason {
    /// The request line or headers are malformed, `400`.
    MalformedRequest,
    /// A header block exceeds `BLASTIT_MAX_HEADER_BYTES`, `431`.
    HeaderTooLarge,
    /// A body exceeds `BLASTIT_MAX_BODY_BYTES`, `413`.
    BodyTooLarge,
    /// `Upgrade` or another unsupported expectation, `501`.
    UpgradeUnsupported,
    /// Anything else.
    Other(String),
}

impl RejectReason {
    /// The name used in the JSON stream.
    pub fn as_str(&self) -> &str {
        match self {
            Self::MalformedRequest => "malformed_request",
            Self::HeaderTooLarge => "header_too_large",
            Self::BodyTooLarge => "body_too_large",
            Self::UpgradeUnsupported => "upgrade_unsupported",
            Self::Other(reason) => reason,
        }
    }
}

/// Why a view process was retired.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RetireReason {
    /// `BLASTIT_VIEW_IDLE_TIMEOUT` expired.
    Idle,
    /// `BLASTIT_VIEW_MAX_REQUESTS` was reached.
    MaxRequests,
    /// Too many threads exceeded `BLASTIT_REQUEST_TIMEOUT`.
    StuckThreads,
    /// The memory high-watermark was crossed.
    MemoryPressure,
    /// The least-recently-used view under `BLASTIT_MAX_VIEW_PROCESSES`.
    LruCap,
    /// Anything else.
    Other(String),
}

impl RetireReason {
    /// The name used in the JSON stream.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Idle => "idle",
            Self::MaxRequests => "max_requests",
            Self::StuckThreads => "stuck_threads",
            Self::MemoryPressure => "memory_pressure",
            Self::LruCap => "lru_cap",
            Self::Other(reason) => reason,
        }
    }
}

/// The routing decision and the outcome of one request (§8.1 `request`).
///
/// There is deliberately no field for the query string, headers, bodies or
/// anything else that carries personal data. Build it with
/// [`RequestInfo::from_target`], which drops the query string.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestInfo {
    /// The request method, as received.
    pub method: String,
    /// The routed `PATH_INFO`: the request target without its query string.
    pub path: String,
    /// Index of the matching `path` regex in the route, `None` when unmatched.
    pub rule: Option<u32>,
    /// The status sent to the client.
    pub status: u16,
    /// How the request ended.
    pub outcome: Outcome,
    /// Time spent queued for a worker slot, in milliseconds.
    pub queued_ms: f64,
    /// Time from admission to the last response byte, in milliseconds.
    pub duration_ms: f64,
    /// Bytes read from the client.
    pub bytes_in: u64,
    /// Bytes written to the client.
    pub bytes_out: u64,
    /// The client address, or the `X-Forwarded-For` value when trusted.
    pub remote_addr: String,
}

impl RequestInfo {
    /// A `request` record for `request_target`, with everything from the first
    /// `?` on (the query string) dropped.
    ///
    /// The remaining fields start at their zero values; set them with the
    /// `with_*` builders.
    pub fn from_target(method: impl Into<String>, request_target: &str) -> Self {
        Self {
            method: method.into(),
            path: strip_query(request_target).to_owned(),
            rule: None,
            status: 0,
            outcome: Outcome::Ok,
            queued_ms: 0.0,
            duration_ms: 0.0,
            bytes_in: 0,
            bytes_out: 0,
            remote_addr: String::new(),
        }
    }

    /// Sets the index of the matching `path` regex.
    #[must_use]
    pub fn with_rule(mut self, rule: u32) -> Self {
        self.rule = Some(rule);
        self
    }

    /// Sets the status sent to the client.
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Sets how the request ended.
    #[must_use]
    pub fn with_outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Sets the queued and total times, in milliseconds.
    #[must_use]
    pub fn with_timings(mut self, queued_ms: f64, duration_ms: f64) -> Self {
        self.queued_ms = queued_ms;
        self.duration_ms = duration_ms;
        self
    }

    /// Sets the byte counters.
    #[must_use]
    pub fn with_bytes(mut self, bytes_in: u64, bytes_out: u64) -> Self {
        self.bytes_in = bytes_in;
        self.bytes_out = bytes_out;
        self
    }

    /// Sets the client address.
    #[must_use]
    pub fn with_remote_addr(mut self, remote_addr: impl Into<String>) -> Self {
        self.remote_addr = remote_addr.into();
        self
    }
}

/// A statement the database proxy refused (§8.1 `sql.denied`).
///
/// `sql` is the normalised statement; parameters stay as placeholders, which
/// is what §8.1 requires.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlDenied {
    /// The view process that ran the statement.
    pub pid: u32,
    /// The proxy name from the manifest.
    pub proxy: String,
    /// What kind of statement it was.
    pub kind: SqlKind,
    /// The normalised statement, parameters left as placeholders.
    pub sql: String,
    /// The SQLSTATE the view received, for example `42501`.
    pub sqlstate: String,
    /// Whether a transaction was open when the statement arrived.
    pub in_transaction: bool,
}

impl SqlDenied {
    /// A denial record.
    pub fn new(
        pid: u32,
        proxy: impl Into<String>,
        kind: SqlKind,
        sql: impl Into<String>,
        sqlstate: impl Into<String>,
        in_transaction: bool,
    ) -> Self {
        Self {
            pid,
            proxy: proxy.into(),
            kind,
            sql: sql.into(),
            sqlstate: sqlstate.into(),
            in_transaction,
        }
    }
}

/// A refused connection to a proxy (§8.1 `pg.refused`).
#[derive(Debug, Clone, PartialEq)]
pub struct PgRefused {
    /// The proxy name from the manifest.
    pub proxy: String,
    /// Why the connection was refused.
    pub reason: PgRefuseReason,
}

impl PgRefused {
    /// A refusal record.
    pub fn new(proxy: impl Into<String>, reason: PgRefuseReason) -> Self {
        Self {
            proxy: proxy.into(),
            reason,
        }
    }
}

/// A request refused before it reached the scheduler (§8.1 `http.rejected`).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRejected {
    /// The client address, as far as http can tell.
    pub remote_addr: String,
    /// The status sent, for example `400`, `413`, `431` or `501`.
    pub status: u16,
    /// Why the request was refused.
    pub reason: RejectReason,
}

impl HttpRejected {
    /// A rejection record.
    pub fn new(remote_addr: impl Into<String>, status: u16, reason: RejectReason) -> Self {
        Self {
            remote_addr: remote_addr.into(),
            status,
            reason,
        }
    }
}

/// A view process was forked (§8.1 `process.spawned`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSpawned {
    /// The new process.
    pub pid: u32,
    /// The uid it runs under.
    pub uid: u32,
    /// How long the fork took, in milliseconds.
    pub fork_ms: f64,
}

impl ProcessSpawned {
    /// A spawn record.
    pub fn new(pid: u32, uid: u32, fork_ms: f64) -> Self {
        Self { pid, uid, fork_ms }
    }
}

/// A view process ended (§8.1 `process.exited`).
///
/// Exactly one of `status` and `signal` is normally set; both are `None` when
/// the wait status could not be obtained.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessExited {
    /// The process that ended.
    pub pid: u32,
    /// The uid it ran under.
    pub uid: u32,
    /// The exit code, when it exited by itself.
    pub status: Option<i32>,
    /// The signal that killed it, when it was signalled.
    pub signal: Option<i32>,
}

impl ProcessExited {
    /// An exit record for a clean exit or an unknown status for `pid`.
    pub fn new(pid: u32, uid: u32) -> Self {
        Self {
            pid,
            uid,
            status: None,
            signal: None,
        }
    }

    /// Sets the exit code.
    #[must_use]
    pub fn with_status(mut self, status: i32) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the signal that killed the process.
    #[must_use]
    pub fn with_signal(mut self, signal: i32) -> Self {
        self.signal = Some(signal);
        self
    }
}

/// A view process was retired on purpose (§8.1 `process.retired`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRetired {
    /// The process that was retired.
    pub pid: u32,
    /// The uid it ran under.
    pub uid: u32,
    /// Why it was retired.
    pub reason: RetireReason,
}

impl ProcessRetired {
    /// A retirement record.
    pub fn new(pid: u32, uid: u32, reason: RetireReason) -> Self {
        Self { pid, uid, reason }
    }
}

/// One line printed by a child process (§8.1 `app.output`).
///
/// init wraps every child line in one of these. A line that does not fit is
/// split by [`split_app_output`] into `parts` events, `part` being 1-based.
#[derive(Debug, Clone, PartialEq)]
pub struct AppOutput {
    /// Which stream the line came from.
    pub stream: Stream,
    /// The line, without its terminating newline.
    pub line: String,
    /// Index of this piece, 1-based.
    pub part: u32,
    /// Number of pieces the original line was split into.
    pub parts: u32,
}

impl AppOutput {
    /// A single, whole line.
    pub fn new(stream: Stream, line: impl Into<String>) -> Self {
        Self {
            stream,
            line: line.into(),
            part: 1,
            parts: 1,
        }
    }

    /// One piece of a line that was split into `parts` pieces.
    #[must_use]
    pub fn with_part(mut self, part: u32, parts: u32) -> Self {
        self.part = part;
        self.parts = parts;
        self
    }
}

/// Payload of the `boot`, `ready` and `shutdown` events (§8.1).
///
/// `outputs` carries the rendered output variable *names* per proxy, so the
/// server's own log can explain what the app's environment will look like
/// without ever printing a value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lifecycle {
    /// Named phase, for example `preload` or `drain`.
    pub stage: Option<String>,
    /// Number of routes in the manifest.
    pub routes: Option<u32>,
    /// Size of the global worker budget.
    pub workers: Option<u32>,
    /// How long the phase took, in milliseconds.
    pub duration_ms: Option<f64>,
    /// Output variable names per proxy; never values.
    pub outputs: BTreeMap<String, Vec<String>>,
}

impl Lifecycle {
    /// Sets the phase name.
    #[must_use]
    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    /// Sets the route count.
    #[must_use]
    pub fn with_routes(mut self, routes: u32) -> Self {
        self.routes = Some(routes);
        self
    }

    /// Sets the worker budget.
    #[must_use]
    pub fn with_workers(mut self, workers: u32) -> Self {
        self.workers = Some(workers);
        self
    }

    /// Sets the phase duration, in milliseconds.
    #[must_use]
    pub fn with_duration_ms(mut self, duration_ms: f64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Records that proxy `proxy` will see output variable `name`.
    #[must_use]
    pub fn with_output(mut self, proxy: impl Into<String>, name: impl Into<String>) -> Self {
        self.outputs
            .entry(proxy.into())
            .or_default()
            .push(name.into());
        self
    }
}

/// A problem found while validating the manifest (§8.1 `manifest.warning`).
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestWarning {
    /// The warning text, for example an unattached group.
    pub message: String,
}

impl ManifestWarning {
    /// A warning record.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// What an event is about; one variant per row of the §8.1 table.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    /// The routing decision and the outcome of a request.
    Request(RequestInfo),
    /// A statement the database proxy refused.
    SqlDenied(SqlDenied),
    /// A connection to a proxy was refused.
    PgRefused(PgRefused),
    /// A request was refused before reaching the scheduler.
    HttpRejected(HttpRejected),
    /// A view process was forked.
    ProcessSpawned(ProcessSpawned),
    /// A view process ended.
    ProcessExited(ProcessExited),
    /// A view process was retired on purpose.
    ProcessRetired(ProcessRetired),
    /// A child process printed a line.
    AppOutput(AppOutput),
    /// The server started a phase of its startup.
    Boot(Lifecycle),
    /// The server is serving.
    Ready(Lifecycle),
    /// The server is shutting down, or has finished draining.
    Shutdown(Lifecycle),
    /// The manifest is usable but has a problem worth reporting.
    ManifestWarning(ManifestWarning),
}

impl EventKind {
    /// The `event` name used in both formats.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Request(_) => "request",
            Self::SqlDenied(_) => "sql.denied",
            Self::PgRefused(_) => "pg.refused",
            Self::HttpRejected(_) => "http.rejected",
            Self::ProcessSpawned(_) => "process.spawned",
            Self::ProcessExited(_) => "process.exited",
            Self::ProcessRetired(_) => "process.retired",
            Self::AppOutput(_) => "app.output",
            Self::Boot(_) => "boot",
            Self::Ready(_) => "ready",
            Self::Shutdown(_) => "shutdown",
            Self::ManifestWarning(_) => "manifest.warning",
        }
    }

    /// The level from §8.1's `Level` column, used by [`Event::new`] and
    /// [`Event::now`].
    pub fn default_level(&self) -> Level {
        match self {
            Self::Request(info) => {
                if info.status >= 500 {
                    Level::Warn
                } else {
                    Level::Info
                }
            }
            Self::SqlDenied(_) | Self::PgRefused(_) | Self::ManifestWarning(_) => Level::Warn,
            Self::AppOutput(output) => match output.stream {
                Stream::Stdout => Level::Info,
                Stream::Stderr => Level::Warn,
            },
            Self::ProcessExited(info) => {
                let crashed =
                    info.signal.is_some() || info.status.is_some_and(|status| status != 0);
                if crashed { Level::Warn } else { Level::Info }
            }
            Self::HttpRejected(_)
            | Self::ProcessSpawned(_)
            | Self::ProcessRetired(_)
            | Self::Boot(_)
            | Self::Ready(_)
            | Self::Shutdown(_) => Level::Info,
        }
    }
}

/// One event: the fields common to every event, plus its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// When the event happened, in UTC; both encoders print milliseconds.
    pub ts: OffsetDateTime,
    /// Severity, normally [`EventKind::default_level`].
    pub level: Level,
    /// The component that emitted the event.
    pub component: Component,
    /// The route the event belongs to, when it applies.
    pub route: Option<String>,
    /// The request id, when it applies. dbproxy never has one (§8.1).
    pub request_id: Option<String>,
    /// What the event is about.
    pub kind: EventKind,
}

impl Event {
    /// An event at `ts`, with the level from [`EventKind::default_level`].
    pub fn at(ts: OffsetDateTime, component: Component, kind: EventKind) -> Self {
        Self {
            ts,
            level: kind.default_level(),
            component,
            route: None,
            request_id: None,
            kind,
        }
    }

    /// An event at the current time, with the level from
    /// [`EventKind::default_level`].
    pub fn now(component: Component, kind: EventKind) -> Self {
        Self::at(OffsetDateTime::now_utc(), component, kind)
    }

    /// Same as [`Event::now`].
    pub fn new(component: Component, kind: EventKind) -> Self {
        Self::now(component, kind)
    }

    /// Overrides the level chosen by [`EventKind::default_level`].
    #[must_use]
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Overrides the timestamp.
    #[must_use]
    pub fn with_ts(mut self, ts: OffsetDateTime) -> Self {
        self.ts = ts;
        self
    }

    /// Sets the route the event belongs to.
    #[must_use]
    pub fn with_route(mut self, route: impl Into<String>) -> Self {
        self.route = Some(route.into());
        self
    }

    /// Sets the request id.
    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

/// The request target without its query string.
///
/// §8.1 never logs query strings, so this is how a request target becomes the
/// `path` of a `request` event. A `#` is kept: it can be part of a path, and
/// fragments are not sent in a request line.
pub fn strip_query(request_target: &str) -> &str {
    match request_target.split_once('?') {
        Some((path, _)) => path,
        None => request_target,
    }
}

/// `2026-10-25T14:02:11.384Z`: RFC 3339, UTC, always three fractional digits.
pub(crate) fn format_timestamp(ts: OffsetDateTime) -> String {
    let ts = ts.to_offset(UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        ts.year(),
        u8::from(ts.month()),
        ts.day(),
        ts.hour(),
        ts.minute(),
        ts.second(),
        ts.millisecond()
    )
}

/// `14:02:11.384`, in UTC.
pub(crate) fn format_clock_time(ts: OffsetDateTime) -> String {
    let ts = ts.to_offset(UtcOffset::UTC);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ts.hour(),
        ts.minute(),
        ts.second(),
        ts.millisecond()
    )
}

/// The largest length at or below `max` that does not split a character.
pub(crate) fn floor_boundary(text: &str, max: usize) -> usize {
    if max >= text.len() {
        return text.len();
    }
    let mut index = max;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn kind_with_levels() -> Vec<(EventKind, Level)> {
        vec![
            (
                EventKind::Request(RequestInfo::from_target("GET", "/a").with_status(200)),
                Level::Info,
            ),
            (
                EventKind::Request(RequestInfo::from_target("GET", "/a").with_status(503)),
                Level::Warn,
            ),
            (
                EventKind::Request(RequestInfo::from_target("GET", "/a").with_status(500)),
                Level::Warn,
            ),
            (
                EventKind::SqlDenied(SqlDenied::new(
                    1,
                    "db",
                    SqlKind::Query,
                    "SELECT 1",
                    "42501",
                    false,
                )),
                Level::Warn,
            ),
            (
                EventKind::PgRefused(PgRefused::new("db", PgRefuseReason::TooManyConnections)),
                Level::Warn,
            ),
            (
                EventKind::HttpRejected(HttpRejected::new(
                    "203.0.113.7",
                    400,
                    RejectReason::MalformedRequest,
                )),
                Level::Info,
            ),
            (
                EventKind::ProcessSpawned(ProcessSpawned::new(57, 20001, 12.5)),
                Level::Info,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001)),
                Level::Info,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001).with_status(0)),
                Level::Info,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001).with_status(1)),
                Level::Warn,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001).with_signal(9)),
                Level::Warn,
            ),
            (
                EventKind::ProcessRetired(ProcessRetired::new(57, 20001, RetireReason::Idle)),
                Level::Info,
            ),
            (
                EventKind::AppOutput(AppOutput::new(Stream::Stdout, "hello")),
                Level::Info,
            ),
            (
                EventKind::AppOutput(AppOutput::new(Stream::Stderr, "boom")),
                Level::Warn,
            ),
            (EventKind::Boot(Lifecycle::default()), Level::Info),
            (EventKind::Ready(Lifecycle::default()), Level::Info),
            (EventKind::Shutdown(Lifecycle::default()), Level::Info),
            (
                EventKind::ManifestWarning(ManifestWarning::new("unattached group")),
                Level::Warn,
            ),
        ]
    }

    #[test]
    fn default_levels_match_the_table() {
        for (kind, level) in kind_with_levels() {
            assert_eq!(
                kind.default_level(),
                level,
                "wrong default level for {}",
                kind.name()
            );
        }
    }

    #[test]
    fn event_constructors_pick_the_level() {
        let kind = EventKind::Request(RequestInfo::from_target("GET", "/a").with_status(503));
        assert_eq!(
            Event::new(Component::Scheduler, kind.clone()).level,
            Level::Warn
        );
        assert_eq!(
            Event::now(Component::Scheduler, kind.clone()).level,
            Level::Warn
        );
        let ts = OffsetDateTime::from_unix_timestamp(0).unwrap();
        let event = Event::at(ts, Component::Scheduler, kind);
        assert_eq!(event.ts, ts);
        assert_eq!(event.level, Level::Warn);
        assert_eq!(event.with_level(Level::Debug).level, Level::Debug);
    }

    #[test]
    fn every_kind_has_a_name() {
        let names: Vec<&str> = kind_with_levels()
            .iter()
            .map(|(kind, _)| kind.name())
            .collect();
        for expected in [
            "request",
            "sql.denied",
            "pg.refused",
            "http.rejected",
            "process.spawned",
            "process.exited",
            "process.retired",
            "app.output",
            "boot",
            "ready",
            "shutdown",
            "manifest.warning",
        ] {
            assert!(names.contains(&expected), "missing event name {expected}");
        }
    }

    #[test]
    fn levels_order_and_parse() {
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
        assert_eq!("INFO".parse::<Level>().unwrap(), Level::Info);
        assert_eq!("warn".parse::<Level>().unwrap(), Level::Warn);
        assert_eq!("WARNING".parse::<Level>().unwrap(), Level::Warn);
        assert_eq!("error".parse::<Level>().unwrap(), Level::Error);
        assert!("nope".parse::<Level>().is_err());
        assert_eq!(Level::Warn.to_string(), "warn");
    }

    #[test]
    fn component_names_match_the_table() {
        let expected = [
            (Component::Init, "init"),
            (Component::Http, "http"),
            (Component::Scheduler, "scheduler"),
            (Component::Dbproxy, "dbproxy"),
            (Component::View, "view"),
        ];
        for (component, name) in expected {
            assert_eq!(component.as_str(), name);
            assert_eq!(component.to_string(), name);
        }
    }

    #[test]
    fn reason_enums_render_snake_case_and_escape_hatches() {
        assert_eq!(Outcome::QueueTimeout.as_str(), "queue_timeout");
        assert_eq!(Outcome::ViewDied.as_str(), "view_died");
        assert_eq!(SqlKind::FunctionCall.as_str(), "function_call");
        assert_eq!(SqlKind::Other("listen".to_owned()).as_str(), "listen");
        assert_eq!(
            PgRefuseReason::CredentialsMismatch.as_str(),
            "credentials_mismatch"
        );
        assert_eq!(RejectReason::HeaderTooLarge.as_str(), "header_too_large");
        assert_eq!(RejectReason::Other("teapot".to_owned()).as_str(), "teapot");
        assert_eq!(RetireReason::MemoryPressure.as_str(), "memory_pressure");
        assert_eq!(RetireReason::Other("oom".to_owned()).as_str(), "oom");
        assert_eq!(Stream::Stderr.as_str(), "stderr");
    }

    #[test]
    fn request_target_strips_the_query_string() {
        assert_eq!(
            strip_query("/products/42/?utm_source=mail"),
            "/products/42/"
        );
        assert_eq!(strip_query("/products/42/?"), "/products/42/");
        assert_eq!(strip_query("/products/42/"), "/products/42/");
        assert_eq!(strip_query("/in?a=1&b=2?c"), "/in");
        assert_eq!(strip_query("/in#frag?x=1"), "/in#frag");
        assert_eq!(strip_query(""), "");
        assert_eq!(strip_query("?only=query"), "");
        assert_eq!(strip_query("/a?token=secret"), "/a");

        let info = RequestInfo::from_target("GET", "/products/42/?token=secret");
        assert_eq!(info.method, "GET");
        assert_eq!(info.path, "/products/42/");
        assert_eq!(info.status, 0);
        assert_eq!(info.outcome, Outcome::Ok);
        assert_eq!(info.rule, None);
    }

    #[test]
    fn timestamp_and_clock_are_utc_with_milliseconds() {
        let ts = OffsetDateTime::from_unix_timestamp(1_792_936_931).unwrap()
            + time::Duration::milliseconds(384);
        assert_eq!(format_timestamp(ts), "2026-10-25T14:02:11.384Z");
        assert_eq!(format_clock_time(ts), "14:02:11.384");
        let epoch = OffsetDateTime::from_unix_timestamp(0).unwrap();
        assert_eq!(format_timestamp(epoch), "1970-01-01T00:00:00.000Z");
        // Any offset is normalised to UTC.
        let offset = ts.to_offset(time::UtcOffset::from_hms(2, 0, 0).unwrap());
        assert_eq!(format_timestamp(offset), "2026-10-25T14:02:11.384Z");
    }

    #[test]
    fn floor_boundary_never_splits_a_character() {
        let text = "aé漢";
        for max in 0..=text.len() + 2 {
            let index = floor_boundary(text, max);
            assert!(text.is_char_boundary(index));
            assert!(index <= max.min(text.len()));
        }
        assert_eq!(floor_boundary("aé漢", 2), 1);
        assert_eq!(floor_boundary("aé漢", 3), 3);
    }

    #[test]
    fn builders_set_every_field() {
        let info = RequestInfo::from_target("POST", "/x?y=1")
            .with_rule(2)
            .with_status(201)
            .with_outcome(Outcome::Ok)
            .with_timings(1.5, 20.0)
            .with_bytes(10, 20)
            .with_remote_addr("198.51.100.9");
        assert_eq!(info.rule, Some(2));
        assert_eq!(info.status, 201);
        assert_eq!(info.queued_ms, 1.5);
        assert_eq!(info.duration_ms, 20.0);
        assert_eq!((info.bytes_in, info.bytes_out), (10, 20));
        assert_eq!(info.remote_addr, "198.51.100.9");

        let lifecycle = Lifecycle::default()
            .with_stage("preload")
            .with_routes(3)
            .with_workers(16)
            .with_duration_ms(42.5)
            .with_output("db", "DATABASE_URL")
            .with_output("db", "PGHOST");
        assert_eq!(lifecycle.stage.as_deref(), Some("preload"));
        assert_eq!(lifecycle.routes, Some(3));
        assert_eq!(lifecycle.workers, Some(16));
        assert_eq!(lifecycle.duration_ms, Some(42.5));
        assert_eq!(
            lifecycle.outputs["db"],
            vec!["DATABASE_URL".to_owned(), "PGHOST".to_owned()]
        );

        let output = AppOutput::new(Stream::Stderr, "boom").with_part(2, 3);
        assert_eq!((output.part, output.parts), (2, 3));
        assert_eq!(ManifestWarning::new("x").message, "x");
        assert_eq!(ProcessSpawned::new(1, 2, 3.0).uid, 2);
        assert_eq!(ProcessExited::new(1, 2).with_signal(9).signal, Some(9));
        assert_eq!(
            ProcessRetired::new(1, 2, RetireReason::Idle).reason,
            RetireReason::Idle
        );
        assert_eq!(
            HttpRejected::new("a", 413, RejectReason::BodyTooLarge).status,
            413
        );
    }

    #[test]
    fn events_are_cloneable_and_comparable() {
        let event = Event::at(
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
            Component::Init,
            EventKind::Ready(Lifecycle::default()),
        )
        .with_route("product-detail")
        .with_request_id("01JB9");
        assert_eq!(event.clone(), event);
        assert_eq!(event.route.as_deref(), Some("product-detail"));
        assert_eq!(event.request_id.as_deref(), Some("01JB9"));
        assert!(
            event
                .with_ts(OffsetDateTime::from_unix_timestamp(1).unwrap())
                .ts
                > OffsetDateTime::from_unix_timestamp(0).unwrap()
        );
    }
}
