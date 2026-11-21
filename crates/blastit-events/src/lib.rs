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
//!   any of them, so no caller can log them. [`RequestInfo::from_target`] and
//!   [`RequestInfo::with_path`] are the only ways to set a `path`, and both
//!   drop everything from the first `?`; SQL events carry the normalised
//!   statement with its parameters left as placeholders.
//! * **No field-by-field construction.** Every payload field is private and is
//!   set through a constructor or a `with_*` builder, so a caller cannot place
//!   a value into a field without passing the invariant that field carries —
//!   the query-string stripping above, for instance.
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

use time::{OffsetDateTime, UtcOffset};

mod json;
mod pretty;
mod split;

pub use json::{decode_json, encode_json, encode_json_into, restamp_json_into};
pub use pretty::{encode_pretty, escape_inline};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// The view answered: it produced a complete response, whatever its status
    /// — a `5xx` an application answers itself (a framework's `500`, a
    /// script's `http_response_code(500)`) is still an answer, and `status`
    /// carries the failure.
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
    /// The handler bailed out of the request — `exit()`, `die()`, or a fatal
    /// the SAPI shim contained — and it had already answered with a `5xx`.
    ///
    /// Distinct from [`Ok`](Self::Ok) because the handler never returned (the
    /// request is a failed one, and `R19` bills it its wall time) and from
    /// [`ViewDied`](Self::ViewDied) because it answered before it went away —
    /// not a death mid-request.
    Bailed,
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
            Self::Bailed => "bailed",
            Self::ClientGone => "client_gone",
        }
    }
}

/// What kind of statement the database proxy refused.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SqlKind {
    /// A query (`SELECT`, `INSERT`, …).
    Query,
    /// A statement that could not be parsed at all.
    Parse,
    /// A function call.
    FunctionCall,
    /// A `COPY` statement.
    Copy,
    /// Anything else, by its protocol-reported name. New kinds arrive here
    /// without changing the crate's version.
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
#[non_exhaustive]
pub enum PgRefuseReason {
    /// The uid that connected is not allowed to use the proxy.
    CredentialsMismatch,
    /// `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` is reached.
    TooManyConnections,
    /// The upstream database could not be reached.
    UpstreamUnreachable,
    /// Anything else, from the proxy's own wording.
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
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// `blastit dev` reloaded every view together after a tracked file changed
    /// (`ADR-0014` decision 3).
    Reload,
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
            Self::Reload => "reload",
            Self::Other(reason) => reason,
        }
    }
}

/// How a view process ended.
///
/// A wait status is either an exit code or a signal, never both, so the model
/// has one enum instead of two options. `process.exited` writes exactly one of
/// the `status` and `signal` fields from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitKind {
    /// The process exited by itself with this code.
    Status(i32),
    /// The process was killed by this signal.
    Signal(i32),
}

impl ExitKind {
    /// The JSON field this variant fills: `status` or `signal`.
    pub const fn field_name(self) -> &'static str {
        match self {
            Self::Status(_) => "status",
            Self::Signal(_) => "signal",
        }
    }

    /// The exit code or the signal number.
    pub const fn code(self) -> i32 {
        match self {
            Self::Status(code) | Self::Signal(code) => code,
        }
    }

    /// Whether the ending counts as a crash, so `process.exited` is a warning:
    /// it was signalled, or it exited non-zero.
    pub const fn is_crash(self) -> bool {
        match self {
            Self::Signal(_) => true,
            Self::Status(0) => false,
            Self::Status(_) => true,
        }
    }
}

