//! The dev-only loaded-file tracker: the module closure and the `open` audit
//! hook a view installs before it imports the application
//! (`docs/design/dev-fanotify-reload.md` §4.4, `ADR-0014`).
//!
//! The state lives in `sys.modules`, so a forked view inherits it rather than
//! rebuilding it; [`LoadedTracker`] is a Rust handle on that state, and
//! [`LoadedReporter`] pairs it with the datagram socket the supervisor reads.
//! The wire format, its bounds and [`Reporter`] are `blastit-view`'s
//! (`blastit_view::loaded`): this module is the interpreter half of the same
//! report, which is why it lives beside the WSGI worker and not in the
//! transport.
//!
//! Nothing here touches the wire's bytes; a reporting failure is swallowed by
//! design, because instrumentation must never fail a request (`ADR-0014` §7).

use std::path::Path;
use std::sync::Mutex;

use blastit_view::loaded::{LoadedReport, Reporter};
use pyo3::prelude::*;
use pyo3::types::PyModule;

/// The private module that holds one dev interpreter's loaded-file state.
///
/// It lives in `sys.modules` so a forked view can find it without a global in
/// this crate and without an environment variable (stage 0 strips every
/// `BLASTIT_*` from a view, and a view must not see one anyway). The name is
/// private to blastit and is not a package an application imports.
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
#[derive(Debug)]
pub struct LoadedTracker {
    module: Py<PyAny>,
}

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

/// One view's loaded-file reporter: the inherited Python state plus its socket.
///
/// Both halves are best-effort. [`LoadedReporter::report`] takes the socket
/// lock with `try_lock` and returns immediately when another thread is already
/// reporting, so no thread ever waits for another request's report, and the
/// send itself is non-blocking, so no request ever waits for the supervisor.
#[derive(Debug)]
pub struct LoadedReporter {
    tracker: LoadedTracker,
    sender: Mutex<Reporter>,
}

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
