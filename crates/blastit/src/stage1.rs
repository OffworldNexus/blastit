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

use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(feature = "python")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(feature = "python")]
use blastit_events::ManifestWarning;
#[cfg(feature = "php")]
use blastit_events::Runtime;
use blastit_events::{Component, Event, EventKind, Level, Lifecycle, Stream};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
#[cfg(feature = "python")]
use blastit_scheduler::derived_view_cap;
use blastit_scheduler::{MemoryProbe, SchedulerConfig, routes_from_manifest};
use blastit_sys::{
    Fork, Pid, SeccompRole, Signal, SignalFd, SignalMask, bind_tcp, drop_privileges, harden_child,
    pipe, restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};
#[cfg(feature = "python")]
use pyo3::exceptions::PyValueError;
#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::{PyBytes, PyDict};

use crate::child::{ChildIo, Filter, Privileges, child_exit, install_filter};
use crate::config::{Config, LogFormat};
#[cfg(feature = "python")]
use crate::dev_reload::{MarkOutcome, Unavailable};
use crate::logs::{Emitter, output_event};
use crate::mem_usage;
use crate::py::Python;
use crate::roles::{HttpFds, SchedulerFds};
use crate::supervise::{ComponentPipes, Parts, Supervisor};
#[cfg(feature = "python")]
use blastit_view_python::immortal::{Decision, Outcome};

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

