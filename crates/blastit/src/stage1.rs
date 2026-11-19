//! Stage 1: the app environment, the front end, the scheduler and the supervisor
//! (§2.1, §4).
//!
//! By the time this stage starts, every secret is gone: the process image was
//! replaced with the app environment (`envp`), dbproxy holds the credentials and
//! init only keeps the read ends of its log pipes. What is left to do is:
//!
//! ```text
//! read the configuration from --config-fd
//! bind 0.0.0.0:$PORT
//! fork http (base+1) and scheduler (base+2) — both before CPython exists
//! initialise CPython, import BLASTIT_APP, preload, warm up, close DB handles
//! gc.collect() · immortalize the preloaded heap (§9.2 #7) · gc.freeze()
//! boot: what the preload cost and the view cap memory buys (§8)
//! ready
//! supervise, holding the GIL
//! ```
//!
//! Nothing here parses HTTP and nothing here talks to the database: init exists
//! to fork, to supervise and to be the only writer to stdout (§2.1).
//!
//! # Two ways to have an interpreter
//!
//! The standalone binary embeds CPython and initialises it here (§9.3). A wheel
//! install is *already* a Python process: `blastit._blastit.main()` is called by
//! the console script, stage 0 re-executes that same interpreter (§4), and this
//! stage uses the interpreter it finds — no second CPython, and the venv's
//! `site-packages` are its own. That is the whole of [`Interpreter`].

#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use std::os::fd::RawFd;
use std::path::PathBuf;

use blastit_events::{Component, Event, EventKind, Lifecycle};
#[cfg(feature = "python")]
use blastit_events::{Level, Stream};
#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::config::Config;
use crate::logs::{Emitter, output_event};
use crate::mem_usage;
use crate::py::Python;

mod adopt;
mod boot;
mod python_env;
mod serve;
mod spawn;

#[cfg(test)]
mod unit_tests;

pub use boot::run;

pub(crate) use adopt::Adoption;

pub use spawn::ViewChildPlans;

/// Whether stage 1 finds an interpreter already running (§4).
pub enum Interpreter<'py> {
    /// The standalone binary: CPython is embedded in this image and is
    /// initialised here (§9.3).
    Embed,
    /// A wheel install: the interpreter that re-executed us for stage 1 is up,
    /// its main thread holds the GIL, and its `site-packages` are the venv's.
    Hosted(Python<'py>),
}

impl Interpreter<'_> {
    /// Whether this is an interpreter the host started, as opposed to one this
    /// process embeds.
    const fn is_hosted(&self) -> bool {
        matches!(self, Self::Hosted(_))
    }
}

/// Where stage 1 finds what stage 0 left it (§4).
#[derive(Debug, Clone)]
pub struct Options {
    /// `--config-fd`: the read end of the configuration pipe.
    pub config_fd: RawFd,
    /// `--dbproxy-pid`: dbproxy's pid, when it was forked.
    pub dbproxy_pid: Option<i32>,
    /// `--dbproxy-events-fd`: dbproxy's events pipe.
    pub dbproxy_events_fd: Option<RawFd>,
    /// `--dbproxy-stdout-fd`: dbproxy's stdout pipe.
    pub dbproxy_stdout_fd: Option<RawFd>,
    /// `--dbproxy-stderr-fd`: dbproxy's stderr pipe.
    pub dbproxy_stderr_fd: Option<RawFd>,
    /// `--dev-root`: the ephemeral runtime root `blastit dev` created (`R20`).
    /// `Some` makes this stage an unprivileged single-uid simulation.
    pub dev_root: Option<PathBuf>,
    /// `--dev-host`: the address the http role binds in `dev`.
    pub dev_host: Option<std::net::IpAddr>,
    /// `--dev-obs-fd`: the read end of dbproxy's `--learn` observation pipe.
    pub dev_obs_fd: Option<RawFd>,
    /// `--dev-ctl-fd`: this stage's end of the `--learn` attribution socket.
    pub dev_ctl_fd: Option<RawFd>,
    /// `--dev-reload=on|off`: whether this boot constructs the live-reload
    /// watcher. Stage 0 forwards the user's `--no-reload` as `off`; absent is
    /// the default, `on`.
    pub dev_reload: bool,
    /// `--dev-adopt-fd`: the read end of a reload cycle's adoption document
    /// (§4.6 step 6). `Some` means this image is the replacement half of a
    /// reload: it binds nothing, forks no `http`/dbproxy, and adopts their pids,
    /// pipes and `http.chan` end from the document.
    pub dev_adopt_fd: Option<RawFd>,
    /// `--dev-policy-fd`: this image's end of the dev-only dbproxy policy
    /// socket (§4.6 step 2). Carried into the next adoption document; the
    /// refresh itself is `MB.23.5.3`'s.
    pub dev_policy_fd: Option<RawFd>,
    /// `--dev-reload-since`: the unix-nanosecond instant a reload cycle began,
    /// for the one-shot `Policy::reconcile` after this image's boot report
    /// (§4.6 step 7).
    pub dev_reload_since: Option<u128>,
}

