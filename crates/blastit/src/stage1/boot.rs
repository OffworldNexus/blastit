#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt;

use crate::child::ChildIo;
use crate::config::{Config, LogFormat};
use crate::logs::{Colour, Emitter, output_event};
use crate::mem_usage;
use crate::roles::{HttpFds, SchedulerFds};
use crate::supervise::ComponentPipes;
#[cfg(not(feature = "python"))]
use crate::supervise::{Parts, Supervisor};
use blastit_events::{Component, Level, Stream};
use blastit_frame::Limits;
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
use blastit_scheduler::routes_from_manifest;
use blastit_sys::{Pid, Signal, SignalFd, SignalMask, bind_tcp};

#[cfg(feature = "python")]
use super::adopt::adopt_py_fds;
use super::adopt::test_swap_pause;
use super::adopt::{BootConfig, read_config};
#[cfg(feature = "python")]
use super::adopt::{dev_root_from_cmdline, stage1_program};
#[cfg(not(feature = "python"))]
use super::emit_ready;
#[cfg(not(feature = "python"))]
use super::serve::boot_and_seed_reload;
use super::serve::learn_routing_table;
#[cfg(feature = "python")]
use super::serve::serve;
use super::spawn::{
    HttpFork, SchedulerFork, ViewChildPlans, component_limits, fork_http, fork_scheduler,
    http_config, pair_ok, pipe_ok, read_memory, scheduler_config, view_child_plans,
};
#[cfg(feature = "php")]
use super::spawn::{emit_php_runtime, php_max_body_bytes, php_max_requests, php_slots};
use super::{Adoption, Options, fail};
#[cfg(all(not(feature = "python"), feature = "php"))]
use crate::dev_reload::Class;

