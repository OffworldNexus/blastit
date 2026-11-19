use std::ffi::{CString, OsStr};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::UnixListener;
use std::path::Path;

use blastit_dbproxy::ListenerSpec;
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Stream,
};
use blastit_manifest::Manifest;
use blastit_sys::{bind_unix_listener, pipe};

use crate::cli::DevOptions;
use crate::config::{Config, LogFormat};
use crate::dev_learn::{DevRoot, PRIVILEGE};
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
use crate::host::{Host, Reexec};
use crate::logs::{Colour, Emitter, fail, output_event};
use crate::mem_usage;
// The `entrypoint php` discovery gate (`MB.36.5`): a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
#[cfg(feature = "php")]
use blastit_view_php::discovery::{PhpBoot, PhpShimScope};
// The discovery surface the `php_boot_gate` cases drive: their subjects are the
// crate's, so the names are the tests' only.

use super::fork_exec::{DevExec, exec_stage1_dev, fork_dbproxy_dev};
use super::gates::{app_rule, emit_runtime, route_declares_proxy, static_root_permission};
#[cfg(not(feature = "php"))]
use super::no_php_support;
#[cfg(not(feature = "python"))]
use super::no_python_support;
#[cfg(feature = "php")]
use super::php_boot_gate;

/// Shared stage-0 context of one `blastit dev` boot, once its [`DevRoot`]
/// exists (`R20`).
pub(super) struct DevBoot<'a> {
    pub(super) config: &'a Config,
    pub(super) host: &'a Host,
    pub(super) root: &'a DevRoot,
}

/// Fails a `blastit dev` boot with a diagnostic on stderr and exit `1`
/// (§1.1: a startup error writes zero bytes on stdout).
///
/// The root is removed by [`fail_dev_root`] or [`fail_with_root`] when one
/// exists; `_exit` runs no destructor, which is why every parent failure after
/// [`DevRoot::create`] goes through one of those helpers.
pub(super) fn dev_fail(message: &str) -> ! {
    eprintln!("blastit dev: {message}");
    crate::coverage::flush();
    // SAFETY: the boot is aborting; `_exit` runs no handler.
    unsafe { libc::_exit(1) }
}

/// Fails a `blastit dev` boot with a sentence that already names it.
///
/// The discovery's refusals start with `blastit {cmd}: `; a printer that added
/// its own head produced `blastit dev: blastit dev: …`. This one writes the
/// sentence verbatim on descriptor 2 and exits `1`.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub(super) fn dev_refuse(message: &str) -> ! {
    eprintln!("{message}");
    crate::coverage::flush();
    // SAFETY: the boot is aborting; `_exit` runs no handler.
    unsafe { libc::_exit(1) }
}

/// Removes `root` (`R20`) and aborts via [`dev_fail`].
pub(super) fn fail_dev_root(root: &DevRoot, message: &str) -> ! {
    root.cleanup();
    dev_fail(message)
}

/// The sentence a `blastit dev` boot above `BLASTIT_MEM_HIGH_WATERMARK` fails
/// with, or `None` when a view process can start (§8 Sizing).
///
/// The reading is taken once, at boot, and is the one the scheduler will take:
/// [`mem_usage::read`] leaves out the page cache and slab the kernel reclaims
/// on demand, so a machine with a warm filesystem cache is not mistaken for a
/// machine without memory. The scheduler retires one idle view for memory
/// pressure and refuses to *spawn* one while aggregate usage is above the
/// watermark; with no view yet there is nothing to evict, so a boot here would
/// serve nothing and say nothing — every request would wait out
/// `BLASTIT_QUEUE_TIMEOUT` and answer `503`. A reading that cannot be taken is
/// not pressure, exactly as in the scheduler (§9.2 #8).
pub(super) fn memory_reading() -> Option<mem_usage::Usage> {
    mem_usage::read(Path::new("/"))
}

/// [`memory_reading`]'s refusal, over the same reading the advisory uses.
pub(super) fn memory_refusal(usage: &mem_usage::Usage, config: &Config) -> Option<String> {
    mem_usage::boot_refusal(usage, config.mem_high_watermark_percent)
}

/// Removes `root` (`R20`) and aborts via [`fail`] on `emitter`.
pub(super) fn fail_with_root(emitter: &Emitter, root: &DevRoot, message: &str) -> ! {
    root.cleanup();
    fail(emitter, message)
}