/// Everything a view needs of its scratch directory, in Python (§5.3).
#[cfg(feature = "python")]
const SITE_PACKAGES: &std::ffi::CStr = c"
try:
    import os, sys, sysconfig
    for _key in ('purelib', 'platlib'):
        _path = sysconfig.get_paths().get(_key)
        if _path and _path not in sys.path and os.path.isdir(_path):
            sys.path.append(_path)
except Exception:
    pass
";

/// The whole `sys.path` a view will import through (§5.3), absolute.
///
/// Not only what blastit put there: the interpreter's own entries, the working
/// directory (§5.3), whatever `PYTHONPATH` named, whatever a `.pth` file added
/// while `site` ran, and anything an import has appended by the time this runs.
/// A writable entry is a cross-route code plant whoever added it — one route
/// writing a module another route imports — so the boot asks about all of them.
/// `abspath` resolves the empty entry to the working directory, which is what
/// the interpreter does with it.
#[cfg(feature = "python")]
const SYS_PATH: &std::ffi::CStr = c"
import os, sys
blastit_sys_path = []
for _entry in sys.path:
    _path = os.path.abspath(_entry)
    if _path not in blastit_sys_path:
        blastit_sys_path.append(_path)
";

/// Everything a view has to be able to read, as Python paths (§7.2).
///
/// A view process imports *after* the fork: a lazy module, a template, a
/// URLconf that is not in `BLASTIT_PRELOAD`. What it imports through is the
/// interpreter (its `sys.prefix` and `sys.base_prefix`), the standard library,
/// `site-packages`, and the directory the application's own module comes from —
/// and a view is not the owner's uid, so the only bits that apply to it are
/// "other"'s. Stage 1 collects them here and [`crate::visibility::unreadable`]
/// decides whether a view could reach them, so a deployment that cannot is
/// refused at boot instead of answering some requests and raising
/// `ModuleNotFoundError: No module named 'stringprep'` inside others.
///
/// Both a path and what it resolves to are collected: a symlinked interpreter is
/// reached through the link *and* read at the target, and both have to be
/// traversable. `blastit_app_module` is the module of `BLASTIT_APP` and is set
/// by the caller; a module that cannot be found at all is not this check's
/// business — the import right after says so, in its own words.
#[cfg(feature = "python")]
const VIEW_PATHS: &std::ffi::CStr = c"
import importlib.util, os, sys, sysconfig
blastit_view_paths = []
def _add_blastit_path(path):
    if not path:
        return
    for _candidate in (path, os.path.realpath(path)):
        if _candidate not in blastit_view_paths:
            blastit_view_paths.append(_candidate)
for _prefix in (getattr(sys, 'base_prefix', ''), sys.prefix):
    _add_blastit_path(_prefix)
for _key, _path in sysconfig.get_paths().items():
    if _key in ('stdlib', 'platstdlib', 'purelib', 'platlib'):
        _add_blastit_path(_path)
if blastit_app_module:
    try:
        _spec = importlib.util.find_spec(blastit_app_module)
    except Exception:
        _spec = None
    if _spec is not None:
        if getattr(_spec, 'origin', None):
            _add_blastit_path(os.path.dirname(_spec.origin))
        for _location in (getattr(_spec, 'submodule_search_locations', None) or ()):
            _add_blastit_path(_location)
";

/// §4: close database handles before the fork.
///
/// A connection opened at import time would be inherited — uselessly and
/// dangerously — by every view. `gc.collect()` comes straight after, so the set
/// `gc.freeze()` freezes a moment later is as small as it can be, and so the
/// immortalisation pass walks a heap with no garbage in it.
#[cfg(feature = "python")]
const CLOSE_AND_COLLECT: &std::ffi::CStr = c"
import gc, sys
if 'django' in sys.modules:
    from django.db import connections
    connections.close_all()
gc.collect()
";

/// §9.2 #7: freeze the heap, whether or not it could be immortalised.
///
/// `gc.freeze()` puts the collector's objects out of its reach before the fork
/// in every case. When the immortalisation pass runs, it runs *before* this,
/// because an immortal object is never freed and its address can therefore
/// never be reused by a later allocation; when the pass is skipped
/// ([`Decision::Off`]), the heap is frozen all the same.
#[cfg(feature = "python")]
const FREEZE: &std::ffi::CStr = c"
import gc
gc.freeze()
";