/// Runs stage 1. Never returns.
///
/// The universal binary is the only installation, so this image starts with no
/// interpreter and `dlopen`s the pinned one below (design §1.2.1 item 1).
pub fn run(options: Options, colour: Colour) -> ! {
    // The configuration itself is not known yet, so a failure here is JSON —
    // which carries no colour.
    let boot = Emitter::stdout(LogFormat::Json, Level::Info, Colour::PLAIN);
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
    let emitter = Emitter::stdout(config.log_format, config.log_level, colour);
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
    // `ADR-0011`: the gate stage 0 refuses with, repeated here
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
    let routes: Vec<blastit_scheduler::RouteInfo> = match (&dev, learn) {
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
    // The debug-only test seam. Stage 0 recorded `(st_dev, st_ino)`
    // for the pair and carried `BLASTIT_TEST_SWAP_POINT` in the configuration
    // document; this is the one point between that record and the re-open below,
    // so a `cargo test` boot pauses here until the test has renamed an object.
    // The rename is then inside the window by construction — the test cannot be
    // early, this process cannot be early — instead of by how fast the runner
    // happens to be. Unset (and, in a release build, absent) it does nothing.
    test_swap_pause(&emitter, dev.as_ref(), test_swap_point.as_deref());
    // Design §1.2.1 item 6: right after the swap point, before TCP bind, the
    // forks or any load, the pinned pair is re-`fstat`ed against the record and
    // `FD_CLOEXEC` is put back. Both the initial handoff (`--py-*`) and a
    // replacement image's `Adoption.py_fds` arrive here.
    let py_fds = {
        #[cfg(feature = "python")]
        {
            match adopt_py_fds(
                &config,
                adopted.as_ref(),
                options.py_lib_fd,
                options.py_shim_fd,
            ) {
                Ok(pair) => pair,
                Err(message) => fail(&emitter, dev.as_ref(), &message),
            }
        }
        #[cfg(not(feature = "python"))]
        {
            let _ = (&config, &adopted);
            None
        }
    };
    // The universal binary starts no interpreter, so the forks below (`http`,
    // the scheduler) and the PHP probe carry no CPython protocol; the per-view
    // forks after `load::embed` take their hooks from the runtime it loaded
    // (`supervise`). There is no second installation whose stage 1 runs inside
    // CPython already.
    let hooks = None;
    // `ADR-0010` §3: stage 1 re-opens the NTS `libphp` and the shim stage 0
    // inspected, asserts `(st_dev, st_ino)` against that record, renders the INI
    // lockdown once and holds the descriptors every PHP worker is forked with.
    // A file that moved between the two stages is refused here — before the
    // listener binds — never by a worker that loads something nobody inspected.
    // A host with no usable library never reaches stage 1: stage 0 refused it.
    //
    // The two values travel as data (`PhpSlot`s), because a view
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
    // gate refuses one). The shadow keeps `routes` immutable in a build that
    // never patches it, so the feature-less build has no `unused_mut` warning
    // (`-D warnings` in `make rs-no-default`) and needs no `#[allow]`.
    #[cfg(feature = "php")]
    let mut routes = routes;
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
            protocol: if hooks.is_some() {
                blastit_view::spawn::ForkProtocol::InterpreterLive
            } else {
                blastit_view::spawn::ForkProtocol::NoInterpreter
            },
        },
        &|protocol| crate::forking::fork_for(protocol, hooks),
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
            let http_pid = match fork_http(HttpFork {
                child: ChildIo {
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
                fds: HttpFds {
                    listener: listener.as_raw_fd(),
                    chan: http_end.as_raw_fd(),
                    events: http_ev_write.as_raw_fd(),
                },
                cfg: http_config(&config, limits),
                uid: drop_http,
                mask,
                seccomp: config.seccomp,
                hooks,
            }) {
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

    let sched_pid = match fork_scheduler(SchedulerFork {
        child: ChildIo {
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
        fds: SchedulerFds {
            chan: sched_http_end.as_raw_fd(),
            ctl: sched_ctl_end.as_raw_fd(),
            events: sched_ev_write.as_raw_fd(),
        },
        cfg: sched_cfg,
        routes: routes.clone(),
        table,
        limits,
        uid: drop_scheduler,
        mask,
        seccomp: config.seccomp,
        hooks,
    }) {
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
        py_fds,
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
    // view, so the scheduler asks for none.
    #[cfg(feature = "python")]
    serve(&emitter, serving);
    #[cfg(not(feature = "python"))]
    serve_without_interpreter(&emitter, serving)
}

/// Supervises the forked roles in a build that carries no interpreter
/// Never returns.
///
/// Nothing here can start a WSGI view: [`crate::stage0::no_python_support`]
/// refused every route that would need one before a listener was bound. It can
/// and does serve `entrypoint php` (the standalone artifact's default feature
/// set is `php,static`), which is why this path boots the same live-reload
/// watcher a WSGI `dev` boot does: a PHP worker needs no
/// interpreter to fork, and a save is served by repinning the entry, not by
/// re-execing an image.
///
/// A `SPAWN` for a slot this build cannot fork is still a protocol violation,
/// and [`Supervisor`] fails the boot on it rather than ignoring it.
#[cfg(not(feature = "python"))]
pub(super) fn serve_without_interpreter(emitter: &Emitter, serving: Serving) -> ! {
    // The same construction, PHP boot-file seeding, marking pass, boot line and
    // §4.6 step 7 `reconcile` a WSGI `dev` boot runs: a PHP worker
    // needs no interpreter, so this build reloads saves the same way. The no-op
    // `between` is the one difference — there is no `preload` import here.
    let (reload, ()) = boot_and_seed_reload(emitter, &serving, |_| ());
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
            #[cfg(not(feature = "python"))]
            _runtime: std::marker::PhantomData,
            ctl: serving.ctl,
            signals: serving.signals,
            mask: serving.mask,
            components: serving.components,
            py_fds: serving.py_fds,
            dev: serving.dev,
            learn: serving.learn,
            reload,
            reload_source: None,
            chan: None,
            policy: None,
            learn_fds: None,
            boot_pairs: serving.boot_pairs,
        },
        emitter,
    );
    supervisor.run()
}

/// Everything stage 1 needs once an interpreter is available (§4).
pub(super) struct Serving {
    /// The configuration, already parsed.
    pub(super) config: Config,
    /// Frame limits of every channel.
    pub(super) limits: Limits,
    /// One entry per view slot, in scheduler routing order.
    pub(super) routes: Vec<blastit_scheduler::RouteInfo>,
    /// One entry per view slot: the embedded-PHP worker it forks, or `None`
    /// for a WSGI view (`ADR-0010` §1). Built and verified at boot.
    #[cfg(feature = "php")]
    pub(super) php: Vec<Option<blastit_view_php::PhpBinding>>,
    /// Number of views declared in the `Blastfile` (reported on `ready`).
    pub(super) declared_routes: usize,
    /// Whether each route may keep `AF_INET`/`AF_INET6`, by route index (§7.3).
    /// Only a build that forks a view reads it.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) egress: Vec<bool>,
    /// The identity and filter each route's view child keeps
    /// (`view-crates-architecture` §4.4 D1), decided above where the boot mode
    /// exists; a build without the `python` feature still lays them out, and
    /// only the WSGI fork reads them (`fork_for` and the fork it feeds).
    pub(super) plans: ViewChildPlans,
    /// The memory reading taken before the fork, for §8 Sizing. Reported by the
    /// preload event, which only a build with an interpreter emits.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) memory: Option<mem_usage::Usage>,
    /// init's end of `ctl.chan`.
    pub(super) ctl: OwnedFd,
    /// The signalfd init listens on.
    pub(super) signals: SignalFd,
    /// The mask to put back in every child.
    pub(super) mask: SignalMask,
    /// The components to supervise, by pid.
    pub(super) components: BTreeMap<i32, (Component, ComponentPipes)>,
    /// The `(libpython, shim)` descriptors this image adopted and verified
    /// (design §1.2.1 item 6). Held until the supervisor either takes them for
    /// a reload or the boot ends.
    pub(super) py_fds: Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)>,
    /// The `blastit dev` runtime root, when this is a dev boot (`R20`).
    pub(super) dev: Option<crate::dev_learn::Dev>,
    /// The `dev --learn` observation and attribution channels (`R29`).
    pub(super) learn: Option<crate::dev_learn::Learn>,
    /// Whether this `dev` boot wants live reload (`--dev-reload=on`, the
    /// default). `false` under `--no-reload`: no watcher, no marks, no report
    /// socket, no audit hook (§4.1, `R1`). Only a build with an interpreter
    /// builds one.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) dev_reload: bool,
    /// The scheduler's end of `http.chan`, kept for a dev boot with reload on
    /// (and for a replacement image that adopted it) so the cycle can hand it
    /// to the new scheduler without `http` losing its peer (§4.6 step 5).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) chan: Option<OwnedFd>,
    /// This image's end of the dev-only dbproxy policy socket.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) policy: Option<OwnedFd>,
    /// The `--learn` descriptor numbers, for the next adoption document.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) learn_fds: Option<(RawFd, Option<RawFd>)>,
    /// What a reload cycle re-execs with; `None` on every boot that cannot
    /// cycle.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) reload_source: Option<crate::supervise::ReloadSource>,
    /// Whether this image is the first half of a run (`true`) or the replacement
    /// half of a reload (`false`, which repeats neither the boot line nor
    /// `ready`).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) announce: bool,
    /// The instant the cycle that started this image began, for the one-shot
    /// post-boot `Policy::reconcile` (§4.6 step 7).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) reload_since: Option<std::time::SystemTime>,
    /// The debug-only reload fault seam stage 0 carried across the `execve`
    /// (`BLASTIT_TEST_RELOAD_FAULT`, compiled out of release builds).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) reload_fault: Option<String>,
    /// The `(route, proxy)` pairs this image's manifest declares, which are the
    /// pairs dbproxy's listeners were bound for (§4.8).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(super) boot_pairs: std::collections::BTreeSet<(String, String)>,
}