/// Runs stage 1. Never returns.
///
/// `interpreter` says whether CPython has to be started here (the standalone
/// binary) or is the interpreter this process already runs on (a wheel install,
/// §4, §9.3).
pub fn run(options: Options, interpreter: Interpreter<'_>) -> ! {
    // Whether a live interpreter is in this image, which decides how the forks
    // below behave (§9.3) and how stage 1 finds the application's modules.
    let hosted = interpreter.is_hosted();
    // The configuration itself is not known yet, so a failure here is JSON.
    let boot = Emitter::stdout(LogFormat::Json, Level::Info);
    // `blastit dev`: the runtime root stage 0 created, and the address http
    // binds. Adopted first so every fatal path in stage 1 — `read_config`,
    // `bind_tcp`, `fork`, `BLASTIT_APP` import — removes the verified `0700`
    // directory before `_exit(1)` (`R20`).
    let dev = match options.dev_root {
        Some(root) => {
            let host = options
                .dev_host
                .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
            match crate::dev_learn::Dev::adopt(root, host) {
                Ok(dev) => Some(dev),
                Err(err) => fail(&boot, None, &format!("blastit dev runtime root: {err}")),
            }
        }
        None => None,
    };
    #[cfg_attr(not(feature = "python"), allow(unused_variables))]
    let BootConfig {
        config,
        probe_timeout: php_probe_timeout,
        swap_point: test_swap_point,
        bytes: config_bytes,
        restart,
        reload_fault,
    } = read_config(&boot, dev.as_ref(), options.config_fd);
    #[cfg(not(feature = "php"))]
    let _ = php_probe_timeout;
    // The replacement half of a reload (§4.6 step 6): read before anything is
    // bound, because an adoption document this image cannot honour refuses the
    // boot instead of quietly starting a second listener.
    let adopted = match options.dev_adopt_fd {
        Some(fd) => match Adoption::read(fd) {
            Ok(adoption) => Some(adoption),
            Err(message) => fail(&boot, dev.as_ref(), &message),
        },
        None => None,
    };
    let emitter = Emitter::stdout(config.log_format, config.log_level);
    // §8.1: a setting that is deprecated is the operator's to hear about, once,
    // and the log is the only place blastit can say it. The decision comes from
    // stage 0's parsing and the sentences name variables, never values.
    for warning in &config.warnings {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, warning.message())
                .with_level(Level::Warn),
        );
    }

    let source = match crate::config::read_manifest(&config.manifest) {
        Ok(source) => source,
        Err(error) => fail(&emitter, dev.as_ref(), &error.sentence(&config.manifest)),
    };
    let (manifest, _warnings) = match Manifest::parse_blastfile(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => fail(&emitter, dev.as_ref(), &errors.to_string()),
    };
    // Warnings are stage 0's: it reads and validates the manifest first, and
    // printing them twice would be noise (§8.1).
    //
    // `ADR-0011` (`MB.3.53`): the gate stage 0 refuses with, repeated here
    // because this is the re-read. `config` carries the manifest's *path* and no
    // digest of what stage 0 approved, so a file swapped between the two stages
    // would otherwise serve a `body = "raw"` route stage 0 never saw. Gated on
    // `dev.is_none()` for the same reason stage 0 is: `blastit dev` is the local
    // simulation and keeps its warning, which stage 0 already printed.
    if dev.is_none()
        && let Err(message) = crate::stage0::raw_ingress_gate(&manifest)
    {
        fail(&emitter, dev.as_ref(), &message);
    }

    let limits = match config.limits() {
        Ok(limits) => limits,
        Err(err) => fail(&emitter, dev.as_ref(), &err.to_string()),
    };
    // §8 Sizing: the machine's memory decides how many view processes fit.
    // `total` is a constant, so it can be read before the fork and handed to
    // the scheduler; `used` is only meaningful after the app is preloaded, so
    // `load` takes a fresh reading of it. This is the one that warns when
    // memory cannot be read at all.
    let memory = read_memory(&emitter);
    let layout = UidLayout::new(config.uid_base);
    let learn = options.dev_obs_fd.is_some();
    let declared_routes = manifest.routes().len();
    // `blastit dev` gives every role one identity — the caller's — so the uid
    // layout is not consulted at all: every `RouteInfo` names the same uid, and
    // the private directories are named by route index instead (`R19`, `R20`).
    // Under `dev --learn`, slot `0` is a dedicated unrouted-request fallback
    // view (`LEARN_FALLBACK_ROUTE`) that holds no pattern and so never matches,
    // while declared routes sit at `1..=N` so no declared route collides with
    // the scheduler's slot-0 fallback (`R26`, `R27`).
    let dev_uid = crate::dev_learn::euid();
    // `ADR-0009` §5: stage 0 already refused such a boot, and this is the same
    // refusal one process later — stage 1 is the process that would have to fork
    // a view, so it repeats the gate rather than trusting a `--config-fd` it
    // cannot see the origin of. Compiled out on the `python` build.
    #[cfg(not(feature = "python"))]
    if let Some(message) = crate::stage0::no_python_support(&config, &manifest, learn) {
        fail(&emitter, dev.as_ref(), &message);
    }
    let mut routes: Vec<blastit_scheduler::RouteInfo> = match (&dev, learn) {
        (Some(_), true) => std::iter::once(blastit_scheduler::RouteInfo::new(
            crate::dev_learn::LEARN_FALLBACK_ROUTE,
            false,
            dev_uid,
        ))
        .chain(manifest.routes().iter().map(|route| {
            blastit_scheduler::RouteInfo::new(route.name(), route.prefork(), dev_uid)
                .with_ingress(route.ingress())
        }))
        .collect(),
        (Some(_), false) => manifest
            .routes()
            .iter()
            .map(|route| {
                blastit_scheduler::RouteInfo::new(route.name(), route.prefork(), dev_uid)
                    .with_ingress(route.ingress())
            })
            .collect(),
        (None, _) => routes_from_manifest(&manifest, &layout),
    };
    // §7.3: a view may keep `AF_INET`/`AF_INET6` only when its route says so;
    // the `--learn` fallback slot (`0`) never gets network egress.
    let egress: Vec<bool> = if dev.is_some() && learn {
        std::iter::once(false)
            .chain(manifest.routes().iter().map(|route| route.egress()))
            .collect()
    } else {
        manifest
            .routes()
            .iter()
            .map(|route| route.egress())
            .collect()
    };
    // `view-crates-architecture` §4.4 D1: the view child's identity and its
    // filter are decided **here**, where the boot mode exists, and travel into
    // the view crate as values. Nothing downstream could decide them: a child
    // runs after the `execve` barrier, where every `BLASTIT_*` is gone, and a
    // child-side choice would be forgeable. [`view_child_plans`] is that
    // decision, and its unit cases are the pin the design asks for.
    let plans = view_child_plans(&routes, &egress, dev.as_ref(), config.seccomp);
    // `MB.3.64`: the debug-only test seam. Stage 0 recorded `(st_dev, st_ino)`
    // for the pair and carried `BLASTIT_TEST_SWAP_POINT` in the configuration
    // document; this is the one point between that record and the re-open below,
    // so a `cargo test` boot pauses here until the test has renamed an object.
    // The rename is then inside the window by construction — the test cannot be
    // early, this process cannot be early — instead of by how fast the runner
    // happens to be. Unset (and, in a release build, absent) it does nothing.
    test_swap_pause(&emitter, dev.as_ref(), test_swap_point.as_deref());
    // `ADR-0010` §3: stage 1 re-opens the NTS `libphp` and the shim stage 0
    // inspected, asserts `(st_dev, st_ino)` against that record, renders the INI
    // lockdown once and holds the descriptors every PHP worker is forked with.
    // A file that moved between the two stages is refused here — before the
    // listener binds — never by a worker that loads something nobody inspected.
    // A host with no usable library never reaches stage 1: stage 0 refused it.
    //
    // `MB.36.5`: the two values travel as data (`PhpSlot`s), because a view
    // crate may not name this module's `Config` or the scheduler's `RouteInfo`
    // (`view-crates-architecture` §4.4 D2). The slots are keyed by **view
    // slot**, so a `--learn` boot's fallback slot 0 keeps its numbering.
    #[cfg(feature = "php")]
    let php_slots = match php_slots(&config, &manifest, &routes, &plans, dev.as_ref()) {
        Ok(slots) => slots,
        Err(message) => fail(&emitter, dev.as_ref(), &message),
    };
    #[cfg(feature = "php")]
    let php = match blastit_view_php::pinning::boot_bindings(
        &manifest,
        &php_slots,
        config.php.as_ref(),
        &config.manifest_dir,
        blastit_view_php::PhpBootLimits {
            request_timeout: config.request_timeout,
            max_requests: php_max_requests(&config),
            max_body_bytes: php_max_body_bytes(&config),
        },
    ) {
        Ok(bindings) => bindings,
        Err(message) => fail(&emitter, dev.as_ref(), &message),
    };
    // `ADR-0015` decision 1 and 9: the composition root writes the provider's
    // concurrency shape as boot data. An NTS `libphp` runs **one** request per
    // process (`max_inflight() == 1`), so a route whose binding slot is bound —
    // the same datum `spawn_now` reads — may own a pool and admits one request
    // per worker; the scheduler then forks one worker per concurrent request
    // under `BLASTIT_MAX_WORKERS` and the memory-derived cap. A route with no
    // binding (a WSGI view, or the `--learn` fallback slot) keeps the threaded
    // default. The slots are built from this same `routes`, so index alignment
    // is what the binding vector promises.
    //
    // `#[cfg(feature = "php")]`: a build without the feature has no `php`
    // vector and no PHP route can exist (`stage0::no_python_support`'s twin
    // gate refuses one).
    #[cfg(feature = "php")]
    for (index, route) in routes.iter_mut().enumerate() {
        if php.get(index).is_some_and(Option::is_some) {
            route.per_process_cap = Some(1); // the provider's max_inflight(), as data
            route.process_pool = true; // one request per process ⇒ a pool
        }
    }
    // `ADR-0010` §3: a contained probe child per `entrypoint php` file loads the
    // pair stage 0 inspected and starts the module **before** anything listens.
    // A shim that will not load, an ABI this build does not speak, a minor that
    // disagrees with the library, or a `worker_init` that hangs is this boot's
    // `exit 1` with §4.1's sentence — never a worker's first request. The
    // `dlopen` happens below the fork; no `libphp` symbol is reachable here.
    #[cfg(feature = "php")]
    match blastit_view_php::probe::probe_bindings(
        blastit_view_php::PhpProbeBoot {
            slots: &php_slots,
            bindings: &php,
            limits: blastit_sys::Rlimits {
                nproc: (dev.is_none()).then(|| {
                    config
                        .view_worker_cap()
                        .saturating_add(blastit_view_php::spawn::VIEW_PROCESS_MARGIN)
                        as u64
                }),
                nofile: Some(blastit_view_php::spawn::VIEW_NOFILE),
                core_zero: true,
            },
            libphp: config.php.as_ref(),
            timeout: php_probe_timeout,
            protocol: if hosted {
                blastit_view::spawn::ForkProtocol::InterpreterLive
            } else {
                blastit_view::spawn::ForkProtocol::NoInterpreter
            },
        },
        &|protocol| crate::forking::fork_for(protocol, hosted),
    ) {
        Ok(report) => emit_php_runtime(&emitter, &config, report),
        Err(message) => fail(&emitter, dev.as_ref(), &message),
    }
    let table = if dev.is_some() && learn {
        match learn_routing_table(&manifest) {
            Ok(table) => table,
            Err(errors) => fail(&emitter, dev.as_ref(), &errors),
        }
    } else {
        match RoutingTable::new(&manifest) {
            Ok(table) => table,
            Err(errors) => fail(&emitter, dev.as_ref(), &errors.to_string()),
        }
    };

    // `blastit dev` binds the address the operator asked for; production binds
    // every interface and is protected by the deployment around it (`R22`).
    // A reload's replacement image binds nothing: `http` already owns `:8080`
    // and adopted connections keep their byte stream (§4.6 step 7).
    let listener = match &adopted {
        Some(_) => None,
        None => {
            let bind = match &dev {
                Some(dev) => SocketAddr::from((dev.host(), config.port)),
                None => SocketAddr::from(([0, 0, 0, 0], config.port)),
            };
            match bind_tcp(bind, config.backlog) {
                Ok(listener) => Some(listener),
                Err(err) => fail(&emitter, dev.as_ref(), &format!("binding {bind}: {err}")),
            }
        }
    };

    // Channels (§3): ctl.chan is this image's own in both halves; `http.chan`
    // and the component pipes are created only by the half that forks `http`.
    let (ctl_end, sched_ctl_end) = pair_ok(&emitter, dev.as_ref(), "ctl.chan");
    let (sched_out_read, sched_out_write) = pipe_ok(&emitter, dev.as_ref(), "scheduler stdout");
    let (sched_err_read, sched_err_write) = pipe_ok(&emitter, dev.as_ref(), "scheduler stderr");
    let (sched_ev_read, sched_ev_write) = pipe_ok(&emitter, dev.as_ref(), "scheduler events");

    // The signals init supervises on have to be blocked before the children
    // exist, so every child inherits the mask and can put it back (§3).
    let signals = match SignalFd::new(&[Signal::CHILD, Signal::TERM, Signal::INT, Signal::HUP]) {
        Ok(signals) => signals,
        Err(err) => fail(&emitter, dev.as_ref(), &format!("signalfd: {err}")),
    };
    let mask = signals.previous_mask();

    let sched_cfg = scheduler_config(&config, &manifest, limits, memory, learn);

    // Production drops each component to its layout uid before it serves;
    // `dev` runs every role as the caller, so there is nobody to drop to and
    // `setresuid`/`setgroups` are never called (`R21`).
    let drop_scheduler = if dev.is_some() {
        None
    } else {
        Some(layout.scheduler())
    };

    let mut components: BTreeMap<i32, (Component, ComponentPipes)> = BTreeMap::new();
    // The scheduler's end of `http.chan`: created here, or adopted. A dev boot
    // with reload on **keeps** it after the fork, so `http`'s peer never sees
    // end-of-file when the cycle stops the scheduler (§4.6 step 5).
    let sched_http_end: OwnedFd = match &adopted {
        Some(adoption) => {
            let http = &adoption.http;
            components.insert(
                http.pid,
                (
                    Component::Http,
                    ComponentPipes {
                        // SAFETY: the adoption document named descriptors this
                        // image inherited with `FD_CLOEXEC` cleared; ownership
                        // is taken here once, as stage 0's own handover is.
                        events: unsafe { OwnedFd::from_raw_fd(http.events) },
                        stdout: unsafe { OwnedFd::from_raw_fd(http.stdout) },
                        stderr: unsafe { OwnedFd::from_raw_fd(http.stderr) },
                    },
                ),
            );
            if let Some(dbproxy) = &adoption.dbproxy {
                components.insert(
                    dbproxy.pid,
                    (
                        Component::Dbproxy,
                        ComponentPipes {
                            events: unsafe { OwnedFd::from_raw_fd(dbproxy.events) },
                            stdout: unsafe { OwnedFd::from_raw_fd(dbproxy.stdout) },
                            stderr: unsafe { OwnedFd::from_raw_fd(dbproxy.stderr) },
                        },
                    ),
                );
            }
            unsafe { OwnedFd::from_raw_fd(adoption.chan) }
        }
        None => {
            let listener = match &listener {
                Some(listener) => listener,
                None => fail(&emitter, dev.as_ref(), "http: no listener to bind"),
            };
            let (http_end, http_chan) = pair_ok(&emitter, dev.as_ref(), "http.chan");
            let (http_out_read, http_out_write) = pipe_ok(&emitter, dev.as_ref(), "http stdout");
            let (http_err_read, http_err_write) = pipe_ok(&emitter, dev.as_ref(), "http stderr");
            let (http_ev_read, http_ev_write) = pipe_ok(&emitter, dev.as_ref(), "http events");
            let drop_http = if dev.is_some() {
                None
            } else {
                Some(layout.http())
            };
            let http_pid = match fork_http(
                ChildIo {
                    stdout: Some(http_out_write),
                    stderr: Some(http_err_write),
                    keep: vec![
                        listener.as_raw_fd(),
                        http_end.as_raw_fd(),
                        http_ev_write.as_raw_fd(),
                    ],
                    limits: component_limits(&config),
                    working_dir: None,
                },
                HttpFds {
                    listener: listener.as_raw_fd(),
                    chan: http_end.as_raw_fd(),
                    events: http_ev_write.as_raw_fd(),
                },
                http_config(&config, limits),
                drop_http,
                mask,
                config.seccomp,
                hosted,
            ) {
                Ok(pid) => pid,
                Err(err) => fail(&emitter, dev.as_ref(), &format!("forking http: {err}")),
            };
            // The parent keeps only the read ends, and never the listening
            // socket.
            drop(http_end);
            drop(http_ev_write);
            components.insert(
                http_pid.as_raw(),
                (
                    Component::Http,
                    ComponentPipes {
                        events: http_ev_read,
                        stdout: http_out_read,
                        stderr: http_err_read,
                    },
                ),
            );
            http_chan
        }
    };
    drop(listener);

    let sched_pid = match fork_scheduler(
        ChildIo {
            stdout: Some(sched_out_write),
            stderr: Some(sched_err_write),
            keep: vec![
                sched_http_end.as_raw_fd(),
                sched_ctl_end.as_raw_fd(),
                sched_ev_write.as_raw_fd(),
            ],
            limits: component_limits(&config),
            working_dir: None,
        },
        SchedulerFds {
            chan: sched_http_end.as_raw_fd(),
            ctl: sched_ctl_end.as_raw_fd(),
            events: sched_ev_write.as_raw_fd(),
        },
        sched_cfg,
        routes.clone(),
        table,
        limits,
        drop_scheduler,
        mask,
        config.seccomp,
        hosted,
    ) {
        Ok(pid) => pid,
        Err(err) => fail(
            &emitter,
            dev.as_ref(),
            &format!("forking the scheduler: {err}"),
        ),
    };
    drop(sched_ctl_end);
    drop(sched_ev_write);

    components.insert(
        sched_pid.as_raw(),
        (
            Component::Scheduler,
            ComponentPipes {
                events: sched_ev_read,
                stdout: sched_out_read,
                stderr: sched_err_read,
            },
        ),
    );
    if adopted.is_none()
        && let (Some(pid), Some(events), Some(stdout), Some(stderr)) = (
            options.dbproxy_pid,
            options.dbproxy_events_fd,
            options.dbproxy_stdout_fd,
            options.dbproxy_stderr_fd,
        )
        && let Some(pid) = Pid::from_raw(pid)
    {
        components.insert(
            pid.as_raw(),
            (
                Component::Dbproxy,
                ComponentPipes {
                    // SAFETY: stage 0 cleared `FD_CLOEXEC` on exactly these
                    // descriptors; this process owns them.
                    events: unsafe { OwnedFd::from_raw_fd(events) },
                    stdout: unsafe { OwnedFd::from_raw_fd(stdout) },
                    stderr: unsafe { OwnedFd::from_raw_fd(stderr) },
                },
            ),
        );
    }
    // A dev boot with reload on keeps the scheduler's end of `http.chan` for
    // the cycle; every other boot closes it right after the fork, as before.
    let reload_chan = match adopted.is_some() || (dev.is_some() && options.dev_reload) {
        true => Some(sched_http_end),
        false => {
            drop(sched_http_end);
            None
        }
    };

    // `dev --learn`: drain dbproxy's observation pipe and attribute statements
    // to the route whose private socket served them (`R26`, `R27`). A reload
    // image adopts the descriptors the document names; the cycle never runs
    // under `--learn`, so this is the first image's path in practice.
    let learn_fds = match &adopted {
        Some(adoption) => adoption.learn,
        None => options.dev_obs_fd.map(|obs| (obs, options.dev_ctl_fd)),
    };
    let learn = match learn_fds {
        Some((fd, ctl)) => match crate::dev_learn::Learn::adopt(fd, ctl) {
            Ok(learn) => Some(learn),
            Err(err) => fail(
                &emitter,
                dev.as_ref(),
                &format!("blastit dev --learn: {err}"),
            ),
        },
        None => None,
    };

    // A replacement image owns no listener of its own, so a route the new
    // manifest just added needs its private directory created here: stage 0
    // made the ones the first boot knew about, and `:8080` is `http`'s.
    if adopted.is_some()
        && let Some(dev) = dev.as_ref()
    {
        for index in 0..routes.len() {
            let dir = dev.view_dir(index);
            if let Err(err) = std::fs::create_dir_all(&dir) {
                fail(&emitter, Some(dev), &format!("view directory: {err}"));
            }
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
    }

    // What a reload cycle re-execs with (§4.6 step 6). Every other boot carries
    // `None` and can never cycle: production, `check`, `--no-reload` and a build
    // without an interpreter.
    let reload_source = match (dev.as_ref(), options.dev_reload) {
        #[cfg(feature = "python")]
        (Some(dev), true) => {
            match stage1_program(restart.as_deref())
                .and_then(|program| dev_root_from_cmdline().map(|dev_root| (program, dev_root)))
            {
                Ok((program, dev_root)) => Some(crate::supervise::ReloadSource {
                    config: config_bytes,
                    program,
                    dev_root,
                    dev_host: dev.host().to_string(),
                }),
                Err(message) => fail(&emitter, Some(dev), &message),
            }
        }
        _ => None,
    };

    // Everything from here on needs an interpreter, and the forks above were the
    // last thing that had to happen while init is single-threaded (§2.1).
    let serving = Serving {
        config,
        limits,
        routes,
        #[cfg(feature = "php")]
        php,
        declared_routes,
        egress,
        plans,
        memory,
        ctl: ctl_end,
        signals,
        mask,
        components,
        #[cfg(feature = "python")]
        hosted: interpreter.is_hosted(),
        dev,
        learn,
        dev_reload: options.dev_reload,
        chan: reload_chan,
        policy: match (adopted.as_ref(), options.dev_policy_fd) {
            (Some(adoption), _) => adoption
                .policy
                .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) }),
            (None, Some(fd)) => Some(unsafe { OwnedFd::from_raw_fd(fd) }),
            (None, None) => None,
        },
        learn_fds,
        reload_source,
        // §4.8: the `(route, proxy)` pairs *this image's* manifest declares, so
        // a save that adds or removes a store is one warning naming the restart
        // instead of a silent refusal. A replacement image boots with the new
        // manifest, which is exactly why a change is warned on the save that
        // makes it (the first image) and not again on every later save.
        boot_pairs: crate::supervise::policy_pairs(&manifest)
            .into_iter()
            .collect(),
        announce: adopted.is_none(),
        reload_fault,
        reload_since: options.dev_reload_since.and_then(|nanos| {
            u64::try_from(nanos)
                .ok()
                .map(|nanos| std::time::UNIX_EPOCH + std::time::Duration::from_nanos(nanos))
        }),
    };
    // Everything from here on needs an interpreter for a WSGI view, and the
    // forks above were the last thing that had to happen while init was
    // single-threaded (§2.1). A build with no interpreter supervises exactly
    // the roles it forked: the boot gate refused every route that would need a
    // view, so the scheduler asks for none (`MB.3.11`).
    #[cfg(feature = "python")]
    match interpreter {
        Interpreter::Embed => {
            Python::initialize();
            Python::attach(|py| serve(py, &emitter, serving))
        }
        Interpreter::Hosted(py) => serve(py, &emitter, serving),
    }
    #[cfg(not(feature = "python"))]
    {
        // There is no interpreter to be hosted by: a `python`-less build can
        // only ever be handed `Embed`, and the variant carries nothing to use.
        let _ = interpreter;
        serve_without_interpreter(&emitter, serving)
    }
}