/// The routing decision and the outcome of one request (§8.1 `request`).
///
/// There is deliberately no field for the query string, headers, bodies or
/// anything else that carries personal data. The `path` is only ever set by
/// [`RequestInfo::from_target`] or [`RequestInfo::with_path`], both of which
/// drop the query string.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestInfo {
    method: String,
    path: String,
    rule: Option<u32>,
    status: u16,
    outcome: Outcome,
    queued_ms: f64,
    duration_ms: f64,
    bytes_in: u64,
    bytes_out: u64,
    remote_addr: String,
    /// What the request cost the view process, in nanoseconds (D5).
    cost_ns: u64,
    /// The view's thread CPU time over the same span, when it reported one.
    cpu_ns: Option<u64>,
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
            cost_ns: 0,
            cpu_ns: None,
        }
    }

    /// Replaces the `path` from a request target, dropping the query string
    /// exactly as [`RequestInfo::from_target`] does.
    #[must_use]
    pub fn with_path(mut self, request_target: &str) -> Self {
        self.path = strip_query(request_target).to_owned();
        self
    }

    /// The request method, as received.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The routed `PATH_INFO`: the request target without its query string.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Index of the matching `path` regex in the route, `None` when unmatched.
    pub fn rule(&self) -> Option<u32> {
        self.rule
    }

    /// The status sent to the client.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// How the request ended.
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// Time spent queued for a worker slot, in milliseconds.
    pub fn queued_ms(&self) -> f64 {
        self.queued_ms
    }

    /// Time from admission to the last response byte, in milliseconds.
    pub fn duration_ms(&self) -> f64 {
        self.duration_ms
    }

    /// Bytes read from the client.
    pub fn bytes_in(&self) -> u64 {
        self.bytes_in
    }

    /// Bytes written to the client.
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out
    }

    /// The client address, or the `X-Forwarded-For` value when trusted.
    pub fn remote_addr(&self) -> &str {
        &self.remote_addr
    }

    /// What the request cost the view process, in nanoseconds (D5).
    ///
    /// This is what the scheduler charged for the request: the view's own
    /// measurement, clamped between the time up to the view's first response
    /// frame and the time the scheduler observed between dispatch and
    /// completion, or that observed time when no report arrived. The exception
    /// is a contained bailout ([`Outcome::Bailed`]): a view that answers a
    /// `5xx` without returning has its report disbelieved and is charged the
    /// request's wall time, `R19`'s crash ceiling. A `5xx` an app *answered*
    /// is charged the clamp like any other answer, and a `5xx` the scheduler
    /// wrote itself (`502`/`503`/`504`) is charged dispatch to completion, so
    /// a request the scheduler failed never pays for the queue wait.
    pub fn cost_ns(&self) -> u64 {
        self.cost_ns
    }

    /// The view's thread CPU time over the request, when it reported one.
    ///
    /// `None` means the end frame carried no `RESP_END_COST` — a view built
    /// before D5, a crash, a timeout, a cancellation — or that the view had no
    /// thread CPU clock to measure with, so nothing was measured on that side.
    pub fn cpu_ns(&self) -> Option<u64> {
        self.cpu_ns
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

    /// Sets what the request cost the view process (D5), in nanoseconds.
    ///
    /// `cost_ns` is what the scheduler charged — the view's own measurement,
    /// clamped between the time to its first response frame and the observed
    /// time, or the observed time when there is no report at all. `cpu_ns` is
    /// the worker's thread CPU time over the same span, or `None` when it had
    /// no measurement to report.
    #[must_use]
    pub fn with_cost(mut self, cost_ns: u64, cpu_ns: Option<u64>) -> Self {
        self.cost_ns = cost_ns;
        self.cpu_ns = cpu_ns;
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
/// `sql` is the normalised statement text, with every parameter left as the
/// placeholder `$?` and the statement's shape normalised with it
/// (ARCHITECTURE.md §6): a list of parameters is `IN ($?, ...)` and a run of
/// identical `VALUES` rows is `(row), ...`. Parameter values are never logged
/// (§8.1), and the type has no field for them.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlDenied {
    pid: u32,
    proxy: String,
    kind: SqlKind,
    /// Why the policy refused the statement (D7): `select on
    /// auth_user.password not granted`, `the statement is not granted`. The
    /// empty string when the refusal has no reason of its own.
    reason: String,
    sql: String,
    sqlstate: String,
    in_transaction: bool,
}

impl SqlDenied {
    /// A denial record.
    ///
    /// **`sql` is the caller's contract**: the normalised statement text, the
    /// same one the proxy matched against its allowlist, with every parameter
    /// left as the placeholder `$?` — a parameter list is `IN ($?, ...)`, a run
    /// of identical rows is `(row), ...`. Parameter *values* are never logged
    /// (§8.1), so a caller that has only the interpolated statement must send
    /// the parameterised form instead — this crate cannot undo an
    /// interpolation that has already happened.
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
            reason: String::new(),
            sql: sql.into(),
            sqlstate: sqlstate.into(),
            in_transaction,
        }
    }

    /// The same record with the policy's reason for the refusal (D7).
    ///
    /// The reason names what is missing — `select on auth_user.password not
    /// granted`, `the function pg_read_file is not granted` — so the person
    /// reading the log knows what to add. It never carries a parameter value:
    /// values are not part of a policy check at all (§8.1).
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// Why the policy refused the statement, empty when it said nothing.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The view process that ran the statement.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The proxy name from the manifest.
    pub fn proxy(&self) -> &str {
        &self.proxy
    }

    /// What kind of statement it was.
    pub fn kind(&self) -> &SqlKind {
        &self.kind
    }

    /// The normalised statement, with every parameter left as `$?`.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The SQLSTATE the view received, for example `42501`.
    pub fn sqlstate(&self) -> &str {
        &self.sqlstate
    }

    /// Whether a transaction was open when the statement arrived.
    pub fn in_transaction(&self) -> bool {
        self.in_transaction
    }
}