/// `blastit_sys::pipe` for `dev` after [`DevRoot::create`]: cleans `root` on
/// failure before exiting (`R20`).
pub(super) fn pipe_dev(emitter: &Emitter, root: &DevRoot, what: &str) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pair) => pair,
        Err(err) => fail_with_root(emitter, root, &format!("{what}: {err}")),
    }
}

/// An argument or path as a [`CString`] after [`DevRoot::create`], cleaning
/// `root` if a NUL byte makes the re-exec impossible (`R20`).
pub(super) fn cstring_dev(root: &DevRoot, value: impl AsRef<OsStr>) -> CString {
    match CString::new(value.as_ref().as_bytes().to_vec()) {
        Ok(value) => value,
        Err(_) => fail_dev_root(root, "an argument contains a NUL byte"),
    }
}

/// [`reexec_argv`] for `dev` after [`DevRoot::create`], cleaning `root` on any
/// NUL-byte failure (`R20`).
pub(super) fn reexec_argv_dev(root: &DevRoot, reexec: &Reexec) -> Vec<CString> {
    let mut argv = vec![cstring_dev(root, &reexec.program)];
    argv.extend(reexec.prefix.iter().map(|arg| cstring_dev(root, arg)));
    argv
}

/// Runs `blastit dev`: the unprivileged single-uid simulation (`R19..R22`).
///
/// It is the same boot shape as [`run`] with three differences, all of them the
/// ones `ADR-0001` names. There is no capability detection and no `setresuid`:
/// every role runs as [`crate::dev_learn::euid`]. The runtime root is ephemeral
/// and `0700`, with one directory per route instead of one per view uid. And the
/// `execve` barrier's environment is the app environment itself — the only one
/// a single-uid simulation has — from which every `[proxy.*]` secret input,
/// every `BLASTIT_*` variable and every `BLASTIT_ENV_STRIP` name is already
/// gone.
///
/// `--learn` additionally hands dbproxy an observation pipe and an attribution
/// socket, and hands both to stage 1, which drains them and writes the manifest
/// on a clean `SIGTERM` (`R29`).
///
/// `colour` is the decision the command line made once (`A-4`).
pub fn run_dev(host: &Host, options: &DevOptions, colour: Colour) -> ! {
    // `S3`: a non-loopback bind has no authentication at all, so it needs the
    // second acknowledgement and says so before anything is written to stdout.
    if !crate::dev_learn::is_loopback(&options.host) && !crate::dev_learn::expose_requested() {
        eprintln!(
            "blastit dev: {}",
            crate::dev_learn::exposure_error(&options.host)
        );
        crate::coverage::flush();
        // SAFETY: nothing of blastit's is running yet; `_exit` runs no handler.
        unsafe { libc::_exit(1) }
    }
    let exposed = !crate::dev_learn::is_loopback(&options.host);

    // `R2`, `R10`: `pretty` by default and `BLASTIT_PRESET` never read.
    let mut config = match Config::for_dev() {
        Ok(config) => config,
        Err(err) => dev_fail(&err.to_string()),
    };
    config.port = options.port;
    if std::env::var_os("SERVER_PORT").is_none() {
        config.server_port = options.port;
    }
    // The privilege field is `root`/`capabilities` in the document; a dev boot
    // is neither, and stage 0 says `unprivileged-dev` in its own event below.
    config.privilege = None;
    let emitter = Emitter::stdout(config.log_format, config.log_level, colour);
    // Every fatal path before the banner writes here, on descriptor 2: a
    // startup error has to leave stdout empty (§1.1).
    let startup = Emitter::new(2, config.log_format, config.log_level, colour.stderr);

    // §8 Sizing: above `BLASTIT_MEM_HIGH_WATERMARK` the scheduler will not start
    // a view process, so a boot that ignores the reading serves nothing and says
    // nothing — every request waits out `BLASTIT_QUEUE_TIMEOUT` and answers
    // `503`. Refuse at boot instead, naming the setting and the reading (§1.1,
    // `docs/troubleshooting.md`). It is one reading for both the refusal and the
    // advisory below, so the two cannot disagree.
    let memory = memory_reading();
    if let Some(message) = memory
        .as_ref()
        .and_then(|usage| memory_refusal(usage, &config))
    {
        dev_fail(&message);
    }

    // The same order as `run`: the missing file first, the missing `app`
    // second (`docs/troubleshooting.md`), with §15.1's one sentence on
    // descriptor 2 — `dev` writes no startup error to stdout.
    let source = match crate::config::read_manifest(&config.manifest) {
        Ok(source) => source,
        Err(error) => dev_fail(&error.sentence(&config.manifest)),
    };
    let (manifest, warnings) = match Manifest::parse_blastfile(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => dev_fail(&format!("{errors}")),
    };
    // `R13`: a site whose mounts answer every request needs no `app`; anything
    // else still names the setting as before (`MB.1.6`), and a `BLASTIT_APP`
    // that could never run is refused rather than ignored.
    //
    // Before it: `ADR-0009` §5's capability gate, so a python-less build says
    // "install the Python package: uv add blastit" (or "add a [route.static]
    // block") instead of sending the operator to a `BLASTIT_APP` that cannot
    // work there.
    #[cfg(not(feature = "python"))]
    if let Some(message) = no_python_support(&config, &manifest, options.learn) {
        dev_fail(&message);
    }
    // Same order as `run`: (1) the `workers` budget, (2) the `libphp`/shim
    // discovery. `dev` may also look in `BLASTIT_PHP_SHIM_DIR` and the
    // `target/php-sapi` directory `make php-sapi` writes (`R8`), which is the
    // only difference, and a host without a usable library is refused here
    // exactly as in a deployment (`ADR-0010` §1: one backend, no fallback).
    // Nothing has been forked or bound yet.
    #[cfg(not(feature = "php"))]
    if let Some(message) = no_php_support(&manifest) {
        dev_fail(&message);
    }
    #[cfg(feature = "php")]
    match php_boot_gate(&config, &manifest, PhpShimScope::Development) {
        PhpBoot::Absent => {}
        PhpBoot::Refused(message) => dev_fail(&message),
        PhpBoot::Proceed(runtime) => {
            // The line naming what was resolved is the probe's
            // (`php_view::probe_bindings`, `MB.25.2`): only the process that ran
            // `php_module_startup` can say which `php.ini` the workers read, and
            // it says so once the boot is known-good. A pre-banner lane would
            // have to keep that sentence on descriptor 2 and still could not
            // name the ini.
            //
            // Stage 1 re-opens these and every PHP worker is forked from the
            // binding built there (`ADR-0010` §3).
            config.php = Some(runtime);
        }
    }
    if let Err(message) = app_rule(&config, &manifest) {
        dev_fail(&message);
    }
    // `R13` steps 4–5: the root has to exist, be listable, and be readable by
    // the uid that answers, which in `dev` is the caller (`R19`).
    if let Err(errors) = manifest.check_static_roots(&config.manifest_dir) {
        dev_fail(&format!("{}: {errors}", config.manifest.display()));
    }
    if let Some(message) =
        static_root_permission(&manifest, &config.manifest_dir, crate::dev_learn::euid())
    {
        dev_fail(&message);
    }
    let prepared = match blastit_env::build_from_process(&manifest, config.env_strip.as_deref()) {
        Ok(prepared) => prepared,
        Err(errors) => dev_fail(&format!("{errors}")),
    };
    config.proxy_outputs = prepared
        .outputs()
        .iter()
        .map(|(name, names)| (name.as_str().to_string(), names.clone()))
        .collect();
    config.sys_path = host.sys_path().to_vec();
    // `MB.40.8`, design §2.1/§2.4: a `dev` Python boot resolves its
    // interpreter, `libpython` and shim the same way a deployment does, but may
    // honour `$VIRTUAL_ENV`, `PATH` and `BLASTIT_PY_SHIM_DIR`. A PHP-only or
    // static-only boot returns before any search.
    #[cfg(feature = "python")]
    let python =
        match super::project_python::boot(&config, &manifest, super::project_python::Boot::Dev) {
            Ok(python) => python,
            Err(message) => dev_refuse(&message),
        };
    #[cfg(feature = "python")]
    if let Some(python) = &python {
        config.python = Some(python.handoff.clone());
        for entry in python.handoff.sys_path.iter().rev() {
            if !config.sys_path.iter().any(|known| known == entry) {
                config.sys_path.insert(0, entry.clone());
            }
        }
    }
    config.cwd = std::env::current_dir()
        .ok()
        .map(|dir| dir.into_os_string().into_vec());

    let root = match DevRoot::create() {
        Ok(root) => root,
        Err(err) => dev_fail(&format!("runtime root: {err}")),
    };
    // Under `--learn`, slot `0` (`views/0`) is the dedicated unrouted-request
    // fallback view (`LEARN_FALLBACK_ROUTE`), and declared routes sit at
    // `1..=N` so no declared route ever shares a socket with the fallback view.
    let view_slots = manifest.routes().len() + usize::from(options.learn);
    if let Err(err) = root.create_view_dirs(view_slots) {
        fail_dev_root(&root, &format!("runtime root: {err}"));
    }
    let boot = DevBoot {
        config: &config,
        host,
        root: &root,
    };

    // `R20`: one listener per (route, proxy) the route may use — every declared
    // `postgresql` proxy under `--learn`, so one pass can observe them all.
    let listeners = match dev_view_sockets(&root, &manifest, &prepared, options.learn) {
        Ok(listeners) => listeners,
        Err(message) => fail_dev_root(&root, &message),
    };

    // `--learn`: dbproxy writes observations to `obs_write` (attributing each
    // statement directly from its per-route `ListenerSpec.route`) and stage 1
    // drains `obs_read`.
    let learn = if options.learn {
        let (obs_read, obs_write) = match pipe() {
            Ok(pair) => pair,
            Err(err) => fail_dev_root(&root, &format!("dev obs pipe: {err}")),
        };
        Some((obs_read, obs_write))
    } else {
        None
    };

    // The dev policy channel (`MB.23.5.3`, §4.6 step 2): one socketpair, stage
    // 1's end carried across the `execve` as `--dev-policy-fd` and dbproxy's end
    // in its configuration document. `--no-reload` creates neither end, exactly
    // like the watcher, the marks and the report sockets.
    let policy = if options.no_reload {
        None
    } else {
        Some(dev_policy_channel(&startup, &root))
    };

    let dbproxy = fork_dbproxy_dev(
        &startup,
        &boot,
        &prepared,
        listeners,
        learn.as_ref().map(|(_, obs)| obs.as_raw_fd()),
        policy.as_ref().map(|(_, dbproxy)| dbproxy.as_raw_fd()),
    );
    // dbproxy has its own copy of its end now; the parent must not keep one, or
    // the channel would stay open after dbproxy exits.
    let policy = match policy {
        Some((stage1, dbproxy)) => {
            drop(dbproxy);
            Some(stage1)
        }
        None => None,
    };

    // The parent keeps the read end only.
    let learn = match learn {
        Some((obs_read, obs_write)) => {
            drop(obs_write);
            Some(obs_read)
        }
        None => None,
    };

    // Only now that the boot is known-good does anything reach stdout: the
    // banner, the boot event and the exposure warning (§1.2, §1.1).
    if config.log_format == LogFormat::Pretty {
        print!(
            "{}",
            crate::dev_learn::banner(
                &options.host,
                config.port,
                &manifest,
                exposed,
                options.learn
            )
        );
    }
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_seccomp(config.seccomp)
                .with_privilege(PRIVILEGE),
        ),
    ));
    for warning in &warnings {
        emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(WarningEvent::new(warning.to_string())),
        ));
    }
    // §8.1: one `runtime` line saying what this boot will run, now that the boot
    // is known-good (the banner and `boot` are already on stdout). PHP's line
    // comes from the probe below the fork; the other two are decided here.
    #[cfg(feature = "python")]
    let runtime_line = python
        .as_ref()
        .map(super::project_python::PythonBoot::runtime);
    #[cfg(not(feature = "python"))]
    let runtime_line = None;
    emit_runtime(&emitter, &config, runtime_line);
    // A boot above the documented default is the operator's call once they
    // raised the knob, and it is also the state in which a view's first spawn
    // may have to wait for memory: say so once, with the reading and the cache
    // the reading left out (§1.1).
    if let Some(message) = memory
        .as_ref()
        .and_then(|usage| mem_usage::advisory(usage, config.mem_high_watermark_percent))
    {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, &message).with_level(Level::Warn),
        );
    }
    if exposed {
        // Names the address, the port and the uid, and no event ever carries a
        // `port` *field* (§1.2, `R22`, `ARCHITECTURE.md` §8.1).
        emitter.emit(
            &output_event(
                Component::Init,
                None,
                Stream::Stderr,
                &format!(
                    "blastit dev is EXPOSED on {}:{} as uid {}: there is no authentication, \
                     so anyone who reaches it drives every route ({})",
                    options.host,
                    config.port,
                    crate::dev_learn::euid(),
                    crate::dev_learn::EXPOSE_VARIABLE
                ),
            )
            .with_level(Level::Warn),
        );
    }

    exec_stage1_dev(DevExec {
        boot: &boot,
        host_addr: options.host,
        app_env: prepared.app_env(),
        dbproxy,
        learn,
        policy,
        no_reload: options.no_reload,
    })
}