/// Supervises the forked roles in a build that carries no interpreter
/// (`MB.3.11`). Never returns.
///
/// Nothing here can start a view: [`crate::stage0::no_python_support`] refused
/// every route that would need one before a listener was bound, so the
/// scheduler only holds `static` mount policies. A `SPAWN` on `ctl.chan`
/// would therefore be a protocol violation, and [`Supervisor`] fails the boot on
/// it rather than ignoring it.
#[cfg(not(feature = "python"))]
fn serve_without_interpreter(emitter: &Emitter, serving: Serving) -> ! {
    emit_ready(emitter, &serving.config, serving.declared_routes);
    let mut supervisor = Supervisor::new(
        Parts {
            cfg: serving.config,
            limits: serving.limits,
            routes: serving.routes,
            #[cfg(feature = "php")]
            php: serving.php,
            egress: serving.egress,
            plans: serving.plans,
            ctl: serving.ctl,
            signals: serving.signals,
            mask: serving.mask,
            components: serving.components,
            dev: serving.dev,
            learn: serving.learn,
            // A build with no interpreter forks no view and imports no app, so
            // it has no report to watch (§4.4).
            reload: None,
            reload_source: None,
            chan: None,
            policy: None,
            learn_fds: None,
            boot_pairs: serving.boot_pairs,
        },
        emitter,
    );
    supervisor.run(crate::py::no_interpreter())
}

/// Everything stage 1 needs once an interpreter is available (§4).
struct Serving {
    /// The configuration, already parsed.
    config: Config,
    /// Frame limits of every channel.
    limits: Limits,
    /// One entry per view slot, in scheduler routing order.
    routes: Vec<blastit_scheduler::RouteInfo>,
    /// One entry per view slot: the embedded-PHP worker it forks, or `None`
    /// for a WSGI view (`ADR-0010` §1). Built and verified at boot.
    #[cfg(feature = "php")]
    php: Vec<Option<blastit_view_php::PhpBinding>>,
    /// Number of views declared in the `Blastfile` (reported on `ready`).
    declared_routes: usize,
    /// Whether each route may keep `AF_INET`/`AF_INET6`, by route index (§7.3).
    /// Only a build that forks a view reads it (`MB.3.11`).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    egress: Vec<bool>,
    /// The identity and filter each route's view child keeps
    /// (`view-crates-architecture` §4.4 D1), decided above where the boot mode
    /// exists; a build without the `python` feature still lays them out, and
    /// only the WSGI fork reads them (`fork_for` and the fork it feeds).
    plans: ViewChildPlans,
    /// The memory reading taken before the fork, for §8 Sizing. Reported by the
    /// preload event, which only a build with an interpreter emits.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    memory: Option<mem_usage::Usage>,
    /// init's end of `ctl.chan`.
    ctl: OwnedFd,
    /// The signalfd init listens on.
    signals: SignalFd,
    /// The mask to put back in every child.
    mask: SignalMask,
    /// The components to supervise, by pid.
    components: BTreeMap<i32, (Component, ComponentPipes)>,
    /// Whether the interpreter was started by the host rather than embedded
    /// here. Only a build with one can ask the question.
    #[cfg(feature = "python")]
    hosted: bool,
    /// The `blastit dev` runtime root, when this is a dev boot (`R20`).
    dev: Option<crate::dev_learn::Dev>,
    /// The `dev --learn` observation and attribution channels (`R29`).
    learn: Option<crate::dev_learn::Learn>,
    /// Whether this `dev` boot wants live reload (`--dev-reload=on`, the
    /// default). `false` under `--no-reload`: no watcher, no marks, no report
    /// socket, no audit hook (§4.1, `R1`). Only a build with an interpreter
    /// builds one.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    dev_reload: bool,
    /// The scheduler's end of `http.chan`, kept for a dev boot with reload on
    /// (and for a replacement image that adopted it) so the cycle can hand it
    /// to the new scheduler without `http` losing its peer (§4.6 step 5).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    chan: Option<OwnedFd>,
    /// This image's end of the dev-only dbproxy policy socket.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    policy: Option<OwnedFd>,
    /// The `--learn` descriptor numbers, for the next adoption document.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    learn_fds: Option<(RawFd, Option<RawFd>)>,
    /// What a reload cycle re-execs with; `None` on every boot that cannot
    /// cycle.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    reload_source: Option<crate::supervise::ReloadSource>,
    /// Whether this image is the first half of a run (`true`) or the replacement
    /// half of a reload (`false`, which repeats neither the boot line nor
    /// `ready`).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    announce: bool,
    /// The instant the cycle that started this image began, for the one-shot
    /// post-boot `Policy::reconcile` (§4.6 step 7).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    reload_since: Option<std::time::SystemTime>,
    /// The debug-only reload fault seam stage 0 carried across the `execve`
    /// (`BLASTIT_TEST_RELOAD_FAULT`, compiled out of release builds).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    reload_fault: Option<String>,
    /// The `(route, proxy)` pairs this image's manifest declares, which are the
    /// pairs dbproxy's listeners were bound for (§4.8).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    boot_pairs: std::collections::BTreeSet<(String, String)>,
}

/// What the post-preload marking pass owes the boot line (§4.7, `T32`).
///
/// `reload: on` is a claim, and only a pass that actually marked may make it: a
/// watcher that latched `ENOSPC`/the cap already emitted the ladder's one
/// warning, and one that latched dead must print nothing at all.
#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
enum BootLine {
    /// `reload: on (N directories)`, with the marks the pass placed.
    Announce(usize),
    /// No line: the boot already owed its one warning.
    Silent,
    /// The ladder's one `dev.reload.unavailable`, then no line.
    Unavailable(Unavailable),
    /// A watcher that latched dead: the ladder's one warning, no line, and the
    /// `Reload` is dropped (`reload = None`).
    Stopped(Unavailable),
}

/// Turns one marking outcome into what the boot does about the boot line.
#[cfg(feature = "python")]
fn after_preload_marks(outcome: MarkOutcome, marking_stopped: bool, dirs: usize) -> BootLine {
    match outcome {
        MarkOutcome::Marked if !marking_stopped => BootLine::Announce(dirs),
        MarkOutcome::Marked => BootLine::Silent,
        MarkOutcome::Exhausted(why) => BootLine::Unavailable(why),
        MarkOutcome::Dead(why) => BootLine::Stopped(why),
    }
}

/// Loads the application, emits `ready`, and supervises. Never returns.
#[cfg(feature = "python")]
fn serve(py: Python<'_>, emitter: &Emitter, serving: Serving) -> ! {
    // `MB.23.5`: the one live-reload watcher of this boot, created **before** the
    // first import so the `preload` phase's modules and file opens are watched
    // too (§4.1 boot order). A production boot, `check` and a `dev` boot with
    // `--no-reload` never reach it: reload is a `dev`-only path, and "off" means
    // no watcher group, no mark, no report socket and no audit hook at all —
    // `--no-reload` is not a watcher that ignores events (§1, `R1`).
    let mut reload = match serving.dev.is_some() && serving.dev_reload {
        true => {
            crate::supervise::boot_reload(&serving.config, emitter, serving.reload_fault.as_deref())
        }
        false => None,
    };
    // The `preload` phase reports on source `0xffff`; its paths are `pre_fork`,
    // so a change to one takes the reload cycle (§4.4). This socket is also the
    // **gate** for the loaded-file hook: no socket, no hook, no marks (`S3`).
    let preload = match reload.as_mut() {
        Some(reload) => match crate::supervise::open_preload_report(reload) {
            Ok(reporter) => Some(reporter),
            Err(err) => {
                emitter.emit(&Event::now(
                    Component::Init,
                    EventKind::ManifestWarning(ManifestWarning::new(format!(
                        "live reload: the preload report socket: {err}"
                    ))),
                ));
                None
            }
        },
        None => None,
    };
    let app = load(
        py,
        &serving.config,
        serving.declared_routes,
        emitter,
        serving.memory,
        serving.hosted,
        Boot {
            dev: serving.dev.as_ref(),
            report: preload,
            announce: serving.announce,
        },
    );
    // Boot order step 5: the parents the preload batch just added are marked,
    // then the one boot line — pretty stdout, never an event (§4.7, §4.9). A
    // marking pass that ran out of room (`ENOSPC`, or the marks cap) emits the
    // ladder's one warning and prints no boot line, and a watcher that latched
    // dead drops the reload entirely: neither may claim `reload: on`.
    //
    // The replacement half of a reload repeats neither the line nor `ready`:
    // both were the first image's, and claiming a boot that did not happen is
    // exactly what the observable contract must not do (§4.6 step 7).
    if let Some(active) = reload.as_mut() {
        let outcome = active.sync_marks();
        let stopped = active.marking_stopped();
        let dirs = active.marked_dirs();
        match after_preload_marks(outcome, stopped, dirs) {
            // `reload: on` is a claim that marking succeeded: a policy that
            // latched `ENOSPC`/the cap in `boot` already emitted its one
            // warning, and printing the line over it would be the lie the
            // ladder exists to prevent (§4.7, `T32`).
            BootLine::Announce(dirs) if serving.announce => {
                crate::supervise::announce_reload(serving.config.log_format, dirs);
            }
            BootLine::Announce(_) => {}
            BootLine::Silent => {}
            BootLine::Unavailable(why) => crate::supervise::reload_unavailable(emitter, &why),
            BootLine::Stopped(why) => {
                crate::supervise::reload_stopped(emitter, &why);
                reload = None;
            }
        }
        // §4.6 step 7: one `stat` per union path, once, so a save that landed
        // while no watcher existed is not lost. Never re-armed by a future
        // mtime.
        if let Some(since) = serving.reload_since
            && let Some(active) = reload.as_mut()
        {
            let now = active.now();
            active.policy().reconcile(since, now);
        }
    }
    if serving.announce {
        emit_ready(emitter, &serving.config, serving.declared_routes);
    }
    let mut supervisor = Supervisor::new(
        Parts {
            cfg: serving.config,
            limits: serving.limits,
            routes: serving.routes,
            #[cfg(feature = "php")]
            php: serving.php,
            egress: serving.egress,
            plans: serving.plans,
            app,
            ctl: serving.ctl,
            signals: serving.signals,
            mask: serving.mask,
            components: serving.components,
            dev: serving.dev,
            learn: serving.learn,
            reload,
            reload_source: serving.reload_source,
            chan: serving.chan,
            policy: serving.policy,
            learn_fds: serving.learn_fds,
            boot_pairs: serving.boot_pairs,
        },
        emitter,
    );
    supervisor.run(py)
}

/// Builds the routing table for `dev --learn`, reserving slot `0` for the
/// scheduler's unrouted-request fallback (`R26`, `R27`).
///
/// Slot `0` carries no pattern — [`RoutingTable::route`] can never select it —
/// and declared routes `0..N` are their manifest entries, shifted to `1..=N`
/// in exact manifest order with their original methods and patterns intact.
/// The table is built from the model [`RoutingTable::for_learn`] already holds:
/// no document is rendered and re-parsed.
fn learn_routing_table(manifest: &Manifest) -> Result<RoutingTable, String> {
    RoutingTable::for_learn(manifest).map_err(|errors| errors.to_string())
}

