//! Stage 0: the privileged half of the boot (§4).
//!
//! ```text
//! boot → config → manifest → env swap → dirs + pg listeners
//!      → fork dbproxy → secrets.pipe → ready
//!      → execve barrier
//! ```
//!
//! Everything secret lives in this stage and is gone when it ends: the
//! platform's real credentials are read here, handed to dbproxy over a pipe, and
//! then the address space is replaced with the app environment (§4). Two rules
//! hold at every step: no secret is ever in `argv` or `environ`, and every
//! `fork` happens while init is single-threaded.
//!
//! One deviation from §4 is forced by dbproxy's own contract: when the manifest
//! grants nothing on any `postgresql` proxy there is no listener to serve and
//! `blastit_dbproxy::run_with_listeners` refuses to start without one. dbproxy is
//! therefore forked exactly when there is at least one listener, and not
//! otherwise.

use std::collections::BTreeMap;
use std::ffi::{CString, OsStr};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixListener;
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Stream,
};
use blastit_manifest::{Manifest, UidLayout};
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, bind_unix_listener, current_signal_mask, exec, harden_child,
    make_private_dir, pipe, restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::config::{Config, LogFormat, Mode, Privilege, view_dir};
use crate::host::{EXEC, Host, Install, Placement, Reexec};
use crate::logs::{Emitter, fail, output_event};

/// How long stage 0 waits for dbproxy's ready byte before giving up (§4).
pub const DBPROXY_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The uid init is running as (D6).
///
/// The effective uid, which is the one a `setuid` from it would have to be
/// authorised for.
fn current_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// The `boot` event of stage 0 (§8.1), with D6's mode when it is known.
fn stage0_lifecycle(privilege: Option<&Privilege>) -> Lifecycle {
    let lifecycle = Lifecycle::default()
        .with_stage("stage0")
        .with_seccomp(crate::config::seccomp_requested());
    match privilege {
        Some(privilege) => privilege.lifecycle(lifecycle),
        None => lifecycle,
    }
}

/// The `boot` event of the phase that parsed the configuration (§8.1, E1).
///
/// The first `boot` line is printed before anything can fail, so it knows
/// nothing of the configuration — not the mode, not the preset. This one is the
/// line that says what the boot *is*: which privilege mode init detected, what
/// the §7.3 filters are set to, and which preset supplied which defaults. It
/// comes before the manifest is read, so a boot that fails later still says
/// which preset it was failing with.
fn config_lifecycle(privilege: &Privilege, config: &Config) -> Lifecycle {
    let lifecycle = privilege.lifecycle(
        Lifecycle::default()
            .with_stage("config")
            .with_seccomp(config.seccomp),
    );
    match &config.preset {
        Some(choice) => choice.lifecycle(lifecycle),
        None => lifecycle,
    }
}