/// A refused connection to a proxy (§8.1 `pg.refused`).
#[derive(Debug, Clone, PartialEq)]
pub struct PgRefused {
    proxy: String,
    reason: PgRefuseReason,
}

impl PgRefused {
    /// A refusal record.
    pub fn new(proxy: impl Into<String>, reason: PgRefuseReason) -> Self {
        Self {
            proxy: proxy.into(),
            reason,
        }
    }

    /// The proxy name from the manifest.
    pub fn proxy(&self) -> &str {
        &self.proxy
    }

    /// Why the connection was refused.
    pub fn reason(&self) -> &PgRefuseReason {
        &self.reason
    }
}

/// A request refused before it reached the scheduler (§8.1 `http.rejected`).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRejected {
    remote_addr: String,
    status: u16,
    reason: RejectReason,
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

    /// The client address, as far as http can tell.
    pub fn remote_addr(&self) -> &str {
        &self.remote_addr
    }

    /// The status sent, for example `400`, `413`, `431` or `501`.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Why the request was refused.
    pub fn reason(&self) -> &RejectReason {
        &self.reason
    }
}

/// A view process was forked (§8.1 `process.spawned`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSpawned {
    pid: u32,
    uid: u32,
    fork_ms: f64,
}

impl ProcessSpawned {
    /// A spawn record.
    pub fn new(pid: u32, uid: u32, fork_ms: f64) -> Self {
        Self { pid, uid, fork_ms }
    }

    /// The new process.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The uid it runs under.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// How long the fork took, in milliseconds.
    pub fn fork_ms(&self) -> f64 {
        self.fork_ms
    }
}

/// A view process ended (§8.1 `process.exited`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessExited {
    pid: u32,
    uid: u32,
    exit: ExitKind,
}

impl ProcessExited {
    /// An exit record for `pid`, which ended with `exit`.
    pub fn new(pid: u32, uid: u32, exit: ExitKind) -> Self {
        Self { pid, uid, exit }
    }

    /// The process that ended.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The uid it ran under.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// How it ended: an exit code or a signal.
    pub fn exit(&self) -> ExitKind {
        self.exit
    }
}

/// A view process was retired on purpose (§8.1 `process.retired`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRetired {
    pid: u32,
    uid: u32,
    reason: RetireReason,
}

impl ProcessRetired {
    /// A retirement record.
    pub fn new(pid: u32, uid: u32, reason: RetireReason) -> Self {
        Self { pid, uid, reason }
    }

    /// The process that was retired.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The uid it ran under.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Why it was retired.
    pub fn reason(&self) -> &RetireReason {
        &self.reason
    }
}

/// The live view process cap in force, and the cost per view process it was
/// derived from (§8.1 `process.cap`).
///
/// The scheduler emits one of these when its measured cost per view, or the
/// cap that follows from it, changes — never once per request. §8 Sizing's
/// derived cap is a divisor away from the configured
/// `BLASTIT_VIEW_MEM_ESTIMATE`, and this event is where the divisor an
/// operator can actually measure appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCap {
    estimate_bytes: u64,
    cap: u32,
}

impl ProcessCap {
    /// A cap of `cap` view processes, from `estimate_bytes` per process.
    pub fn new(estimate_bytes: u64, cap: u32) -> Self {
        Self {
            estimate_bytes,
            cap,
        }
    }

    /// The cost of one view process the cap was derived from, in bytes.
    pub fn estimate_bytes(&self) -> u64 {
        self.estimate_bytes
    }

    /// The live view process cap in force.
    pub fn cap(&self) -> u32 {
        self.cap
    }
}

/// One line printed by a child process (§8.1 `app.output`).
///
/// init wraps every child line in one of these. A line that does not fit is
/// split by [`split_app_output`] into `parts` events, `part` being 1-based.
#[derive(Debug, Clone, PartialEq)]
pub struct AppOutput {
    stream: Stream,
    line: String,
    part: u32,
    parts: u32,
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