/// The dev-only dbproxy policy channel (`MB.23.5.3`, `ADR-0014` §4).
///
/// One `AF_UNIX`/`SOCK_STREAM` socketpair, created here and nowhere else: stage
/// 1's end travels across the `execve` as `--dev-policy-fd`, dbproxy's end
/// travels in its configuration document as `policy_fd` (legal only with
/// `dev_policy`). Both ends are close-on-exec at birth and switch to
/// non-blocking, because the supervisor writes its document through its poll set
/// and treats a short write as "wait for writability", never as a blocking call.
///
/// `--no-reload`, production, `check` and `test` never call this: the channel is
/// as dev-only as the watcher it exists to serve.
pub(super) fn dev_policy_channel(emitter: &Emitter, root: &DevRoot) -> (OwnedFd, OwnedFd) {
    let (stage1, dbproxy) = match blastit_sys::socketpair_stream() {
        Ok(pair) => pair,
        Err(err) => fail_with_root(emitter, root, &format!("dev policy socket: {err}")),
    };
    for end in [&stage1, &dbproxy] {
        if let Err(err) = rustix::fs::fcntl_setfl(end, rustix::fs::OFlags::NONBLOCK) {
            fail_with_root(emitter, root, &format!("dev policy socket: {err}"));
        }
    }
    (stage1, dbproxy)
}