/// Warns when another process already runs as init's uid (D6).
///
/// The file that may carry the capabilities is mode `0500`, owned by the uid
/// init runs as, so that uid is the *only* one that can run it — and running it
/// is becoming init (`setuid(0)`, every sandbox opened). A worker, a migration
/// or an operator's shell that shares that uid therefore inherits init's whole
/// authority, and no amount of checking inside blastit can undo that: it is a
/// deployment decision. This is the one place blastit can see it, at boot, when
/// nothing of its own is running yet but somebody else may be.
///
/// It is a warning and not a refusal: a container legitimately has other
/// processes of that uid (`docker exec` for a debug shell, a sidecar that drops
/// privileges, this very check's own shell), and blastit cannot tell a debug
/// shell from a threat.
fn warn_about_siblings(emitter: &Emitter) {
    let uid = current_uid();
    let siblings = others_with_uid(uid);
    if siblings.is_empty() {
        return;
    }
    emitter.emit(
        &output_event(
            Component::Init,
            None,
            Stream::Stderr,
            &format!(
                "{} other process(es) run as uid {uid} ({}): that uid can run the file \
                 blastit's capabilities are on, so a worker, a migration or a shell \
                 should run as another uid (D6)",
                siblings.len(),
                siblings
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .with_level(Level::Warn),
    );
}

/// The pids, other than this process, that run as `uid` (D6).
///
/// Pure except for `/proc`: reading it is best effort — a pid that disappears
/// between the listing and the read is not a failure — and a `/proc` that cannot
/// be read at all reports nothing rather than failing a boot over a warning.
fn others_with_uid(uid: u32) -> Vec<i32> {
    let ours = std::process::id() as i32;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<i32> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<i32>().ok())
        .filter(|pid| *pid != ours)
        .filter(|pid| effective_uid(*pid) == Some(uid))
        .collect();
    pids.sort_unstable();
    pids
}

/// The effective uid of `pid`, from `/proc/<pid>/status` (D6).
///
/// `None` when the process is gone, or when its status cannot be read: a warning
/// is not worth failing a boot over.
fn effective_uid(pid: i32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}
fn privilege_from(held: &blastit_sys::Held) -> Privilege {
    Privilege {
        mode: match held.mode {
            blastit_sys::PrivilegeMode::Root => Mode::Root,
            blastit_sys::PrivilegeMode::Capabilities => Mode::Capabilities,
        },
        capabilities: match held.mode {
            // A rootless init is exactly what its permitted set says.
            blastit_sys::PrivilegeMode::Capabilities => held
                .capabilities
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            // Root's own set is the container's: §10 documents it, and the
            // event does not pretend to list it.
            blastit_sys::PrivilegeMode::Root => Vec::new(),
        },
        bounding_set_dropped: held.bounding_set_dropped,
    }
}

/// The sentence a boot that lacks capabilities fails with (D6/B1, B2).
///
/// [`blastit_sys::caps::Missing`] only states the fact (the uid and the missing
/// names), because *which file* should have been given them depends on the
/// installation — and this is the only place that knows it. Root is never told
/// to "start as root": it already is, and its capabilities come from the
/// container.
fn capability_advice(host: &Host, err: &std::io::Error) -> String {
    let install = host.install();
    let Some(blastit_sys::Error::MissingCapabilities { uid, missing }) =
        blastit_sys::Error::from_io(err)
    else {
        return err.to_string();
    };
    if *uid == 0 {
        return format!(
            "this boot runs as uid 0 and the container's effective capability set is \
             missing {missing}: uid 0 needs them as much as any other uid (they are what \
             `chown`s the view directories, `open`s them and `kill`s a view). Docker's \
             default capabilities include them — check `--cap-drop`, `--cap-add` and any \
             bounding-set restriction on the container.\n  \
             A rootless deployment gives them to a *dedicated copy* of the {} instead:\n  \
             {}\n  {}",
            install.file(),
            copy_recipe(install),
            install.naming()
        );
    }
    format!(
        "init runs as uid {uid} and is missing {missing}.\n  \
         A rootless deployment gives the capabilities to a *dedicated copy* of the {}, \
         never to the shared one:\n  {}\n  {}",
        install.file(),
        copy_recipe(install),
        install.naming()
    )
}

/// The four commands that make the capable copy, in this installation's words
/// (D6/B1, B2).
fn copy_recipe(install: Install) -> String {
    match install {
        Install::Wheel => format!(
            "cp \"$(readlink -f <the interpreter>)\" <same directory>/<interpreter>-init\n  \
             chown <the uid init runs as>:<its gid> <the copy>\n  chmod 0500 <the copy>\n  \
             setcap '{}+ep' <the copy>\n  \
             then start `<the copy> -m blastit` (the copy stays next to the original: \
             CPython finds its stdlib and its venv from the executable's own path)",
            blastit_sys::caps::Capability::SETCAP
        ),
        Install::Binary => format!(
            "cp <the blastit binary> <the binary>-init\n  \
             chown <the uid init runs as>:<its gid> <the copy>\n  chmod 0500 <the copy>\n  \
             setcap '{}+ep' <the copy>\n  \
             then start <the copy>, and set {EXEC}=<the blastit binary> so that \
             `--role=dbproxy` runs the capability-free copy",
            blastit_sys::caps::Capability::SETCAP,
        ),
    }
}

/// Which layout process, if any, already runs under `uid` (D6).
///
/// Pure, so the collision check is a unit test rather than a deployment
/// surprise. A rootless `init` runs as an ordinary uid, and the layout hands
/// every other process its own uid out of the same space; if init's uid *is* one
/// of them, the process that was supposed to be sandboxed under it shares init's
/// identity — it could read init's private state, and a `setuid` back to that
/// uid would be no change at all. `None` means the uid is free.
fn layout_collision(manifest: &Manifest, layout: &UidLayout, uid: u32) -> Option<String> {
    if uid == layout.http() {
        return Some("the http process".to_string());
    }
    if uid == layout.scheduler() {
        return Some("the scheduler".to_string());
    }
    if uid == layout.dbproxy() {
        return Some("dbproxy".to_string());
    }
    manifest
        .routes()
        .iter()
        .enumerate()
        .find(|(index, _)| layout.view(*index) == uid)
        .map(|(_, route)| format!("route `{}`", route.name()))
}

/// How long stage 0 keeps reading a failed dbproxy's output before giving up
/// (§8.1).
///
/// Short on purpose: init is already failing the boot, and the child that never
/// reported ready is still alive and still holds the write end of its pipes —
/// see [`emit_child_output`].
pub const CHILD_OUTPUT_TIMEOUT: Duration = Duration::from_millis(250);

/// Largest configuration document init will write into a pipe.
///
/// The pipe holds 64 KiB and stage 1 is not reading yet, so a larger document
/// would deadlock the writer. A real configuration is a few hundred bytes.
pub const MAX_CONFIG_DOCUMENT: usize = 60 * 1024;

/// The limits dbproxy runs under: a descriptor budget sized from the
/// configuration, and no core file (§7.2).
///
/// `RLIMIT_NPROC` belongs to the view uids, so it is left alone here.
fn dbproxy_limits(config: &Config) -> Rlimits {
    Rlimits {
        nproc: None,
        nofile: Some(config.component_nofile()),
        core_zero: true,
    }
}

/// Runs stage 0. Never returns: it `execve`s stage 1 or exits non-zero.
///
/// `host` says how blastit was installed: which program the barrier and
/// dbproxy's own `execve` start, and — for a wheel — the interpreter's import
/// path, which travels with the configuration because stage 1 is re-executed
/// with `-I` (§4).
///
/// `interpreter` is `true` when a live CPython is in this image (a wheel: the
/// console script's), which is what its `fork` has to account for (§9.3, and
/// [`crate::forking`]).
///
/// `preset` is `--preset` when the command line carried one, and it wins over
/// `BLASTIT_PRESET` (E1, [`Config::from_env`]).
pub fn run(host: &Host, interpreter: bool, preset: Option<&str>) -> ! {
    // §4: `boot` is the first event, emitted before anything can fail. The
    // configuration is not parsed yet, so this one line is always JSON — and the
    // §7.3 seccomp flag is read straight from its variable, because an operator
    // has to be able to see whether the filters are on (§8.1).
    let boot = Emitter::stdout(LogFormat::Json, Level::Info);
    // D6: root or a handful of capabilities is read from the kernel, and it
    // decides whether the boot can build its sandboxes at all. It comes before
    // the first event, because the event has to report it.
    let held = match blastit_sys::caps::detect() {
        Ok(held) => held,
        Err(err) => {
            // A boot that cannot read its own capability sets cannot say which
            // mode it is in. It says nothing about the mode rather than
            // guessing, and stops: a child that keeps a capability nobody
            // looked for is exactly what §4 refuses to risk. The advice is the
            // one an operator can act on, which is why it comes from here
            // (`Host` knows how blastit was installed) and not from the error.
            boot.emit(&Event::now(
                Component::Init,
                EventKind::Boot(stage0_lifecycle(None)),
            ));
            fail(&boot, &capability_advice(host, &err));
        }
    };
    let privilege = privilege_from(&held);
    // D6/B1, B2: the file that may carry the capabilities is init's alone. A
    // child that has dropped its bounding set cannot `execve` such a file at
    // all, and every process of init's uid can run it — so the boot checks
    // where each role's program comes from, and says what to fix.
    let placement = match host.placement(held.bounding_set_dropped) {
        Ok(placement) => placement,
        Err(message) => {
            boot.emit(&Event::now(
                Component::Init,
                EventKind::Boot(stage0_lifecycle(Some(&privilege))),
            ));
            fail(&boot, &message);
        }
    };
    boot.emit(&Event::now(
        Component::Init,
        EventKind::Boot(stage0_lifecycle(Some(&privilege))),
    ));
    if !held.bounding_set_dropped {
        // M1: the boot runs, and says so, once.
        boot.emit(
            &output_event(
                Component::Init,
                None,
                Stream::Stderr,
                "cap_setpcap is not held: the children keep their bounding set. Every \
                 other capability set is still cleared, and `no_new_privs` still stops \
                 an `execve` from granting what the bounding set allows (D6/M1)",
            )
            .with_level(Level::Warn),
        );
    }
    // D6: init's uid is a dedicated one. Whatever else runs as it can run the
    // file the capabilities are on — and becoming init is `setuid(0)`. A
    // deployment decides who that uid is (a worker, a migration, an operator's
    // shell all belong somewhere else); this says when it did not, once, before
    // anything is forked.
    warn_about_siblings(&boot);

    let mut config = match Config::from_env(preset) {
        Ok(config) => config,
        Err(err) => fail(&boot, &err.to_string()),
    };
    // D6: init's mode belongs in the configuration, because stage 1's preload
    // `boot` event reports the same thing stage 0's does. It is set here, before
    // anything can fail with it, so the `boot` line below can say it — and so
    // E1's preset assumption can be checked against it.
    config.privilege = Some(privilege.clone());
    let emitter = Emitter::stdout(config.log_format, config.log_level);
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(config_lifecycle(&privilege, &config)),
    ));
    // E1: a preset rests on a platform that hands init a particular identity, so
    // a boot that contradicts one says so here — before the manifest is read,
    // because a boot that fails on the manifest, on the environment rewrite or on
    // a view socket never reaches stage 1, and this is the line that explains
    // that boot to the operator who picked the preset.
    if let Some(warning) = config.preset_assumption_warning(privilege.mode) {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, warning.message())
                .with_level(Level::Warn),
        );
    }

    if config.app.is_none() {
        fail(
            &emitter,
            "BLASTIT_APP: the WSGI callable (`module:attribute`) is required",
        );
    }

    let source = match std::fs::read_to_string(&config.manifest) {
        Ok(source) => source,
        Err(err) => fail(&emitter, &format!("{}: {err}", config.manifest.display())),
    };
    let (manifest, warnings) = match Manifest::parse_and_validate(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    for warning in &warnings {
        emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(WarningEvent::new(warning.to_string())),
        ));
    }

    let prepared = match blastit_env::build_from_process(&manifest, config.env_strip.as_deref()) {
        Ok(prepared) => prepared,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    let layout = UidLayout::new(config.uid_base);
    // The rendered output variable *names* are what the `ready` event reports
    // (§8.1): they only exist here, after the rewrite, and stage 1 needs them.
    config.proxy_outputs = prepared
        .outputs()
        .iter()
        .map(|(name, names)| (name.as_str().to_string(), names.clone()))
        .collect();
    // D6: init is an ordinary uid in rootless mode, and the layout hands out
    // the uids of every other process. A collision would give a view init's
    // identity, so it is refused before any directory is created with it.
    if let Some(process) = layout_collision(&manifest, &layout, current_uid()) {
        fail(
            &emitter,
            &format!(
                "init runs as uid {}, which is the uid of {process} in the layout \
                 (BLASTIT_UID_BASE={}): run it as a uid below the layout, or raise \
                 BLASTIT_UID_BASE (D6)",
                current_uid(),
                config.uid_base
            ),
        );
    }

    let listeners = bind_view_sockets(&emitter, &manifest, &prepared, &layout);
    let dbproxy = fork_dbproxy(
        &emitter,
        &config,
        &placement,
        interpreter,
        &prepared,
        listeners,
    );

    // The interpreter's import path travels in the configuration, not in the
    // environment: stage 1 is `execve`'d as an interpreter of its own (§4), and
    // a wheel re-executes it with `-I`, which drops `PYTHONPATH` and the
    // working directory. Empty for the standalone binary.
    config.sys_path = host.sys_path().to_vec();

    // Everything a secret touched is gone from the process image in one step.
    exec_stage1(
        &emitter,
        &config,
        host,
        &placement,
        prepared.app_env(),
        dbproxy,
    )
}