    /// Marks this event as piece `part` of `parts`.
    ///
    /// Returns [`PartOutOfRange`] unless `1 <= part <= parts`: a part number
    /// outside the set it belongs to would make a reader drop or duplicate
    /// app output, so it is rejected rather than clamped.
    pub fn with_part(self, part: u32, parts: u32) -> Result<Self, PartOutOfRange> {
        if part < 1 || part > parts {
            return Err(PartOutOfRange { part, parts });
        }
        Ok(Self {
            part,
            parts,
            ..self
        })
    }

    /// One measured piece of a line, built by [`split_app_output`] while it
    /// works out how many pieces there are.
    pub(crate) fn piece(stream: Stream, line: String, part: u32, parts: u32) -> Self {
        Self {
            stream,
            line,
            part,
            parts,
        }
    }

    /// Which stream the line came from.
    pub fn stream(&self) -> Stream {
        self.stream
    }

    /// The line, without its terminating newline.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Index of this piece, 1-based.
    pub fn part(&self) -> u32 {
        self.part
    }

    /// Number of pieces the original line was split into.
    pub fn parts(&self) -> u32 {
        self.parts
    }
}

/// Returned by [`AppOutput::with_part`] for a `part` outside `1..=parts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("part {part} is not within 1..={parts}")]
pub struct PartOutOfRange {
    part: u32,
    parts: u32,
}