/// Emits the `ready` event with the route count and the output variable names
/// (§8.1).
fn emit_ready(emitter: &Emitter, config: &Config, routes: usize) {
    let mut lifecycle = Lifecycle::default()
        .with_stage("serving")
        .with_routes(routes as u32)
        .with_workers(config.max_workers);
    for (proxy, names) in &config.proxy_outputs {
        for name in names {
            lifecycle = lifecycle.with_output(proxy.clone(), name.clone());
        }
    }
    emitter.emit(&Event::now(Component::Init, EventKind::Ready(lifecycle)));
}

/// Cleans up an adopted `dev` runtime root, if any (`R20`), then logs `message`
/// and exits non-zero.
fn fail(emitter: &Emitter, dev: Option<&crate::dev_learn::Dev>, message: &str) -> ! {
    if let Some(dev) = dev {
        dev.cleanup();
    }
    crate::logs::fail(emitter, message)
}

/// Prints a Python failure as events, cleans up an adopted `dev` runtime root
/// (`R20`), and exits non-zero (§4).
#[cfg(feature = "python")]
fn python_fail(
    py: Python<'_>,
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    what: &str,
    err: &PyErr,
) -> ! {
    emitter.emit(
        &output_event(
            Component::Init,
            None,
            Stream::Stderr,
            &format!("{what}: {err}"),
        )
        .with_level(Level::Error),
    );
    let text = blastit_view_python::zygote::traceback_text(py, err);
    for line in text.lines() {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, line).with_level(Level::Error),
        );
    }
    if let Some(dev) = dev {
        dev.cleanup();
    }
    crate::coverage::flush();
    // SAFETY: init is exiting; `_exit` runs no handler and cannot deadlock with
    // the GIL held.
    unsafe { libc::_exit(1) }
}

#[cfg(feature = "python")]
use pyo3::types::PyDict;
#[cfg(feature = "python")]
use std::os::unix::ffi::OsStringExt;

#[cfg(feature = "python")]
use self::adopt::{emit_preload, immortalize, import_callable, refresh_memory};
#[cfg(feature = "python")]
use self::python_env::{put_cwd_first, restore_sys_path};
#[cfg(feature = "python")]
use self::spawn::check_view_paths;

/// What a `dev` boot hands [`load`] beyond the configuration: the runtime root
/// its failure paths clean up, and the `preload` phase's report socket.
///
/// The socket is the loaded-file hook's gate (§5 `S3`): a boot that has none —
/// production, `check`, and a `dev` boot with reload off — installs no hook.
#[cfg(feature = "python")]
pub(super) struct Boot<'a> {
    /// The `blastit dev` runtime root, when this is a dev boot (`R20`).
    pub(super) dev: Option<&'a crate::dev_learn::Dev>,
    /// The `preload` phase's report socket, when live reload is on (`ADR-0014`).
    pub(super) report: Option<blastit_view::Reporter>,
    /// Whether this image is the first half of a run. A reload's replacement
    /// image repeats neither the `boot` event nor `ready` (§4.6 step 7).
    pub(super) announce: bool,
}