/// Creates every view's private directory and binds the pg sockets it may use
/// (§2.2).
///
/// One listener per (route, `postgresql` proxy) the route has a grant on. The
/// socket's port comes from `blastit_env`'s per-proxy assignment, so every
/// process derives the same number from the same manifest (§5.2).
fn bind_view_sockets(
    emitter: &Emitter,
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    layout: &UidLayout,
) -> Vec<(UnixListener, ListenerSpec)> {
    let mut listeners = Vec::new();
    for (index, route) in manifest.routes().iter().enumerate() {
        let uid = layout.view(index);
        let dir = view_dir(uid);
        // Every route gets a directory: a view `chdir`s into it and uses it as
        // `HOME` and `TMPDIR` (§5.3, §7.2).
        if let Err(err) = make_private_dir(&dir, uid, 0o700) {
            fail(emitter, &format!("{}: {err}", dir.display()));
        }
        for (proxy_name, grant) in manifest.effective_grants(index) {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            let Some(proxy_index) = prepared
                .upstreams()
                .iter()
                .position(|(name, _)| name.as_str() == proxy_name.as_str())
            else {
                continue;
            };
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                fail(
                    emitter,
                    &format!(
                        "{}: no local port left for the postgresql proxies",
                        proxy_name.as_str()
                    ),
                );
            };
            let path = dir.join(format!(".s.PGSQL.{port}"));
            let listener = match bind_unix_listener(&path, uid, 0o600) {
                Ok(listener) => listener,
                Err(err) => fail(emitter, &format!("{}: {err}", path.display())),
            };
            listeners.push((
                listener,
                ListenerSpec {
                    fd: 0,
                    route: route.name().to_string(),
                    proxy: proxy_name.as_str().to_string(),
                    uid,
                    queries: pg.queries().to_vec(),
                    transaction_control: pg.transaction_control(),
                    unrestricted: pg.is_unrestricted(),
                    tables: pg.tables().clone(),
                },
            ));
        }
    }
    // The descriptor number of each listener is only known once the list is
    // built, and it is the number the child sees after the fork.
    for (listener, spec) in &mut listeners {
        spec.fd = listener.as_raw_fd();
    }
    listeners
}