/// Reads the configuration document stage 0 wrote into the pipe (§4), the
/// `entrypoint php` probe timeout that travelled with it (`ADR-0010` §3), and —
/// in a debug build — the test-only swap point (`MB.3.64`).
///
/// The timeout is a **stage-0 decision**: `BLASTIT_PROBE_TIMEOUT_MS` is read
/// before the `execve` that swaps this process's environment, because
/// `blastit-env` strips every `BLASTIT_*` variable (`R12`), so stage 1 finds it
/// in the document and nowhere else. The swap point crosses the same barrier for
/// the same reason, and only a `debug_assertions` build has the member at all.
/// What stage 0 handed this image on the configuration pipe.
#[derive(Debug)]
struct BootConfig {
    /// The parsed configuration.
    config: Config,
    /// The `entrypoint php` probe's patience (`ADR-0010` §3).
    probe_timeout: Duration,
    /// The debug-only `BLASTIT_TEST_SWAP_POINT` rendezvous (`MB.3.64`).
    swap_point: Option<PathBuf>,
    /// The exact document bytes, re-supplied through a fresh pipe by a reload
    /// cycle (§4.6 step 6).
    bytes: Vec<u8>,
    /// Stage 0's stage-1 program prefix, for the cycle.
    restart: Option<Vec<String>>,
    /// The debug-only reload fault seam (`MB.23.5.2.2`).
    reload_fault: Option<String>,
}

fn read_config(emitter: &Emitter, dev: Option<&crate::dev_learn::Dev>, fd: RawFd) -> BootConfig {
    // SAFETY: stage 0 cleared `FD_CLOEXEC` on this descriptor and stage 1 is its
    // only owner.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    if let Err(err) = file.read_to_end(&mut bytes) {
        fail(emitter, dev, &format!("configuration pipe: {err}"));
    }
    drop(file);
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => fail(emitter, dev, &format!("configuration document: {err}")),
    };
    let probe_timeout = value
        .get(crate::stage0::PROBE_TIMEOUT_MEMBER)
        .and_then(serde_json::Value::as_u64)
        .map_or(crate::stage0::PROBE_TIMEOUT_DEFAULT, Duration::from_millis);
    #[cfg(debug_assertions)]
    let swap_point = value
        .get(crate::stage0::TEST_SWAP_POINT_MEMBER)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    #[cfg(not(debug_assertions))]
    let swap_point: Option<PathBuf> = None;
    // Stage 0's handover of the stage-1 program prefix (§4.6 step 6). Kept in
    // the configuration document, never in the environment: a `BLASTIT_*`
    // variable would be inherited by views, and this token must not be.
    let restart = value
        .get(crate::stage0::RESTART_MEMBER)
        .and_then(serde_json::Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .filter(|argv| !argv.is_empty());
    // `MB.23.5.2.2`: the debug-only reload fault seam, read back where the swap
    // point is. A release build has neither the member's name nor this read.
    #[cfg(debug_assertions)]
    let reload_fault = value
        .get(crate::stage0::TEST_RELOAD_FAULT_MEMBER)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    #[cfg(not(debug_assertions))]
    let reload_fault: Option<String> = None;
    match Config::from_json(&value) {
        Ok(config) => BootConfig {
            config,
            probe_timeout,
            swap_point,
            bytes,
            restart,
            reload_fault,
        },
        Err(err) => fail(emitter, dev, &err.to_string()),
    }
}

/// One component a reload's replacement image adopts instead of forking: its
/// pid and the read ends of its three pipes (§4.6 step 6).
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdoptedComponent {
    pub(crate) pid: i32,
    pub(crate) stdout: RawFd,
    pub(crate) stderr: RawFd,
    pub(crate) events: RawFd,
}

/// The adoption document a reload's replacement image is started with.
///
/// UTF-8 JSON, at most 4096 bytes. `learn` is present exactly when the first
/// boot had `--learn`; `policy` exactly when it had a dev policy socket. An
/// unknown key, a missing key or a descriptor this image does not hold refuses
/// the boot: an adoption that half-succeeds would bind nothing and fork `http`
/// twice.
#[derive(Debug)]
pub(crate) struct Adoption {
    pub(crate) http: AdoptedComponent,
    pub(crate) dbproxy: Option<AdoptedComponent>,
    pub(crate) chan: RawFd,
    pub(crate) learn: Option<(RawFd, Option<RawFd>)>,
    pub(crate) policy: Option<RawFd>,
}

impl Adoption {
    /// Reads and validates the document on `fd`.
    ///
    /// # Errors
    ///
    /// One sentence for the boot: a size, a JSON, a shape or an open-descriptor
    /// refusal.
    pub(crate) fn read(fd: RawFd) -> Result<Self, String> {
        // SAFETY: `exec_cycle` cleared `FD_CLOEXEC` on exactly this descriptor
        // and handed it over; this image is its only owner.
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|err| format!("adoption document: {err}"))?;
        drop(file);
        if bytes.len() > MAX_ADOPTION_DOCUMENT {
            return Err(format!(
                "the adoption document is {} bytes, more than the {MAX_ADOPTION_DOCUMENT}-byte limit",
                bytes.len()
            ));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|err| format!("adoption document: {err}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| "adoption document: the document is not an object".to_string())?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "version" | "http" | "dbproxy" | "chan" | "learn" | "policy"
            ) {
                return Err(format!("adoption document: unknown key `{key}`"));
            }
        }
        if object.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err("adoption document: `version` must be 1".to_string());
        }
        let component = |key: &str| -> Result<AdoptedComponent, String> {
            let value = object
                .get(key)
                .ok_or_else(|| format!("adoption document: missing `{key}`"))?;
            let component = AdoptedComponent {
                pid: member(value, key, "pid")?,
                stdout: member(value, key, "stdout")?,
                stderr: member(value, key, "stderr")?,
                events: member(value, key, "events")?,
            };
            for fd in [component.stdout, component.stderr, component.events] {
                open(fd).map_err(|err| format!("adoption document: `{key}`: {err}"))?;
            }
            Ok(component)
        };
        let http = component("http")?;
        let dbproxy = match object.contains_key("dbproxy") {
            true => Some(component("dbproxy")?),
            false => None,
        };
        let chan: RawFd = object
            .get("chan")
            .ok_or_else(|| "adoption document: missing `chan`".to_string())
            .and_then(|value| number(value, "chan"))?;
        open(chan).map_err(|err| format!("adoption document: `chan`: {err}"))?;
        let learn = match object.get("learn") {
            Some(value) => {
                let obs = member(value, "learn", "obs")?;
                open(obs).map_err(|err| format!("adoption document: `learn.obs`: {err}"))?;
                let ctl = match value.get("ctl") {
                    Some(ctl) => {
                        let ctl = number(ctl, "learn.ctl")?;
                        open(ctl)
                            .map_err(|err| format!("adoption document: `learn.ctl`: {err}"))?;
                        Some(ctl)
                    }
                    None => None,
                };
                Some((obs, ctl))
            }
            None => None,
        };
        let policy = match object.get("policy") {
            Some(value) => {
                let fd = number(value, "policy")?;
                open(fd).map_err(|err| format!("adoption document: `policy`: {err}"))?;
                Some(fd)
            }
            None => None,
        };
        Ok(Self {
            http,
            dbproxy,
            chan,
            learn,
            policy,
        })
    }
}

/// One integer member of a component object.
fn member(value: &serde_json::Value, key: &str, name: &str) -> Result<RawFd, String> {
    value
        .get(name)
        .ok_or_else(|| format!("adoption document: `{key}` has no `{name}`"))
        .and_then(|value| number(value, name))
}

/// One descriptor number from a JSON value.
fn number(value: &serde_json::Value, name: &str) -> Result<RawFd, String> {
    let raw = value
        .as_u64()
        .filter(|raw| i32::try_from(*raw).is_ok())
        .ok_or_else(|| format!("adoption document: `{name}` is not a descriptor number"))?;
    i32::try_from(raw).map_err(|_| format!("adoption document: `{name}` is not a descriptor"))
}

/// Whether this image really holds `fd`.
fn open(fd: RawFd) -> Result<(), String> {
    // SAFETY: `F_GETFD` reads descriptor flags and takes no pointer.
    match unsafe { libc::fcntl(fd, libc::F_GETFD) } {
        -1 => Err(format!("descriptor {fd} is not open")),
        _ => Ok(()),
    }
}

/// The stage-1 program prefix a reload cycle re-execs (§4.6 step 6).
///
/// Stage 0's handover when the configuration carried one, else
/// `/proc/self/cmdline` up to the first `--stage=1` token — never
/// `/proc/self/exe`, which for a wheel is the interpreter. At most eight
/// tokens, and never an empty prefix.
#[cfg(feature = "python")]
fn stage1_program(restart: Option<&[String]>) -> Result<Vec<std::ffi::CString>, String> {
    let argv: Vec<String> = match restart {
        Some(argv) => argv.to_vec(),
        None => {
            let raw = std::fs::read("/proc/self/cmdline")
                .map_err(|err| format!("reload: reading /proc/self/cmdline: {err}"))?;
            raw.split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .take_while(|token| !token.starts_with("--stage=1"))
                .collect()
        }
    };
    if argv.is_empty() || argv.len() > 8 {
        return Err(format!(
            "reload: the stage-1 program prefix has {} token(s), not 1..=8",
            argv.len()
        ));
    }
    argv.iter()
        .map(|token| {
            std::ffi::CString::new(token.as_str())
                .map_err(|_| "reload: the stage-1 program prefix contains a NUL byte".to_string())
        })
        .collect()
}

/// The `--dev-root` this image was started with, from its own argv.
#[cfg(feature = "python")]
///
/// The runtime root is stage 0's, and `crate::dev_learn::Dev` keeps it private;
/// the replacement image needs the same absolute path to re-exec itself with.
fn dev_root_from_cmdline() -> Result<std::path::PathBuf, String> {
    let raw = std::fs::read("/proc/self/cmdline")
        .map_err(|err| format!("reload: reading /proc/self/cmdline: {err}"))?;
    raw.split(|byte| *byte == 0)
        .filter_map(|part| std::str::from_utf8(part).ok())
        .find_map(|token| token.strip_prefix("--dev-root="))
        .filter(|root| !root.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "reload: this image has no `--dev-root` in its argv".to_string())
}

/// The adoption document's size bound (§4.6 step 6).
const MAX_ADOPTION_DOCUMENT: usize = 4096;

/// `MB.3.64`: pauses stage 1 at the test-only rendezvous, when — and only when —
/// a `debug_assertions` build with the `php` capability was handed one.
///
/// Stage 0 read `BLASTIT_TEST_SWAP_POINT` before the barrier and carried the
/// path in the configuration document; the wait itself
/// ([`blastit_view_php::pinning::await_test_swap`]) sits between stage 0's inode record
/// and the re-open `boot_bindings` performs, so a rename the test makes while
/// the boot is paused is *in* the window by construction.
///
/// A failure to create the rendezvous, or to be released from it, is this boot's
/// refusal through [`fail`] (`exit 1`, before any listener) — never a hang.
#[cfg(all(debug_assertions, feature = "php"))]
fn test_swap_pause(emitter: &Emitter, dev: Option<&crate::dev_learn::Dev>, point: Option<&Path>) {
    if let Some(point) = point
        && let Err(message) = blastit_view_php::pinning::await_test_swap(point)
    {
        fail(emitter, dev, &message);
    }
}

/// The other two configurations — a release build, or one without the `php`
/// capability — have no seam to pause at: their whole body is this.
#[cfg(not(all(debug_assertions, feature = "php")))]
fn test_swap_pause(
    _emitter: &Emitter,
    _dev: Option<&crate::dev_learn::Dev>,
    _point: Option<&Path>,
) {
}

/// The front end's configuration (§8.2).
fn http_config(config: &Config, limits: Limits) -> HttpConfig {
    HttpConfig {
        max_header_bytes: config.max_header_bytes,
        max_body_bytes: config.max_body_bytes,
        header_timeout: config.header_timeout,
        keepalive_timeout: config.keepalive_timeout,
        client_ip: config.client_ip(),
        frame_limits: limits,
        server_port: config.server_port,
        max_connections: config.max_connections,
        ..HttpConfig::default()
    }
}

