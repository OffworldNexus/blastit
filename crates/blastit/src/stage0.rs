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
use std::ffi::CString;
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent,
};
use blastit_manifest::{Manifest, UidLayout};
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, bind_unix_listener, current_signal_mask, exe_path, exec, fork,
    harden_child, make_private_dir, pipe, restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::config::{Config, LogFormat, view_dir};
use crate::logs::{Emitter, fail};

/// How long stage 0 waits for dbproxy's ready byte before giving up (§4).
pub const DBPROXY_READY_TIMEOUT: Duration = Duration::from_secs(30);

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
pub fn run() -> ! {
    // §4: `boot` is the first event, emitted before anything can fail. The
    // configuration is not parsed yet, so this one line is always JSON — and the
    // §7.3 seccomp flag is read straight from its variable, because an operator
    // has to be able to see whether the filters are on (§8.1).
    let boot = Emitter::stdout(LogFormat::Json, Level::Info);
    boot.emit(&Event::now(
        Component::Init,
        EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_seccomp(crate::config::seccomp_requested()),
        ),
    ));

    let mut config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => fail(&boot, &err.to_string()),
    };
    let emitter = Emitter::stdout(config.log_format, config.log_level);

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

    let listeners = bind_view_sockets(&emitter, &manifest, &prepared, &layout);
    let dbproxy = fork_dbproxy(&emitter, &config, &prepared, listeners);

    // Everything a secret touched is gone from the process image in one step.
    exec_stage1(&emitter, &config, prepared.app_env(), dbproxy)
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

    match fork_ok(emitter) {
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

            wait_ready(emitter, &ready_read);
            Some(DbproxyHandover {
                pid,
                events: events_read,
                stdout: stdout_read,
                stderr: stderr_read,
            })
        }
        Fork::Child => {
            let argv = vec![
                cstring(emitter, "blastit"),
                cstring(emitter, "--role=dbproxy"),
                cstring(
                    emitter,
                    &format!("--secrets-fd={}", secrets_read.as_raw_fd()),
                ),
                cstring(
                    emitter,
                    &format!("--events-fd={}", events_write.as_raw_fd()),
                ),
                cstring(emitter, &format!("--ready-fd={}", ready_write.as_raw_fd())),
                // dbproxy is exec'd with an empty environment (§4), so §7.3's
                // `BLASTIT_SECCOMP` reaches it as an argument instead.
                cstring(emitter, &format!("--seccomp={}", u8::from(config.seccomp))),
            ];
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
            let err = exec(&exe_cstring(emitter), &argv, &[]);
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
fn wait_ready(emitter: &Emitter, ready: &OwnedFd) {
    let deadline = Deadline::after(DBPROXY_READY_TIMEOUT);
    loop {
        if deadline.expired() {
            fail(
                emitter,
                &format!(
                    "dbproxy did not report ready within {}s",
                    DBPROXY_READY_TIMEOUT.as_secs()
                ),
            );
        }
        match wait_readable(ready.as_fd(), deadline.left()) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => fail(emitter, &format!("waiting for dbproxy: {err}")),
        }
        match read_one(ready.as_fd()) {
            Ok(Some(_)) => return,
            Ok(None) => fail(emitter, "dbproxy exited before it was ready"),
            Err(err) => fail(emitter, &format!("waiting for dbproxy: {err}")),
        }
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

    let mut argv = vec![
        cstring(emitter, "blastit"),
        cstring(emitter, "--stage=1"),
        cstring(emitter, &format!("--config-fd={}", config_read.as_raw_fd())),
    ];
    let mut keep = vec![config_read.as_raw_fd()];
    if let Some(dbproxy) = &dbproxy {
        argv.push(cstring(
            emitter,
            &format!("--dbproxy-pid={}", dbproxy.pid.as_raw()),
        ));
        for (flag, fd) in [
            ("--dbproxy-events-fd", &dbproxy.events),
            ("--dbproxy-stdout-fd", &dbproxy.stdout),
            ("--dbproxy-stderr-fd", &dbproxy.stderr),
        ] {
            argv.push(cstring(emitter, &format!("{flag}={}", fd.as_raw_fd())));
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
    let err = exec(&exe_cstring(emitter), &argv, &env);
    fail(emitter, &format!("exec stage 1: {err}"))
}

/// `/proc/self/exe` as a C string.
fn exe_cstring(emitter: &Emitter) -> CString {
    let path: PathBuf = exe_path().to_path_buf();
    let bytes = path.as_os_str().as_bytes().to_vec();
    match CString::new(bytes) {
        Ok(path) => path,
        Err(_) => fail(emitter, "the executable path contains a NUL byte"),
    }
}

/// A C string, or a fatal error naming the value.
fn cstring(emitter: &Emitter, value: &str) -> CString {
    match CString::new(value) {
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

/// `blastit_sys::fork`, fatal on failure.
fn fork_ok(emitter: &Emitter) -> Fork {
    match fork() {
        Ok(fork) => fork,
        Err(err) => fail(emitter, &format!("fork: {err}")),
    }
}
