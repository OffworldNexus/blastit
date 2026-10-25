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
//! gc.collect() · gc.freeze()
//! ready
//! supervise, holding the GIL
//! ```
//!
//! Nothing here parses HTTP and nothing here talks to the database: init exists
//! to fork, to supervise and to be the only writer to stdout (§2.1).

use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use blastit_events::{Component, Event, EventKind, Level, Lifecycle, Stream};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
use blastit_scheduler::{SPAWN_BACKOFF, SchedulerConfig, routes_from_manifest};
use blastit_sys::{
    Fork, Pid, Signal, SignalFd, SignalMask, bind_tcp, drop_privileges, fork, harden_child, pipe,
    restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::child::{ChildIo, child_exit};
use crate::config::{Config, LogFormat};
use crate::logs::{Emitter, fail, output_event};
use crate::roles::{HttpFds, SchedulerFds};
use crate::supervise::{ComponentPipes, Parts, Supervisor};

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

/// §4: close database handles before the fork, then make the heap stable.
///
/// A connection opened at import time would be inherited — uselessly and
/// dangerously — by every view; `gc.collect()` before `gc.freeze()` is what
/// keeps the frozen set small.
const CLOSE_AND_FREEZE: &std::ffi::CStr = c"
import gc, sys
if 'django' in sys.modules:
    from django.db import connections
    connections.close_all()
gc.collect()
gc.freeze()
";

/// Runs stage 1. Never returns.
pub fn run(options: Options) -> ! {
    // The configuration itself is not known yet, so a failure here is JSON.
    let boot = Emitter::stdout(LogFormat::Json, Level::Info);
    let config = read_config(&boot, options.config_fd);
    let emitter = Emitter::stdout(config.log_format, config.log_level);

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
    let layout = UidLayout::new(config.uid_base);
    let routes = routes_from_manifest(&manifest, &layout);
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
    let sched_cfg = scheduler_config(&config, limits);

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

    Python::initialize();
    Python::attach(|py| {
        let app = load(py, &config, &emitter);
        emit_ready(&emitter, &config, routes.len());
        let mut supervisor = Supervisor::new(
            Parts {
                cfg: config,
                limits,
                routes,
                app,
                ctl: ctl_end,
                signals,
                mask,
                components,
            },
            &emitter,
        );
        supervisor.run(py)
    })
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
        trust_forwarded: config.trust_forwarded,
        frame_limits: limits,
        server_port: config.server_port,
        max_connections: config.max_connections,
        ..HttpConfig::default()
    }
}

/// The scheduler's configuration (§8.2).
fn scheduler_config(config: &Config, limits: Limits) -> SchedulerConfig {
    SchedulerConfig {
        max_workers: config.max_workers,
        view_max_workers: config.view_max_workers,
        queue_size: config.queue_size,
        queue_timeout: config.queue_timeout,
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
    }
}

/// Forks the http process, which drops to `uid` and serves (§4).
fn fork_http(
    emitter: &Emitter,
    child: ChildIo,
    fds: HttpFds,
    cfg: HttpConfig,
    uid: u32,
    mask: SignalMask,
) -> Pid {
    match fork() {
        Ok(Fork::Parent(pid)) => pid,
        Ok(Fork::Child) => {
            if let Err(message) = prepare_child(child, uid, mask, "http") {
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
) -> Pid {
    match fork() {
        Ok(Fork::Parent(pid)) => pid,
        Ok(Fork::Child) => {
            if let Err(message) = prepare_child(child, uid, mask, "scheduler") {
                child_exit(&message, 1);
            }
            let code = crate::roles::scheduler(fds, cfg, routes, table, limits);
            child_exit("", code)
        }
        Err(err) => fail(emitter, &format!("forking the scheduler: {err}")),
    }
}

/// The child sequence http and the scheduler share: descriptors, privileges,
/// hardening, signal mask (§4).
///
/// # Errors
///
/// A message for the log when one of those steps fails; there is no recovery in
/// a child.
fn prepare_child(child: ChildIo, uid: u32, mask: SignalMask, role: &str) -> Result<(), String> {
    child
        .prepare()
        .map_err(|err| format!("{role}: {err}"))
        .and_then(|()| drop_privileges(uid).map_err(|err| format!("{role}: {err}")))
        .and_then(|()| harden_child(Signal::KILL).map_err(|err| format!("{role}: {err}")))?;
    let _ = set_dumpable(false);
    restore_signal_mask_in_child(mask).map_err(|err| format!("{role}: {err}"))
}

/// Imports and prepares the application, and returns the WSGI callable (§4).
fn load(py: Python<'_>, config: &Config, emitter: &Emitter) -> Py<PyAny> {
    // §9.3: an embedded interpreter does not find the distribution's
    // site-packages on its own. Best effort — a system without sysconfig is not
    // a reason to refuse to start.
    let _ = py.run(SITE_PACKAGES, None, None);

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
    if let Err(err) = py.run(CLOSE_AND_FREEZE, None, None) {
        python_fail(py, emitter, "close_all/gc.freeze", &err);
    }
    app
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