/// Forks dbproxy, hands it the secrets and waits for its ready byte (§4).
///
/// Returns the pipes stage 1 needs to collect dbproxy's logs, or `None` when
/// there was no listener to serve.
fn fork_dbproxy(
    emitter: &Emitter,
    config: &Config,
    placement: &Placement,
    interpreter: bool,
    prepared: &blastit_env::Prepared,
    listeners: Vec<(UnixListener, ListenerSpec)>,
) -> Option<DbproxyHandover> {
    if listeners.is_empty() {
        return None;
    }

    let proxies: Vec<ProxySpec> = prepared
        .upstreams()
        .iter()
        .map(|(name, upstream)| ProxySpec {
            name: name.as_str().to_string(),
            host: upstream.host().to_string(),
            port: upstream.port(),
            user: upstream.user().to_string(),
            password: upstream.password().expose().to_string().into(),
            database: upstream.database().to_string(),
            sslmode: upstream.sslmode().map(str::to_string),
            ca_cert: upstream.ca_cert().map(|ca| ca.expose().to_string().into()),
            options: upstream.options().to_vec(),
            max_connections: config.max_connections_for(name.as_str()),
        })
        .collect();
    let document = DbproxyConfig {
        proxies,
        listeners: listeners.iter().map(|(_, spec)| spec.clone()).collect(),
    };

    let (secrets_read, secrets_write) = pipe_ok(emitter, "secrets.pipe");
    let (events_read, events_write) = pipe_ok(emitter, "dbproxy events pipe");
    let (stdout_read, stdout_write) = pipe_ok(emitter, "dbproxy stdout pipe");
    let (stderr_read, stderr_write) = pipe_ok(emitter, "dbproxy stderr pipe");
    let (ready_read, ready_write) = pipe_ok(emitter, "dbproxy ready pipe");

    let mut keep = vec![
        secrets_read.as_raw_fd(),
        events_write.as_raw_fd(),
        ready_write.as_raw_fd(),
    ];
    keep.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));

    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: dbproxy_limits(config),
        working_dir: None,
    };
    let uid = UidLayout::new(config.uid_base).dbproxy();

    match fork_ok(emitter, interpreter) {
        Fork::Parent(pid) => {
            // The parent keeps only the read ends, and neither end of the pg
            // listeners: dbproxy owns those after the fork.
            drop(secrets_read);
            drop(events_write);
            drop(ready_write);
            drop(child);
            drop(listeners);

            let mut secrets = std::fs::File::from(secrets_write);
            if let Err(err) = write_config(&document, &mut secrets) {
                fail(emitter, &format!("secrets.pipe: {err}"));
            }
            drop(secrets);

            if let Err(reason) = wait_ready(&ready_read, DBPROXY_READY_TIMEOUT) {
                // Whatever dbproxy said on the way out is in the pipes init is
                // holding for stage 1, and a boot that fails with nothing but
                // "exited" is a boot nobody can debug (§8.1). On a *timeout* the
                // child is still alive and still holds the write ends, which is
                // why the drain is bounded (see `emit_child_output`).
                emit_child_output(emitter, &stdout_read, Stream::Stdout);
                emit_child_output(emitter, &stderr_read, Stream::Stderr);
                fail(emitter, &reason);
            }
            Some(DbproxyHandover {
                pid,
                events: events_read,
                stdout: stdout_read,
                stderr: stderr_read,
            })
        }
        Fork::Child => {
            // D6/B2: the *checked* program, not `host.dbproxy()`: a standalone
            // install whose own file carries the capabilities must run this role
            // as the capability-free copy, or the `execve` itself fails.
            let mut argv = reexec_argv(emitter, &Reexec::program(&placement.dbproxy));
            argv.push(cstring(emitter, "--role=dbproxy"));
            argv.push(cstring(
                emitter,
                format!("--secrets-fd={}", secrets_read.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--events-fd={}", events_write.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--ready-fd={}", ready_write.as_raw_fd()),
            ));
            // dbproxy is exec'd with an empty environment (§4), so §7.3's
            // `BLASTIT_SECCOMP` reaches it as an argument instead. A wheel
            // install hands the same argument to the Python-free
            // `blastit-dbproxy` binary it ships, which takes the same flags.
            argv.push(cstring(
                emitter,
                format!("--seccomp={}", u8::from(config.seccomp)),
            ));
            let outcome = child
                .clear_cloexec(&[])
                .and_then(|()| child.prepare())
                .and_then(|()| blastit_sys::drop_privileges(uid))
                .and_then(|()| harden_child(Signal::KILL));
            if let Err(err) = outcome {
                fail(emitter, &format!("dbproxy child: {err}"));
            }
            let _ = set_dumpable(false);
            match current_signal_mask().and_then(restore_signal_mask_in_child) {
                Ok(()) => {}
                Err(err) => fail(emitter, &format!("dbproxy child: {err}")),
            }
            // An empty environment: dbproxy's `/proc/<pid>/environ` is empty,
            // and the credentials arrive over the pipe it already holds (§4).
            let err = exec(
                &program(emitter, &Reexec::program(&placement.dbproxy)),
                &argv,
                &[],
            );
            fail(emitter, &format!("exec dbproxy: {err}"));
        }
    }
}