/// Imports and prepares the application, and returns the WSGI callable (§4).
///
/// `memory` is the reading `run` took before the fork: it is the one that
/// warned, once, when the machine's memory cannot be read at all (§8 Sizing).
///
/// `hosted` is `true` when the interpreter is the one the wheel was installed
/// into: its `site-packages` are already on `sys.path`, and what `-I` dropped —
/// `PYTHONPATH`, the working directory — is put back before the first import.
#[cfg(feature = "python")]
pub(super) fn load(
    py: Python<'_>,
    config: &Config,
    routes: usize,
    emitter: &Emitter,
    memory: Option<mem_usage::Usage>,
    hosted: bool,
    boot: Boot<'_>,
) -> Py<PyAny> {
    let Boot {
        dev,
        report,
        announce,
    } = boot;
    // `R13`: an all-static site has no app to import — every answer comes from
    // the scheduler, so there is nothing to preload, nothing to warm up and no
    // heap a view would inherit. `None`, never a missing callable.
    if config.app.is_none() {
        return py.None();
    }
    // What blastit itself puts on `sys.path` is recorded here, because a
    // directory another uid could write is refused below: the working directory
    // it was started in, and whatever an embedded interpreter had to be told
    // about.
    let scope = PyDict::new(py);
    if !hosted {
        // §9.3: an embedded interpreter does not find the distribution's
        // site-packages on its own. Best effort — a system without sysconfig is
        // not a reason to refuse to start.
        let _ = py.run(SITE_PACKAGES, Some(&scope), None);
    }
    // The entries stage 0 recorded go in **front** of everything above: a
    // wheel's `-I` dropped `PYTHONPATH` and the working directory, and an
    // embedded binary's project `.venv` has to win over the base interpreter's
    // purelib or the application's own dependencies are never imported
    // (`MB.40.8`). Appending `SITE_PACKAGES` first and inserting these at the
    // front is exactly that order.
    if let Err(err) = restore_sys_path(py, &config.sys_path) {
        python_fail(py, emitter, dev, "sys.path", &err);
    }
    if let Err(err) = put_cwd_first(py, config.cwd.as_deref()) {
        python_fail(py, emitter, dev, "the working directory on sys.path", &err);
    }
    // §7.2: what a view imports after the fork has to be readable by *its* uid,
    // which is not the one that installed the interpreter. A deployment that
    // fails this is refused here, at boot, rather than answering requests that
    // only some paths can serve.
    //
    // `blastit dev` is the one mode where there is no other uid: every view runs
    // as the caller, so the question has no content (`R19`).
    if dev.is_none() {
        check_view_paths(py, config, routes, &scope, emitter);
    }

    let spec = config.app.as_deref().unwrap_or_default();
    // `MB.23.4`: the dev-only loaded-file report. Installed **before** the first
    // import so the `preload` phase's modules and file opens are seen, and
    // harvested once after `BLASTIT_WARMUP` so the cold `sys.modules` scan is
    // paid here, in the immortal parent every view is forked from. That is why
    // the preloaded closure reaches the supervisor by inheritance, with no
    // second socket in this process (`ADR-0014` §2).
    //
    // The gate is the **report socket's presence**, which is what a `dev` boot
    // with reload off does not have (§5 `S3`): no socket, no hook, no mark, and
    // a production boot, `check` and `test` have no socket either. There is no
    // second `Config` field to keep in step with the flag.
    #[cfg(feature = "python")]
    let loaded = if dev.is_some() {
        let root = config
            .cwd
            .as_deref()
            .map(|cwd| PathBuf::from(std::ffi::OsString::from_vec(cwd.to_vec())))
            .filter(|_| report.is_some());
        root.and_then(|root| blastit_view_python::LoadedTracker::install(py, &root))
    } else {
        None
    };
    // `§1.2`: a line before the blocking import, so a slow application never
    // looks hung. The preload modules and the warm-up are the same wait.
    crate::logs::phase(emitter, "app", &format!("importing {spec}"));
    let app = match import_callable(py, spec) {
        Ok(app) => app,
        Err(err) => python_fail(py, emitter, dev, &format!("BLASTIT_APP ({spec})"), &err),
    };
    for module in &config.preload {
        if let Err(err) = py.import(module.as_str()) {
            python_fail(
                py,
                emitter,
                dev,
                &format!("BLASTIT_PRELOAD ({module})"),
                &err,
            );
        }
    }
    if let Some(warmup) = config.warmup.as_deref() {
        let callable = match import_callable(py, warmup) {
            Ok(callable) => callable,
            Err(err) => python_fail(
                py,
                emitter,
                dev,
                &format!("BLASTIT_WARMUP ({warmup})"),
                &err,
            ),
        };
        if let Err(err) = callable.call0(py) {
            python_fail(
                py,
                emitter,
                dev,
                &format!("BLASTIT_WARMUP ({warmup})"),
                &err,
            );
        }
    }
    // The baseline scan: every module the boot imported is now in
    // `sys.modules`, so one cold harvest here means every view's first report
    // is warm and the preloaded closure is part of it (`MB.23.4`). Best effort:
    // instrumentation must never fail a boot. The preload phase's own batch goes
    // out here, with `boot` set, so the supervisor replaces source `0xffff`'s
    // set before the first view exists (§4.1 step 4).
    if let Some(loaded) = &loaded {
        let _ = loaded.harvest(py);
        if let Some(mut sender) = report
            && let Some((paths, resync)) = loaded.flush(py)
        {
            if resync {
                sender.arm_resync();
            }
            let _ = sender.send(true, &paths);
        }
    }
    if let Err(err) = py.run(CLOSE_AND_COLLECT, None, None) {
        python_fail(py, emitter, dev, "close_all/gc.collect", &err);
    }
    // §9.2 #7, between `gc.collect()` and `gc.freeze()`: nothing left in the
    // collected heap is garbage, and every object a request could touch stops
    // counting references — on an interpreter whose marker blastit knows, which
    // `BLASTIT_IMMORTALIZE` and the version decide.
    let outcome = immortalize(py, config, emitter, dev);
    if let Err(err) = py.run(FREEZE, None, None) {
        python_fail(py, emitter, dev, "gc.freeze", &err);
    }
    if announce {
        emit_preload(
            emitter,
            config,
            &outcome,
            refresh_memory(memory),
            dev.is_some(),
        );
    }
    app
}
