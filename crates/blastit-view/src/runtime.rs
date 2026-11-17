//! The worker side of the runtime seam (design `docs/design/backend-runtimes.md`
//! §4.1, `ADR-0009`).
//!
//! A route worker process runs exactly one [`WorkerRuntime`]: the thing that
//! executes one route's application code, whatever language that is, on the
//! `view.chan` transport. On the scheduler's side of the same seam,
//! `blastit_backend::Upstream` (`ADR-0009`)
//! decides *when* to dispatch and how to charge the result; a
//! [`WorkerRuntime`] only answers a request once it has been dispatched.
//!
//! The two sides are deliberately independent types, in crates whose edges
//! (§4.2) do not meet: `blastit-view` depends on `blastit-frame` and nothing
//! that parses HTTP or emits events, and the scheduler holds framed sessions,
//! never a trait object of this one. What they share is the vocabulary —
//! [`blastit_frame::ReqHead`], [`RespHead`] and [`DoneOutcome`] are the wire's
//! own types, so a runtime cannot invent a second, subtly different request
//! shape that dispatch would then have to translate (the mismatch §4.1's review
//! closed).
//!
//! # What a runtime may be
//!
//! Threaded CPython over PyO3, NTS `libphp` behind a shim, and any future
//! in-process runtime are all implementations of [`WorkerRuntime`]. That is why
//! the trait's surface names **no language and no socket**: an interpreter
//! handle, a `libphp` pointer or a socket type in these signatures would leak
//! one implementation's private business into the contract every other
//! implementation has to satisfy. A runtime reaches the client through
//! [`RequestIo`], whose methods are the `view.chan` framing — the `wire`
//! transport and `flow` credit windows of `ARCHITECTURE.md` §2.5 and §3.1 —
//! and through nothing else.
//!
//! # One request, start to finish
//!
//! The transport's request thread builds a [`RequestIo`] for one `REQ_HEAD`,
//! calls [`WorkerRuntime::serve`], and then writes the frames that close the
//! request from the [`RequestEnd`] it gets back:
//!
//! ```text
//! serve(req, io)
//!   ├─ io.head()                    what the scheduler sent
//!   ├─ io.read_body(..)             paced by the request-body window
//!   ├─ io.write_head(RespHead)      at most once
//!   ├─ io.write_body(..)            paced by the response window
//!   ├─ io.output(..)                the runtime's stdout/stderr, never the response
//!   └─ io.flush_response()          everything written is on the wire
//! returns RequestEnd { outcome, cpu_ns }
//!   └─ the caller sends RESP_END (+ RESP_END_COST) and DONE
//! ```
//!
//! The runtime never sends `DONE` itself: it does not know the request's id on
//! the channel, and the frame must still be sent when a request panics
//! (`SlotGuard` in the crate root owns that). The caller which can see both is
//! the one that closes the request.

use std::io;

use blastit_frame::{DoneOutcome, ReqHead, RespHead};

#[cfg(feature = "python")]
use std::os::unix::net::UnixDatagram;
#[cfg(feature = "python")]
use std::path::Path;
#[cfg(feature = "python")]
use std::sync::Mutex;

#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::PyModule;

#[cfg(feature = "python")]
use crate::loaded::Reporter;

/// Which of the worker's two output streams a line came from.
///
/// `wsgi.errors`, and PHP's own `STDOUT`/`STDERR` outside the response, are
/// diagnostics: they are never part of the client's response, and a runtime may
/// interleave them with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    /// The runtime's standard output.
    Stdout,
    /// The runtime's standard error: tracebacks, Zend notices, `wsgi.errors`.
    Stderr,
}

/// The most bytes of one output line that [`RequestIo::output`] keeps.
///
/// An application can print a megabyte on one line; the runtime hands it over
/// in any number of calls, and the sink keeps this prefix and drops the rest
/// rather than let one `echo` bury the log it lands in.
pub const OUTPUT_LINE_CAP: usize = 2048;