/// What stage 1 needs from the dbproxy handover (§4).
pub struct DbproxyHandover {
    /// dbproxy's pid, so stage 1 can tell its exit from a view's.
    pub pid: Pid,
    /// The read end of dbproxy's events pipe.
    pub events: OwnedFd,
    /// The read end of dbproxy's stdout pipe.
    pub stdout: OwnedFd,
    /// The read end of dbproxy's stderr pipe.
    pub stderr: OwnedFd,
}

/// Waits for dbproxy's ready byte, with a deadline (§4).
///
/// `timeout` is the whole budget, and it is a parameter so that a test can watch
/// it expire in milliseconds instead of waiting out [`DBPROXY_READY_TIMEOUT`].
///
/// # Errors
///
/// A message saying how the wait ended — the deadline, a failed `poll`, or the
/// child exiting first. The caller prints it together with whatever the child
/// left in its output pipes.
fn wait_ready(ready: &OwnedFd, timeout: Duration) -> Result<(), String> {
    let deadline = Deadline::after(timeout);
    loop {
        if deadline.expired() {
            return Err(format!(
                "dbproxy did not report ready within {}s",
                timeout.as_secs_f32()
            ));
        }
        match wait_readable(ready.as_fd(), deadline.left()) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
        match read_one(ready.as_fd()) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => return Err("dbproxy exited before it was ready".to_string()),
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
    }
}

