use std::collections::BTreeMap;
use std::ffi::{CString, OsStr};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixListener;
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{Component, Level, Stream};
use blastit_manifest::UidLayout;
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, current_signal_mask, exec, harden_child, pipe,
    restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::config::Config;
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
use crate::host::{Host, Install, Placement, Reexec};
use crate::logs::{Emitter, fail, output_event};
// The `entrypoint php` discovery gate (`MB.36.5`): a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
// The discovery surface the `php_boot_gate` cases drive: their subjects are the
// crate's, so the names are the tests' only.

use super::dev::{DevBoot, cstring_dev, fail_dev_root, fail_with_root, pipe_dev, reexec_argv_dev};
use super::gates::{RESTART_MEMBER, config_document};
use super::{CHILD_OUTPUT_TIMEOUT, DBPROXY_READY_TIMEOUT, MAX_CONFIG_DOCUMENT};

/// How long dbproxy has to report ready before the wait is worth a line (§1.2).
///
/// The ordinary ready byte lands in a few milliseconds. A boot still waiting
/// after this is the slow one the phase line exists for, so the line is printed
/// then — and never on the fast path, where it reads as a stall above a banner
/// that is already there.
const SLOW_READY_LINE: Duration = Duration::from_millis(200);

/// The limits dbproxy runs under: a descriptor budget sized from the
/// configuration, and no core file (§7.2).
///
/// `RLIMIT_NPROC` belongs to the view uids, so it is left alone here.
pub(super) fn dbproxy_limits(config: &Config) -> Rlimits {
    Rlimits {
        nproc: None,
        nofile: Some(config.component_nofile()),
        core_zero: true,
    }
}
/// Forks dbproxy for a `dev` boot (`R19`, `R21`).
///
/// The child is not `setresuid`'d: it already runs as the caller. Everything
/// else is the production sequence — empty `environ`, `PDEATHSIG`, no
/// `no_new_privs` exceptions, the role's seccomp filter — and the configuration
/// carries the `--learn` descriptors when the boot asked for them.
pub(super) fn fork_dbproxy_dev(
    emitter: &Emitter,
    boot: &DevBoot<'_>,
    interpreter: bool,
    prepared: &blastit_env::Prepared,
    listeners: Vec<(UnixListener, ListenerSpec)>,
    obs_fd: Option<std::os::fd::RawFd>,
    policy_fd: Option<std::os::fd::RawFd>,
) -> Option<DbproxyHandover> {
    if listeners.is_empty() {
        return None;
    }
    let uid = crate::dev_learn::euid();
    let proxies: Vec<ProxySpec> =
        crate::dbproxy_wiring::proxy_specs(prepared, |name| boot.config.max_connections_for(name));
    let mut document = DbproxyConfig::enforcing(
        proxies,
        listeners.iter().map(|(_, spec)| spec.clone()).collect(),
    );
    document.mode = if obs_fd.is_some() {
        blastit_dbproxy::ListenerMode::Learn
    } else {
        blastit_dbproxy::ListenerMode::Enforce
    };
    // In `dev` the listener's uid is the caller's; root is only ever the caller
    // when the whole suite runs as root, and then it *is* the view's uid.
    document.allow_root_listener = uid == 0;
    document.obs_fd = obs_fd;
    // The dev policy channel's dbproxy end (`MB.23.5.3`): the document is the
    // only way dbproxy learns the descriptor, and `dev_policy` is what makes it
    // legal. Both are `None`/`false` on every boot this function is not called
    // for, which is what keeps a production dbproxy unable to acquire it.
    document.policy_fd = policy_fd;
    document.dev_policy = policy_fd.is_some();

    let (secrets_read, secrets_write) = pipe_dev(emitter, boot.root, "secrets.pipe");
    let (events_read, events_write) = pipe_dev(emitter, boot.root, "dbproxy events pipe");
    let (stdout_read, stdout_write) = pipe_dev(emitter, boot.root, "dbproxy stdout pipe");
    let (stderr_read, stderr_write) = pipe_dev(emitter, boot.root, "dbproxy stderr pipe");
    let (ready_read, ready_write) = pipe_dev(emitter, boot.root, "dbproxy ready pipe");

    let mut keep = vec![
        secrets_read.as_raw_fd(),
        events_write.as_raw_fd(),
        ready_write.as_raw_fd(),
    ];
    keep.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));
    if let Some(obs) = obs_fd {
        keep.push(obs);
    }
    if let Some(policy) = policy_fd {
        keep.push(policy);
    }

    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: dbproxy_limits(boot.config),
        working_dir: None,
    };

    let forked = match crate::forking::fork(crate::py::runtime_hooks(interpreter)) {
        Ok(fork) => fork,
        Err(err) => fail_with_root(emitter, boot.root, &format!("fork: {err}")),
    };
    match forked {
        Fork::Parent(pid) => {
            drop(secrets_read);
            drop(events_write);
            drop(ready_write);
            drop(child);
            drop(listeners);

            let mut secrets = std::fs::File::from(secrets_write);
            if let Err(err) = write_config(&document, &mut secrets) {
                fail_with_root(emitter, boot.root, &format!("secrets.pipe: {err}"));
            }
            drop(secrets);

            // `§1.2`: a line only if the wait is actually slow — a ready byte in
            // the ordinary few milliseconds is not worth a stall warning above
            // the banner.
            if let Err(reason) = wait_ready_with_line(emitter, &ready_read) {
                emit_child_output(emitter, &stdout_read, Stream::Stdout);
                emit_child_output(emitter, &stderr_read, Stream::Stderr);
                fail_with_root(emitter, boot.root, &reason);
            }
            Some(DbproxyHandover {
                pid,
                events: events_read,
                stdout: stdout_read,
                stderr: stderr_read,
            })
        }
        Fork::Child => {
            let mut argv = reexec_argv(emitter, boot.host.dbproxy());
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
            argv.push(cstring(
                emitter,
                format!("--seccomp={}", u8::from(boot.config.seccomp)),
            ));
            let outcome = child
                .clear_cloexec(&[])
                .and_then(|()| child.prepare())
                .and_then(|()| harden_child(Signal::KILL));
            if let Err(err) = outcome {
                fail(emitter, &format!("dbproxy child: {err}"));
            }
            let _ = set_dumpable(false);
            match current_signal_mask().and_then(restore_signal_mask_in_child) {
                Ok(()) => {}
                Err(err) => fail(emitter, &format!("dbproxy child: {err}")),
            }
            let err = exec(&program(emitter, boot.host.dbproxy()), &argv, &[]);
            fail(emitter, &format!("exec dbproxy: {err}"));
        }
    }
}