/// The scheduler's configuration (§8.2).
///
/// `memory` is the reading taken before the fork: §8 Sizing's watermark is a
/// percentage of `total`, which does not change, and the *usage* is what the
/// scheduler watches for itself on its timer (§9.2 #8: only aggregates are
/// reliable under gVisor).
fn scheduler_config(
    config: &Config,
    manifest: &blastit_manifest::Manifest,
    limits: Limits,
    memory: Option<mem_usage::Usage>,
    learn: bool,
) -> SchedulerConfig {
    let watermark = memory
        .map(|usage| (usage.total() as f64 * config.mem_high_watermark_percent / 100.0) as u64);
    SchedulerConfig {
        // `dev --learn`: an unrouted request is served by the first route's
        // view instead of `404` (`R26`).
        learn_mode: learn,
        max_workers: config.max_workers,
        view_max_workers: config.view_max_workers,
        queue_size: config.queue_size,
        queue_timeout: config.queue_timeout,
        fair_queuing: config.fair_queuing,
        fair: config.fair_config(),
        request_timeout: config.request_timeout,
        stuck_threads: config.stuck_threads,
        eager_spawn: config.view_spawn == crate::config::SpawnMode::Eager,
        idle_timeout: config.view_idle_timeout,
        max_requests: config.view_max_requests,
        max_view_processes: config.max_view_processes,
        health_path: config.health_path.clone(),
        script_name: config.script_name.clone(),
        frame_limits: limits,
        memory_usage: memory.map(|_| MemoryProbe::new(current_usage)),
        mem_high_watermark_bytes: watermark,
        mem_total_bytes: memory.map(|usage| usage.total()),
        view_mem_estimate: config.view_mem_estimate,
        // `R2`, `R13`: the scheduler opens every `static` mount's root beneath
        // the manifest's directory, and the one file ceiling this release has
        // comes from `STATIC_MAX_FILE_BYTES` alone. Both come from stage 0's
        // configuration, so stage 1 never re-derives a path from its own
        // working directory.
        manifest_dir: config.manifest_dir.clone(),
        // A build without the responder cannot resolve that constant, and needs
        // no ceiling: it serves no file and `run` refuses any mount at boot
        // (`SchedulerError::StaticNotCompiled`). The update syntax below then
        // supplies `SchedulerConfig::default()`'s own feature-less value, so the
        // ceiling is still spelled once (design §4.6).
        #[cfg(feature = "static")]
        static_max_bytes: blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES,
        // §9.2: the file's mounts travel beside the route table, because a
        // mount owns no uid and starts no process.
        mounts: manifest.mounts().to_vec(),
        // `SPAWN_BACKOFF` and `MEMORY_CHECK_INTERVAL` are the struct defaults,
        // so a field a later release adds needs no edit here.
        ..SchedulerConfig::default()
    }
}

/// The machine's aggregate memory usage in bytes, or `None` when it cannot be
/// read (§9.2 #8).
///
/// This is the number the scheduler compares with `BLASTIT_MEM_HIGH_WATERMARK`
/// and derives the view cap from, so it is [`mem_usage`]'s: the cgroup's
/// `memory.current` less the page cache and slab the kernel reclaims on
/// demand, because a warm filesystem cache is not pressure.
fn current_usage() -> Option<u64> {
    crate::mem_usage::read(Path::new("/")).map(|usage| usage.used())
}

/// The machine's memory, or a warning that §8 Sizing could not read it.
///
/// Unreadable memory leaves the view cap alone rather than guessing (§8), so
/// this is reported once and the caller carries on with `None`.
fn read_memory(emitter: &Emitter) -> Option<mem_usage::Usage> {
    match mem_usage::sample(Path::new("/")) {
        Ok(usage) => Some(usage),
        Err(err) => {
            emitter.emit(
                &output_event(
                    Component::Init,
                    None,
                    Stream::Stderr,
                    &format!("memory: {err}: no view process cap is derived from memory (§8)"),
                )
                .with_level(Level::Warn),
            );
            None
        }
    }
}

/// Forks the http process, which drops to `uid` and serves (§4).
///
/// `hosted` runs the fork under CPython's protocol when an interpreter is live
/// in this image (§9.3, [`crate::forking`]).
fn fork_http(
    child: ChildIo,
    fds: HttpFds,
    cfg: HttpConfig,
    uid: Option<u32>,
    mask: SignalMask,
    seccomp: bool,
    hosted: bool,
) -> std::io::Result<Pid> {
    match crate::forking::fork(hosted)? {
        Fork::Parent(pid) => Ok(pid),
        Fork::Child => {
            if let Err(message) = prepare_child(child, uid, mask, SeccompRole::Http, seccomp) {
                child_exit(&message, 1);
            }
            let code = crate::roles::http(fds, cfg);
            child_exit("", code)
        }
    }
}

/// Forks the scheduler process, which drops to `uid` and routes (§4).
// Rationale: one-shot child handover carrying owned table/routes/limits into the fork.
#[allow(clippy::too_many_arguments)]
fn fork_scheduler(
    child: ChildIo,
    fds: SchedulerFds,
    cfg: SchedulerConfig,
    routes: Vec<blastit_scheduler::RouteInfo>,
    table: RoutingTable,
    limits: Limits,
    uid: Option<u32>,
    mask: SignalMask,
    seccomp: bool,
    hosted: bool,
) -> std::io::Result<Pid> {
    match crate::forking::fork(hosted)? {
        Fork::Parent(pid) => Ok(pid),
        Fork::Child => {
            if let Err(message) = prepare_child(child, uid, mask, SeccompRole::Scheduler, seccomp) {
                child_exit(&message, 1);
            }
            let code = crate::roles::scheduler(fds, cfg, routes, table, limits);
            child_exit("", code)
        }
    }
}

/// The child sequence http and the scheduler share: descriptors, privileges,
/// hardening, the seccomp filter, the signal mask (§4, §7.3).
///
/// # Errors
///
/// A message for the log when one of those steps fails; there is no recovery in
/// a child.
fn prepare_child(
    child: ChildIo,
    uid: Option<u32>,
    mask: SignalMask,
    role: SeccompRole,
    seccomp: bool,
) -> Result<(), String> {
    let name = role.name();
    child.prepare().map_err(|err| format!("{name}: {err}"))?;
    // `None` is `blastit dev`: every role already runs as the caller, and
    // `setresuid`/`setgroups` are exactly what the unprivileged simulation may
    // not do (`R21`).
    if let Some(uid) = uid {
        drop_privileges(uid).map_err(|err| format!("{name}: {err}"))?;
    }
    harden_child(Signal::KILL)
        .map_err(|err| format!("{name}: {err}"))
        .and_then(|()| install_filter(seccomp, role).map_err(|err| format!("{name}: {err}")))?;
    let _ = set_dumpable(false);
    restore_signal_mask_in_child(mask).map_err(|err| format!("{name}: {err}"))
}

/// What a `dev` boot hands [`load`] beyond the configuration: the runtime root
/// its failure paths clean up, and the `preload` phase's report socket.
///
/// The socket is the loaded-file hook's gate (§5 `S3`): a boot that has none —
/// production, `check`, and a `dev` boot with reload off — installs no hook.
#[cfg(feature = "python")]
struct Boot<'a> {
    /// The `blastit dev` runtime root, when this is a dev boot (`R20`).
    dev: Option<&'a crate::dev_learn::Dev>,
    /// The `preload` phase's report socket, when live reload is on (`ADR-0014`).
    report: Option<blastit_view::Reporter>,
    /// Whether this image is the first half of a run. A reload's replacement
    /// image repeats neither the `boot` event nor `ready` (§4.6 step 7).
    announce: bool,
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
fn load(
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
    if hosted {
        if let Err(err) = restore_sys_path(py, &config.sys_path) {
            python_fail(py, emitter, dev, "sys.path", &err);
        }
    } else {
        // §9.3: an embedded interpreter does not find the distribution's
        // site-packages on its own. Best effort — a system without sysconfig is
        // not a reason to refuse to start.
        let _ = py.run(SITE_PACKAGES, Some(&scope), None);
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

/// §7.2: refuses a boot whose interpreter or code a view uid could not read.
///
/// `app` is `BLASTIT_APP`; its module names the directory the application's own
/// code comes from, which a view imports through as much as it imports the
/// standard library. Called *before* the application is imported: this is the
/// check that has to happen while a failure is a boot failure, because the
/// imports it stands for happen after the fork, inside one view, long after
/// everything looked healthy.
///
/// A module that cannot be found at all is skipped: the import right after says
/// so, with the name of the variable an operator has to fix.
#[cfg(feature = "python")]
fn check_view_paths(
    py: Python<'_>,
    config: &Config,
    routes: usize,
    scope: &Bound<'_, PyDict>,
    emitter: &Emitter,
) {
    let layout = UidLayout::new(config.uid_base);
    let dropped = crate::visibility::DroppedTo::of(layout, routes);
    let module = config.app.as_deref().unwrap_or_default();
    let module = module.split(':').next().unwrap_or_default();
    // The snippet defines a function, so its names have to live in a *globals*
    // mapping rather than in a locals one: Python resolves a function's globals,
    // not its caller's locals. The dict is this call's own, and nothing in it
    // travels anywhere.
    if let Err(err) = scope.set_item("blastit_app_module", module) {
        python_fail(py, emitter, None, "BLASTIT_APP (the module to check)", &err);
    }
    if let Err(err) = py.run(VIEW_PATHS, Some(scope), None) {
        python_fail(py, emitter, None, "the paths a view imports through", &err);
    }
    let paths: Vec<String> =
        match scope
            .get_item("blastit_view_paths")
            .and_then(|value| match value {
                Some(value) => value.extract::<Vec<String>>(),
                None => Ok(Vec::new()),
            }) {
            Ok(paths) => paths,
            Err(err) => python_fail(py, emitter, None, "the paths a view imports through", &err),
        };
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    // A live interpreter is in this image — stage 1 is past the point where
    // CPython is up in both installations — and this thread holds the GIL, which
    // is what the fork protocol needs (§9.3).
    match crate::visibility::unreadable(&paths, &dropped, true) {
        Ok(Some(blocker)) => fail(emitter, None, &blocker.message()),
        Ok(None) => {}
        Err(timed_out) => fail(emitter, None, &timed_out.message()),
    }

    // §5.3: the whole `sys.path` — not only the directories blastit added. An
    // entry a view imports through that another uid can write is a route
    // planting a module another route imports, whoever put the entry there:
    // blastit, `PYTHONPATH`, a `.pth` file, or the interpreter itself. Refused
    // at boot rather than served.
    if let Err(err) = py.run(SYS_PATH, Some(scope), None) {
        python_fail(py, emitter, None, "sys.path", &err);
    }
    match scope.get_item("blastit_sys_path") {
        Ok(Some(value)) => match value.extract::<Vec<String>>() {
            Ok(entries) => {
                let mut entries: Vec<PathBuf> = entries.into_iter().map(PathBuf::from).collect();
                // The working directory blastit was started in is a `sys.path`
                // entry like any other — but it is also the one place this check
                // has to cover even if `put_cwd_first` did not have to insert
                // it, because that is where the application's own module comes
                // from.
                if let Some(cwd) = config.cwd.as_deref() {
                    let cwd = PathBuf::from(std::ffi::OsString::from_vec(cwd.to_vec()));
                    if !entries.contains(&cwd) {
                        entries.insert(0, cwd);
                    }
                }
                match crate::visibility::writable(&entries, &dropped, true) {
                    Ok(Some(writable)) => fail(emitter, None, &writable.message()),
                    Ok(None) => {}
                    Err(timed_out) => fail(emitter, None, &timed_out.message()),
                }
            }
            Err(err) => python_fail(py, emitter, None, "the paths on sys.path", &err),
        },
        Ok(None) => {}
        Err(err) => python_fail(py, emitter, None, "the paths on sys.path", &err),
    }
}

/// Puts the working directory `blastit` was started in first on `sys.path`
/// (§5.3, D2).
///
/// `python -I` implies `-P`, so the interpreter that runs stage 1 starts with
/// neither `PYTHONPATH` nor the working directory on `sys.path` — and a project
/// that is not installed (`pip install -e .` never run) has nowhere else for its
/// `wsgi:application` to come from. gunicorn, uwsgi and granian all make the
/// directory they were started in importable, so a project ported from one of
/// them must not need `PYTHONPATH=$PWD` to serve itself.
///
/// This is the **only** thing `-I`'s isolation is given back besides
/// [`restore_sys_path`]'s recorded entries: no `PYTHON*` variable changes how
/// the interpreter starts, because none is read. An entry that is already there
/// is moved to the front rather than duplicated, and `None` — a stage 0 that
/// could not read its own working directory — does nothing.
///
/// # Errors
///
/// Whatever the interpreter raises while `sys.path` is edited, which is a boot
/// that cannot say where its own application lives.
#[cfg(feature = "python")]
fn put_cwd_first(py: Python<'_>, cwd: Option<&[u8]>) -> PyResult<()> {
    let Some(cwd) = cwd else {
        return Ok(());
    };
    let decoded = py
        .import("os")?
        .getattr("fsdecode")?
        .call1((PyBytes::new(py, cwd),))?;
    let path = py.import("sys")?.getattr("path")?;
    // The directory is *moved* to the front when it is already on the path —
    // `sys.path` is searched in order, and "first" is the whole point — and
    // added when it is not. The index is found before anything is edited, so
    // the list is never mutated while it is being walked.
    let length = path.len()?;
    let mut found = None;
    for index in 0..length {
        if path.get_item(index)?.eq(&decoded)? {
            found = Some(index);
            break;
        }
    }
    if let Some(at) = found {
        path.call_method1("pop", (at,))?;
    }
    path.call_method1("insert", (0, decoded))?;
    Ok(())
}

/// Puts the interpreter's import path back where stage 0 found it (§4).
///
/// A wheel's stage 1 runs with `-I` — the same interpreter, but no `PYTHONPATH`
/// and no working directory on `sys.path`. The entries the user's interpreter
/// had — bytes, as stage 0 recorded them with `os.fsencode` — are re-inserted in
/// front, in their original order, skipping the ones `-I` already provides: the
/// result is exactly the `sys.path` the console script saw, which is where the
/// application's modules are found. `os.fsdecode` is what turns those bytes back
/// into the text `sys.path` is made of, without dropping a path that is not
/// valid UTF-8.
///
/// # Errors
///
/// Whatever the interpreter raises when `sys.path` is not what CPython promises
/// it is.
#[cfg(feature = "python")]
fn restore_sys_path(py: Python<'_>, entries: &[Vec<u8>]) -> PyResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let fsdecode = py.import("os")?.getattr("fsdecode")?;
    let path = py.import("sys")?.getattr("path")?;
    for entry in entries.iter().rev() {
        let decoded = fsdecode.call1((PyBytes::new(py, entry),))?;
        if !path.contains(decoded.clone())? {
            path.call_method1("insert", (0, decoded))?;
        }
    }
    Ok(())
}

/// §9.2 #7: makes the preloaded heap immortal, when the interpreter's marker is
/// known here.
///
/// The decision is `BLASTIT_IMMORTALIZE` against the running interpreter
/// ([`Decision`]): every interpreter blastit knows gets the
/// marker *it* reads — 3.14 CPython's own split word — and one whose marker
/// blastit does not know is skipped, so the server starts rather than refusing
/// and every view process simply copies more of the preloaded heap (§9.2 #6).
///
/// A skip is one `warn` line saying why; the same reason travels in the
/// preload `boot` event ([`preload_lifecycle`]). A pass that was *decided* and
/// then failed is still fatal: a heap that was not immortalised when it could
/// have been is the copy §9.2 #6 measured.
#[cfg(feature = "python")]
fn immortalize(
    py: Python<'_>,
    config: &Config,
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
) -> Outcome {
    let outcome = match blastit_view_python::immortal::apply(py, config.immortalize.setting()) {
        Ok(outcome) => outcome,
        Err(err) => python_fail(py, emitter, dev, "heap immortalisation", &err),
    };
    if let Decision::Off(reason) = &outcome.decision {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, reason).with_level(Level::Warn),
        );
    }
    outcome
}

