//! The dbproxy side of one `blastit check` run (`§4.2 D.3`).
//!
//! One wildcard listener per declared `postgresql` proxy is bound here, in
//! [`ListenerMode::DynamicCheck`]: the listener never refuses a statement a
//! missing grant would have refused (`R26`), every statement it sees is written
//! to the observation pipe, and the route a statement belongs to comes from the
//! control channel. Both pipes are drained on their own threads for the whole
//! of the suite, because their writers block once the kernel buffer is full
//! (`docs/knowledge/LESSONS.md`).

use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use blastit_dbproxy::{
    DbproxyConfig, ListenerMode, ListenerSpec, ProxySpec, RoutePolicySpec, WILDCARD_ROUTE,
    write_config,
};
use blastit_events::Component;
use blastit_manifest::Manifest;

use crate::host::Host;
use crate::logs::{Emitter, Pipe};

use super::session::{clear_cloexec, stdio_stderr};

/// How long dbproxy may take to report that its listeners are ready.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long dbproxy gets to leave after pytest, before it is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// One bound wildcard socket per declared `postgresql` proxy.
///
/// In `DynamicCheck` the listener is shared by every route: which route a
/// statement belongs to comes from the control channel, and the route policies
/// [`route_policies`] builds are what the listener evaluates against.
pub fn bind_listeners(
    prepared: &blastit_env::Prepared,
    socket_dir: &Path,
) -> Result<Vec<(UnixListener, ListenerSpec)>, String> {
    let uid = rustix::process::geteuid().as_raw();
    let mut listeners = Vec::new();
    for (index, (name, _upstream)) in prepared.upstreams().iter().enumerate() {
        let Some(port) = blastit_env::postgresql::local_port(index) else {
            return Err(format!(
                "{}: no local port left for the postgresql proxies",
                name.as_str()
            ));
        };
        let path = socket_dir.join(format!(".s.PGSQL.{port}"));
        let listener =
            UnixListener::bind(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        // `0600` and the caller's own uid: the suite, running as this user, is
        // the only client the check sockets accept (`R30`).
        rustix::fs::chmod(&path, rustix::fs::Mode::from_bits_truncate(0o600))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        listeners.push((
            listener,
            ListenerSpec {
                fd: 0,
                route: WILDCARD_ROUTE.to_string(),
                proxy: name.as_str().to_string(),
                uid,
                queries: Vec::new(),
                transaction_control: false,
                unrestricted: false,
                tables: blastit_sqlpolicy::TableGrants::default(),
            },
        ));
    }
    Ok(listeners)
}

/// The route policies of every `(route, proxy)` pair.
pub(super) fn route_policies(manifest: &Manifest) -> Vec<RoutePolicySpec> {
    let mut policies = Vec::new();
    for index in 0..manifest.route_count() {
        let Some(route) = manifest.route(index) else {
            continue;
        };
        for (proxy, grant) in manifest.effective_grants(index) {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            policies.push(RoutePolicySpec {
                route: route.name().to_string(),
                proxy: proxy.as_str().to_string(),
                queries: pg.queries().to_vec(),
                transaction_control: pg.transaction_control(),
                unrestricted: pg.is_unrestricted(),
                tables: pg.tables().clone(),
            });
        }
    }
    policies
}

/// The inherited descriptors dbproxy adopts after its `execve`.
pub struct Inherited {
    pub obs: OwnedFd,
    pub ctl: OwnedFd,
    pub ready: OwnedFd,
}

/// The running `blastit-dbproxy`.
pub struct Dbproxy {
    child: Child,
}

impl Dbproxy {
    /// Forks and `execve`s `blastit-dbproxy` with an empty environment.
    ///
    /// Its events go to a pipe of their own rather than to stderr: `pytest -q`
    /// is drawing its progress line there, and a JSON line written into it
    /// corrupts the row an operator reads (A10.3). The returned reader is the
    /// pipe's read end, which the caller hands to an [`EventDrain`] for the
    /// whole of the suite. It stays a [`std::io::PipeReader`]: a pipe is not a
    /// socket, and reading it through a `UnixStream` fails with `ENOTSOCK`.
    pub fn start(
        host: &Host,
        manifest: &Manifest,
        prepared: &blastit_env::Prepared,
        mut listeners: Vec<(UnixListener, ListenerSpec)>,
        inherited: Inherited,
        ready_read: OwnedFd,
    ) -> Result<(Self, std::io::PipeReader), String> {
        for (listener, spec) in &mut listeners {
            spec.fd = listener.as_raw_fd();
        }
        let uid = rustix::process::geteuid().as_raw();
        let proxies: Vec<ProxySpec> = crate::dbproxy_wiring::proxy_specs(prepared, |_| 4);
        let mut document = DbproxyConfig::enforcing(
            proxies,
            listeners.iter().map(|(_, spec)| spec.clone()).collect(),
        );
        document.mode = ListenerMode::DynamicCheck;
        document.allow_root_listener = uid == 0;
        document.route_policies = route_policies(manifest);
        document.obs_fd = Some(inherited.obs.as_raw_fd());
        document.check_ctl_fd = Some(inherited.ctl.as_raw_fd());
        document
            .validate()
            .map_err(|error| format!("the dbproxy configuration: {error}"))?;

        let (secrets_read, mut secrets_write) =
            std::io::pipe().map_err(|error| format!("the dbproxy secrets pipe: {error}"))?;
        let (events_read, events_write) =
            std::io::pipe().map_err(|error| format!("the dbproxy events pipe: {error}"))?;

        // Every descriptor the child must inherit: the configuration pipe, the
        // two channels, the ready pipe, the events pipe, and one per listener.
        let mut inherited_fds = vec![
            secrets_read.as_raw_fd(),
            events_write.as_raw_fd(),
            inherited.obs.as_raw_fd(),
            inherited.ctl.as_raw_fd(),
            inherited.ready.as_raw_fd(),
        ];
        inherited_fds.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));
        for fd in &inherited_fds {
            clear_cloexec(*fd)?;
        }

        let program = dbproxy_program(host);
        let mut command = Command::new(&program);
        command.args(host.dbproxy().prefix.iter());
        command.arg("--role=dbproxy");
        command.arg(format!("--secrets-fd={}", secrets_read.as_raw_fd()));
        command.arg(format!("--events-fd={}", events_write.as_raw_fd()));
        command.arg(format!("--ready-fd={}", inherited.ready.as_raw_fd()));
        command.arg("--seccomp=1");
        command.env_clear();
        command.stdin(Stdio::null());
        command.stdout(stdio_stderr()?);
        command.stderr(stdio_stderr()?);
        // A coverage build boots dbproxy under the same profile file, so the
        // role's coverage is written when it exits (`-C instrument-coverage`).
        crate::coverage::inherit(&mut command);
        let child = command.spawn().map_err(|error| {
            format!(
                "{}: {error} (the check needs the `blastit-dbproxy` binary this \
                 installation ships)",
                program.display()
            )
        })?;
        // The parent's copies of the child's ends are closed right after the
        // `execve`: dbproxy owns them from here.
        drop(events_write);
        drop(inherited.obs);
        drop(inherited.ctl);
        drop(listeners);
        write_config(&document, &mut secrets_write)
            .map_err(|error| format!("the dbproxy configuration: {error}"))?;
        drop(secrets_write);
        drop(secrets_read);

        let mut ready = Ready::new(ready_read);
        if let Err(reason) = ready.wait() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason);
        }
        Ok((Self { child }, events_read))
    }

    /// Stops dbproxy: `SIGTERM`, a short grace, then `SIGKILL`.
    pub fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let pid = i32::try_from(self.child.id()).ok();
        if let Some(pid) = pid.and_then(rustix::process::Pid::from_raw) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Dbproxy {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Waits for dbproxy's ready byte.
struct Ready {
    stream: UnixStream,
}

impl Ready {
    fn new(fd: OwnedFd) -> Self {
        Self {
            stream: UnixStream::from(fd),
        }
    }

    fn wait(&mut self) -> Result<(), String> {
        self.stream
            .set_read_timeout(Some(READY_TIMEOUT))
            .map_err(|error| format!("the dbproxy ready pipe: {error}"))?;
        let mut byte = [0u8; 1];
        match self.stream.read(&mut byte) {
            Ok(0) => Err("dbproxy exited before it was ready".to_string()),
            Ok(_) => Ok(()),
            Err(error) => Err(format!(
                "dbproxy did not report ready: {error} (its own diagnostics are on stderr)"
            )),
        }
    }
}

/// Drains dbproxy's observation pipe on its own thread (`LESSONS.md`).
///
/// The writer blocks once the socketpair's buffer is full, so the reader has to
/// run for the whole of the suite rather than after it.
pub struct ObsReader {
    handle: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl ObsReader {
    pub fn start(read: OwnedFd) -> Self {
        let mut stream = UnixStream::from(read);
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => out.extend_from_slice(&buffer[..read]),
                    Err(_) => break,
                }
            }
            out
        });
        Self {
            handle: Some(handle),
        }
    }

    /// Waits for the pipe's end and returns what it held.
    pub fn stop(mut self) -> String {
        let Some(handle) = self.handle.take() else {
            return String::new();
        };
        let bytes = handle.join().unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Drains dbproxy's *events* pipe, so its lines never share pytest's row
/// (A10.3).
///
/// A `pg.refused` is one JSON line, and dbproxy writes it synchronously: pointed
/// at fd 2 it lands in the middle of the `-q` progress line pytest is drawing on
/// the same terminal. The pipe is drained here for the whole of the suite (the
/// writer blocks otherwise), and every line is forwarded through the run's own
/// [`Emitter`] once the suite is over — the same component restamp, the same
/// level filter (`BLASTIT_LOG_LEVEL=error` silences a `warn`) and the same
/// pretty or JSON shape as any other forwarded pipe.
///
/// [`Drop`] is the guarantee that "once the suite is over" is true on *every*
/// path out of `execute`, including the `?` that abandons the run: a drain that
/// is only stopped by hand would take its thread and its bytes with it.
pub struct EventDrain {
    handle: Option<std::thread::JoinHandle<Vec<u8>>>,
    emitter: Emitter,
}

impl EventDrain {
    pub fn start(read: std::io::PipeReader, emitter: Emitter) -> Self {
        let mut stream = read;
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buffer = [0u8; 8 * 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => out.extend_from_slice(&buffer[..read]),
                    Err(_) => break,
                }
            }
            out
        });
        Self {
            handle: Some(handle),
            emitter,
        }
    }

    /// Waits for the pipe's end and forwards every line. Idempotent, so an
    /// explicit stop and [`Drop`] never print a line twice.
    pub fn stop(mut self) {
        self.drain();
    }

    fn drain(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        let bytes = handle.join().unwrap_or_default();
        let pipe = Pipe::Events {
            component: Component::Dbproxy,
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            self.emitter.forward(&pipe, line);
        }
    }
}

impl Drop for EventDrain {
    fn drop(&mut self) {
        self.drain();
    }
}

/// The `blastit-dbproxy` program this installation runs.
fn dbproxy_program(host: &Host) -> PathBuf {
    let program = &host.dbproxy().program;
    if program.is_file() {
        return program.clone();
    }
    std::env::current_exe()
        .map(|exe| exe.with_file_name("blastit-dbproxy"))
        .unwrap_or_else(|_| program.clone())
}