/// What one `blastit dev` boot hands the `execve` barrier (`R20`).
///
/// The fields are declared in the order the barrier has always taken them and
/// destructured back into the same locals, so the `argv`, the `keep` list and
/// every drop point below are the ones that were there before. `dbproxy`,
/// `learn` and `policy` are the only owning descriptors, and every path out of
/// [`exec_stage1_dev`] is an `execve` or an `_exit`: none of them is dropped, so
/// their order cannot close one early.
pub(super) struct DevExec<'a> {
    pub(super) boot: &'a DevBoot<'a>,
    pub(super) host_addr: std::net::IpAddr,
    pub(super) app_env: &'a BTreeMap<String, String>,
    pub(super) dbproxy: Option<DbproxyHandover>,
    pub(super) learn: Option<OwnedFd>,
    pub(super) policy: Option<OwnedFd>,
    pub(super) no_reload: bool,
}

/// Crosses the `dev` `execve` barrier into stage 1 (`R20`).
pub(super) fn exec_stage1_dev(request: DevExec<'_>) -> ! {
    let DevExec {
        boot,
        host_addr,
        app_env,
        dbproxy,
        learn,
        policy,
        no_reload,
    } = request;
    let stage1 = boot.host.stage1();
    let mut document_value = match config_document(boot.config) {
        Ok(value) => value,
        Err(message) => fail_dev_root(boot.root, &message),
    };
    // The stage-1 program prefix a reload cycle re-execs (§4.6 step 6): the
    // standalone binary, or `python -I -m blastit` for a wheel. Stage 1 is the
    // only stage that can cycle and the only one that cannot see `Host`, so
    // stage 0 records the answer once here rather than let stage 1 guess from
    // `/proc/self/exe`.
    if let Some(object) = document_value.as_object_mut() {
        let mut restart: Vec<String> = vec![stage1.program.to_string_lossy().into_owned()];
        restart.extend(
            stage1
                .prefix
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned()),
        );
        object.insert(RESTART_MEMBER.to_string(), serde_json::Value::from(restart));
    }
    let document = match serde_json::to_vec(&document_value) {
        Ok(document) => document,
        Err(err) => fail_dev_root(boot.root, &format!("configuration document: {err}")),
    };
    if document.len() > MAX_CONFIG_DOCUMENT {
        fail_dev_root(
            boot.root,
            &format!(
                "the configuration document is {} bytes, more than the {MAX_CONFIG_DOCUMENT}-byte limit",
                document.len()
            ),
        );
    }
    let (config_read, config_write) = match pipe() {
        Ok(pair) => pair,
        Err(err) => fail_dev_root(boot.root, &format!("config pipe: {err}")),
    };
    let mut file = std::fs::File::from(config_write);
    if let Err(err) = file.write_all(&document) {
        fail_dev_root(boot.root, &format!("config pipe: {err}"));
    }
    drop(file);

    let mut argv = reexec_argv_dev(boot.root, stage1);
    argv.push(cstring_dev(boot.root, "--stage=1"));
    argv.push(cstring_dev(
        boot.root,
        format!("--config-fd={}", config_read.as_raw_fd()),
    ));
    argv.push(cstring_dev(
        boot.root,
        format!("--dev-root={}", boot.root.path().display()),
    ));
    argv.push(cstring_dev(boot.root, format!("--dev-host={host_addr}")));
    let mut keep = vec![config_read.as_raw_fd()];
    if let Some(dbproxy) = &dbproxy {
        argv.push(cstring_dev(
            boot.root,
            format!("--dbproxy-pid={}", dbproxy.pid.as_raw()),
        ));
        for (flag, fd) in [
            ("--dbproxy-events-fd", &dbproxy.events),
            ("--dbproxy-stdout-fd", &dbproxy.stdout),
            ("--dbproxy-stderr-fd", &dbproxy.stderr),
        ] {
            argv.push(cstring_dev(boot.root, format!("{flag}={}", fd.as_raw_fd())));
            keep.push(fd.as_raw_fd());
        }
    }
    if let Some(obs) = &learn {
        argv.push(cstring_dev(
            boot.root,
            format!("--dev-obs-fd={}", obs.as_raw_fd()),
        ));
        keep.push(obs.as_raw_fd());
    }
    // Design §2.4: the two pinned descriptors cross as numbers, and only these
    // two lose `FD_CLOEXEC` for this `execve` — stage 1 re-sets it at once.
    if let Some(python) = &boot.config.python {
        let (lib, shim) = python.fds();
        argv.push(cstring_dev(boot.root, format!("--py-lib-fd={lib}")));
        argv.push(cstring_dev(boot.root, format!("--py-shim-fd={shim}")));
        keep.push(lib);
        keep.push(shim);
    }
    // The dev policy channel's stage-1 end (`MB.23.5.3`): the supervisor writes
    // the document a `Blastfile` save produced and waits for the one-byte ack,
    // and carries this descriptor into the reload adoption document.
    if let Some(policy) = &policy {
        argv.push(cstring_dev(
            boot.root,
            format!("--dev-policy-fd={}", policy.as_raw_fd()),
        ));
        keep.push(policy.as_raw_fd());
    }
    // `--no-reload` is the user's flag and stage 1 is where the watcher would be
    // built, so this is the one token that carries it across the `execve`
    // (§4.1: off means no group, no mark, no hook, no report socket).
    if no_reload {
        argv.push(cstring_dev(boot.root, "--dev-reload=off"));
    }
    for fd in &keep {
        // SAFETY: every descriptor in `keep` is one this function created and
        // still owns.
        let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
        if let Err(err) = set_cloexec(borrowed, false) {
            fail_dev_root(boot.root, &format!("config pipe: {err}"));
        }
    }

    let env = stage1_env(app_env, boot.config, boot.host.install());
    let err = exec(&cstring_dev(boot.root, &stage1.program), &argv, &env);
    fail_dev_root(boot.root, &format!("exec stage 1: {err}"))
}