/// What a worker runtime is, from the transport's point of view.
///
/// One implementation per language and linkage: threaded CPython
/// (`WsgiRuntime`), NTS `libphp` (`PhpEmbedRuntime`), and whatever a later
/// backend embeds. The scheduler-side `Upstream` implemented by `MB.9.3`
/// consumes this trait and serves both `Executor::View` and `Executor::Php`
/// without branching on [`kind`](WorkerRuntime::kind) in request dispatch; the
/// two provider kinds differ in ownership, `SeccompRole` and `max_inflight`,
/// not in the request path. Wiring the scheduler crate to `blastit-view` is
/// `MB.9.3`'s decision, not this node's.
///
/// # Implementing it from another crate
///
/// The trait is the seam, not a private convenience: an implementation written
/// outside this crate has everything it needs here, and this example is
/// compiled and run as its own crate by `cargo test`:
///
/// ```
/// use std::io;
///
/// use blastit_frame::{DoneOutcome, ReqHead, RespHead};
/// use blastit_view::{OutputStream, RequestEnd, RequestIo, WorkerRuntime};
///
/// /// A runtime that echoes the request body back, without an interpreter.
/// struct Echo;
///
/// impl WorkerRuntime for Echo {
///     fn kind(&self) -> &'static str {
///         "echo"
///     }
///
///     fn max_inflight(&self) -> u32 {
///         1
///     }
///
///     fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
///         let mut body = Vec::new();
///         let mut buf = [0u8; 1024];
///         loop {
///             match io.read_body(&mut buf) {
///                 Ok(0) | Err(_) => break,
///                 Ok(n) => body.extend_from_slice(&buf[..n]),
///             }
///         }
///         let _ = io.write_head(RespHead::default());
///         let _ = io.write_body(&body);
///         let _ = io.flush_response();
///         let _ = &req;
///         RequestEnd::new(DoneOutcome::Completed, None)
///     }
///
///     fn recycle(&self) -> bool {
///         false
///     }
/// }
///
/// /// What the transport gives one request: here, a recording stand-in for
/// /// `view.chan`.
/// struct Record {
///     head: ReqHead,
/// }
///
/// impl RequestIo for Record {
///     fn head(&self) -> &ReqHead {
///         &self.head
///     }
///
///     fn read_body(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
///         Ok(0)
///     }
///
///     fn write_head(&mut self, _head: RespHead) -> io::Result<()> {
///         Ok(())
///     }
///
///     fn write_body(&mut self, _data: &[u8]) -> io::Result<()> {
///         Ok(())
///     }
///
///     fn output(&mut self, _stream: OutputStream, _line: &str) {}
///
///     fn flush_response(&mut self) -> io::Result<()> {
///         Ok(())
///     }
/// }
///
/// fn main() {
///     // Dispatch holds the implementation as a trait object, never the type.
///     let runtime: Box<dyn WorkerRuntime + Send> = Box::new(Echo);
///     let mut io = Record {
///         head: ReqHead::default(),
///     };
///
///     let end = runtime.serve(ReqHead::default(), &mut io);
///     assert_eq!(end.outcome, DoneOutcome::Completed);
///     assert!(!runtime.recycle());
/// }
/// ```
pub trait WorkerRuntime: Send + Sync + 'static {
    /// `"wsgi"` or `"php"` — for diagnostics and conformance keys, and never
    /// for per-request dispatch.
    ///
    /// The two provider kinds answer the same request path; a scheduler that
    /// matched on this string would have to grow a branch per language, which
    /// is exactly the coupling `ADR-0009`'s dual seam removes.
    fn kind(&self) -> &'static str;

    /// How many requests this worker process runs at once.
    ///
    /// The per-route concurrency the scheduler admits, not a thread count:
    /// threaded WSGI gets `view_worker_cap`, an NTS PHP process gets `1`
    /// because it can run one request per OS process. This is a ceiling, not an
    /// admission queue: the scheduler's `provider_ceiling` must cover
    /// `route_allocation`, and this value is the one each `ViewSet` charges
    /// `inflight` against (`provider_ceiling >= route_allocation`, §4 R4).
    fn max_inflight(&self) -> u32;

    /// Run one dispatched request to completion.
    ///
    /// Blocking, on the transport's request thread, with `&self`: a WSGI
    /// runtime attaches the interpreter for the call, and an NTS PHP runtime
    /// takes its process-wide handle behind a lock. Both are synchronous by
    /// construction — `libphp` is not re-entrant and CPython's GIL is not a
    /// future — so the contract is a call, not an `async fn`. An async caller
    /// wraps it in `spawn_blocking`; the `Send + Sync + 'static` bounds and
    /// [`RequestEnd`] being `Send` are what make that task movable between
    /// threads.
    ///
    /// The response is written through `io` before this returns. The caller —
    /// which knows the request's id on the channel — writes `RESP_END`,
    /// `RESP_END_COST` and `DONE` from the [`RequestEnd`].
    ///
    /// # Errors
    ///
    /// Nothing here is a `Result`: a broken channel is the [`RequestIo`]'s to
    /// report on each call, and a request that fails is still a request whose
    /// outcome the scheduler has to hear about. A transport error the runtime
    /// sees while writing is folded into the returned
    /// [`outcome`](RequestEnd::outcome).
    fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd;

    /// Whether the worker must exit now instead of taking another request.
    ///
    /// Set by `max_requests` being reached, by a fatal Zend bailout that left
    /// the interpreter unusable, or by anything else the runtime knows it
    /// cannot come back from. The transport retires the process after
    /// [`serve`](WorkerRuntime::serve) returns; the scheduler respawns lazily.
    fn recycle(&self) -> bool;
}