/// The reading §8 Sizing's `boot` event reports, taken now that the app is
/// preloaded.
///
/// `before` is the reading from before the fork, and it is also the *proof*
/// that memory can be read: when it is `None` the machine has already said so
/// at `warn`, and re-reading here would say it twice. When the second reading
/// fails but the first succeeded, `before` is still a usable figure.
#[cfg(feature = "python")]
fn refresh_memory(before: Option<mem_usage::Usage>) -> Option<mem_usage::Usage> {
    let before = before?;
    Some(mem_usage::read(Path::new("/")).unwrap_or(before))
}

/// §8 Sizing and §9.2 #7: what the preload cost, and the view process cap the
/// memory that is left buys.
///
/// This runs after the app was imported and the heap immortalised — or left
/// alone on purpose — so `mem_used` is the number §8 asks for ("usage after
/// preload") and the derived cap is the one the scheduler measures for itself,
/// from the same function.
#[cfg(feature = "python")]
fn emit_preload(
    emitter: &Emitter,
    config: &Config,
    outcome: &Outcome,
    memory: Option<mem_usage::Usage>,
    dev: bool,
) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(preload_lifecycle(config, outcome, memory, dev)),
    ));
}

/// The `boot` payload of the preload phase (§8.1).
///
/// `immortalize` says whether §9.2 #7's pass ran, and `immortalize_reason` why
/// it did not; `immortalized` and `immortalize_ms` describe the pass itself
/// and are therefore a zero and a nothing when there was none. Apart from the
/// emitting, so that a test can read the fields back without a log.
#[cfg(feature = "python")]
fn preload_lifecycle(
    config: &Config,
    outcome: &Outcome,
    memory: Option<mem_usage::Usage>,
    dev: bool,
) -> Lifecycle {
    let mut lifecycle = Lifecycle::default()
        .with_stage("preload")
        .with_immortalize(outcome.decision.is_on())
        .with_immortalized(outcome.objects());
    // `R22`, `R23`: adopting a `dev` root stamps `"unprivileged-dev"` (and the
    // binary's build) in stage 1 itself, even if `--stage=1 --dev-root=…` was
    // started without stage 0. Otherwise D6's stage-0 `Privilege` supplies both.
    if dev {
        lifecycle = lifecycle
            .with_build(env!("CARGO_PKG_VERSION"))
            .with_privilege(crate::dev_learn::PRIVILEGE);
    } else if let Some(privilege) = &config.privilege {
        lifecycle = privilege.lifecycle(lifecycle);
    }
    // E1: and the preset, for the same reason — a `boot` line in the middle of a
    // deployment's log has to be readable on its own.
    if let Some(choice) = &config.preset {
        lifecycle = choice.lifecycle(lifecycle);
    }
    if let Some(reason) = outcome.decision.reason() {
        lifecycle = lifecycle.with_immortalize_reason(reason);
    }
    if let Some(millis) = outcome.millis() {
        lifecycle = lifecycle.with_immortalize_ms(millis);
    }
    if let Some(usage) = memory {
        lifecycle = lifecycle
            .with_mem_total(usage.total())
            .with_mem_used(usage.used())
            .with_max_view_processes(derived_view_cap(
                usage.total(),
                usage.used(),
                config.view_mem_estimate,
            ));
    }
    lifecycle
}

/// `module:attribute` as a Python object.
///
/// # Errors
///
/// A [`PyValueError`] when the spec is malformed, and whatever the import or
/// the attribute lookup raises.
#[cfg(feature = "python")]
fn import_callable(py: Python<'_>, spec: &str) -> PyResult<Py<PyAny>> {
    let Some((module, attribute)) = spec.split_once(':') else {
        return Err(PyValueError::new_err(format!(
            "`{spec}` is not `module:attribute`"
        )));
    };
    let module = py.import(module)?;
    Ok(module.getattr(attribute)?.unbind())
}

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
    // SAFETY: init is exiting; `_exit` runs no handler and cannot deadlock with
    // the GIL held.
    unsafe { libc::_exit(1) }
}

/// The identity and the filter every view child is forked with
/// (`view-crates-architecture` §4.4 D1) — the boot data a view crate receives,
/// in routing order.
///
/// One struct instead of two vectors so the two can never be paired by index:
/// a route's identity and its filter are decided together, in
/// [`view_child_plans`], and travel together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewChildPlans {
    /// One entry per manifest route: the identity its view child keeps.
    pub privileges: Vec<Privileges>,
    /// One entry per manifest route: the seccomp filter its view child
    /// installs.
    pub filter: Vec<Filter>,
}

/// The identity and the filter every view child is forked with
/// The PHP slots stage 1 hands the boot's two PHP halves
/// (`view-crates-architecture` §4.4 D2).
///
/// One entry per **view slot**, `None` for a slot no manifest route names — the
/// `--learn` fallback slot 0 — so the binding vector keeps today's numbering.
/// Everything here is a value: the route's name and uid, its private directory
/// (the fork's `chdir` and PHP's `sys_temp_dir`, one value), its egress, the
/// mount point, and the identity/filter [`view_child_plans`] decided — with the
/// `EmbeddedView` role, which also refuses `execve` and process creation.
///
/// A PHP slot's filter is **not** the WSGI `View` role: `libphp` must never make
/// those calls, and no script may reach them through an extension.
///
/// # Errors
///
/// The routing-table disagreement the binding builder used to refuse: a manifest
/// route with no view slot means the manifest and the scheduler's table were
/// built from different inputs, which is a boot refusal rather than a guess.
#[cfg(feature = "php")]
fn php_slots(
    config: &Config,
    manifest: &Manifest,
    routes: &[blastit_scheduler::RouteInfo],
    plans: &ViewChildPlans,
    dev: Option<&crate::dev_learn::Dev>,
) -> Result<Vec<Option<blastit_view_php::PhpSlot>>, String> {
    let mut slots: Vec<Option<blastit_view_php::PhpSlot>> = routes.iter().map(|_| None).collect();
    let Some(php) = manifest.php() else {
        return Ok(slots);
    };
    let offset = routes.len().saturating_sub(manifest.routes().len());
    let script_name = crate::config::mount_point(config.script_name.as_deref());
    for (index, route) in manifest.routes().iter().enumerate() {
        let slot = index + offset;
        let Some(info) = routes.get(slot) else {
            return Err(format!(
                "route {:?}: no view slot for entrypoint php {:?}; the manifest and the \
                 scheduler's routing table disagree",
                route.name(),
                php.entry
            ));
        };
        let (Some(privileges), Some(view_filter)) = (
            plans.privileges.get(slot).copied(),
            plans.filter.get(slot).copied(),
        ) else {
            return Err(format!(
                "route {:?} (slot {slot}): this boot has no view child plan for the slot",
                route.name()
            ));
        };
        // The role, not the mode: `view_child_plans` decided the mode once
        // (`BLASTIT_SECCOMP`, §7.3) and this names the PHP role. The `other` arm
        // forwards the decision verbatim, so the design's D1 census keeps its
        // single off-mode site — the explicit `BLASTIT_SECCOMP=0` — and this
        // arm adds no second one.
        let filter = match view_filter {
            blastit_sys::child::Filter::Seccomp(_) => {
                blastit_sys::child::Filter::Seccomp(blastit_sys::SeccompRole::EmbeddedView {
                    egress: route.egress(),
                })
            }
            other => other,
        };
        slots[slot] = Some(blastit_view_php::PhpSlot {
            route: info.name.clone(),
            uid: info.uid,
            dir: match dev {
                Some(dev) => dev.view_dir(slot),
                None => crate::config::view_dir(info.uid),
            },
            egress: route.egress(),
            script_name: script_name.clone(),
            privileges,
            filter,
        });
    }
    Ok(slots)
}

/// Requests per worker before it retires. `0` means "no cap" in the
/// configuration and `u32::MAX` in the worker, which is the same rule.
#[cfg(feature = "php")]
fn php_max_requests(config: &Config) -> u32 {
    if config.view_max_requests == 0 {
        u32::MAX
    } else {
        u32::try_from(config.view_max_requests).unwrap_or(u32::MAX)
    }
}

/// The body the worker buffers before PHP runs (`post_max_size`'s twin).
#[cfg(feature = "php")]
fn php_max_body_bytes(config: &Config) -> usize {
    usize::try_from(config.max_body_bytes)
        .unwrap_or(usize::MAX)
        .max(1)
}