/// The environment the stage-1 `execve` hands its program.
///
/// The application's secret-free environment plus the one variable a Python
/// boot must add: the `PYTHONHOME` stage 0 discovered, unless the app already
/// names one. Stage 1 owns no `PYTHONPATH` and no working directory (§4), and
/// under `env -i` nothing else tells CPython where its stdlib is. The value
/// comes from [`Config::python`] — the document — never from the environment:
/// `blastit-env` strips every `BLASTIT_*` variable across this barrier.
fn stage1_env(
    app_env: &BTreeMap<String, String>,
    config: &Config,
    install: Install,
) -> Vec<(String, String)> {
    // Only a `python` build ever pushes the home, so the binding is not
    // mutated in the standalone archive's build.
    #[allow(unused_mut)]
    let mut env: Vec<(String, String)> = app_env
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    #[cfg(feature = "python")]
    if let Some(home) = super::project_python::python_home(install, app_env) {
        env.push(("PYTHONHOME".to_string(), home));
    }
    #[cfg(not(feature = "python"))]
    let _ = (config, install);
    #[cfg(feature = "python")]
    let _ = config;
    env
}
/// Forks dbproxy, hands it the secrets and waits for its ready byte (§4).
///
/// Returns the pipes stage 1 needs to collect dbproxy's logs, or `None` when
/// there was no listener to serve.
pub(super) fn fork_dbproxy(
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

    // §7.2: dbproxy is `execve`'d as `BLASTIT_UID_BASE + 3`, so the child has
    // to be able to *reach* the file before it is forked. A wheel installed
    // under a directory the dropped-to uids cannot pass — a checkout inside a
    // `0750` home, say — otherwise fails the boot with `exec dbproxy: Permission
    // denied (os error 13)`, which names neither the directory nor the fix.
    let layout = UidLayout::new(config.uid_base);
    // The route count is not needed here: the execute question never reads the
    // sticky exemption, which is the only thing the uid list is for.
    match crate::visibility::unexecutable(
        std::slice::from_ref(&placement.dbproxy),
        layout.dbproxy(),
        crate::py::runtime_hooks(interpreter),
        &crate::visibility::DroppedTo::of(layout, 0),
    ) {
        Ok(Some(blocker)) => fail(emitter, &blocker.message()),
        Ok(None) => {}
        Err(timed_out) => fail(emitter, &timed_out.message()),
    }

    let proxies: Vec<ProxySpec> =
        crate::dbproxy_wiring::proxy_specs(prepared, |name| config.max_connections_for(name));
    let document = DbproxyConfig::enforcing(
        proxies,
        listeners.iter().map(|(_, spec)| spec.clone()).collect(),
    );

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

            // `§1.2`: a line only if the wait is actually slow (see
            // `wait_ready_with_line`).
            if let Err(reason) = wait_ready_with_line(emitter, &ready_read) {
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
///
/// Only the stage-0 unit tests call this: the boot itself goes through
/// [`wait_ready_with_line`], because it also decides whether the wait is worth a
/// phase line.
#[cfg(test)]
pub(super) fn wait_ready(ready: &OwnedFd, timeout: Duration) -> Result<(), String> {
    if wait_ready_until(ready, Deadline::after(timeout))? {
        return Ok(());
    }
    Err(format!(
        "dbproxy did not report ready within {}s",
        timeout.as_secs_f32()
    ))
}

/// Waits for dbproxy's ready byte until `deadline` (§4).
///
/// `Ok(true)` is the byte, `Ok(false)` the deadline, `Err` the end nobody can
/// retry — a failed `poll`, or the child exiting first.
fn wait_ready_until(ready: &OwnedFd, deadline: Deadline) -> Result<bool, String> {
    loop {
        if deadline.expired() {
            return Ok(false);
        }
        match wait_readable(ready.as_fd(), deadline.left()) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
        match read_one(ready.as_fd()) {
            Ok(Some(_)) => return Ok(true),
            Ok(None) => return Err("dbproxy exited before it was ready".to_string()),
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
    }
}

/// Waits for dbproxy's ready byte, printing the §1.2 phase line **only** when
/// the wait is actually slow.
///
/// The ordinary boot is ready in a few milliseconds, and a line saying
/// "waiting for the ready byte" above a banner that appears immediately beside
/// it is noise a developer reads as a stall. So the first [`SLOW_READY_LINE`]
/// gets no line at all; only a wait that outlives it — a slow host, a cold
/// page cache, a debugger — prints one and then keeps waiting inside the same
/// [`DBPROXY_READY_TIMEOUT`] budget.
///
/// # Errors
///
/// [`wait_ready`]'s sentence, on the deadline the boot started with.
pub(super) fn wait_ready_with_line(emitter: &Emitter, ready: &OwnedFd) -> Result<(), String> {
    let budget = Deadline::after(DBPROXY_READY_TIMEOUT);
    if wait_ready_until(ready, Deadline::after(SLOW_READY_LINE))? {
        return Ok(());
    }
    crate::logs::phase(emitter, "dbproxy", "waiting for the ready byte");
    if wait_ready_until(ready, budget)? {
        return Ok(());
    }
    Err(format!(
        "dbproxy did not report ready within {}s",
        DBPROXY_READY_TIMEOUT.as_secs_f32()
    ))
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
pub(super) fn emit_child_output(emitter: &Emitter, fd: &OwnedFd, stream: Stream) {
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
pub(super) fn exec_stage1(
    emitter: &Emitter,
    config: &Config,
    host: &Host,
    placement: &Placement,
    app_env: &BTreeMap<String, String>,
    dbproxy: Option<DbproxyHandover>,
) -> ! {
    let document_value = match config_document(config) {
        Ok(value) => value,
        Err(message) => fail(emitter, &message),
    };
    let document = match serde_json::to_vec(&document_value) {
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
    // Design §2.4: the two pinned descriptors cross as numbers, and only these
    // two lose `FD_CLOEXEC` for this `execve` — stage 1 re-sets it at once.
    if let Some(python) = &config.python {
        let (lib, shim) = python.fds();
        argv.push(cstring(emitter, format!("--py-lib-fd={lib}")));
        argv.push(cstring(emitter, format!("--py-shim-fd={shim}")));
        keep.push(lib);
        keep.push(shim);
    }
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

    let env = stage1_env(app_env, config, host.install());
    let err = exec(&program(emitter, &stage1), &argv, &env);
    fail(emitter, &format!("exec stage 1: {err}"))
}

/// The program and its prefix, as the `argv` a re-exec starts with (§4).
///
/// `argv[0]` is the program itself, as `execve`'s callers conventionally do: for
/// the standalone binary that is `/proc/self/exe`, and for a wheel install it is
/// the interpreter — which is also what CPython wants there, because it reads
/// `argv[0]` to find the interpreter whose `site-packages` the app needs.
pub(super) fn reexec_argv(emitter: &Emitter, reexec: &Reexec) -> Vec<CString> {
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
pub(super) fn program(emitter: &Emitter, reexec: &Reexec) -> CString {
    cstring(emitter, &reexec.program)
}

/// An argument, or a path, as a C string, or a fatal error naming what is
/// wrong.
pub(super) fn cstring(emitter: &Emitter, value: impl AsRef<OsStr>) -> CString {
    match CString::new(value.as_ref().as_bytes().to_vec()) {
        Ok(value) => value,
        Err(_) => fail(emitter, "an argument contains a NUL byte"),
    }
}

/// `blastit_sys::pipe`, fatal on failure.
pub(super) fn pipe_ok(emitter: &Emitter, what: &str) -> (OwnedFd, OwnedFd) {
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
pub(super) fn fork_ok(emitter: &Emitter, interpreter: bool) -> Fork {
    match crate::forking::fork(crate::py::runtime_hooks(interpreter)) {
        Ok(fork) => fork,
        Err(err) => fail(emitter, &format!("fork: {err}")),
    }
}