/// How one dispatched request ended.
///
/// The same two facts `RESP_END`/`RESP_END_COST` and `DONE` carry: how the
/// request ended — completed, abandoned, errored, or bailed out of a handler
/// that never returned (`DoneOutcome::Bailed`, `MB.3.41`) — and what the
/// request cost in CPU time. The wall time on the wire is the transport's
/// business, and the scheduler ignores it (`R18`), so it is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEnd {
    /// What to report in `DONE`, which is what frees the scheduler's slot.
    pub outcome: DoneOutcome,
    /// The request's CPU time, or `None` when the runtime could not measure
    /// it.
    ///
    /// Absent is not zero: a measurement of zero is a claim about the request,
    /// and a runtime with no readable clock (`CLOCK_THREAD_CPUTIME_ID` for
    /// WSGI, `CLOCK_PROCESS_CPUTIME_ID` for embedded PHP, §4.3) makes no claim
    /// at all. The scheduler charges a `None` as its kernel floor, never as
    /// zero (`ADR-0009`).
    pub cpu_ns: Option<u64>,
}

impl RequestEnd {
    /// A request that ended with `outcome` and cost `cpu_ns`.
    #[must_use]
    pub const fn new(outcome: DoneOutcome, cpu_ns: Option<u64>) -> Self {
        Self { outcome, cpu_ns }
    }
}

/// The transport, as one request sees it: the `view.chan` framing and nothing
/// underneath it.
///
/// Implemented by the transport that owns the channel; used by
/// [`WorkerRuntime::serve`]. Every method is a frame or a flow-control
/// operation on `view.chan` — `REQ_BODY`/`WINDOW` for the body, `RESP_HEAD`,
/// `RESP_BODY` and the window for the response — so a runtime never holds a
/// socket, a file descriptor or a scheduler handle, and cannot write to another
/// request's stream.
///
/// The methods are `&mut self` because a request owns its stream for its
/// lifetime; the implementation is free to share the process-wide writer behind
/// it, as `view.chan` requires one writer per frame.
pub trait RequestIo {
    /// The head the scheduler dispatched.
    ///
    /// The same value `serve` was given; it is here for code that builds a
    /// request environment from the transport's view of the request.
    fn head(&self) -> &ReqHead;

    /// Read request-body bytes.
    ///
    /// Returns the number of bytes read, or `Ok(0)` when the body has ended.
    /// Paced by the request-body credit window (§3.1): a runtime that reads
    /// slowly is granted more credit slowly, so a slow application slows its
    /// own client and no one else. Consuming bytes is what returns the credit.
    ///
    /// # Errors
    ///
    /// The channel could not be read, the stream was cancelled, or the body
    /// exceeded its window.
    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Send the response head.
    ///
    /// At most once per request: the head must precede the body, and a second
    /// head is a protocol error the transport refuses.
    ///
    /// # Errors
    ///
    /// The frame could not be encoded or written, or the stream was cancelled.
    fn write_head(&mut self, head: RespHead) -> io::Result<()>;

    /// Write response-body bytes.
    ///
    /// Paced by the response credit window (§3.1): a runtime that has spent its
    /// credit blocks here until the scheduler grants more, as it would on a
    /// slow client's socket, rather than buffering without bound. A cancelled
    /// stream wakes the write and reports it as an error.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn write_body(&mut self, data: &[u8]) -> io::Result<()>;

    /// Record one line of the runtime's own output.
    ///
    /// Not the response: this is the runtime's `STDOUT`/`STDERR` —
    /// `wsgi.errors`, PHP's output outside the response — where a traceback or
    /// a Zend notice goes. Implementations keep at most [`OUTPUT_LINE_CAP`]
    /// bytes of each line, so one huge `print` cannot bury the log it lands in,
    /// and they never fail the request for an output error: losing a diagnostic
    /// line is better than failing a response that was otherwise fine.
    fn output(&mut self, stream: OutputStream, line: &str);

    /// Push everything written so far onto the channel.
    ///
    /// Called once, at the end of [`WorkerRuntime::serve`], so the response a
    /// runtime reports is on the wire before the caller closes the request with
    /// `RESP_END`/`DONE`.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn flush_response(&mut self) -> io::Result<()>;
}

/// The private module that holds one dev interpreter's loaded-file state.
///
/// It lives in `sys.modules` so a forked view can find it without a global in
/// this crate and without an environment variable (stage 0 strips every
/// `BLASTIT_*` from a view, and a view must not see one anyway). The name is
/// private to blastit and is not a package an application imports.
#[cfg(feature = "python")]
pub const LOADED_MODULE: &str = "_blastit_loaded";