/// Prints what a child that died before reporting ready left in one of its
/// output pipes, as `app.output` events (§8.1).
///
/// This is the only place init reads a child's stdout or stderr itself: normally
/// stage 1 collects both, but a dbproxy that failed to start never gets that
/// far, and the reason it failed — a missing binary, a permission a dropped uid
/// does not have — is nothing but its own last words.
///
/// # Why the reads are non-blocking and bounded
///
/// A drain only *starts* once the wait for dbproxy's ready byte has failed, and
/// one of those failures is the timeout: there the child is very much alive and
/// still holds the write ends, so a plain read to `EOF` would block init
/// forever — turning a failure into a hang. The pipe is therefore switched to
/// non-blocking, and whatever has not arrived when [`CHILD_OUTPUT_TIMEOUT`] is
/// spent is not worth waiting for: init is on its way out.
fn emit_child_output(emitter: &Emitter, fd: &OwnedFd, stream: Stream) {
    /// Largest amount of a failed child's output init will log.
    const MAX: usize = 8 * 1024;

    let deadline = Deadline::after(CHILD_OUTPUT_TIMEOUT);
    // Best effort: a pipe that cannot be switched is still read to its current
    // end, and the deadline below bounds that too.
    let _ = rustix::fs::fcntl_setfl(fd, rustix::fs::OFlags::NONBLOCK);
    let mut bytes = vec![0_u8; MAX];
    let mut total = 0;
    while total < MAX {
        match rustix::io::read(fd, &mut bytes[total..]) {
            Ok(0) => break,
            Ok(count) => total += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => {
                // Nothing there *yet*: the child may still be alive and about to
                // say something, so wait for it — but not past the deadline.
                if deadline.expired()
                    || !wait_readable(fd.as_fd(), deadline.left()).unwrap_or(false)
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    bytes.truncate(total);
    for line in String::from_utf8_lossy(&bytes).lines() {
        emitter
            .emit(&output_event(Component::Dbproxy, None, stream, line).with_level(Level::Error));
    }
}

/// Crosses the `execve` barrier into stage 1 (§4).
///
/// The configuration is written into a pipe — it holds no secret, but it does
/// name every proxy variable — and exactly the descriptors stage 1 needs get
/// their `FD_CLOEXEC` cleared. Nothing secret is in the arguments or the
/// environment: `envp` is the app environment, from which every `BLASTIT_*`
/// variable, every proxy input and everything on `BLASTIT_ENV_STRIP` is already
/// gone.
fn exec_stage1(
    emitter: &Emitter,
    config: &Config,
    host: &Host,
    placement: &Placement,
    app_env: &BTreeMap<String, String>,
    dbproxy: Option<DbproxyHandover>,
) -> ! {
    let document = match serde_json::to_vec(&config.to_json()) {
        Ok(document) => document,
        Err(err) => fail(emitter, &format!("configuration document: {err}")),
    };
    if document.len() > MAX_CONFIG_DOCUMENT {
        fail(
            emitter,
            &format!(
                "the configuration document is {} bytes, more than the {MAX_CONFIG_DOCUMENT}-byte limit",
                document.len()
            ),
        );
    }
    let (config_read, config_write) = pipe_ok(emitter, "config pipe");
    let mut file = std::fs::File::from(config_write);
    if let Err(err) = file.write_all(&document) {
        fail(emitter, &format!("config pipe: {err}"));
    }
    drop(file);

    // D6/B1, B2: init's own image — the file a deployment may have given the
    // capabilities to. Stage 1 forks the children and drops their privileges,
    // so it needs them, and this `execve` comes from stage 0, whose bounding
    // set is still whole: the file capabilities apply.
    let stage1 = Reexec {
        program: placement.init.clone(),
        prefix: host.stage1().prefix.clone(),
    };
    let mut argv = reexec_argv(emitter, &stage1);
    argv.push(cstring(emitter, "--stage=1"));
    argv.push(cstring(
        emitter,
        format!("--config-fd={}", config_read.as_raw_fd()),
    ));
    let mut keep = vec![config_read.as_raw_fd()];
    if let Some(dbproxy) = &dbproxy {
        argv.push(cstring(
            emitter,
            format!("--dbproxy-pid={}", dbproxy.pid.as_raw()),
        ));
        for (flag, fd) in [
            ("--dbproxy-events-fd", &dbproxy.events),
            ("--dbproxy-stdout-fd", &dbproxy.stdout),
            ("--dbproxy-stderr-fd", &dbproxy.stderr),
        ] {
            argv.push(cstring(emitter, format!("{flag}={}", fd.as_raw_fd())));
            keep.push(fd.as_raw_fd());
        }
    }
    for fd in &keep {
        // SAFETY: every descriptor in `keep` is one this function created and
        // still owns.
        let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
        if let Err(err) = set_cloexec(borrowed, false) {
            fail(emitter, &format!("config pipe: {err}"));
        }
    }

    let env: Vec<(String, String)> = app_env
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let err = exec(&program(emitter, &stage1), &argv, &env);
    fail(emitter, &format!("exec stage 1: {err}"))
}

/// The program and its prefix, as the `argv` a re-exec starts with (§4).
///
/// `argv[0]` is the program itself, as `execve`'s callers conventionally do: for
/// the standalone binary that is `/proc/self/exe`, and for a wheel install it is
/// the interpreter — which is also what CPython wants there, because it reads
/// `argv[0]` to find the interpreter whose `site-packages` the app needs.
fn reexec_argv(emitter: &Emitter, reexec: &Reexec) -> Vec<CString> {
    let mut argv = vec![cstring(emitter, &reexec.program)];
    argv.extend(reexec.prefix.iter().map(|arg| cstring(emitter, arg)));
    argv
}

/// The `execve` path of a re-exec, as a C string.
///
/// # Panics
///
/// Never: a path with a NUL byte in it cannot be passed to `execve`, so
/// [`cstring`] turns it into a fatal error with a message instead.
fn program(emitter: &Emitter, reexec: &Reexec) -> CString {
    cstring(emitter, &reexec.program)
}

/// An argument, or a path, as a C string, or a fatal error naming what is
/// wrong.
fn cstring(emitter: &Emitter, value: impl AsRef<OsStr>) -> CString {
    match CString::new(value.as_ref().as_bytes().to_vec()) {
        Ok(value) => value,
        Err(_) => fail(emitter, "an argument contains a NUL byte"),
    }
}

/// `blastit_sys::pipe`, fatal on failure.
fn pipe_ok(emitter: &Emitter, what: &str) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pipe) => pipe,
        Err(err) => fail(emitter, &format!("{what}: {err}")),
    }
}