/// Payload of the `boot`, `ready` and `shutdown` events (§8.1).
///
/// `outputs` is the caller's contract: the *names* of the environment
/// variables each proxy renders for the app, never their values. A value is
/// the secret the proxy exists to hide (§5), so it must not reach this type;
/// the names alone are what lets an operator line the log up with the
/// manifest.
///
/// The memory fields (`immortalize`, `immortalize_reason`, `immortalized`,
/// `immortalize_ms`, `mem_total`, `mem_used`, `max_view_processes`) are the
/// preload phase of §4 and §8 Sizing: whether the heap was immortalised and why
/// not when it was not, how much of it was (§9.2 #7), and how the view process
/// cap was derived from the machine's memory (§8). They are counts, byte
/// figures and interpreter names, never content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lifecycle {
    stage: Option<String>,
    routes: Option<u32>,
    workers: Option<u32>,
    duration_ms: Option<f64>,
    seccomp: Option<bool>,
    privilege: Option<String>,
    /// The build the `preload` `boot` event reports (`R23`).
    ///
    /// Only the `preload` `boot` carries it: the version of the binary that is
    /// about to serve. Every other lifecycle event leaves it `None`, so the line
    /// is the one place a log reader can learn which build a deployment runs.
    build: Option<String>,
    capabilities: Vec<String>,
    bounding_set: Option<bool>,
    immortalize: Option<bool>,
    immortalize_reason: Option<String>,
    immortalized: Option<u64>,
    immortalize_ms: Option<f64>,
    mem_total: Option<u64>,
    mem_used: Option<u64>,
    max_view_processes: Option<u32>,
    outputs: BTreeMap<String, Vec<String>>,
    preset: Option<String>,
    preset_settings: BTreeMap<String, String>,
    preset_overridden: Vec<String>,
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

    /// Whether the §7.3 seccomp filters are on.
    ///
    /// The `boot` event carries this: a booted server with the filters off
    /// looks exactly like one with them on from the outside, and the log is
    /// where an operator sees it.
    #[must_use]
    pub fn with_seccomp(mut self, seccomp: bool) -> Self {
        self.seccomp = Some(seccomp);
        self
    }

    /// Sets the privilege mode init itself runs in: `root` or `capabilities`
    /// (D6).
    ///
    /// A server started as an unprivileged user holding a few capabilities
    /// looks exactly like one started as root from the outside, and the two
    /// have different failure modes when a platform forgets a capability — so
    /// which one is in force belongs in the log, next to `seccomp` and for the
    /// same reason.
    #[must_use]
    pub fn with_privilege(mut self, privilege: impl Into<String>) -> Self {
        self.privilege = Some(privilege.into());
        self
    }

    /// Sets the build string the `preload` `boot` event reports (`R23`).
    ///
    /// `build` is blastit's own version (`env!("CARGO_PKG_VERSION")` in the
    /// crate that constructs the event), never a value from the environment.
    /// Only the `preload` `Boot` event is built with it; `stage0`, `config`,
    /// `ready` and `shutdown` leave it out, so a reader can point at exactly one
    /// line to learn which build is running.
    #[must_use]
    pub fn with_build(mut self, build: impl Into<String>) -> Self {
        self.build = Some(build.into());
        self
    }

    /// Records that the process holds the capability `name` (D6).
    ///
    /// A capability *name* (`cap_setuid`), never a value: this is a list of the
    /// fixed names the kernel defines.
    #[must_use]
    pub fn with_capability(mut self, name: impl Into<String>) -> Self {
        self.capabilities.push(name.into());
        self
    }

    /// Whether the children's **bounding** capability set is emptied (D6, M1).
    ///
    /// `false` means `cap_setpcap` was not held: every other set is still
    /// cleared on the way down, and `no_new_privs` still stops an `execve` from
    /// granting what the bounding set allows, so the boot runs and says so
    /// rather than refusing.
    #[must_use]
    pub fn with_bounding_set(mut self, dropped: bool) -> Self {
        self.bounding_set = Some(dropped);
        self
    }

    /// Sets whether §9.2 #7's pass ran at all, which the preload `boot` event
    /// reports as `on` or `off`.
    ///
    /// The pass is skipped on an interpreter where its marker is unsafe or
    /// unknown ([`Self::with_immortalize_reason`]), and a skipped pass looks
    /// exactly like a run that marked nothing from the outside.
    #[must_use]
    pub fn with_immortalize(mut self, immortalize: bool) -> Self {
        self.immortalize = Some(immortalize);
        self
    }

    /// Sets why §9.2 #7's pass was skipped (§9.2 #7).
    ///
    /// The reason names the interpreter or the setting that turned it off; it
    /// is a sentence built by blastit, never a value from the environment.
    #[must_use]
    pub fn with_immortalize_reason(mut self, reason: impl Into<String>) -> Self {
        self.immortalize_reason = Some(reason.into());
        self
    }

    /// Sets how many heap objects were made immortal (§9.2 #7).
    #[must_use]
    pub fn with_immortalized(mut self, objects: u64) -> Self {
        self.immortalized = Some(objects);
        self
    }

    /// Sets how long the immortalisation pass took, in milliseconds (§9.2 #7).
    #[must_use]
    pub fn with_immortalize_ms(mut self, immortalize_ms: f64) -> Self {
        self.immortalize_ms = Some(immortalize_ms);
        self
    }

    /// Sets the machine's total memory, in bytes (§8 Sizing).
    #[must_use]
    pub fn with_mem_total(mut self, mem_total: u64) -> Self {
        self.mem_total = Some(mem_total);
        self
    }

    /// Sets the memory in use when the cap was derived, in bytes (§8 Sizing).
    #[must_use]
    pub fn with_mem_used(mut self, mem_used: u64) -> Self {
        self.mem_used = Some(mem_used);
        self
    }

    /// Sets the live view process cap in force (§8 Sizing).
    #[must_use]
    pub fn with_max_view_processes(mut self, cap: u32) -> Self {
        self.max_view_processes = Some(cap);
        self
    }

    /// Records that proxy `proxy` will see an output variable called `name`.
    ///
    /// `name` is the variable *name*; passing the rendered value instead would
    /// put a secret on stdout.
    #[must_use]
    pub fn with_output(mut self, proxy: impl Into<String>, name: impl Into<String>) -> Self {
        self.outputs
            .entry(proxy.into())
            .or_default()
            .push(name.into());
        self
    }

    /// Names the configuration preset in force (E1).
    ///
    /// A `boot` event with no preset set says so in the encoder: the preset is
    /// `null`, not absent, so an operator never has to tell "no preset" from
    /// "this build does not know about presets".
    #[must_use]
    pub fn with_preset(mut self, preset: impl Into<String>) -> Self {
        self.preset = Some(preset.into());
        self
    }

    /// Records that the preset supplied the setting `name` (E1).
    ///
    /// `value` is the preset's own value — blastit's, not the operator's — and
    /// the *name* is the `BLASTIT_*` variable it would have been set by. The
    /// pair is what §8.1's "which values the boot decided" needs; the value an
    /// explicit variable replaced it with is never reported.
    #[must_use]
    pub fn with_preset_setting(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.preset_settings.insert(name.into(), value.into());
        self
    }

    /// Records that an explicit variable overrode what the preset supplied
    /// (E1).
    ///
    /// The *name* only: the value came from the environment, and §8.1 keeps
    /// environment values out of the log.
    #[must_use]
    pub fn with_preset_override(mut self, name: impl Into<String>) -> Self {
        self.preset_overridden.push(name.into());
        self
    }

    /// Named phase, for example `preload` or `drain`.
    pub fn stage(&self) -> Option<&str> {
        self.stage.as_deref()
    }

    /// Number of routes in the manifest.
    pub fn routes(&self) -> Option<u32> {
        self.routes
    }

    /// Size of the global worker budget.
    pub fn workers(&self) -> Option<u32> {
        self.workers
    }

    /// How long the phase took, in milliseconds.
    pub fn duration_ms(&self) -> Option<f64> {
        self.duration_ms
    }

    /// Whether the §7.3 seccomp filters are on, when the event says.
    pub fn seccomp(&self) -> Option<bool> {
        self.seccomp
    }

    /// `root` or `capabilities`: the mode init runs in, when the event says
    /// (D6).
    pub fn privilege(&self) -> Option<&str> {
        self.privilege.as_deref()
    }

    /// The build the `preload` `boot` event reports, when this is that event
    /// (`R23`).
    pub fn build(&self) -> Option<&str> {
        self.build.as_deref()
    }

    /// The capability names the process holds, `cap_...`; empty in root mode
    /// (D6).
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// Whether the children's bounding set is emptied, when the event says
    /// (D6, M1).
    pub fn bounding_set(&self) -> Option<bool> {
        self.bounding_set
    }

    /// Whether §9.2 #7's pass ran, when the event says.
    pub fn immortalize(&self) -> Option<bool> {
        self.immortalize
    }

    /// Why §9.2 #7's pass was skipped, when it was.
    pub fn immortalize_reason(&self) -> Option<&str> {
        self.immortalize_reason.as_deref()
    }

    /// How many heap objects were made immortal, when the phase did that.
    pub fn immortalized(&self) -> Option<u64> {
        self.immortalized
    }

    /// How long the immortalisation pass took, in milliseconds.
    pub fn immortalize_ms(&self) -> Option<f64> {
        self.immortalize_ms
    }

    /// The machine's total memory, in bytes.
    pub fn mem_total(&self) -> Option<u64> {
        self.mem_total
    }

    /// The memory in use when the view cap was derived, in bytes.
    pub fn mem_used(&self) -> Option<u64> {
        self.mem_used
    }

    /// The live view process cap in force, when it is known.
    pub fn max_view_processes(&self) -> Option<u32> {
        self.max_view_processes
    }

    /// Output variable names per proxy; never values.
    pub fn outputs(&self) -> &BTreeMap<String, Vec<String>> {
        &self.outputs
    }

    /// The preset in force, when one is (E1).
    pub fn preset(&self) -> Option<&str> {
        self.preset.as_deref()
    }

    /// The settings the preset supplied and the values it gave them: the
    /// `BLASTIT_*` variable name to the preset's own value (E1).
    ///
    /// Only the ones still in force: a setting an explicit variable overrode is
    /// in [`Self::preset_overridden`] instead.
    pub fn preset_settings(&self) -> &BTreeMap<String, String> {
        &self.preset_settings
    }

    /// The variables that overrode what the preset supplied, by name (E1).
    pub fn preset_overridden(&self) -> &[String] {
        &self.preset_overridden
    }
}