/// The whole Python side of loaded-file tracking: the `sys.modules` closure and
/// the `open`-only audit hook (`docs/design/dev-fanotify-reload.md` §4.4,
/// research §2.1).
///
/// Every resolver returns an empty tuple instead of failing, and [`arm`] takes
/// the app root and the working directory already resolved in Rust, so nothing
/// here needs an environment variable or a config file. `_record_open` is one
/// function call from the hook body for a reason: the guarantee "an exception
/// inside the hook never aborts the application's own `open`" is only provable
/// if the body can be replaced by something that raises, and the busy flag's
/// recursion guard is only provable by replacing it with something that opens a
/// file.
///
/// What it deliberately does **not** do: no `abspath`, no `realpath`, no
/// `stat`, no I/O and no import inside the hook. The only work in the hook is a
/// suffix test, a prefix test and a set insertion — the research measured that
/// at 0.24-0.7 µs, which disappears inside the `open()` it observes.
#[cfg(feature = "python")]
const LOADED_TRACKER: &str = r#"
"""Dev-only loaded-file tracking: the module closure plus the `open` hook."""

import importlib.util
import os
import sys
import sysconfig
import threading

_SOURCE_SUFFIXES = (
    ".py", ".pyi", ".pyc", ".pyo", ".html", ".htm", ".jinja", ".jinja2", ".j2",
    ".json", ".yaml", ".yml", ".toml", ".ini", ".cfg", ".conf", ".env", ".txt",
    ".md", ".css", ".js", ".sql",
)
_SKIP = ("/.git/", "/node_modules/")
_CACHE = "/__pycache__/"
_MAX_PATHS = 8192
_MAX_BYTES = 4 * 1024 * 1024

_root = None
_cwd = None
_stdlib = None
_resolved = {}
_seen = set()
_reported = set()
_pending = set()
_busy = threading.local()
_count = 0
_bytes = 0
_over = False


def arm(root, cwd):
    """Fix the app root (realpath, trailing separator) and the cwd, once."""
    global _root, _cwd, _stdlib
    _root = root
    _cwd = cwd
    try:
        _stdlib = sysconfig.get_paths().get("stdlib")
    except BaseException:
        _stdlib = None


def _source_of(cached):
    """The `.py` a `.pyc` was compiled from, or None (no existence check)."""
    try:
        return importlib.util.source_from_cache(cached)
    except BaseException:
        return None


def _wanted(path, cached=False):
    """Whether a path is watch-worthy; `cached` admits a bytecode path.

    The hook admits a `.pyc`/`.pyo` under `__pycache__` so `flush` can map it
    to its source; a path that is still under `__pycache__` after that mapping
    is dropped there, which is why the cache check is not in the hook.
    """
    if _stdlib is not None:
        if path == _stdlib or path.startswith(_stdlib + "/"):
            return False
    for part in _SKIP:
        if part in path:
            return False
    if not cached and _CACHE in path:
        return False
    return True


def _module_paths(module):
    """The files one module was loaded from, in the research's measured order.

    `__spec__.origin` is authoritative; `__file__` is only consulted for a
    module with no spec at all (`__main__` from a script). A frozen module
    reports nothing and never falls back to `__file__`: `os.__file__` is a real
    `<stdlib>/os.py` even though nothing was loaded from it.
    """
    try:
        spec = getattr(module, "__spec__", None)
        if spec is None:
            path = getattr(module, "__file__", None)
            if isinstance(path, str) and path:
                return (os.path.realpath(path),)
            return ()
        origin = getattr(spec, "origin", None)
        if origin in ("built-in", "frozen"):
            return ()
        locations = getattr(spec, "submodule_search_locations", None)
        if origin is None:
            if locations:
                return tuple(
                    os.path.realpath(location)
                    for location in locations
                    if isinstance(location, str) and location
                )
            return ()
        if not isinstance(origin, str):
            return ()
        loader = getattr(spec, "loader", None)
        archive = getattr(loader, "archive", None)
        if isinstance(archive, str) and archive:
            return (os.path.realpath(archive),)
        if origin.endswith((".pyc", ".pyo")):
            source = _source_of(origin)
            if source is not None and os.path.isfile(source):
                return (os.path.realpath(source),)
            return (os.path.realpath(origin),)
        path = os.path.realpath(origin)
        if os.path.isfile(path):
            return (path,)
        return ()
    except BaseException:
        return ()


def _add(path):
    """Queue one absolute path, inside the view's path and byte budget."""
    global _count, _bytes, _over
    if not path or not path.startswith("/"):
        return
    if path in _reported or path in _pending or not _wanted(path, cached=True):
        return
    try:
        raw = path.encode("utf-8")
    except BaseException:
        return
    if _count >= _MAX_PATHS or _bytes + len(raw) > _MAX_BYTES:
        _over = True
        return
    _pending.add(path)
    _count += 1
    _bytes += len(raw)