/// `fork()`, fatal on failure.
///
/// With a live interpreter in this image the fork runs under CPython's protocol
/// ([`crate::forking`]): a wheel's stage 0 forks dbproxy from inside the
/// console script's interpreter, and §9.3 applies to it as much as to a view.
fn fork_ok(emitter: &Emitter, interpreter: bool) -> Fork {
    match crate::forking::fork(interpreter) {
        Ok(fork) => fork,
        Err(err) => fail(emitter, &format!("fork: {err}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Read;
    use std::time::Instant;

    use super::*;

    /// Everything an emitter wrote to `read_end`, once its peer is closed.
    fn captured(fd: OwnedFd) -> String {
        let mut file = std::fs::File::from(fd);
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    /// Drains `child_output` and returns what was logged, with how long the
    /// drain itself took.
    fn drain(child_output: &OwnedFd, stream: Stream) -> (String, Duration) {
        let (capture_read, capture_write) = pipe().unwrap();
        let emitter = Emitter::new(
            capture_write.as_raw_fd(),
            LogFormat::Json,
            Level::Info,
            false,
        );
        let started = Instant::now();
        emit_child_output(&emitter, child_output, stream);
        let elapsed = started.elapsed();
        drop(capture_write);
        (captured(capture_read), elapsed)
    }

    /// The read end of a pipe whose write end the caller keeps for itself.
    fn output_pipe() -> (OwnedFd, OwnedFd) {
        pipe().unwrap()
    }

    /// A manifest with `routes` routes, in the order they matter here.
    fn manifest_with(routes: usize) -> Manifest {
        let mut source = String::from("version = 1\n");
        for index in 0..routes {
            source.push_str(&format!(
                "\n[[route]]\nname = \"r{index}\"\npath = ['/r{index}/']\n"
            ));
        }
        Manifest::parse_and_validate(&source, 20_000).unwrap().0
    }

    #[test]
    fn the_layout_collision_check_knows_every_process_it_hands_a_uid_to() {
        // D6: init is an ordinary uid in rootless mode, so its own uid must not
        // be one the layout gives away.
        let manifest = manifest_with(3);
        let layout = UidLayout::new(20_000);
        assert_eq!(
            layout_collision(&manifest, &layout, 20_001).unwrap(),
            "the http process"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_002).unwrap(),
            "the scheduler"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_003).unwrap(),
            "dbproxy"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_000).unwrap(),
            "route `r0`"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_002).unwrap(),
            "route `r2`"
        );
    }

    #[test]
    fn a_uid_outside_the_layout_is_free() {
        let manifest = manifest_with(2);
        let layout = UidLayout::new(20_000);
        // The ordinary case: a service uid, and root.
        assert!(layout_collision(&manifest, &layout, 0).is_none());
        assert!(layout_collision(&manifest, &layout, 1000).is_none());
        // A view uid of a route this manifest does not have is free too.
        assert!(layout_collision(&manifest, &layout, 21_002).is_none());
        // And the whole check disappears when the base is a high, private one.
        let layout = UidLayout::new(100_000);
        assert!(layout_collision(&manifest, &layout, 20_001).is_none());
        assert_eq!(
            layout_collision(&manifest, &layout, 101_001).unwrap(),
            "route `r1`"
        );
    }

    #[test]
    fn the_stage_zero_boot_event_says_the_mode() {
        let root = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        }));
        assert_eq!(root.stage(), Some("stage0"));
        assert_eq!(root.privilege(), Some("root"));
        assert!(root.capabilities().is_empty());
        assert_eq!(root.bounding_set(), Some(true));

        let rootless = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: false,
        }));
        assert_eq!(rootless.privilege(), Some("capabilities"));
        assert_eq!(rootless.capabilities(), ["cap_chown", "cap_setuid"]);
        assert_eq!(
            rootless.bounding_set(),
            Some(false),
            "M1: a deployment without cap_setpcap says so"
        );

        // A boot that could not read its own capability sets says nothing about
        // the mode rather than guessing one.
        let unknown = stage0_lifecycle(None);
        assert_eq!(unknown.stage(), Some("stage0"));
        assert_eq!(unknown.privilege(), None);
        assert_eq!(unknown.bounding_set(), None);
    }

    #[test]
    fn the_configuration_boot_event_says_the_mode_and_the_preset() {
        // E1: the first `boot` line is printed before the configuration is
        // read, so this is the one that says what the boot is: the mode init
        // detected, whether the filters are on, and which preset supplied which
        // defaults.
        let privilege = Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_setuid".to_string()],
            bounding_set_dropped: true,
        };
        let mut config = Config::default();
        assert_eq!(
            config_lifecycle(&privilege, &config).stage(),
            Some("config")
        );
        assert_eq!(
            config_lifecycle(&privilege, &config).preset(),
            None,
            "no preset: the event says null, which the encoder writes"
        );

        config.preset = Some(crate::config::PresetChoice {
            name: "compose-traefik".to_string(),
            supplied: std::collections::BTreeMap::from([(
                "BLASTIT_CLIENT_IP_HEADER".to_string(),
                "x-forwarded-for".to_string(),
            )]),
            overridden: vec!["BLASTIT_LOG_FORMAT".to_string()],
        });
        let lifecycle = config_lifecycle(&privilege, &config);
        assert_eq!(lifecycle.stage(), Some("config"));
        assert_eq!(lifecycle.privilege(), Some("capabilities"));
        assert_eq!(lifecycle.capabilities(), ["cap_setuid"]);
        assert_eq!(lifecycle.seccomp(), Some(true), "the effective setting");
        assert_eq!(lifecycle.preset(), Some("compose-traefik"));
        assert_eq!(
            lifecycle.preset_settings().get("BLASTIT_CLIENT_IP_HEADER"),
            Some(&"x-forwarded-for".to_string())
        );
        assert_eq!(lifecycle.preset_overridden(), ["BLASTIT_LOG_FORMAT"]);

        // The filters being off is the configuration's word, not the
        // variable's: a `boot` line that disagrees with the running server is
        // the failure this field exists to prevent (§7.3).
        config.seccomp = false;
        assert_eq!(config_lifecycle(&privilege, &config).seccomp(), Some(false));
    }

    #[test]
    fn the_detected_mode_becomes_the_boot_field() {
        let root = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        });
        assert_eq!(root.mode, Mode::Root);
        assert!(root.capabilities.is_empty());
        assert!(root.bounding_set_dropped);
        let rootless = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Capabilities,
            capabilities: vec!["cap_chown", "cap_setuid"],
            bounding_set_dropped: false,
        });
        assert_eq!(rootless.mode, Mode::Capabilities);
        assert_eq!(rootless.capabilities, ["cap_chown", "cap_setuid"]);
        assert!(!rootless.bounding_set_dropped);
        assert_eq!(rootless.mode.as_str(), "capabilities");
    }
    /// A dbproxy that printed something and then hung holds its write end open:
    /// the drain has to read what it said without waiting for the process.
    #[test]
    fn a_live_child_that_printed_is_still_logged() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"exec dbproxy: Permission denied\n").unwrap();

        let (text, elapsed) = drain(&read_end, Stream::Stdout);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain waited for a live child: {elapsed:?}"
        );
        assert!(text.contains("Permission denied"), "{text}");
        assert!(text.contains(r#""component":"dbproxy""#), "{text}");
        assert!(text.contains(r#""event":"app.output""#), "{text}");
    }

    /// And a live child that said *nothing* must not block the boot either: this
    /// is the timeout case, where the process is still alive and still holds the
    /// write end of both pipes.
    #[test]
    fn a_silent_live_child_does_not_block_the_drain() {
        let (read_end, _write_end) = output_pipe();

        let (text, elapsed) = drain(&read_end, Stream::Stderr);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain hung: {elapsed:?}"
        );
        assert!(text.is_empty(), "nothing was written: {text}");
    }

    /// The ordinary failure: dbproxy is gone, and what it printed on the way out
    /// is the only explanation of why.
    #[test]
    fn a_dead_child_output_becomes_app_output_events() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"first\nsecond\n").unwrap();
        drop(write_end);

        let (text, _) = drain(&read_end, Stream::Stdout);
        assert!(text.contains("first"), "{text}");
        assert!(text.contains("second"), "{text}");
    }

    /// The wait itself: a dbproxy that never writes must expire the deadline —
    /// and quickly, which is why the timeout is a parameter.
    #[test]
    fn a_dbproxy_that_never_reports_ready_times_out() {
        let (ready, _write_end) = output_pipe();
        let started = Instant::now();
        let err = wait_ready(&ready, Duration::from_millis(50)).unwrap_err();
        assert!(err.contains("did not report ready within"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the deadline was not honoured: {:?}",
            started.elapsed()
        );
    }

    /// And a dbproxy that died first is reported as that, not as a timeout.
    #[test]
    fn a_dbproxy_that_exits_before_ready_is_reported_as_such() {
        let (ready, write_end) = output_pipe();
        drop(write_end);
        let err = wait_ready(&ready, Duration::from_secs(30)).unwrap_err();
        assert_eq!(err, "dbproxy exited before it was ready");
    }
}
