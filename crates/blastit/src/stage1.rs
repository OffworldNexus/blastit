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

use blastit_events::{Component, Event, EventKind, Level, Lifecycle, Stream};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
use blastit_scheduler::{
    MEMORY_CHECK_INTERVAL, MemoryProbe, SPAWN_BACKOFF, SchedulerConfig, derived_view_cap,
    routes_from_manifest,
};
use blastit_sys::mem;
use blastit_sys::{
    Fork, Pid, SeccompRole, Signal, SignalFd, SignalMask, bind_tcp, drop_privileges, harden_child,
    pipe, restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::child::{ChildIo, child_exit, install_filter};
use crate::config::{Config, LogFormat};
use crate::immortal::{Decision, Outcome};
use crate::logs::{Emitter, fail, output_event};
use crate::roles::{HttpFds, SchedulerFds};
use crate::supervise::{ComponentPipes, Parts, Supervisor};

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
#[derive(Debug, Clone, Copy)]
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
}

/// Everything a view needs of its scratch directory, in Python (§5.3).
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

/// §4: close database handles before the fork.
///
/// A connection opened at import time would be inherited — uselessly and
/// dangerously — by every view. `gc.collect()` comes straight after, so the set
/// `gc.freeze()` freezes a moment later is as small as it can be, and so the
/// immortalisation pass walks a heap with no garbage in it.
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
/// ([`crate::immortal::Decision::Off`]), the heap is frozen all the same.
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
    let config = read_config(&boot, options.config_fd);
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

    let source = match std::fs::read_to_string(&config.manifest) {
        Ok(source) => source,
        Err(err) => fail(&emitter, &format!("{}: {err}", config.manifest.display())),
    };
    let (manifest, _warnings) = match Manifest::parse_and_validate(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    // Warnings are stage 0's: it reads and validates the manifest first, and
    // printing them twice would be noise (§8.1).

    let limits = match config.limits() {
        Ok(limits) => limits,
        Err(err) => fail(&emitter, &err.to_string()),
    };
    // §8 Sizing: the machine's memory decides how many view processes fit.
    // `total` is a constant, so it can be read before the fork and handed to
    // the scheduler; `used` is only meaningful after the app is preloaded, so
    // `load` takes a fresh reading of it. This is the one that warns when
    // memory cannot be read at all.
    let memory = read_memory(&emitter);
    let layout = UidLayout::new(config.uid_base);
    let routes = routes_from_manifest(&manifest, &layout);
    // §7.3: a view may keep `AF_INET`/`AF_INET6` only when its route says so.
    let egress: Vec<bool> = manifest
        .routes()
        .iter()
        .map(|route| route.egress())
        .collect();
    let table = match RoutingTable::new(&manifest) {
        Ok(table) => table,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };

    let listener = match bind_tcp(
        SocketAddr::from(([0, 0, 0, 0], config.port)),
        config.backlog,
    ) {
        Ok(listener) => listener,
        Err(err) => fail(&emitter, &format!("binding 0.0.0.0:{}: {err}", config.port)),
    };

    // Channels (§3): http.chan, ctl.chan, and three pipes per component.
    let (http_end, sched_http_end) = pair_ok(&emitter, "http.chan");
    let (ctl_end, sched_ctl_end) = pair_ok(&emitter, "ctl.chan");
    let (http_out_read, http_out_write) = pipe_ok(&emitter, "http stdout");
    let (http_err_read, http_err_write) = pipe_ok(&emitter, "http stderr");
    let (http_ev_read, http_ev_write) = pipe_ok(&emitter, "http events");
    let (sched_out_read, sched_out_write) = pipe_ok(&emitter, "scheduler stdout");
    let (sched_err_read, sched_err_write) = pipe_ok(&emitter, "scheduler stderr");
    let (sched_ev_read, sched_ev_write) = pipe_ok(&emitter, "scheduler events");

    // The signals init supervises on have to be blocked before the children
    // exist, so every child inherits the mask and can put it back (§3).
    let signals = match SignalFd::new(&[Signal::CHILD, Signal::TERM, Signal::INT, Signal::HUP]) {
        Ok(signals) => signals,
        Err(err) => fail(&emitter, &format!("signalfd: {err}")),
    };
    let mask = signals.previous_mask();

    let http_cfg = http_config(&config, limits);
    let sched_cfg = scheduler_config(&config, limits, memory);

    let http_pid = fork_http(
        &emitter,
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
        http_cfg,
        layout.http(),
        mask,
        config.seccomp,
        hosted,
    );
    // The parent keeps only the read ends, and never the listening socket.
    drop(http_end);
    drop(http_ev_write);
    drop(listener);

    let sched_pid = fork_scheduler(
        &emitter,
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
        layout.scheduler(),
        mask,
        config.seccomp,
        hosted,
    );
    drop(sched_http_end);
    drop(sched_ctl_end);
    drop(sched_ev_write);

    let mut components: BTreeMap<i32, (Component, ComponentPipes)> = BTreeMap::new();
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
    if let (Some(pid), Some(events), Some(stdout), Some(stderr)) = (
        options.dbproxy_pid,
        options.dbproxy_events_fd,
        options.dbproxy_stdout_fd,
        options.dbproxy_stderr_fd,
    ) && let Some(pid) = Pid::from_raw(pid)
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

    // Everything from here on needs an interpreter, and the forks above were the
    // last thing that had to happen while init was single-threaded (§2.1).
    let serving = Serving {
        config,
        limits,
        routes,
        egress,
        memory,
        ctl: ctl_end,
        signals,
        mask,
        components,
        hosted: interpreter.is_hosted(),
    };
    match interpreter {
        Interpreter::Embed => {
            Python::initialize();
            Python::attach(|py| serve(py, &emitter, serving))
        }
        Interpreter::Hosted(py) => serve(py, &emitter, serving),
    }
}