def harvest():
    """Resolve every module in `sys.modules` once; return how many were new.

    Memoised by `id(module)`, which is why a warm re-harvest resolves nothing
    (the cold scan is the research's 21 ms; a warm one is 33 us and adds no
    path).
    """
    new = 0
    for module in list(sys.modules.values()):
        key = id(module)
        if key in _seen:
            continue
        _seen.add(key)
        paths = _module_paths(module)
        _resolved[key] = paths
        new += 1
        for path in paths:
            _add(path)
    return new


def flush():
    """(paths, resync): everything new since the last flush, sorted.

    `.pyc`/`.pyo` becomes its source here (never in the hook), the skip list and
    the standard library are dropped, and a budget overrun reports `resync` once
    so the consumer reloads conservatively instead of believing a truncated set.
    """
    global _pending, _over
    harvest()
    paths = set()
    for path in _pending:
        if path.endswith((".pyc", ".pyo")):
            source = _source_of(path)
            if source is not None and _wanted(source) and os.path.isfile(source):
                path = source
        if _wanted(path):
            paths.add(path)
    _pending = set()
    resync = _over
    _over = False
    paths -= _reported
    _reported.update(paths)
    return sorted(paths), resync


def _record_open(args):
    """The hook's whole body, so a test can replace it with something worse."""
    if len(args) < 1:
        return
    path = args[0]
    if not isinstance(path, str):
        return
    if not path.endswith(_SOURCE_SUFFIXES):
        return
    if not path.startswith("/"):
        if _cwd is None:
            return
        path = _cwd + "/" + path
    if _root is None or not path.startswith(_root):
        return
    _add(path)


def _audit(event, args):
    """`sys.addaudithook`: the `open` event only, and it can never raise."""
    if event != "open":
        return
    if getattr(_busy, "on", False):
        return
    _busy.on = True
    try:
        _record_open(args)
    except BaseException:
        pass
    finally:
        _busy.on = False
"#;

/// One dev interpreter's Python loaded-file state (the module above).
///
/// A Rust handle, not a copy: the state lives in `sys.modules` so the fork
/// inherits it. Every method is best-effort and returns `None` on any Python
/// error — a reporting failure must never fail a request, and a view that
/// cannot report simply reports nothing.
#[cfg(feature = "python")]
#[derive(Debug)]
pub struct LoadedTracker {
    module: Py<PyAny>,
}

#[cfg(feature = "python")]
impl LoadedTracker {
    /// Installs the module and the `open` hook, and returns the handle.
    ///
    /// Called once, by the dev boot, **before** the application is imported, so
    /// the preload phase's imports and file opens are seen. A production build
    /// never calls this (`ADR-0014` §7). Returns `None` — and installs nothing
    /// — when the interpreter refuses one of the steps, because a missing
    /// report is a smaller problem than a boot that fails for instrumentation.
    pub fn install(py: Python<'_>, root: &Path) -> Option<Self> {
        let root = std::fs::canonicalize(root).ok()?;
        let mut root = root.to_string_lossy().into_owned();
        if !root.ends_with('/') {
            root.push('/');
        }
        let cwd = std::fs::canonicalize(std::env::current_dir().ok()?).ok()?;
        let cwd = cwd.to_string_lossy().into_owned();

        let module = PyModule::new(py, LOADED_MODULE).ok()?;
        let code = std::ffi::CString::new(LOADED_TRACKER).ok()?;
        let dict = module.dict();
        py.run(&code, Some(&dict), None).ok()?;
        module.call_method1("arm", (root, cwd)).ok()?;

        let sys = py.import("sys").ok()?;
        sys.getattr("modules")
            .ok()?
            .set_item(LOADED_MODULE, &module)
            .ok()?;
        let hook = module.getattr("_audit").ok()?;
        sys.call_method1("addaudithook", (hook,)).ok()?;

        Some(Self {
            module: module.into_any().unbind(),
        })
    }

    /// The state a freshly forked view inherits, or `None` when the boot never
    /// installed it (a production build, or a view with reload off).
    pub fn adopt(py: Python<'_>) -> Option<Self> {
        let sys = py.import("sys").ok()?;
        let modules = sys.getattr("modules").ok()?;
        let module = modules.get_item(LOADED_MODULE).ok()?;
        Some(Self {
            module: module.unbind(),
        })
    }

    /// Resolves every module `sys.modules` holds; the number that were new.
    ///
    /// The dev boot calls this once after `BLASTIT_WARMUP`, which pays the cold
    /// scan in the immortal parent and leaves the memoised result for every
    /// view to inherit.
    pub fn harvest(&self, py: Python<'_>) -> Option<u64> {
        self.module
            .bind(py)
            .call_method0("harvest")
            .ok()?
            .extract()
            .ok()
    }