/// A problem found while validating the manifest (§8.1 `manifest.warning`).
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestWarning {
    message: String,
}

impl ManifestWarning {
    /// A warning record.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The warning text, for example an unattached group.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Which entrypoint a boot resolved, and therefore which fields its `runtime`
/// line carries (§8.1 `runtime`).
///
/// A closed set: the three words `docs/reference/log-events.md` freezes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeEntrypoint {
    /// `entrypoint php`: one embedded `libphp` front controller.
    Php,
    /// `entrypoint python`: the WSGI callable a view forwards to.
    Python,
    /// Static mounts and nothing else: no language runtime is loaded.
    Static,
}

impl RuntimeEntrypoint {
    /// The name used in the JSON stream, and the first word of the pretty line.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Python => "python",
            Self::Static => "static",
        }
    }
}

/// What an `entrypoint php` resolved to (§8.1 `runtime`).
///
/// `libphp` is the host library the workers `dlopen`, which is public
/// knowledge; the internal shim that carries the SAPI is never part of it.
/// `php_ini` is the file PHP actually opened — `None` when it opened none, so
/// a host running on PHP's built-in defaults says so instead of naming a path
/// nobody read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpRuntime {
    version: String,
    libphp: String,
    php_ini: Option<String>,
}

impl PhpRuntime {
    /// The PHP version the library reports, e.g. `"8.5.4"`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The resolved `libphp` the workers load.
    pub fn libphp(&self) -> &str {
        &self.libphp
    }

