#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use std::os::fd::OwnedFd;
use std::path::Path;

use crate::child::{ChildIo, Filter, Privileges, child_exit, install_filter};
use crate::config::Config;
use crate::logs::{Emitter, output_event};
use crate::mem_usage;
use crate::py::PyForkHooks;
use crate::roles::{HttpFds, SchedulerFds};
#[cfg(feature = "php")]
use blastit_events::Runtime;
use blastit_events::{Component, Event, EventKind, Level, Stream};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::{Manifest, RoutingTable};
use blastit_scheduler::{MemoryProbe, SchedulerConfig};
use blastit_sys::{
    Fork, Pid, SeccompRole, Signal, SignalMask, drop_privileges, harden_child, pipe,
    restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};

use super::fail;

/// The front end's configuration (§8.2).
pub(super) fn http_config(config: &Config, limits: Limits) -> HttpConfig {
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
pub(super) fn scheduler_config(
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
pub(super) fn current_usage() -> Option<u64> {
    crate::mem_usage::read(Path::new("/")).map(|usage| usage.used())
}

/// The machine's memory, or a warning that §8 Sizing could not read it.
///
/// Unreadable memory leaves the view cap alone rather than guessing (§8), so
/// this is reported once and the caller carries on with `None`.
pub(super) fn read_memory(emitter: &Emitter) -> Option<mem_usage::Usage> {
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

/// The child handover [`fork_http`] forks with (§4).
///
/// One value instead of seven arguments. The fields are declared in the order
/// the call has always read, and the body destructures them back into the same
/// locals, so the fork sees exactly what it saw before. None of them owns a
/// descriptor another one needs: `child` and `fds` are handed to
/// [`prepare_child`] and the role together, and the only early return is a
/// failed `fork`, where every end is being torn down anyway.
pub(super) struct HttpFork<'a> {
    pub(super) child: ChildIo,
    pub(super) fds: HttpFds,
    pub(super) cfg: HttpConfig,
    pub(super) uid: Option<u32>,
    pub(super) mask: SignalMask,
    pub(super) seccomp: bool,
    /// The interpreter's three `PyOS_*Fork` hooks, when one is live in this
    /// image (§9.3): `None` in the `Embed` boot, whose forks precede the load.
    pub(super) hooks: Option<PyForkHooks<'a>>,
}

/// Forks the http process, which drops to `uid` and serves (§4).
///
/// `hosted` runs the fork under CPython's protocol when an interpreter is live
/// in this image (§9.3, [`crate::forking`]).
pub(super) fn fork_http(request: HttpFork<'_>) -> std::io::Result<Pid> {
    let HttpFork {
        child,
        fds,
        cfg,
        uid,
        mask,
        seccomp,
        hooks,
    } = request;
    match crate::forking::fork(hooks)? {
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

/// The child handover [`fork_scheduler`] forks with (§4).
///
/// The scheduler's one-shot handover, carrying the owned table, routes and
/// limits into the fork. The fields are declared in the order the call has
/// always read and destructured back into the same locals; nothing here owns a
/// descriptor another field depends on, and the only early return is a failed
/// `fork`.
pub(super) struct SchedulerFork<'a> {
    pub(super) child: ChildIo,
    pub(super) fds: SchedulerFds,
    pub(super) cfg: SchedulerConfig,
    pub(super) routes: Vec<blastit_scheduler::RouteInfo>,
    pub(super) table: RoutingTable,
    pub(super) limits: Limits,
    pub(super) uid: Option<u32>,
    pub(super) mask: SignalMask,
    pub(super) seccomp: bool,
    /// The same three hooks as `HttpFork`'s: `Some` when an interpreter is live.
    pub(super) hooks: Option<PyForkHooks<'a>>,
}

/// Forks the scheduler process, which drops to `uid` and routes (§4).
pub(super) fn fork_scheduler(request: SchedulerFork<'_>) -> std::io::Result<Pid> {
    let SchedulerFork {
        child,
        fds,
        cfg,
        routes,
        table,
        limits,
        uid,
        mask,
        seccomp,
        hooks,
    } = request;
    match crate::forking::fork(hooks)? {
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
pub(super) fn prepare_child(
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
pub(super) fn php_slots(
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
pub(super) fn php_max_requests(config: &Config) -> u32 {
    if config.view_max_requests == 0 {
        u32::MAX
    } else {
        u32::try_from(config.view_max_requests).unwrap_or(u32::MAX)
    }
}

/// The body the worker buffers before PHP runs (`post_max_size`'s twin).
#[cfg(feature = "php")]
pub(super) fn php_max_body_bytes(config: &Config) -> usize {
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
pub(super) fn emit_php_runtime(
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
pub(super) fn view_child_plans(
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
pub(super) fn component_limits(config: &Config) -> blastit_sys::Rlimits {
    blastit_sys::Rlimits {
        nproc: None,
        nofile: Some(config.component_nofile()),
        core_zero: true,
    }
}

/// `blastit_sys::pipe`, fatal on failure.
pub(super) fn pipe_ok(
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
pub(super) fn pair_ok(
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    what: &str,
) -> (OwnedFd, OwnedFd) {
    match socketpair_stream() {
        Ok(pair) => pair,
        Err(err) => fail(emitter, dev, &format!("{what}: {err}")),
    }
}