/// §8.1's `runtime` line for a resolved `entrypoint php`, emitted by the process
/// that *knows* it: the probe below the fork ran `php_module_startup`, so the
/// `php.ini` it reports is the one PHP opened, not a search path re-derived from
/// `PHPRC` (stripped), the cwd (`php_ini_ignore_cwd = 1`) or a guessed
/// `PHP_CONFIG_FILE_PATH`. The version is the same child's `php_version()`, so
/// the line carries the full `"8.4.27"` rather than the major.minor the
/// discovery record states.
///
/// One line per resolution, so the latest line names what is serving. The
/// library is public knowledge; the shim that carries the SAPI is internal and
/// is never named here (`critical_php_backend`'s whole-boot scan keeps that
/// true).
///
/// The probe returns the two facts as a value ([`blastit_view_php::PhpProbeReport`])
/// because the event stream is this crate's: a view crate builds no events.
#[cfg(feature = "php")]
fn emit_php_runtime(
    emitter: &Emitter,
    config: &Config,
    report: Option<blastit_view_php::PhpProbeReport>,
) {
    let Some((version, php_ini)) = report else {
        return;
    };
    let Some(record) = config.php.as_ref() else {
        return;
    };
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Runtime(Runtime::php(
            version,
            record.libphp.display().to_string(),
            php_ini,
        )),
    ));
}

/// The identity and the filter every view child is forked with
/// turned into the two values a view crate receives.
///
/// This is a **pure function of the boot**, deliberately: the decision is the
/// security property (“production drops to the route's uid and installs a
/// filter”), so it is not buried in the boot sequence where only an end-to-end
/// run could see it. `crates/blastit/src/stage1.rs`'s `child_plans` cases pin
/// the three rows (production, `dev`, an explicit `BLASTIT_SECCOMP=0`).
///
/// Two things are *not* decided here and must not be: the uid the child drops
/// to is the route's own (`RouteInfo::uid`, laid out by stage 0), and the
/// egress bit is the route's (`route.egress()`), so a route can never be given
/// another route's identity or reach.
fn view_child_plans(
    routes: &[blastit_scheduler::RouteInfo],
    egress: &[bool],
    dev: Option<&crate::dev_learn::Dev>,
    seccomp: bool,
) -> ViewChildPlans {
    let privileges = routes
        .iter()
        .map(|route| {
            if dev.is_none() {
                Privileges::DropTo(route.uid)
            } else {
                // `blastit dev`: every role shares the caller's uid, so the
                // layout's per-route uids are not used at all (`R19`).
                // (`check` never boots stage 1 and forks no view.)
                Privileges::Shared
            }
        })
        .collect();
    let filter = egress
        .iter()
        .map(|egress| {
            if seccomp {
                Filter::Seccomp(SeccompRole::View { egress: *egress })
            } else {
                // Only the explicit `BLASTIT_SECCOMP=0` (`R18`, `S6`), which
                // stage 0 already warned about.
                Filter::Off
            }
        })
        .collect();
    ViewChildPlans { privileges, filter }
}

/// The limits http, the scheduler and dbproxy run under: a descriptor budget
/// sized from the configuration, and no core file (§7.2).
///
/// `RLIMIT_NPROC` is left alone here; it belongs to the view uids.
fn component_limits(config: &Config) -> blastit_sys::Rlimits {
    blastit_sys::Rlimits {
        nproc: None,
        nofile: Some(config.component_nofile()),
        core_zero: true,
    }
}

/// `blastit_sys::pipe`, fatal on failure.
fn pipe_ok(
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    what: &str,
) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pipe) => pipe,
        Err(err) => fail(emitter, dev, &format!("{what}: {err}")),
    }
}

/// `blastit_sys::socketpair_stream`, fatal on failure.
fn pair_ok(
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    what: &str,
) -> (OwnedFd, OwnedFd) {
    match socketpair_stream() {
        Ok(pair) => pair,
        Err(err) => fail(emitter, dev, &format!("{what}: {err}")),
    }
}