    /// The `php.ini` PHP opened, or `None` when it opened none.
    pub fn php_ini(&self) -> Option<&str> {
        self.php_ini.as_deref()
    }
}

/// What an `entrypoint python` resolved to (§8.1 `runtime`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonRuntime {
    version: String,
    environment: String,
}

impl PythonRuntime {
    /// The interpreter's version, e.g. `"3.14.0"`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The environment it serves from: `VIRTUAL_ENV` when set, else
    /// `sys.prefix`.
    pub fn environment(&self) -> &str {
        &self.environment
    }
}

/// What the entrypoint resolved to, at boot and after a change that alters it
/// (§8.1 `runtime`).
///
/// The variant *is* the field set: a row that does not list a field cannot
/// carry it, so a `python` line can never grow a `libphp` and a `static` line
/// can never carry a version. That is the frozen table expressed as a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runtime {
    /// `entrypoint php` resolved a library.
    Php(PhpRuntime),
    /// `entrypoint python` resolved an interpreter.
    Python(PythonRuntime),
    /// Static mounts only: no language runtime is loaded.
    Static,
}

impl Runtime {
    /// A resolved `entrypoint php`.
    pub fn php(
        version: impl Into<String>,
        libphp: impl Into<String>,
        php_ini: Option<String>,
    ) -> Self {
        Self::Php(PhpRuntime {
            version: version.into(),
            libphp: libphp.into(),
            php_ini,
        })
    }

    /// A resolved `entrypoint python`.
    pub fn python(version: impl Into<String>, environment: impl Into<String>) -> Self {
        Self::Python(PythonRuntime {
            version: version.into(),
            environment: environment.into(),
        })
    }

    /// Which entrypoint this line reports.
    pub fn entrypoint(&self) -> RuntimeEntrypoint {
        match self {
            Self::Php(_) => RuntimeEntrypoint::Php,
            Self::Python(_) => RuntimeEntrypoint::Python,
            Self::Static => RuntimeEntrypoint::Static,
        }
    }
}

/// What changed in `blastit dev`, and therefore why every view was reloaded
/// (§4.9 `dev.reload`).
///
/// A closed set: the four words `docs/reference/log-events.md` freezes. There
/// is deliberately no free-form arm — a reason this crate cannot name is not a
/// value it may invent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReloadReason {
    /// A file the app loaded changed.
    Loaded,
    /// A module the manifest's `preload` list names changed.
    Preload,
    /// `./Blastfile` changed.
    Blastfile,
    /// The kernel's event queue overflowed, so the changed set is unknown.
    Overflow,
}

impl ReloadReason {
    /// The name used in the JSON stream.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::Preload => "preload",
            Self::Blastfile => "blastfile",
            Self::Overflow => "overflow",
        }
    }
}

/// Every view was reloaded together in `blastit dev` (§4.9 `dev.reload`).
///
/// `routes` is every route serving after the reload, in manifest order, and
/// `files` the absolute paths that triggered it, sorted. Both arrays are cut
/// from the tail, whole entries only, when the encoded event would pass
/// [`MAX_EVENT_BYTES`]; the *transaction* is never bounded by the log, so every
/// route and file still takes part in the reload itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevReload {
    routes: Vec<String>,
    files: Vec<String>,
    reason: ReloadReason,
}

impl DevReload {
    /// A reload record.
    pub fn new(routes: Vec<String>, files: Vec<String>, reason: ReloadReason) -> Self {
        Self {
            routes,
            files,
            reason,
        }
    }

    /// Every route serving after the reload, in manifest order.
    pub fn routes(&self) -> &[String] {
        &self.routes
    }

    /// The absolute paths that triggered the reload, sorted.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// What changed.
    pub fn reason(&self) -> ReloadReason {
        self.reason
    }
}

/// `blastit dev` could not watch anything at boot, and serves without live
/// reload (§4.7 `dev.reload.unavailable`, §4.9).
///
/// `errno` is the symbolic name (`EPERM`); a value outside the failure ladder
/// is spelled `E<number>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevReloadUnavailable {
    errno: String,
    filesystem: String,
}

impl DevReloadUnavailable {
    /// An unavailable-watcher record.
    pub fn new(errno: impl Into<String>, filesystem: impl Into<String>) -> Self {
        Self {
            errno: errno.into(),
            filesystem: filesystem.into(),
        }
    }

    /// The symbolic errno, for example `EPERM`.
    pub fn errno(&self) -> &str {
        &self.errno
    }

    /// The filesystem type, for example `overlayfs`.
    pub fn filesystem(&self) -> &str {
        &self.filesystem
    }
}