    /// The paths loaded since the last flush, and whether the budget was hit.
    pub fn flush(&self, py: Python<'_>) -> Option<(Vec<String>, bool)> {
        self.module
            .bind(py)
            .call_method0("flush")
            .ok()?
            .extract()
            .ok()
    }

    /// The module object, for tests that have to make the hook misbehave.
    pub fn object<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.module.bind(py).clone()
    }
}

/// What a view is handed so it can report: its own datagram socket and the
/// source id its batches name.
///
/// The socket is the write end of the per-view socketpair; the supervisor keeps
/// the read end and attributes a datagram by the descriptor it arrived on. The
/// source is a diagnostic (`seq` space), never a route to restart (§4.4).
#[cfg(feature = "python")]
#[derive(Debug)]
pub struct LoadedReport {
    /// The write end of the view's `SOCK_DGRAM` socketpair.
    pub socket: UnixDatagram,
    /// The id this view's batches name: its route index, or
    /// [`crate::loaded::PRELOAD_SOURCE`] for the preload phase.
    pub source: u16,
}

/// One view's loaded-file reporter: the inherited Python state plus its socket.
///
/// Both halves are best-effort. [`LoadedReporter::report`] takes the socket
/// lock with `try_lock` and returns immediately when another thread is already
/// reporting, so no thread ever waits for another request's report, and the
/// send itself is non-blocking, so no request ever waits for the supervisor.
#[cfg(feature = "python")]
#[derive(Debug)]
pub struct LoadedReporter {
    tracker: LoadedTracker,
    sender: Mutex<Reporter>,
}

#[cfg(feature = "python")]
impl LoadedReporter {
    /// Adopts the inherited tracker and wraps the view's socket.
    ///
    /// `None` when the boot never installed the tracker: a view then reports
    /// nothing and serves exactly as it did before.
    pub fn adopt(py: Python<'_>, report: LoadedReport) -> Option<Self> {
        let tracker = LoadedTracker::adopt(py)?;
        let sender = Reporter::new(report.socket, report.source).ok()?;
        Some(Self {
            tracker,
            sender: Mutex::new(sender),
        })
    }

    /// The boot batch: the inherited module closure plus anything already open.
    ///
    /// Sent once per view, with `boot` set, so the consumer replaces that
    /// source's set. A boot always starts a fresh `seq` stream (`seq = 0`), so a
    /// reload that keeps this process must adopt a new reporter rather than
    /// send a second boot on this one: a `boot` with `seq != 0` is refused by
    /// the codec (`loaded::Batch::new`), which treats it as a dropped batch.
    /// The `preload` phase is in here by inheritance — the modules were
    /// imported before the fork — which is why no parent-side reporter exists.
    pub fn boot(&self, py: Python<'_>) {
        self.report(py, true);
    }

    /// Reports one request's delta, or nothing when nothing changed.
    pub fn report(&self, py: Python<'_>, boot: bool) {
        // Another thread is already reporting: skip this one rather than make a
        // request wait for it. Nothing is consumed, so the next request reports.
        let Ok(mut sender) = self.sender.try_lock() else {
            return;
        };
        let Some((paths, resync)) = self.tracker.flush(py) else {
            return;
        };
        if resync {
            sender.arm_resync();
        }
        if paths.is_empty() {
            return;
        }
        let _ = sender.send(boot, &paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::ReqHead;

    /// A runtime that echoes the body back, without a transport behind it.
    struct Echo;

    impl WorkerRuntime for Echo {
        fn kind(&self) -> &'static str {
            "echo"
        }

        fn max_inflight(&self) -> u32 {
            1
        }

        fn serve(&self, _req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd {
            let mut body = Vec::new();
            let mut buf = [0u8; 8];
            loop {
                match io.read_body(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&buf[..n]),
                }
            }
            if io.write_head(RespHead::default()).is_err() || io.write_body(&body).is_err() {
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            io.output(OutputStream::Stderr, "echoed");
            let outcome = match io.flush_response() {
                Ok(()) => DoneOutcome::Completed,
                Err(_) => DoneOutcome::Aborted,
            };
            RequestEnd::new(outcome, Some(7))
        }

        fn recycle(&self) -> bool {
            false
        }
    }

    /// What a request writes, in the order it wrote it.
    #[derive(Debug, PartialEq, Eq)]
    enum Frame {
        Head,
        Body(Vec<u8>),
        Line(OutputStream, String),
        Flush,
    }

    /// A [`RequestIo`] that records frames, with a body supplied up front.
    struct Recorder {
        head: ReqHead,
        body: Vec<u8>,
        frames: Vec<Frame>,
    }

    impl RequestIo for Recorder {
        fn head(&self) -> &ReqHead {
            &self.head
        }

        fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.body.len());
            buf[..n].copy_from_slice(&self.body[..n]);
            self.body.drain(..n);
            Ok(n)
        }

        fn write_head(&mut self, _head: RespHead) -> io::Result<()> {
            self.frames.push(Frame::Head);
            Ok(())
        }

        fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
            self.frames.push(Frame::Body(data.to_vec()));
            Ok(())
        }