/// Everything stage 1 needs once an interpreter is available (§4).
struct Serving {
    /// The configuration, already parsed.
    config: Config,
    /// Frame limits of every channel.
    limits: Limits,
    /// One entry per manifest route, in routing order.
    routes: Vec<blastit_scheduler::RouteInfo>,
    /// Whether each route may keep `AF_INET`/`AF_INET6`, by route index (§7.3).
    egress: Vec<bool>,
    /// The memory reading taken before the fork, for §8 Sizing.
    memory: Option<mem::Sample>,
    /// init's end of `ctl.chan`.
    ctl: OwnedFd,
    /// The signalfd init listens on.
    signals: SignalFd,
    /// The mask to put back in every child.
    mask: SignalMask,
    /// The components to supervise, by pid.
    components: BTreeMap<i32, (Component, ComponentPipes)>,
    /// Whether the interpreter was started by the host rather than embedded
    /// here.
    hosted: bool,
}

/// Loads the application, emits `ready`, and supervises. Never returns.
fn serve(py: Python<'_>, emitter: &Emitter, serving: Serving) -> ! {
    let app = load(py, &serving.config, emitter, serving.memory, serving.hosted);
    emit_ready(emitter, &serving.config, serving.routes.len());
    let mut supervisor = Supervisor::new(
        Parts {
            cfg: serving.config,
            limits: serving.limits,
            routes: serving.routes,
            egress: serving.egress,
            app,
            ctl: serving.ctl,
            signals: serving.signals,
            mask: serving.mask,
            components: serving.components,
        },
        emitter,
    );
    supervisor.run(py)
}

/// Reads the configuration document stage 0 wrote into the pipe (§4).
fn read_config(emitter: &Emitter, fd: RawFd) -> Config {
    // SAFETY: stage 0 cleared `FD_CLOEXEC` on this descriptor and stage 1 is its
    // only owner.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    if let Err(err) = file.read_to_end(&mut bytes) {
        fail(emitter, &format!("configuration pipe: {err}"));
    }
    drop(file);
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => fail(emitter, &format!("configuration document: {err}")),
    };
    match Config::from_json(&value) {
        Ok(config) => config,
        Err(err) => fail(emitter, &err.to_string()),
    }
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
    limits: Limits,
    memory: Option<mem::Sample>,
) -> SchedulerConfig {
    let watermark = memory
        .map(|sample| (sample.total() as f64 * config.mem_high_watermark_percent / 100.0) as u64);
    SchedulerConfig {
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
        spawn_backoff: SPAWN_BACKOFF,
        memory_usage: memory.map(|_| MemoryProbe::new(current_usage)),
        mem_high_watermark_bytes: watermark,
        memory_check_interval: MEMORY_CHECK_INTERVAL,
        mem_total_bytes: memory.map(|sample| sample.total()),
        view_mem_estimate: config.view_mem_estimate,
    }
}

/// The machine's aggregate memory usage in bytes, or `None` when it cannot be
/// read (§9.2 #8).
fn current_usage() -> Option<u64> {
    mem::Reader::new("/")
        .sample()
        .ok()
        .map(|sample| sample.used())
}

