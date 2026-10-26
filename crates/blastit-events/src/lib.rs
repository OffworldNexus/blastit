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
    capabilities: Vec<String>,
    immortalize: Option<bool>,
    immortalize_reason: Option<String>,
    immortalized: Option<u64>,
    immortalize_ms: Option<f64>,
    mem_total: Option<u64>,
    mem_used: Option<u64>,
    max_view_processes: Option<u32>,
    outputs: BTreeMap<String, Vec<String>>,
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

    /// Records that the process holds the capability `name` (D6).
    ///
    /// A capability *name* (`cap_setuid`), never a value: this is a list of the
    /// fixed names the kernel defines.
    #[must_use]
    pub fn with_capability(mut self, name: impl Into<String>) -> Self {
        self.capabilities.push(name.into());
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

    /// The capability names the process holds, `cap_...`; empty in root mode
    /// (D6).
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
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
            | Self::Shutdown(_) => Level::Info,
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
                EventKind::ProcessExited(ProcessExited::new(57, 20001, ExitKind::Status(0))),
                Level::Info,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001, ExitKind::Status(1))),
                Level::Warn,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001, ExitKind::Status(-1))),
                Level::Warn,
            ),
            (
                EventKind::ProcessExited(ProcessExited::new(57, 20001, ExitKind::Signal(9))),
                Level::Warn,
            ),
            (
                EventKind::ProcessRetired(ProcessRetired::new(57, 20001, RetireReason::Idle)),
                Level::Info,
            ),
            (
                EventKind::ProcessCap(ProcessCap::new(135 * 1024 * 1024, 4)),
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
            Event::new(Component::Scheduler, kind.clone()).level(),
            Level::Warn
        );
        assert_eq!(
            Event::now(Component::Scheduler, kind.clone()).level(),
            Level::Warn
        );
        let ts = OffsetDateTime::from_unix_timestamp(0).unwrap();
        let event = Event::at(ts, Component::Scheduler, kind);
        assert_eq!(event.ts(), ts);
        assert_eq!(event.level(), Level::Warn);
        assert_eq!(event.with_level(Level::Debug).level(), Level::Debug);
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
            "process.cap",
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
        let error = "nope".parse::<Level>().unwrap_err();
        assert_eq!(error.to_string(), "unknown log level `nope`");
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
    fn exit_kind_names_and_crashes() {
        assert_eq!(ExitKind::Status(0).field_name(), "status");
        assert_eq!(ExitKind::Signal(9).field_name(), "signal");
        assert_eq!(ExitKind::Status(3).code(), 3);
        assert_eq!(ExitKind::Signal(9).code(), 9);
        assert!(!ExitKind::Status(0).is_crash());
        assert!(ExitKind::Status(1).is_crash());
        assert!(ExitKind::Signal(15).is_crash());
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
        assert_eq!(info.method(), "GET");
        assert_eq!(info.path(), "/products/42/");
        assert_eq!(info.status(), 0);
        assert_eq!(info.outcome(), Outcome::Ok);
        assert_eq!(info.rule(), None);
    }

    /// The `path` has no setter that keeps a query string: every route into
    /// the field goes through `strip_query`.
    #[test]
    fn no_setter_can_put_a_query_string_in_the_path() {
        let cases = [
            "/products/42/?token=secret",
            "/?",
            "/a?x=1?y=2",
            "/a;b?c=d#e",
            "?only=query",
        ];
        for target in cases {
            let from_target = RequestInfo::from_target("GET", target);
            assert!(
                !from_target.path().contains('?'),
                "from_target kept a query string: {}",
                from_target.path()
            );
            assert_eq!(from_target.path(), strip_query(target));

            let with_path = RequestInfo::from_target("GET", "/")
                .with_path(target)
                .with_status(200);
            assert!(
                !with_path.path().contains('?'),
                "with_path kept a query string: {}",
                with_path.path()
            );
            assert_eq!(with_path.path(), strip_query(target));
        }

        // The builders that take other fields cannot touch `path`.
        let built = RequestInfo::from_target("GET", "/a?x=1")
            .with_rule(0)
            .with_status(200)
            .with_outcome(Outcome::Ok)
            .with_timings(0.1, 1.0)
            .with_bytes(1, 2)
            .with_remote_addr("203.0.113.7?fake=1");
        assert_eq!(built.path(), "/a");
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
        assert_eq!(info.rule(), Some(2));
        assert_eq!(info.status(), 201);
        assert_eq!(info.queued_ms(), 1.5);
        assert_eq!(info.duration_ms(), 20.0);
        assert_eq!((info.bytes_in(), info.bytes_out()), (10, 20));
        assert_eq!(info.remote_addr(), "198.51.100.9");

        let lifecycle = Lifecycle::default()
            .with_stage("preload")
            .with_routes(3)
            .with_workers(16)
            .with_duration_ms(42.5)
            .with_immortalize(true)
            .with_immortalized(9)
            .with_immortalize_ms(1.5)
            .with_mem_total(1024)
            .with_mem_used(256)
            .with_max_view_processes(22)
            .with_output("db", "DATABASE_URL")
            .with_output("db", "PGHOST");
        assert_eq!(lifecycle.stage(), Some("preload"));
        assert_eq!(lifecycle.routes(), Some(3));
        assert_eq!(lifecycle.workers(), Some(16));
        assert_eq!(lifecycle.duration_ms(), Some(42.5));
        assert_eq!(lifecycle.immortalize(), Some(true));
        assert_eq!(lifecycle.immortalized(), Some(9));
        assert_eq!(lifecycle.immortalize_ms(), Some(1.5));
        assert_eq!(lifecycle.mem_total(), Some(1024));
        assert_eq!(lifecycle.mem_used(), Some(256));
        assert_eq!(lifecycle.max_view_processes(), Some(22));
        assert_eq!(
            lifecycle.outputs()["db"],
            vec!["DATABASE_URL".to_owned(), "PGHOST".to_owned()]
        );

        let output = AppOutput::new(Stream::Stderr, "boom")
            .with_part(2, 3)
            .unwrap();
        assert_eq!((output.part(), output.parts()), (2, 3));
        assert_eq!(ManifestWarning::new("x").message(), "x");
        assert_eq!(ProcessSpawned::new(1, 2, 3.0).uid(), 2);
        assert_eq!(
            ProcessExited::new(1, 2, ExitKind::Signal(9)).exit(),
            ExitKind::Signal(9)
        );
        assert_eq!(
            ProcessRetired::new(1, 2, RetireReason::Idle).reason(),
            &RetireReason::Idle
        );
        assert_eq!(
            HttpRejected::new("a", 413, RejectReason::BodyTooLarge).status(),
            413
        );
    }

    #[test]
    fn getters_expose_every_field() {
        let request = RequestInfo::from_target("GET", "/a?b=1")
            .with_rule(1)
            .with_status(200)
            .with_outcome(Outcome::Ok)
            .with_timings(0.5, 1.5)
            .with_bytes(3, 4)
            .with_remote_addr("203.0.113.7");
        assert_eq!(request.method(), "GET");
        assert_eq!(request.path(), "/a");
        assert_eq!(request.rule(), Some(1));
        assert_eq!(request.status(), 200);
        assert_eq!(request.outcome(), Outcome::Ok);
        assert_eq!(request.queued_ms(), 0.5);
        assert_eq!(request.duration_ms(), 1.5);
        assert_eq!(request.bytes_in(), 3);
        assert_eq!(request.bytes_out(), 4);
        assert_eq!(request.remote_addr(), "203.0.113.7");

        let denied = SqlDenied::new(7, "db", SqlKind::Copy, "COPY x", "42501", true);
        assert_eq!(denied.pid(), 7);
        assert_eq!(denied.proxy(), "db");
        assert_eq!(denied.kind(), &SqlKind::Copy);
        assert_eq!(denied.sql(), "COPY x");
        assert_eq!(denied.sqlstate(), "42501");
        assert!(denied.in_transaction());

        let refused = PgRefused::new("db", PgRefuseReason::TooManyConnections);
        assert_eq!(refused.proxy(), "db");
        assert_eq!(refused.reason(), &PgRefuseReason::TooManyConnections);

        let rejected = HttpRejected::new("198.51.100.1", 431, RejectReason::HeaderTooLarge);
        assert_eq!(rejected.remote_addr(), "198.51.100.1");
        assert_eq!(rejected.status(), 431);
        assert_eq!(rejected.reason(), &RejectReason::HeaderTooLarge);

        let spawned = ProcessSpawned::new(11, 20_001, 12.5);
        assert_eq!(
            (spawned.pid(), spawned.uid(), spawned.fork_ms()),
            (11, 20_001, 12.5)
        );

        let exited = ProcessExited::new(11, 20_001, ExitKind::Signal(15));
        assert_eq!(
            (exited.pid(), exited.uid(), exited.exit()),
            (11, 20_001, ExitKind::Signal(15))
        );

        let retired = ProcessRetired::new(11, 20_001, RetireReason::LruCap);
        assert_eq!(retired.pid(), 11);
        assert_eq!(retired.uid(), 20_001);
        assert_eq!(retired.reason(), &RetireReason::LruCap);

        // §8 Sizing: the cap event carries the divisor it was derived from.
        let cap = ProcessCap::new(141_557_760, 4);
        assert_eq!(cap.estimate_bytes(), 141_557_760);
        assert_eq!(cap.cap(), 4);
        assert_eq!(cap, ProcessCap::new(141_557_760, 4));

        let output = AppOutput::new(Stream::Stdout, "hi");
        assert_eq!(output.stream(), Stream::Stdout);
        assert_eq!(output.line(), "hi");
        assert_eq!((output.part(), output.parts()), (1, 1));

        let warning = ManifestWarning::new("unattached group");
        assert_eq!(warning.message(), "unattached group");

        // §5: an output carries variable *names*, never values.
        let lifecycle = Lifecycle::default()
            .with_stage("preload")
            .with_routes(1)
            .with_workers(2)
            .with_duration_ms(3.0)
            .with_output("db", "DATABASE_URL");
        assert_eq!(lifecycle.stage(), Some("preload"));
        assert_eq!(lifecycle.routes(), Some(1));
        assert_eq!(lifecycle.workers(), Some(2));
        assert_eq!(lifecycle.duration_ms(), Some(3.0));
        assert_eq!(lifecycle.outputs()["db"], vec!["DATABASE_URL".to_owned()]);
        assert_eq!(Lifecycle::default().stage(), None);
        assert_eq!(Lifecycle::default().routes(), None);
        assert_eq!(Lifecycle::default().workers(), None);
        assert_eq!(Lifecycle::default().duration_ms(), None);
        assert_eq!(Lifecycle::default().immortalize(), None);
        assert_eq!(Lifecycle::default().immortalize_reason(), None);
        assert_eq!(Lifecycle::default().immortalized(), None);
        assert_eq!(Lifecycle::default().immortalize_ms(), None);
        assert_eq!(Lifecycle::default().mem_total(), None);
        assert_eq!(Lifecycle::default().mem_used(), None);
        assert_eq!(Lifecycle::default().max_view_processes(), None);
        assert!(Lifecycle::default().outputs().is_empty());
    }

    #[test]
    fn with_part_rejects_parts_outside_the_set() {
        let whole = AppOutput::new(Stream::Stdout, "hi");
        assert!(whole.clone().with_part(1, 1).is_ok());
        assert!(whole.clone().with_part(1, 3).is_ok());
        assert!(whole.clone().with_part(3, 3).is_ok());
        let error = whole.clone().with_part(0, 3).unwrap_err();
        assert_eq!(error.to_string(), "part 0 is not within 1..=3");
        assert!(whole.clone().with_part(4, 3).is_err());
        assert!(whole.clone().with_part(0, 0).is_err());
        assert!(whole.with_part(u32::MAX, u32::MAX).is_ok());
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
        assert_eq!(event.route(), Some("product-detail"));
        assert_eq!(event.request_id(), Some("01JB9"));
        assert_eq!(event.component(), Component::Init);
        assert_eq!(event.kind().name(), "ready");
        assert!(event.truncated_fields().is_empty());
        let later = event
            .clone()
            .with_ts(OffsetDateTime::from_unix_timestamp(1).unwrap());
        assert!(later.ts() > event.ts());
        assert_eq!(later.route(), event.route());
    }

    #[test]
    fn declared_truncated_fields_are_deduplicated_and_ordered() {
        let event = Event::at(
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
            Component::Init,
            EventKind::Ready(Lifecycle::default()),
        )
        .with_truncated(["route"])
        .with_truncated(["route", "request_id"]);
        assert_eq!(event.truncated_fields().to_vec(), ["route", "request_id"]);
    }

    #[test]
    fn event_defaults_have_no_route_or_request_id() {
        let event = Event::now(Component::Init, EventKind::Shutdown(Lifecycle::default()));
        assert_eq!(event.route(), None);
        assert_eq!(event.request_id(), None);
    }
}