        fn output(&mut self, stream: OutputStream, line: &str) {
            self.frames.push(Frame::Line(stream, line.to_string()));
        }

        fn flush_response(&mut self) -> io::Result<()> {
            self.frames.push(Frame::Flush);
            Ok(())
        }
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn the_output_line_cap_is_the_documented_2048() {
        // §4.1 documents `output`'s line cap as 2048 bytes; the WSGI sink
        // `MB.3.24` builds and the PHP sink `MB.3.13` builds must not drift.
        assert_eq!(OUTPUT_LINE_CAP, 2048);
    }

    #[test]
    fn the_seam_is_object_safe_and_its_result_can_cross_a_thread() {
        // The contract `MB.9.3`/`MB.9.5` need: a `ViewUpstream` holds a boxed
        // runtime and moves the whole thing into a `spawn_blocking` task, so no
        // downstream code can name the concrete type or the language.
        let runtime: Option<Box<dyn WorkerRuntime + Send + Sync>> = None;
        let io: Option<&mut dyn RequestIo> = None;
        assert!(runtime.is_none() && io.is_none());

        assert_send::<RequestEnd>();
        assert_send::<Box<dyn WorkerRuntime + Send + Sync>>();
    }

    #[test]
    fn a_request_writes_head_then_body_then_closes_before_reporting() {
        // The ordering the transport relies on: everything the runtime produces
        // is on the wire by the time `serve` returns, so the caller can close
        // the request with `RESP_END`/`DONE` from the run's `RequestEnd`.
        let runtime: Box<dyn WorkerRuntime + Send> = Box::new(Echo);
        let mut io = Recorder {
            head: ReqHead::default(),
            body: b"hello".to_vec(),
            frames: Vec::new(),
        };

        let end = runtime.serve(ReqHead::default(), &mut io);

        assert_eq!(
            io.frames,
            vec![
                Frame::Head,
                Frame::Body(b"hello".to_vec()),
                Frame::Line(OutputStream::Stderr, "echoed".to_string()),
                Frame::Flush,
            ]
        );
        assert_eq!(end, RequestEnd::new(DoneOutcome::Completed, Some(7)));
        assert!(!runtime.recycle(), "a served request keeps its worker");
        assert_eq!(runtime.kind(), "echo");
        assert_eq!(runtime.max_inflight(), 1);
    }

    /// A source-text witness for the hook (`MB.23.4`): what `_record_open`
    /// must **not** contain, and the three things `_audit` must contain.
    ///
    /// It pins, by token, that the function runs on every application `open`
    /// with no path resolution, no `os` call, no import and no I/O of its own,
    /// and that the guard around it stays a one-event check behind a
    /// `threading.local()` busy flag and a `BaseException` catch. Research §2.1
    /// measured `abspath` at 4× the predicate's own cost and found that a raise
    /// (or a recursion) in the hook aborts the application's real `open`, which
    /// is why those tokens matter at all.
    ///
    /// This is **not behavioural proof**, and the distinction is the point: a
    /// source scan cannot show that a helper such as `_add` does no I/O, cannot
    /// see a path through an alias, and would still pass if `_record_open` were
    /// gutted to `pass`. The runtime half — a raising hook never aborts the
    /// real `open`, the busy flag stops recursion, no `__pycache__` path
    /// reaches the report — is witnessed by
    /// `crates/blastit/tests/critical_loaded_files.rs`. What this buys is a
    /// cheap tripwire on the edit this code is one keystroke away from.
    #[cfg(feature = "python")]
    #[test]
    fn the_open_hook_does_no_io_no_import_and_no_path_resolution() {
        // One function's own text, up to the next top-level `def`.
        let body_of = |def: &'static str| {
            let start = LOADED_TRACKER
                .find(def)
                .expect("the module declares this function");
            let rest = &LOADED_TRACKER[start + def.len()..];
            match rest.find("\ndef ") {
                Some(end) => &rest[..end],
                None => rest,
            }
        };