/// `blastit dev`'s live-reload boot line as an event (§4.7 `reload`).
///
/// One per boot whose marking pass actually placed marks: `directories` is how
/// many directories it marked, and the human form spells `1 directory` in the
/// singular. It is an event so that it takes the same columns as every other
/// line and the JSON stream carries it too, rather than a free-text line beside
/// the stream that no reader could parse and no column lined up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadBoot {
    directories: u32,
}

impl ReloadBoot {
    /// The boot line's record, from the marks the pass placed.
    pub fn new(directories: u32) -> Self {
        Self { directories }
    }

    /// How many directories the marking pass placed.
    pub fn directories(&self) -> u32 {
        self.directories
    }
}

/// What an event is about; one variant per row of the §8.1 table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
    /// The live view process cap and the measured cost per view.
    ProcessCap(ProcessCap),
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
    /// What the entrypoint resolved to, at boot (§8.1 `runtime`).
    Runtime(Runtime),
    /// `blastit dev` reloaded every view together (`ADR-0014`).
    DevReload(DevReload),
    /// `blastit dev` could not watch anything at boot and serves without live
    /// reload (`ADR-0014`).
    DevReloadUnavailable(DevReloadUnavailable),
    /// `blastit dev` started live reload and marked this many directories
    /// (§4.7 `reload`).
    Reload(ReloadBoot),
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
            Self::ProcessCap(_) => "process.cap",
            Self::AppOutput(_) => "app.output",
            Self::Boot(_) => "boot",
            Self::Ready(_) => "ready",
            Self::Shutdown(_) => "shutdown",
            Self::ManifestWarning(_) => "manifest.warning",
            Self::Runtime(_) => "runtime",
            Self::DevReload(_) => "dev.reload",
            Self::DevReloadUnavailable(_) => "dev.reload.unavailable",
            Self::Reload(_) => "reload",
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
            Self::SqlDenied(_)
            | Self::PgRefused(_)
            | Self::ManifestWarning(_)
            | Self::DevReloadUnavailable(_) => Level::Warn,
            Self::AppOutput(output) => match output.stream {
                Stream::Stdout => Level::Info,
                Stream::Stderr => Level::Warn,
            },
            Self::ProcessExited(info) => {
                if info.exit.is_crash() {
                    Level::Warn
                } else {
                    Level::Info
                }
            }
            Self::HttpRejected(_)
            | Self::ProcessSpawned(_)
            | Self::ProcessRetired(_)
            | Self::ProcessCap(_)
            | Self::Boot(_)
            | Self::Ready(_)
            | Self::Shutdown(_)
            | Self::Runtime(_)
            | Self::DevReload(_)
            | Self::Reload(_) => Level::Info,
        }
    }
}

/// One event: the fields common to every event, plus its kind.
///
/// Built with [`Event::at`], [`Event::now`] or [`Event::new`] and the `with_*`
/// builders.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    ts: OffsetDateTime,
    level: Level,
    component: Component,
    route: Option<String>,
    request_id: Option<String>,
    kind: EventKind,
    truncated: Vec<&'static str>,
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
            truncated: Vec::new(),
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

    /// When the event happened, in UTC.
    pub fn ts(&self) -> OffsetDateTime {
        self.ts
    }

    /// Severity, normally [`EventKind::default_level`].
    pub fn level(&self) -> Level {
        self.level
    }

    /// The component that emitted the event.
    pub fn component(&self) -> Component {
        self.component
    }

    /// The route the event belongs to, when it applies.
    pub fn route(&self) -> Option<&str> {
        self.route.as_deref()
    }

    /// The request id, when it applies. dbproxy never has one (§8.1).
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// What the event is about.
    pub fn kind(&self) -> &EventKind {
        &self.kind
    }

    /// Fields the caller already shortened before building this event.
    ///
    /// The JSON encoder copies these into `truncated`; it never adds them
    /// itself. [`split_app_output`] uses this to report a shared `route` or
    /// `request_id` that it cut once instead of carrying a copy in every part.
    pub fn truncated_fields(&self) -> &[&'static str] {
        &self.truncated
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

    /// Declares fields this event already had shortened, using their JSON key
    /// names. They are reported in `truncated` even when the event fits.
    #[must_use]
    pub fn with_truncated<I: IntoIterator<Item = &'static str>>(mut self, fields: I) -> Self {
        for field in fields {
            if !self.truncated.contains(&field) {
                self.truncated.push(field);
            }
        }
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