#[cfg(all(test, feature = "python"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs::File;
    use std::os::unix::ffi::OsStrExt;
    use std::time::Duration;

    use blastit_events::encode_json;

    use super::*;
    use crate::config::Immortalize;
    use blastit_view_python::immortal::Outcome;

    /// Runs `f` with an emitter on a pipe, and returns the lines it wrote.
    ///
    /// Nothing here touches the heap: the setting below is the one that never
    /// walks it, so these tests cannot race the whole-interpreter passes in
    /// `blastit_view_python::immortal`'s own tests.
    fn capture<T>(f: impl FnOnce(&Emitter) -> T) -> (String, T) {
        let (read, write) = pipe().unwrap();
        let emitter = Emitter::new(write.as_raw_fd(), LogFormat::Json, Level::Debug, false);
        let value = f(&emitter);
        drop(write);
        let mut text = String::new();
        File::from(read).read_to_string(&mut text).unwrap();
        (text, value)
    }

    /// The preload `boot` event as JSON, which is what an operator reads.
    fn boot_json(config: &Config, outcome: &Outcome) -> serde_json::Value {
        let event = Event::now(
            Component::Init,
            EventKind::Boot(preload_lifecycle(config, outcome, None, false)),
        );
        serde_json::from_str(&encode_json(&event)).unwrap()
    }

    #[test]
    fn preload_stamps_unprivileged_dev_and_build_when_adopting_a_dev_root() {
        let config = Config {
            // Even if a caller forged `privilege: "root"` in `--config-fd`,
            // adopting `--dev-root` stamps `"unprivileged-dev"`.
            privilege: Some(crate::config::Privilege {
                mode: crate::config::Mode::Root,
                capabilities: Vec::new(),
                bounding_set_dropped: true,
            }),
            ..Config::default()
        };
        let outcome = Outcome {
            decision: Decision::Off("for the test".to_string()),
            stats: None,
        };
        let event = Event::now(
            Component::Init,
            EventKind::Boot(preload_lifecycle(&config, &outcome, None, true)),
        );
        let value: serde_json::Value = serde_json::from_str(&encode_json(&event)).unwrap();
        assert_eq!(value["stage"], "preload");
        assert_eq!(value["privilege"], "unprivileged-dev");
        assert_eq!(value["build"], env!("CARGO_PKG_VERSION"));
        assert!(value.get("bounding_set").is_none(), "{value}");
    }

    #[test]
    fn learn_routing_table_reserves_slot_zero_and_preserves_declared_matches() {
        let source = "\
view a GET, POST regex '/a/[0-9]+', regex '/café/1' {}\n\
\n\
view b regex '/robots\\.txt' {}\n";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let table = learn_routing_table(&manifest).unwrap();
        assert_eq!(
            table.route("GET", "/a/42"),
            Some(blastit_manifest::Match { route: 1, rule: 0 })
        );
        assert_eq!(
            table.route("POST", "/café/1"),
            Some(blastit_manifest::Match { route: 1, rule: 1 })
        );
        assert_eq!(table.route("DELETE", "/a/42"), None);
        assert_eq!(
            table.route("GET", "/robots.txt"),
            Some(blastit_manifest::Match { route: 2, rule: 0 })
        );
        // Slot 0 is the never-matching fallback: no request is routed into it,
        // and it contributes no method to a `405`.
        assert_eq!(table.route("GET", "/unrouted"), None);
        assert_eq!(table.route("GET", "/"), None);
        assert_eq!(table.methods_for("/unrouted"), None);
    }

    /// `blastit` keeps `blastit-ingress` as a direct dependency (`MA.2.1.1`):
    /// the supervisor and `check` read the same codec the manifest writes.
    #[test]
    fn the_ingress_types_are_importable_and_speak_the_manifest_spelling() {
        use blastit_ingress::{BodyCodec, IngressPolicy};

        let policy = IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false,
        };
        assert_eq!(
            serde_json::to_value(policy).unwrap(),
            serde_json::json!({"body": "json", "allow_raw": false})
        );
        assert_eq!(
            serde_json::to_value(BodyCodec::Raw).unwrap(),
            serde_json::json!("raw")
        );
    }

    #[test]
    fn a_skipped_pass_is_reported_in_the_boot_event() {
        let config = Config::default();
        let outcome = Outcome {
            decision: Decision::Off("CPython 3.14: unsupported here".to_string()),
            stats: None,
        };
        let value = boot_json(&config, &outcome);
        assert_eq!(value["event"], "boot");
        assert_eq!(value["stage"], "preload");
        assert_eq!(value["immortalize"], "off");
        assert_eq!(value["immortalized"], 0);
        assert_eq!(
            value["immortalize_reason"],
            "CPython 3.14: unsupported here"
        );
        assert!(value.get("immortalize_ms").is_none(), "{value}");
    }

    #[test]
    fn a_pass_that_ran_is_reported_in_the_boot_event() {
        let config = Config::default();
        let outcome = Outcome {
            decision: Decision::On,
            stats: Some(blastit_view_python::immortal::Stats {
                objects: 1_610_300,
                elapsed: Duration::from_secs_f64(1.7854),
            }),
        };
        let value = boot_json(&config, &outcome);
        assert_eq!(value["immortalize"], "on");
        assert_eq!(value["immortalized"], 1_610_300);
        assert!(value.get("immortalize_reason").is_none(), "{value}");
        let millis = value["immortalize_ms"].as_f64().unwrap();
        assert!((millis - 1785.4).abs() < 1.0, "{millis}");
        assert!(value["preset"].is_null(), "no preset: {value}");
    }

    #[test]
    fn the_preload_boot_event_carries_the_preset_stage_zero_chose() {
        // E1: the preset is stage 0's decision and travels in the document, and
        // the preload `boot` line repeats it — a line in the middle of a log is
        // read on its own, and the mode it describes came from the same source.
        let config = Config {
            preset: Some(crate::config::PresetChoice {
                name: "compose-traefik".to_string(),
                supplied: std::collections::BTreeMap::from([(
                    "BLASTIT_TRUSTED_PROXIES".to_string(),
                    "*".to_string(),
                )]),
                overridden: vec!["BLASTIT_LOG_FORMAT".to_string()],
            }),
            ..Config::default()
        };
        let outcome = Outcome {
            decision: Decision::Off("for the test".to_string()),
            stats: None,
        };
        let value = boot_json(&config, &outcome);
        assert_eq!(value["stage"], "preload");
        assert_eq!(value["preset"], "compose-traefik");
        assert_eq!(value["preset_settings"]["BLASTIT_TRUSTED_PROXIES"], "*");
        assert_eq!(
            value["preset_overridden"],
            serde_json::json!(["BLASTIT_LOG_FORMAT"])
        );
    }

    #[test]
    fn skipping_immortalisation_warns_once_with_the_reason() {
        // `BLASTIT_IMMORTALIZE=0` never walks the heap, on any interpreter: the
        // decision is the setting's, whatever version this is.
        let config = Config {
            immortalize: Immortalize::Off,
            ..Config::default()
        };
        Python::initialize();
        Python::attach(|py| {
            let (text, outcome) = capture(|emitter| immortalize(py, &config, emitter, None));
            assert!(!outcome.decision.is_on());
            let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
            assert_eq!(lines.len(), 1, "one warn line, got {text:?}");
            let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(value["level"], "warn");
            assert_eq!(value["event"], "app.output");
            assert_eq!(value["component"], "init");
            let line = value["line"].as_str().unwrap();
            assert!(line.contains("BLASTIT_IMMORTALIZE=0"), "{line}");
            assert!(line.contains("views will copy"), "{line}");
        });
    }

    /// §5.3: the directory blastit was started in is first on `sys.path`, once.
    ///
    /// This is what a project that is not installed serves itself from, so the
    /// two things that matter are that it is *first* — ahead of the recorded
    /// `sys.path` of the console script, and of the interpreter's own entries —
    /// and that a directory already on the path is moved rather than added
    /// twice. Everything happens under one attached interpreter, and the
    /// interpreter is put back afterwards, so no other test in this binary sees
    /// the directory.
    #[test]
    fn the_working_directory_goes_first_on_sys_path() {
        let dir = std::env::temp_dir().join(format!("blastit-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = dir.as_os_str().as_bytes().to_vec();
        let decoded = dir.to_str().unwrap().to_string();
        Python::initialize();
        Python::attach(|py| {
            let path = py.import("sys").unwrap().getattr("path").unwrap();
            let before: Vec<String> = path.extract().unwrap();

            put_cwd_first(py, Some(&bytes)).unwrap();
            assert_eq!(
                path.get_item(0).unwrap().extract::<String>().unwrap(),
                decoded,
                "the working directory is first: {before:?}"
            );
            assert_eq!(
                path.call_method1("count", (decoded.as_str(),))
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                1,
                "and it is on the path exactly once"
            );

            // A second call moves it back to the front instead of duplicating
            // it, which is what a `sys.path` that already holds the directory
            // (a `PYTHONPATH=.` on the console script) looks like.
            put_cwd_first(py, Some(&bytes)).unwrap();
            assert_eq!(
                path.get_item(0).unwrap().extract::<String>().unwrap(),
                decoded
            );
            assert_eq!(
                path.call_method1("count", (decoded.as_str(),))
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                1
            );

            // `None` is a stage 0 that could not read its own working
            // directory: nothing changes.
            let with_dir: Vec<String> = path.extract().unwrap();
            put_cwd_first(py, None).unwrap();
            assert_eq!(path.extract::<Vec<String>>().unwrap(), with_dir);

            // Put the interpreter back as it was for every other test here.
            path.call_method1("remove", (decoded.as_str(),)).unwrap();
            assert_eq!(
                path.extract::<Vec<String>>().unwrap(),
                before,
                "the path is exactly as it was found"
            );
        });
        let _ = std::fs::remove_dir(&dir);
    }

    /// Finding: the post-preload `sync_marks` can come back `Dead` — the boot's
    /// own pass succeeded and a *later* one died — and that row used to have no
    /// test at all. It must drop the `Reload` **and** keep `reload: on` off the
    /// transcript: printing it would claim a watcher that is gone.
    ///
    /// Grey-box: the real `Reload`, a real `Policy`/`ReportTable` and a scripted
    /// watcher whose second marking pass fails `EIO`.
    #[test]
    fn a_watcher_that_dies_after_the_preload_keeps_the_boot_line_off() {
        use std::os::fd::OwnedFd;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use crate::dev_reload::{
            Class, FakeWatcher, MAX_PATHS, Reload, ReloadConfig, TestClock, report_pair,
        };

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "blastit-stage1-marks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app/views")).unwrap();
        let loaded = root.join("app/views/cart.py");
        std::fs::write(&loaded, "TAG = 1\n").unwrap();

        // `Default`, not the constructor named `new`: the no-inotify guard
        // counts that call in `src/` and allows exactly one, in `dev_reload.rs`.
        let mut watcher = FakeWatcher::default();
        // Call 1 is the boot's app-root mark, the only one it needs; the
        // preload pass is call 2 and dies.
        watcher.fail_from = Some((2, libc::EIO));
        let poll = std::os::unix::net::UnixStream::pair().unwrap().0;
        let mut reload = Reload::boot(
            ReloadConfig::new(root.clone(), root.join("Blastfile"), "tmpfs".to_string()),
            Box::new(watcher),
            OwnedFd::from(poll),
            Box::new(TestClock::new(std::time::Instant::now())),
        )
        .expect("the boot's own marking pass succeeds");
        assert_eq!(
            reload.marked_dirs(),
            1,
            "the app root, and nothing else yet"
        );

        // The preload batch adds a parent the boot had not marked.
        let _write = report_pair(reload.policy().reports(), 0, Class::Loaded, true).unwrap();
        let absorbed =
            reload
                .policy()
                .reports()
                .absorb(0, Class::Loaded, true, &[loaded], true, MAX_PATHS);
        assert!(!absorbed.refused(), "the loaded file joined the union");
        assert_eq!(absorbed.added, 1);

        let outcome = reload.sync_marks();
        assert!(reload.is_dead(), "the second pass latched the watcher dead");
        let dirs = reload.marked_dirs();
        match after_preload_marks(outcome, false, dirs) {
            BootLine::Stopped(why) => assert_eq!(why.errno_name(), "EIO"),
            other => panic!(
                "a watcher that dies after the preload prints no boot line and drops the \
                 reload, got {other:?}"
            ),
        }
        // The positive control: a pass that marked prints the line it claims.
        assert_eq!(
            after_preload_marks(MarkOutcome::Marked, false, 3),
            BootLine::Announce(3)
        );
        assert_eq!(
            after_preload_marks(MarkOutcome::Marked, true, 3),
            BootLine::Silent,
            "a boot that already owed its one warning never prints the line"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ------------------------------------------- the adoption document (§4.6)

    /// Three open pipe read ends and their keepers, for a document that must
    /// pass the open-descriptor check.
    struct OpenFds {
        fds: [RawFd; 4],
        _keep: Vec<std::io::PipeWriter>,
    }

    fn open_fds() -> OpenFds {
        let mut keep = Vec::new();
        let mut fds = [0 as RawFd; 4];
        for slot in &mut fds {
            let (read, write) = std::io::pipe().expect("a pipe");
            *slot = std::os::fd::IntoRawFd::into_raw_fd(read);
            keep.push(write);
        }
        OpenFds { fds, _keep: keep }
    }

    /// Reads `document` through [`Adoption::read`] exactly as a boot would.
    fn read_document(document: &[u8]) -> Result<Adoption, String> {
        use std::io::Write as _;
        let (read, mut write) = std::io::pipe().expect("a pipe");
        write.write_all(document).expect("the document");
        drop(write);
        Adoption::read(std::os::fd::IntoRawFd::into_raw_fd(read))
    }

    fn component_json(fds: [RawFd; 3], pid: i32) -> serde_json::Value {
        serde_json::json!({
            "pid": pid,
            "stdout": fds[0],
            "stderr": fds[1],
            "events": fds[2],
        })
    }

    /// Every refusal in the adoption contract is a boot sentence, and each one
    /// is a distinct reason: an unknown key, a missing key, the wrong version,
    /// a descriptor this image does not hold, and the size bound.
    #[test]
    fn the_adoption_document_refuses_every_shape_it_cannot_honour() {
        let open = open_fds();
        let http = component_json([open.fds[0], open.fds[1], open.fds[2]], 4242);
        let chan = open.fds[3];
        let cases: Vec<(serde_json::Value, &str)> = vec![
            (
                serde_json::json!({"version": 1, "http": http, "chan": chan, "bogus": 0}),
                "unknown key `bogus`",
            ),
            (
                serde_json::json!({"version": 1, "http": http, "chan": chan, "dbproxy": {"pid": 1, "stdout": 999_999, "stderr": 999_999, "events": 999_999}}),
                "descriptor 999999 is not open",
            ),
            (
                serde_json::json!({"version": 2, "http": http, "chan": chan}),
                "`version` must be 1",
            ),
            (
                serde_json::json!({"version": 1, "chan": chan}),
                "missing `http`",
            ),
            (
                serde_json::json!({"version": 1, "http": http}),
                "missing `chan`",
            ),
            (
                serde_json::json!({"version": 1, "http": component_json([999_999, open.fds[1], open.fds[2]], 7), "chan": chan}),
                "descriptor 999999 is not open",
            ),
        ];
        for (document, expected) in cases {
            let bytes = serde_json::to_vec(&document).expect("a document");
            let error = read_document(&bytes).expect_err("this document must be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} from {document}, got {error:?}"
            );
        }

        // The size bound is checked before parsing, so an oversized document
        // never reaches `serde_json`.
        let oversized = vec![b' '; MAX_ADOPTION_DOCUMENT + 1];
        let error = read_document(&oversized).expect_err("an oversized document is refused");
        assert!(error.contains("more than the 4096-byte limit"), "{error}");

        // The positive control: these descriptors and this shape are accepted.
        let good = serde_json::to_vec(&serde_json::json!({
            "version": 1, "http": http, "chan": chan,
        }))
        .expect("a document");
        let adoption = read_document(&good).expect("a well-formed document is adopted");
        assert_eq!(adoption.http.pid, 4242);
        assert_eq!(adoption.chan, chan);
    }

    /// The stage-1 program prefix is collected in full and refused past eight
    /// tokens — never silently truncated (§4.6 step 6).
    #[test]
    fn a_stage1_prefix_past_eight_tokens_is_refused_not_truncated() {
        let long: Vec<String> = (0..9).map(|n| format!("--arg{n}")).collect();
        let error = stage1_program(Some(&long)).expect_err("nine tokens is refused");
        assert!(error.contains("9 token(s), not 1..=8"), "{error}");
        let ok = stage1_program(Some(&long[..8])).expect("eight tokens are accepted");
        assert_eq!(ok.len(), 8);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod child_plans {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A `blastit dev` root, removed when dropped.
    ///
    /// [`crate::dev_learn::Dev::adopt`] refuses anything that is not a `0700`
    /// directory owned by the caller, so the fixture builds exactly that and
    /// takes it away again even when an assertion fails.
    struct DevRoot(std::path::PathBuf);

    impl DevRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "blastit-view-child-plans-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the fixture's dev root");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("the fixture's dev root mode");
            Self(path)
        }

        fn dev(&self) -> crate::dev_learn::Dev {
            let host = std::net::IpAddr::V4(std::net::Ipv4Addr::from([127, 0, 0, 1]));
            crate::dev_learn::Dev::adopt(self.0.clone(), host)
                .expect("a 0700 dev root owned by the caller")
        }
    }

    impl Drop for DevRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Two routes with **different** uids and different egress, so “every route
    /// gets its own identity and its own reach” is asserted rather than implied
    /// by a single-route fixture.
    fn fixture() -> (Vec<blastit_scheduler::RouteInfo>, Vec<bool>) {
        (
            vec![
                blastit_scheduler::RouteInfo::new("shop", false, 20_001),
                blastit_scheduler::RouteInfo::new("api", false, 20_002),
            ],
            vec![false, true],
        )
    }

    /// D1, row 1: a production boot drops each view to **its own** uid and
    /// installs its own `View` filter.
    ///
    /// This is the pin the design asks for: inverting the boot-mode branch (or
    /// swapping two routes' uids) fails here.
    #[test]
    fn a_production_boot_drops_each_view_to_its_own_uid_and_filters_it() {
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, None, true);
        assert_eq!(
            plans.privileges.len(),
            routes.len(),
            "one identity per route"
        );
        assert_eq!(plans.filter.len(), routes.len(), "one filter per route");
        assert_eq!(
            plans.privileges,
            vec![Privileges::DropTo(20_001), Privileges::DropTo(20_002)],
            "each route drops to its own uid, in routing order"
        );
        assert_eq!(
            plans.filter,
            vec![
                Filter::Seccomp(SeccompRole::View { egress: false }),
                Filter::Seccomp(SeccompRole::View { egress: true }),
            ],
            "each route's filter keeps its own egress"
        );
    }

    /// D1, row 2: a `blastit dev` boot shares the caller's uid and still
    /// installs the filter — `dev` is filtered today (`Config::seccomp`
    /// defaults to on).
    ///
    /// The shared variant is asserted as "not a per-route drop": `Privileges`
    /// has exactly two variants, and the census that proves no view crate
    /// constructs the shared one greps this directory — so the literal spelling
    /// lives in `view_child_plans` alone and a test does not become a false
    /// positive there.
    #[test]
    fn a_dev_boot_shares_the_uid_and_still_filters() {
        let root = DevRoot::new("dev");
        let dev = root.dev();
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, Some(&dev), true);
        assert_eq!(plans.privileges.len(), routes.len());
        assert!(
            plans
                .privileges
                .iter()
                .all(|privileges| !matches!(privileges, Privileges::DropTo(_))),
            "dev shares the caller's uid, whatever the layout says: {:?}",
            plans.privileges
        );
        assert_eq!(
            plans.filter,
            vec![
                Filter::Seccomp(SeccompRole::View { egress: false }),
                Filter::Seccomp(SeccompRole::View { egress: true }),
            ],
            "dev is filtered: only the explicit BLASTIT_SECCOMP=0 is not"
        );
    }

    /// D1, row 3: only the explicit `BLASTIT_SECCOMP=0` switches the filter
    /// off, and it does so without changing the identity. Same spelling rule as
    /// row 2: `Filter` has exactly two variants, so "not a `Seccomp`" *is* the
    /// off variant, and the census stays clean.
    #[test]
    fn an_explicit_off_switch_installs_no_filter_but_keeps_the_identity() {
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, None, false);
        assert_eq!(
            plans.privileges,
            vec![Privileges::DropTo(20_001), Privileges::DropTo(20_002)],
            "BLASTIT_SECCOMP=0 is not an identity switch"
        );
        assert_eq!(plans.filter.len(), routes.len());
        assert!(
            plans
                .filter
                .iter()
                .all(|filter| !matches!(filter, Filter::Seccomp(_))),
            "no route may keep a filter under BLASTIT_SECCOMP=0: {:?}",
            plans.filter
        );
    }
}