/// Binds the per-route sockets of a `dev` boot (`R20`).
///
/// In production the (route, proxy) pairs are the route's effective grants; in
/// `dev --learn` every declared `postgresql` proxy is bound for every route, so
/// one pass sees the statements a route would need grants for.
pub(super) fn dev_view_sockets(
    root: &DevRoot,
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    learn: bool,
) -> Result<Vec<(UnixListener, ListenerSpec)>, String> {
    let uid = crate::dev_learn::euid();
    let mut listeners: Vec<(UnixListener, ListenerSpec)> = Vec::new();
    if learn {
        for (proxy_index, (proxy_name, _upstream)) in prepared.upstreams().iter().enumerate() {
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                return Err(format!(
                    "{}: no local port left for the postgresql proxies",
                    proxy_name.as_str()
                ));
            };
            let path = root.view_dir(0).join(format!(".s.PGSQL.{port}"));
            let listener = bind_unix_listener(&path, uid, 0o600)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            listeners.push((
                listener,
                ListenerSpec::enforcing(
                    crate::dev_learn::LEARN_FALLBACK_ROUTE,
                    proxy_name.as_str(),
                    uid,
                    Vec::new(),
                    false,
                    false,
                    Default::default(),
                ),
            ));
        }
    }
    let slot_offset = usize::from(learn);
    for (index, route) in manifest.routes().iter().enumerate() {
        let slot = index + slot_offset;
        for (proxy_index, (proxy_name, _upstream)) in prepared.upstreams().iter().enumerate() {
            let grant = manifest
                .effective_grants(index)
                .get(proxy_name)
                .and_then(|grant| grant.as_postgresql())
                .cloned();
            // A route with no declaration at all gets no socket. A route that
            // declares the proxy with an empty grant still gets one, so its
            // statements are denied with `42501` instead of the connection
            // failing with `ENOENT` (`docs/local.md`).
            if grant.is_none() && !learn && !route_declares_proxy(manifest, index, proxy_name) {
                continue;
            }
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                return Err(format!(
                    "{}: no local port left for the postgresql proxies",
                    proxy_name.as_str()
                ));
            };
            let path = root.view_dir(slot).join(format!(".s.PGSQL.{port}"));
            // `SO_PASSCRED` and `0600`, owned by the one uid that may connect:
            // in `dev` that is the caller, the only identity a view has.
            let listener = bind_unix_listener(&path, uid, 0o600)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let (queries, transaction_control, unrestricted, tables) = match &grant {
                Some(grant) => (
                    grant.queries().to_vec(),
                    grant.transaction_control(),
                    grant.is_unrestricted(),
                    grant.tables().clone(),
                ),
                None => (Vec::new(), false, false, Default::default()),
            };
            listeners.push((
                listener,
                ListenerSpec::enforcing(
                    route.name(),
                    proxy_name.as_str(),
                    uid,
                    queries,
                    transaction_control,
                    unrestricted,
                    tables,
                ),
            ));
        }
    }
    for (listener, spec) in &mut listeners {
        spec.fd = listener.as_raw_fd();
    }
    Ok(listeners)
}