        let hook = body_of("def _record_open(");
        for banned in [
            // Any `os` use, and specifically the resolvers and predicates.
            "os.",
            "os.path",
            "abspath",
            "realpath",
            "getcwd",
            "chdir",
            "isfile",
            "islink",
            "exists(",
            ".stat(",
            "listdir",
            "walk(",
            "glob",
            "readlink",
            // Anything that would spawn, import or talk to a peer.
            "subprocess",
            "socket",
            "urllib",
            "import ",
            "logging",
            "traceback",
            "builtins",
            "sys.",
            "eval(",
            "exec(",
            "compile(",
            // The application's own call: the hook may not open or print.
            "open(",
            "print(",
            "Path(",
        ] {
            assert!(
                !hook.contains(banned),
                "the hook may not use {banned:?} (research §2.1)"
            );
        }

        let audit = body_of("def _audit(");
        // `_record_open(args)` is the one call the guard is allowed to make;
        // strip it so `open(` only means the builtin.
        assert_eq!(
            audit.matches("_record_open(").count(),
            1,
            "the guard delegates once, to `_record_open`"
        );
        let guard = audit.replace("_record_open(", "");
        for banned in ["os.", "open(", "print(", "import ", ".stat(", "abspath"] {
            assert!(
                !guard.contains(banned),
                "the hook's guard may not use {banned:?} either"
            );
        }
        assert!(
            audit.contains("if event != \"open\":"),
            "the hook fires for one event only"
        );
        assert!(
            audit.contains("except BaseException"),
            "a raise in the hook would abort the application's own open"
        );
        assert!(
            audit.contains("_busy.on"),
            "the recursion guard is what keeps a hook from re-entering itself"
        );
        assert!(
            LOADED_TRACKER.contains("_busy = threading.local()"),
            "the busy flag is per thread, because the hook fires on every thread"
        );
    }

    /// The Python half (`MB.23.4`): one install, the module in `sys.modules`,
    /// and a harvest that reports the module and a template it opened.
    ///
    /// This is the only test in this binary that initialises the interpreter,
    /// so the tracker's process-wide state is not shared with another test; the
    /// full matrix (`.pyc`, frozen modules, a raising hook, the busy flag, the
    /// warm path) is `crates/blastit/tests/critical_loaded_files.rs`.
    #[cfg(feature = "python")]
    #[test]
    fn the_loaded_tracker_installs_once_and_reports_a_module_and_a_template() {
        use std::fs;

        let root = std::env::temp_dir().join(format!(
            "blastit-view-loaded-tracker-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("templates")).expect("the fixture root");
        fs::write(root.join("tracked_helper.py"), "VALUE = 7\n").expect("the module");
        fs::write(root.join("templates/index.html"), "<p>hi</p>\n").expect("the template");
        let module = root.join("tracked_helper.py");

        Python::initialize();
        Python::attach(|py| {
            assert!(
                LoadedTracker::adopt(py).is_none(),
                "nothing is adopted before the one install"
            );
            let tracker = LoadedTracker::install(py, &root).expect("the tracker installs");
            assert!(LoadedTracker::adopt(py).is_some(), "a forked view finds it");

            let code = format!(
                "import sys\nsys.path.insert(0, {root:?})\nimport tracked_helper\n\
                 open({template:?}).read()\n",
                root = root.to_string_lossy(),
                template = root.join("templates/index.html").to_string_lossy(),
            );
            let code = std::ffi::CString::new(code).expect("no NUL in the snippet");
            py.run(&code, None, None)
                .expect("the application code runs");

            // The cold scan resolves the closure; the flush reports it.
            assert!(tracker.harvest(py).expect("a harvest") > 0);
            let (paths, resync) = tracker.flush(py).expect("a flush");
            assert!(!resync, "the view is far inside its budget");
            assert!(
                paths.contains(&module.to_string_lossy().into_owned()),
                "the module's own .py is reported: {paths:?}"
            );
            assert!(
                paths.contains(
                    &root
                        .join("templates/index.html")
                        .to_string_lossy()
                        .into_owned()
                ),
                "the template the app opened is reported: {paths:?}"
            );
            assert!(
                !paths.iter().any(|path| path.ends_with("/__pycache__/")),
                "no bytecode cache path leaks: {paths:?}"
            );
            assert!(
                !paths.iter().any(|path| path.ends_with("/os.py")),
                "a frozen module never reports its __file__: {paths:?}"
            );

            // Warm: the same unchanged `sys.modules` resolves nothing new and
            // reports nothing, which is the measured 33 us path asserted as a
            // call count rather than a timer.
            assert_eq!(
                tracker.harvest(py),
                Some(0),
                "the warm scan resolves nothing"
            );
            let (paths, resync) = tracker.flush(py).expect("a second flush");
            assert!(paths.is_empty(), "nothing new to report: {paths:?}");
            assert!(!resync);
        });

        let _ = fs::remove_dir_all(&root);
    }
}