/// The machine's memory, or a warning that §8 Sizing could not read it.
///
/// Unreadable memory leaves the view cap alone rather than guessing (§8), so
/// this is reported once and the caller carries on with `None`.
fn read_memory(emitter: &Emitter) -> Option<mem::Sample> {
    match mem::Reader::new("/").sample() {
        Ok(sample) => Some(sample),
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
#[allow(clippy::too_many_arguments)]
fn fork_http(
    emitter: &Emitter,
    child: ChildIo,
    fds: HttpFds,
    cfg: HttpConfig,
    uid: u32,
    mask: SignalMask,
    seccomp: bool,
    hosted: bool,
) -> Pid {
    match crate::forking::fork(hosted) {
        Ok(Fork::Parent(pid)) => pid,
        Ok(Fork::Child) => {
            if let Err(message) = prepare_child(child, uid, mask, SeccompRole::Http, seccomp) {
                child_exit(&message, 1);
            }
            let code = crate::roles::http(fds, cfg);
            child_exit("", code)
        }
        Err(err) => fail(emitter, &format!("forking http: {err}")),
    }
}

/// Forks the scheduler process, which drops to `uid` and routes (§4).
#[allow(clippy::too_many_arguments)]
fn fork_scheduler(
    emitter: &Emitter,
    child: ChildIo,
    fds: SchedulerFds,
    cfg: SchedulerConfig,
    routes: Vec<blastit_scheduler::RouteInfo>,
    table: RoutingTable,
    limits: Limits,
    uid: u32,
    mask: SignalMask,
    seccomp: bool,
    hosted: bool,
) -> Pid {
    match crate::forking::fork(hosted) {
        Ok(Fork::Parent(pid)) => pid,
        Ok(Fork::Child) => {
            if let Err(message) = prepare_child(child, uid, mask, SeccompRole::Scheduler, seccomp) {
                child_exit(&message, 1);
            }
            let code = crate::roles::scheduler(fds, cfg, routes, table, limits);
            child_exit("", code)
        }
        Err(err) => fail(emitter, &format!("forking the scheduler: {err}")),
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
    uid: u32,
    mask: SignalMask,
    role: SeccompRole,
    seccomp: bool,
) -> Result<(), String> {
    let name = role.name();
    child
        .prepare()
        .map_err(|err| format!("{name}: {err}"))
        .and_then(|()| drop_privileges(uid).map_err(|err| format!("{name}: {err}")))
        .and_then(|()| harden_child(Signal::KILL).map_err(|err| format!("{name}: {err}")))
        .and_then(|()| install_filter(seccomp, role).map_err(|err| format!("{name}: {err}")))?;
    let _ = set_dumpable(false);
    restore_signal_mask_in_child(mask).map_err(|err| format!("{name}: {err}"))
}

/// Imports and prepares the application, and returns the WSGI callable (§4).
///
/// `memory` is the reading `run` took before the fork: it is the one that
/// warned, once, when the machine's memory cannot be read at all (§8 Sizing).
///
/// `hosted` is `true` when the interpreter is the one the wheel was installed
/// into: its `site-packages` are already on `sys.path`, and what `-I` dropped —
/// `PYTHONPATH`, the working directory — is put back before the first import.
fn load(
    py: Python<'_>,
    config: &Config,
    emitter: &Emitter,
    memory: Option<mem::Sample>,
    hosted: bool,
) -> Py<PyAny> {
    if hosted {
        if let Err(err) = restore_sys_path(py, &config.sys_path) {
            python_fail(py, emitter, "sys.path", &err);
        }
    } else {
        // §9.3: an embedded interpreter does not find the distribution's
        // site-packages on its own. Best effort — a system without sysconfig is
        // not a reason to refuse to start.
        let _ = py.run(SITE_PACKAGES, None, None);
    }

    let spec = config.app.as_deref().unwrap_or_default();
    let app = match import_callable(py, spec) {
        Ok(app) => app,
        Err(err) => python_fail(py, emitter, &format!("BLASTIT_APP ({spec})"), &err),
    };
    for module in &config.preload {
        if let Err(err) = py.import(module.as_str()) {
            python_fail(py, emitter, &format!("BLASTIT_PRELOAD ({module})"), &err);
        }
    }
    if let Some(warmup) = config.warmup.as_deref() {
        let callable = match import_callable(py, warmup) {
            Ok(callable) => callable,
            Err(err) => python_fail(py, emitter, &format!("BLASTIT_WARMUP ({warmup})"), &err),
        };
        if let Err(err) = callable.call0(py) {
            python_fail(py, emitter, &format!("BLASTIT_WARMUP ({warmup})"), &err);
        }
    }
    if let Err(err) = py.run(CLOSE_AND_COLLECT, None, None) {
        python_fail(py, emitter, "close_all/gc.collect", &err);
    }
    // §9.2 #7, between `gc.collect()` and `gc.freeze()`: nothing left in the
    // collected heap is garbage, and every object a request could touch stops
    // counting references — on an interpreter whose marker blastit knows, which
    // `BLASTIT_IMMORTALIZE` and the version decide.
    let outcome = immortalize(py, config, emitter);
    if let Err(err) = py.run(FREEZE, None, None) {
        python_fail(py, emitter, "gc.freeze", &err);
    }
    emit_preload(emitter, config, &outcome, refresh_memory(memory));
    app
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
/// ([`crate::immortal::decide`]): every interpreter blastit knows gets the
/// marker *it* reads — 3.14 CPython's own split word — and one whose marker
/// blastit does not know is skipped, so the server starts rather than refusing
/// and every view process simply copies more of the preloaded heap (§9.2 #6).
///
/// A skip is one `warn` line saying why; the same reason travels in the
/// preload `boot` event ([`preload_lifecycle`]). A pass that was *decided* and
/// then failed is still fatal: a heap that was not immortalised when it could
/// have been is the copy §9.2 #6 measured.
fn immortalize(py: Python<'_>, config: &Config, emitter: &Emitter) -> Outcome {
    let outcome = match crate::immortal::apply(py, config.immortalize) {
        Ok(outcome) => outcome,
        Err(err) => python_fail(py, emitter, "heap immortalisation", &err),
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
fn refresh_memory(before: Option<mem::Sample>) -> Option<mem::Sample> {
    let before = before?;
    Some(mem::Reader::new("/").sample().unwrap_or(before))
}

/// §8 Sizing and §9.2 #7: what the preload cost, and the view process cap the
/// memory that is left buys.
///
/// This runs after the app was imported and the heap immortalised — or left
/// alone on purpose — so `mem_used` is the number §8 asks for ("usage after
/// preload") and the derived cap is the one the scheduler measures for itself,
/// from the same function.
fn emit_preload(
    emitter: &Emitter,
    config: &Config,
    outcome: &Outcome,
    memory: Option<mem::Sample>,
) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(preload_lifecycle(config, outcome, memory)),
    ));
}

/// The `boot` payload of the preload phase (§8.1).
///
/// `immortalize` says whether §9.2 #7's pass ran, and `immortalize_reason` why
/// it did not; `immortalized` and `immortalize_ms` describe the pass itself
/// and are therefore a zero and a nothing when there was none. Apart from the
/// emitting, so that a test can read the fields back without a log.
fn preload_lifecycle(config: &Config, outcome: &Outcome, memory: Option<mem::Sample>) -> Lifecycle {
    let mut lifecycle = Lifecycle::default()
        .with_stage("preload")
        .with_immortalize(outcome.decision.is_on())
        .with_immortalized(outcome.objects());
    if let Some(reason) = outcome.decision.reason() {
        lifecycle = lifecycle.with_immortalize_reason(reason);
    }
    if let Some(millis) = outcome.millis() {
        lifecycle = lifecycle.with_immortalize_ms(millis);
    }
    if let Some(sample) = memory {
        lifecycle = lifecycle
            .with_mem_total(sample.total())
            .with_mem_used(sample.used())
            .with_max_view_processes(derived_view_cap(
                sample.total(),
                sample.used(),
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

/// Prints a Python failure as events and exits non-zero (§4).
fn python_fail(py: Python<'_>, emitter: &Emitter, what: &str, err: &PyErr) -> ! {
    emitter.emit(
        &output_event(
            Component::Init,
            None,
            Stream::Stderr,
            &format!("{what}: {err}"),
        )
        .with_level(Level::Error),
    );
    let text = crate::zygote::traceback_text(py, err);
    for line in text.lines() {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, line).with_level(Level::Error),
        );
    }
    // SAFETY: init is exiting; `_exit` runs no handler and cannot deadlock with
    // the GIL held.
    unsafe { libc::_exit(1) }
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
fn pipe_ok(emitter: &Emitter, what: &str) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pipe) => pipe,
        Err(err) => fail(emitter, &format!("{what}: {err}")),
    }
}

/// `blastit_sys::socketpair_stream`, fatal on failure.
fn pair_ok(emitter: &Emitter, what: &str) -> (OwnedFd, OwnedFd) {
    match socketpair_stream() {
        Ok(pair) => pair,
        Err(err) => fail(emitter, &format!("{what}: {err}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs::File;
    use std::time::Duration;

    use blastit_events::encode_json;

    use super::*;
    use crate::config::Immortalize;
    use crate::immortal::Outcome;

    /// Runs `f` with an emitter on a pipe, and returns the lines it wrote.
    ///
    /// Nothing here touches the heap: the setting below is the one that never
    /// walks it, so these tests cannot race the whole-interpreter passes in
    /// `crate::immortal`'s own tests.
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
            EventKind::Boot(preload_lifecycle(config, outcome, None)),
        );
        serde_json::from_str(&encode_json(&event)).unwrap()
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
            stats: Some(crate::immortal::Stats {
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
            let (text, outcome) = capture(|emitter| immortalize(py, &config, emitter));
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
}
