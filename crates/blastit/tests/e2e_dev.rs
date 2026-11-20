//! `TESTING_PLAN.md` §2 (applications & runtimes) and §4 (`blastit dev`) — the
//! **Python WSGI** half, driven through the real binary.
//!
//! This file is one of three slices that share it: `.1` (this one) owns the
//! Python story, `.2` appends the PHP story, `.3` appends static assets, live
//! reload, `--learn` and shutdown. The harness at the top — a temporary project
//! root, a port from `28080+`, a boot whose whole stdout/stderr is collected, and
//! deadline-based waits — is the part every slice reuses, so a new story appends
//! tests and fixtures below it and never a fourth helper.
//!
//! The cases here re-prove what `MB.56.2.1`/`MB.56.2.2` inventoried and what the
//! deleted `critical_python_shim`, `python_activation_gate`,
//! `embed_venv_sys_path` and the WSGI half of `critical_parallel_views` covered:
//!
//! 1. a multi-view WSGI project on CPython 3.13 **and** 3.14 whose shared
//!    `trait` grants a parameterized `$1` read and write, with an ungranted
//!    statement and an ungranted column each refused at runtime and logged
//!    normalised, and **zero** database credentials in a worker's environment;
//! 2. the host-interpreter refusals, verbatim: an unsupported minor, a
//!    free-threaded build, a host without a shared `libpython` (naming every
//!    remedy), and an unusable `BLASTIT_PYTHON`;
//! 3. the venv rule: a project `.venv`'s `site-packages` is what the view
//!    imports from, and an activated `$VIRTUAL_ENV` that is not a venv is
//!    skipped — the boot serves, and its `runtime` line names the environment it
//!    actually serves;
//! 4. a threaded WSGI route really answers four 200 ms requests at once, on one
//!    process, against a serialized control measured in the same function.
//!
//! Every leg is a real `blastit dev` process answering on loopback, and the
//! database legs dial a real PostgreSQL v3 peer (`tests/common/pg_upstream.rs`)
//! through the route's own dbproxy socket. Nothing is `#[ignore]`d and nothing
//! skips: a host that cannot run a leg fails with the sentence that says what to
//! install.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// The PHP/static stories (`.2`, `.3`) use this harness; until they land, a build
// without the `python` feature has no test that reaches it.
#![cfg_attr(not(feature = "python"), allow(dead_code))]

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(feature = "python")]
#[path = "common/pg_upstream.rs"]
mod pg_upstream;

/// The HTTP client, request burst and calibration both concurrency suites read
/// (`views::CONTROL_FLOOR`, `views::CONTROL_MARGIN`, `views::WIDTH`), kept once
/// for this file and `critical_parallel_views.rs`.
#[path = "common/views.rs"]
mod views;

use views::{Client, Response};

// The burst and its assertion are used by the Python concurrency leg only.
#[cfg(feature = "python")]
use views::{assert_200, burst};

/// The binary cargo built for this suite.
const BINARY: &str = env!("CARGO_BIN_EXE_blastit");

/// The first port a fixture may take: `28080+`, so no boot races a developer's
/// own server in the ephemeral range.
const FIRST_PORT: u16 = 28_080;

/// The last port a fixture may take before the suite blames a leaked listener.
const LAST_PORT: u16 = 28_999;

/// The next port to try; every boot takes one so two fixtures never collide.
static NEXT_PORT: AtomicU32 = AtomicU32::new(28_080);

/// How long a boot may take to answer, or to be refused.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

/// How many times a boot is retried when another process took the port between
/// [`free_port`]'s probe and the child's `bind` (`EADDRINUSE`).
const PORT_ATTEMPTS: u32 = 3;

/// How long a boot may take to leave after `SIGTERM`.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// A port nothing listens on right now, taken from [`FIRST_PORT`] upward.
fn free_port() -> u16 {
    loop {
        let candidate = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
        assert!(
            candidate <= u32::from(LAST_PORT),
            "no free port in {FIRST_PORT}..={LAST_PORT}: a boot in this suite leaked a listener"
        );
        let port = u16::try_from(candidate).unwrap_or(FIRST_PORT);
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

/// The directory every fixture lives under: `/tmp/opencode` when it exists, the
/// platform's temporary directory otherwise.
fn scratch_root() -> PathBuf {
    let preferred = PathBuf::from("/tmp/opencode");
    if std::fs::create_dir_all(&preferred).is_ok() {
        return preferred;
    }
    std::env::temp_dir()
}

/// `target/py-sapi`, where `make py-sapi` writes the per-minor CPython shims.
fn python_shim_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/py-sapi");
    dir.is_dir().then_some(dir)
}

/// A temporary project, removed when dropped.
struct Project {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Project {
    /// A fresh directory for one case, with no `Blastfile` yet.
    fn new(tag: &str) -> Self {
        let dir = scratch_root().join(format!("blastit-e2e-dev-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory");
        Self { dir }
    }

    /// The absolute path of `relative` inside the project.
    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// Writes `contents` to `relative`, creating its parent directories.
    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a fixture parent directory");
        }
        std::fs::write(&path, contents).expect("a fixture file");
        path
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The documented `sql.denied` line whose normalised statement carries `needle`
/// for `route`.
fn denial_line<'a>(log: &'a str, needle: &str, route: &str) -> Option<&'a str> {
    log.lines().find(|line| {
        line.contains("sql.denied")
            && line.contains(needle)
            && line.contains(&format!("  {route}  "))
    })
}

/// Waits until the denial for `needle` on `route` is on stdout (the event
/// travels on init's pipe, so it is one tick behind the request).
fn wait_for_denial(dev: &Dev, needle: &str, route: &str) -> String {
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        if let Some(line) = denial_line(&dev.text(), needle, route) {
            return line.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "no `sql.denied` line for {route} naming {needle:?}:\n{}",
            dev.text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The dev command line every case runs: an empty environment plus the loader
/// and interpreter variables the embedded CPython needs, exactly as `make check`
/// sets them.
///
/// `env_clear` is what a developer's shell is not: every `BLASTIT_*` a case
/// needs is named by the case, so nothing ambient can make a probe and the boot
/// disagree. `BLASTIT_MEM_HIGH_WATERMARK=100` keeps a loaded host from turning a
/// story about WSGI into the documented memory refusal.
fn dev_command(dir: &Path, port: u16, extra: &[(&str, &str)]) -> Command {
    let mut command = Command::new(BINARY);
    command
        .arg("dev")
        .args(["--no-reload", "--port"])
        .arg(port.to_string())
        .current_dir(dir)
        .env_clear()
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ["LD_LIBRARY_PATH", "HOME", "TMPDIR", "PATH"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    if let Some(shim) = python_shim_dir() {
        command.env("BLASTIT_PY_SHIM_DIR", shim);
    }
    for (name, value) in extra {
        command.env(name, value);
    }
    command
}

/// A boot that ran to an end — a refusal, or a serving boot the deadline killed.
struct Outcome {
    /// The exit status, or `None` when [`BOOT_TIMEOUT`] ended the run.
    status: Option<ExitStatus>,
    /// Everything the boot wrote on fd 1.
    stdout: String,
    /// Everything the boot wrote on fd 2.
    stderr: String,
}

impl Outcome {
    /// Asserts the boot refused: a non-zero exit, and nothing on stdout — a
    /// startup error is prose on descriptor 2 (§1.1).
    fn refused_with(&self, what: &str) {
        assert!(
            self.status.is_some_and(|status| !status.success()),
            "{what}:\n{}",
            self.stderr
        );
        assert!(
            self.stdout.is_empty(),
            "{what}: a startup refusal writes nothing on stdout: {:?}",
            self.stdout
        );
    }
}

/// One boot at a time across this file.
///
/// The suite's own cases boot real processes; letting eight of them run at once
/// starves the timing-sensitive reload suites in the same `cargo test` run, and
/// it is also what makes a wall-clock leg honest (nothing else of this binary is
/// forking or answering while one leg's clock runs).
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `blastit dev` to an end and returns what it said.
///
/// The run is bounded by [`BOOT_TIMEOUT`]: a fixture that unexpectedly *served*
/// is killed at the deadline instead of hanging the suite, and its transcript is
/// what the assertion shows.
fn run_dev(project: &Project, extra: &[(&str, &str)]) -> Outcome {
    let _serial = boot_lock();
    let mut attempt = 1;
    loop {
        let port = free_port();
        let mut child = dev_command(&project.dir, port, extra)
            .spawn()
            .expect("spawn `blastit dev`");
        let deadline = Instant::now() + BOOT_TIMEOUT;
        let status = loop {
            match child.try_wait().expect("the boot's state") {
                Some(status) => break Some(status),
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    break child.wait().ok();
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        };
        let outcome = Outcome {
            status,
            stdout: drain(child.stdout.take()),
            stderr: drain(child.stderr.take()),
        };
        // A refusal never binds, so `EADDRINUSE` here means the boot *tried* to
        // serve and another process held the port: a harness isolation failure,
        // retried with a fresh port and never blamed on the case.
        let lost = outcome.stderr.contains("Address already in use")
            || outcome.stdout.contains("Address already in use");
        if !lost || attempt >= PORT_ATTEMPTS {
            return outcome;
        }
        eprintln!(
            "blastit dev: port {port} was taken between free_port() and the child's bind \
             (EADDRINUSE); retrying with a fresh port ({attempt}/{PORT_ATTEMPTS})"
        );
        attempt += 1;
    }
}

/// Everything left in `pipe`, read to end of file.
fn drain<R: Read>(pipe: Option<R>) -> String {
    let mut text = String::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_string(&mut text);
    }
    text
}

/// What a wait saw on one turn.
enum Poll {
    /// The boot reached the state the wait is for.
    Ready,
    /// Not yet: poll again after a short pause.
    Waiting,
    /// The child is gone and its transcript says another process took the port.
    LostThePort,
    /// The child exited for any other reason.
    Exited(ExitStatus),
}

/// A running `blastit dev`, with both streams collected line by line.
struct Dev {
    /// The directory the boot runs in (so a lost port can be retried).
    dir: PathBuf,
    /// The `BLASTIT_*` this boot was started with, as owned strings, for the
    /// same retry.
    extra: Vec<(String, String)>,
    /// The boot's own process (it `execve`s into stage 1 in place, so the pid is
    /// stable).
    child: Child,
    /// The port it bound.
    port: u16,
    /// Every stdout line, in order (JSON events, the server default).
    stdout: Arc<Mutex<Vec<String>>>,
    /// Everything stderr has said.
    stderr: Arc<Mutex<Vec<String>>>,
    /// The threads draining the two pipes.
    readers: Vec<JoinHandle<()>>,
    /// Held for the boot's lifetime and released by [`Dev::stop`], so a case that
    /// stops one boot may start the next: see [`boot_lock`].
    #[allow(dead_code)]
    serial: Option<std::sync::MutexGuard<'static, ()>>,
}

impl Dev {
    /// Starts a boot in `project` with `extra` on top of an empty environment.
    fn start(project: &Project, extra: &[(&str, &str)]) -> Self {
        Self::spawn(
            project.dir.clone(),
            extra
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            free_port(),
            boot_lock(),
        )
    }

    /// One spawn at `port`: the process, both readers, and the lock that makes
    /// it the only boot of this file.
    fn spawn(
        dir: PathBuf,
        extra: Vec<(String, String)>,
        port: u16,
        serial: std::sync::MutexGuard<'static, ()>,
    ) -> Self {
        let pairs: Vec<(&str, &str)> = extra
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let mut child = dev_command(&dir, port, &pairs)
            .spawn()
            .expect("spawn `blastit dev`");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&stdout);
        let reader = child.stdout.take().expect("the boot's stdout");
        let mut readers = vec![std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("the stdout buffer").push(line);
            }
        })];
        let collected = Arc::clone(&stderr);
        let reader = child.stderr.take().expect("the boot's stderr");
        readers.push(std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("the stderr buffer").push(line);
            }
        }));
        Self {
            dir,
            extra,
            child,
            port,
            stdout,
            stderr,
            readers,
            serial: Some(serial),
        }
    }

    /// Waits until the boot answers `path`, or panics naming what it said.
    fn wait_ready(&mut self, path: &str) {
        self.wait(&format!("answer {path}"), |dev| {
            if dev.try_request("GET", path).is_some() {
                Poll::Ready
            } else {
                dev.poll()
            }
        });
    }

    /// Waits until init announces `name`, failing with the boot's transcript.
    ///
    /// Readiness is init's own statement, so a wait that only needs "the boot
    /// came up" (and must not warm a view) uses this rather than a request.
    fn wait_event(&mut self, name: &str) {
        self.wait(&format!("announce {name}"), |dev| {
            if dev.events().iter().any(|event| event["event"] == name) {
                Poll::Ready
            } else {
                dev.poll()
            }
        });
    }

    /// Runs `poll` to [`Poll::Ready`], retrying a launch that lost its port.
    ///
    /// When another process takes the port between [`free_port`]'s probe and the
    /// child's `bind`, the boot exits with `EADDRINUSE` — a property of the
    /// host's load, not of the behaviour a case is testing — so the launch is
    /// retried with a fresh port ([`PORT_ATTEMPTS`]) and never blamed on the
    /// case (`MB.21.4`, the same rule `critical_dev_and_fail_closed` follows).
    fn wait(&mut self, what: &str, mut poll: impl FnMut(&mut Self) -> Poll) {
        let mut attempt = 1;
        loop {
            let deadline = Instant::now() + BOOT_TIMEOUT;
            let failed = loop {
                match poll(self) {
                    Poll::Ready => return,
                    Poll::Waiting => {}
                    other => break Some(other),
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            match failed {
                Some(Poll::LostThePort) if attempt < PORT_ATTEMPTS => {
                    eprintln!(
                        "blastit dev: port {} was taken between free_port() and the child's \
                         bind (EADDRINUSE); retrying with a fresh port ({attempt}/{PORT_ATTEMPTS})",
                        self.port
                    );
                    attempt += 1;
                    self.relaunch();
                }
                Some(Poll::LostThePort) => panic!(
                    "`blastit dev` could not bind port {} in {PORT_ATTEMPTS} attempts \
                     (EADDRINUSE): a harness isolation failure, not a `{what}` failure:\n{}",
                    self.port,
                    self.text()
                ),
                Some(Poll::Exited(status)) => panic!(
                    "`blastit dev` exited before it could {what} ({status}):\n{}",
                    self.text()
                ),
                _ => panic!(
                    "`blastit dev` did not {what} on 127.0.0.1:{} within {BOOT_TIMEOUT:?}:\n{}",
                    self.port,
                    self.text()
                ),
            }
        }
    }

    /// Whether the child exited, and if it lost the port.
    fn poll(&mut self) -> Poll {
        match self.child.try_wait() {
            Ok(Some(_)) if self.text().contains("Address already in use") => Poll::LostThePort,
            Ok(Some(status)) => Poll::Exited(status),
            _ => Poll::Waiting,
        }
    }

    /// Stops a dead or wedged launch and starts a fresh one on a new port.
    fn relaunch(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in std::mem::take(&mut self.readers) {
            let _ = reader.join();
        }
        let port = free_port();
        let serial = self.serial.take().unwrap_or_else(boot_lock);
        let dir = self.dir.clone();
        let extra = std::mem::take(&mut self.extra);
        *self = Self::spawn(dir, extra, port, serial);
    }

    /// One request, or `None` while the boot does not answer.
    fn try_request(&self, method: &str, path: &str) -> Option<Response> {
        views::request(self.port, method, path)
    }

    /// One request that must be answered, with the boot's transcript on failure.
    fn request(&self, method: &str, path: &str) -> Response {
        self.try_request(method, path).unwrap_or_else(|| {
            panic!(
                "no answer to {method} {path} on 127.0.0.1:{}:\n{}",
                self.port,
                self.text()
            )
        })
    }

    /// One `GET` that must be answered.
    fn get(&self, path: &str) -> Response {
        self.request("GET", path)
    }

    /// One request whose body is JSON.
    fn json_request(&self, method: &str, path: &str) -> serde_json::Value {
        let response = self.request(method, path);
        serde_json::from_str(&response.body).unwrap_or_else(|err| {
            panic!(
                "{path} did not answer JSON ({err}): {:?}\n{}",
                response.body,
                self.text()
            )
        })
    }

    /// The client half, for a request thread that cannot borrow the process.
    fn client(&self) -> Client {
        Client { port: self.port }
    }

    /// Every stdout line parsed as an event (the case asked for
    /// `BLASTIT_LOG_FORMAT=json`).
    fn events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lock()
            .expect("the stdout buffer")
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// The pids init has announced for `route`, sorted and de-duplicated.
    fn spawned_pids(&self, route: &str) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .events()
            .iter()
            .filter(|event| event["event"] == "process.spawned" && event["route"] == route)
            .filter_map(|event| event["pid"].as_u64())
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The spawned pid set once init's event stream has stopped growing, so "no
    /// new process" is a settled set rather than a race with the pipe.
    fn settled_spawned(&self, route: &str) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let quiet = Duration::from_millis(300);
        let mut last = self.spawned_pids(route);
        let mut since = Instant::now();
        loop {
            if Instant::now() > deadline {
                return last;
            }
            std::thread::sleep(Duration::from_millis(20));
            let now = self.spawned_pids(route);
            if now == last {
                if since.elapsed() >= quiet {
                    return now;
                }
            } else {
                last = now;
                since = Instant::now();
            }
        }
    }

    /// The boot's whole transcript: stdout events then stderr, one per line.
    fn text(&self) -> String {
        let stdout = self.stdout.lock().expect("the stdout buffer").join("\n");
        let stderr = self.stderr.lock().expect("the stderr buffer").join("\n");
        format!("{stdout}\n{stderr}")
    }

    /// Waits for the readers to reach end of file (the process is gone).
    fn join_readers(&mut self) {
        for reader in std::mem::take(&mut self.readers) {
            let _ = reader.join();
        }
    }

    /// Stops the boot: `SIGTERM` first (so `dev` removes its runtime root and
    /// drains its children), then `SIGKILL`, then the readers are joined.
    fn stop(&mut self) {
        if self.child.try_wait().expect("the boot's state").is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.child.id().to_string()])
                .status();
            let deadline = Instant::now() + STOP_TIMEOUT;
            loop {
                if self.child.try_wait().expect("the boot's state").is_some() {
                    break;
                }
                if Instant::now() > deadline {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        self.join_readers();
        // The next boot in this file may start: one `blastit dev` at a time is
        // the rule, not "one at a time until the case's scope ends".
        self.serial = None;
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A boot stopped by a panicking assertion still removes its process.
struct StopOnDrop<'a>(&'a mut Dev);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

// ---------------------------------------------------------------------------
// §2/§4: a multi-view WSGI project on both supported minors.
// ---------------------------------------------------------------------------

/// A two-view WSGI project whose only grants come from one shared trait.
///
/// Neither view declares a grant of its own: what answers a statement is the
/// `trait`'s block, attached by `use shop` — which is the rule the story names.
#[cfg(feature = "python")]
const GRANT_MANIFEST: &str = "\
app \"shop\"
entrypoint python \"app:application\"

postgres $DATABASE_URL

trait shop {
    postgres {
        select products(id, name)
        insert products(id, name)
    }
}

view read GET \"/a/*\" {
    use shop
}

view write POST \"/b/*\" {
    use shop
}
";

/// The app: a minimal PostgreSQL v3 client over the route's own dbproxy socket,
/// so the grants are exercised by the real wire and the credentials stay behind
/// the proxy.
///
/// Every statement is a **prepared** statement with `$1` binds and never an
/// inlined literal, so the suite proves the parameterized form the story names.
#[cfg(feature = "python")]
const GRANT_APP: &str = r#"
import json
import os
import socket
import struct


def _read_exact(sock, count):
    body = b""
    while len(body) < count:
        chunk = sock.recv(count - len(body))
        if not chunk:
            raise OSError(0, "the server closed the session")
        body += chunk
    return body


def _until_ready(sock):
    while True:
        tag = _read_exact(sock, 1)
        length = struct.unpack("!I", _read_exact(sock, 4))[0]
        body = _read_exact(sock, length - 4)
        if tag == b"E":
            raise RuntimeError("pg: " + body.decode("utf-8", "replace"))
        if tag == b"Z":
            return


def _session():
    url = os.environ["DATABASE_URL"]
    query = url.split("?", 1)[1] if "?" in url else ""
    params = dict(part.split("=", 1) for part in query.split("&") if "=" in part)
    path = os.path.join(params.get("host", "/proc/self/cwd"),
                        ".s.PGSQL." + params.get("port", "5432"))
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    sock.connect(path)
    startup = struct.pack("!I", 196608)
    for name, value in (("user", "blastit"), ("database", "app")):
        startup += name.encode() + b"\0" + value.encode() + b"\0"
    startup += b"\0"
    sock.sendall(struct.pack("!I", len(startup) + 4) + startup)
    _until_ready(sock)
    return sock


def pg(sql, *values):
    sock = _session()
    try:
        body = b"\0" + sql.encode() + b"\0" + struct.pack("!H", 0)
        sock.sendall(b"P" + struct.pack("!I", len(body) + 4) + body)
        body = b"\0\0" + struct.pack("!HH", 0, len(values))
        for value in values:
            raw = value.encode()
            body += struct.pack("!I", len(raw)) + raw
        body += struct.pack("!H", 0)
        sock.sendall(b"B" + struct.pack("!I", len(body) + 4) + body)
        sock.sendall(b"D" + struct.pack("!I", 6) + b"P\0")
        body = b"\0" + struct.pack("!I", 0)
        sock.sendall(b"E" + struct.pack("!I", len(body) + 4) + body)
        sock.sendall(b"S" + struct.pack("!I", 4))
        _until_ready(sock)
    finally:
        sock.close()


READ = "SELECT id, name FROM products WHERE id = $1"
WRITE = "INSERT INTO products (id, name) VALUES ($1, $2)"
DENIED_STATEMENT = "SELECT id, name FROM orders WHERE id = $1"
DENIED_COLUMN = "SELECT id, name, cost FROM products WHERE id = $1"


def application(environ, start_response):
    path = environ.get("PATH_INFO", "/")
    status = "200 OK"
    try:
        if path.endswith("/env"):
            payload = {"pid": os.getpid(), "env": dict(os.environ)}
        elif path.endswith("/read"):
            pg(READ, "7")
            payload = {"ok": True}
        elif path.endswith("/write"):
            pg(WRITE, "7", "widget")
            payload = {"ok": True}
        elif path.endswith("/deny_statement"):
            pg(DENIED_STATEMENT, "7")
            payload = {"ok": True}
        else:
            pg(DENIED_COLUMN, "7")
            payload = {"ok": True}
    except Exception as err:  # noqa: BLE001 - the body is the diagnosis
        payload = {"error": str(err)}
        status = "500 Internal Server Error"
    body = json.dumps(payload).encode()
    start_response(status, [("Content-Type", "application/json"),
                            ("Content-Length", str(len(body)))])
    return [body]
"#;

/// §2: on CPython 3.13 **and** 3.14, each view runs only the statements the
/// shared trait grants, and no database credential reaches a worker.
#[cfg(feature = "python")]
#[test]
fn a_multi_view_wsgi_project_runs_only_its_trait_grants_on_both_minors() {
    for minor in ["3.13", "3.14"] {
        grant_leg(minor);
    }
}

/// One minor's leg of [`a_multi_view_wsgi_project_runs_only_its_trait_grants_on_both_minors`].
#[cfg(feature = "python")]
fn grant_leg(minor: &str) {
    use pg_upstream::PgUpstream;

    let server = PgUpstream::start();
    let secret = format!("real-secret-{}", std::process::id());
    let url = format!(
        "postgresql://real-user:{secret}@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new(&format!("grants-{}", minor.replace('.', "")));
    project.write("Blastfile", GRANT_MANIFEST);
    project.write("app.py", GRANT_APP);
    let interpreter = python_for(minor);
    let mut dev = Dev::start(
        &project,
        &[
            ("DATABASE_URL", url.as_str()),
            (
                "BLASTIT_PYTHON",
                interpreter.to_str().expect("a UTF-8 interpreter path"),
            ),
        ],
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/a/env");

    // Zero credentials: the answer is the worker's whole environment, so a
    // password that reached it would be in this body.
    for (route, method) in [("a", "GET"), ("b", "POST")] {
        let answer = dev.0.json_request(method, &format!("/{route}/env"));
        let database = answer["env"]["DATABASE_URL"]
            .as_str()
            .unwrap_or_else(|| panic!("route {route} was not handed the rendered URL: {answer}"));
        assert!(
            !database.contains(&secret),
            "route {route} can read the real database password: {database}"
        );
        assert!(
            database.contains("host="),
            "route {route} was handed the local endpoint instead: {database}"
        );
        assert!(
            !database.contains("real-user"),
            "the proxy's real user reached route {route}: {database}"
        );
        assert!(
            answer["env"]["BLASTIT_PYTHON"].is_null(),
            "a BLASTIT_* variable reached route {route}: {answer}"
        );
        assert!(
            !dev.0.get(&format!("/{route}/env")).body.contains(&secret),
            "route {route} leaked the password in its answer"
        );
    }

    // The trait's grants answer for **both** views: neither declares one.
    for (route, method, path) in [
        ("a", "GET", "/a/read"),
        ("a", "GET", "/a/write"),
        ("b", "POST", "/b/read"),
        ("b", "POST", "/b/write"),
    ] {
        let response = dev.0.request(method, path);
        assert_eq!(
            response.status,
            200,
            "route {route} may run {method} {path} through the trait's grant: {}\n{}",
            response.body,
            dev.0.text()
        );
    }

    // An ungranted statement and an ungranted column are refused at runtime **in
    // each view** (§2: each view executes only its declared operations), and the
    // request that asked for one is a server error.
    for (view, prefix, method) in [("read", "/a", "GET"), ("write", "/b", "POST")] {
        for (path, needle, what) in [
            (
                "deny_statement",
                "FROM orders WHERE id = $?",
                "the ungranted statement",
            ),
            (
                "deny_column",
                "cost FROM products WHERE id = $?",
                "the ungranted column",
            ),
        ] {
            let response = dev.0.request(method, &format!("{prefix}/{path}"));
            assert_eq!(
                response.status,
                500,
                "{what} in view {view} must fail the request: {}\n{}",
                response.body,
                dev.0.text()
            );
            assert!(
                response.body.contains("42501") || response.body.contains("denied"),
                "{what} is refused by the proxy, not by the fixture: {}",
                response.body
            );
            // The documented line: this view's route, the proxy, the `parse` kind
            // and the statement normalised with `$?` — never a parameter value.
            let line = wait_for_denial(dev.0, needle, view);
            assert!(
                line.contains("  db  parse  "),
                "the denial names the proxy and the parse kind: {line}"
            );
            assert!(
                !line.contains("= 7") && !line.contains("'7'"),
                "a parameter value reached the log: {line}"
            );
        }
    }

    // `docs/local.md`: "the request itself is a `500`, and every other view is
    // untouched" — the view that was refused still serves its grant, and the
    // sibling answers as well.
    assert_eq!(
        dev.0.get("/a/read").status,
        200,
        "the denied view must keep serving its granted statement:\n{}",
        dev.0.text()
    );
    assert_eq!(
        dev.0.request("POST", "/b/read").status,
        200,
        "the sibling view must be untouched by the other's denial:\n{}",
        dev.0.text()
    );

    // The proxy really reached a database — granted statements forwarded, denied
    // ones never sent — and it authenticated with the operator's own user.
    let statements = server.statements();
    assert!(
        statements
            .iter()
            .any(|statement| statement.contains("FROM products")),
        "the read reached the upstream: {statements:?}\n{}",
        dev.0.text()
    );
    assert!(
        statements
            .iter()
            .any(|statement| statement.contains("INSERT INTO products")),
        "the write reached the upstream: {statements:?}\n{}",
        dev.0.text()
    );
    assert!(
        !statements
            .iter()
            .any(|statement| statement.contains("orders") || statement.contains("cost")),
        "a denied statement reached the upstream: {statements:?}"
    );
    assert!(
        server
            .startups()
            .iter()
            .any(|startup| startup.user == "real-user"),
        "dbproxy authenticated with the proxy's own credential: {:?}",
        server.startups()
    );

    println!(
        "cpython {minor}: both views ran the trait's `$1` read and write, two denials logged, \
         no credential in the worker environment"
    );
}

// ---------------------------------------------------------------------------
// §2/§4: the host-interpreter rules, verbatim.
// ---------------------------------------------------------------------------

/// A one-view project, and the interpreter refusals' subject.
#[cfg(feature = "python")]
const SIMPLE_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:application\"

view hello GET \"/hello/*\" {}
";

/// The smallest WSGI app: a body with no imports, so a failure is the boot's.
#[cfg(feature = "python")]
const SIMPLE_APP: &str = r#"
def application(environ, start_response):
    body = b"hello\n"
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The interpreter for `minor`, from `BLASTIT_TEST_PYTHON_<digits>`, `uv`, then
/// `PATH` — canonicalized, so a venv's own `bin/python` resolves to its base.
#[cfg(feature = "python")]
fn python_for(minor: &str) -> PathBuf {
    let digits = minor.replace('.', "");
    if let Some(path) = std::env::var_os(format!("BLASTIT_TEST_PYTHON_{digits}")) {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "BLASTIT_TEST_PYTHON_{digits} ({}) is not a file",
            path.display()
        );
        return std::fs::canonicalize(&path).unwrap_or(path);
    }
    let from_uv = Command::new("uv")
        .args(["python", "find", "--no-project", minor])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
        .filter(|path| path.is_file())
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    if let Some(path) = from_uv {
        return path;
    }
    let named = PathBuf::from(format!("python{minor}"));
    let from_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(&named))
            .find(|candidate| candidate.is_file())
    });
    from_path.unwrap_or_else(|| {
        panic!(
            "no CPython {minor} interpreter: install it with `uv python install {minor}` or set \
             BLASTIT_TEST_PYTHON_{digits} to its path"
        )
    })
}

/// Writes an executable regular file at `relative`, with no content: stage 0
/// reads names and versions, never executes the interpreter it refuses.
#[cfg(feature = "python")]
fn executable(project: &Project, relative: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = project.write(relative, b"");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("an executable fixture");
    path
}

/// §2: a CPython 3.12 interpreter is refused with the sentence that says which
/// minors a WSGI view runs on and how to install one.
#[cfg(feature = "python")]
#[test]
fn a_wsgi_boot_refuses_cpython_3_12_verbatim() {
    let project = Project::new("py312");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", SIMPLE_APP);
    let interpreter = executable(&project, "py312/bin/python3.12");
    let outcome = run_dev(
        &project,
        &[(
            "BLASTIT_PYTHON",
            interpreter.to_str().expect("a UTF-8 interpreter path"),
        )],
    );
    outcome.refused_with("an unsupported minor must refuse the boot");
    let expected = format!(
        "blastit dev: CPython 3.12 at {} is not supported for WSGI views (CPython 3.13 and 3.14 \
         are supported; Python 3.12+ runs blastit check, fmt, and fold): run uv python install \
         3.14 && uv venv --python 3.14",
        interpreter.display()
    );
    assert!(
        outcome.stderr.contains(&expected),
        "the refusal is the documented sentence `{expected}`:\n{}",
        outcome.stderr
    );
}

/// §2: a free-threaded build (`3.13t`/`3.14t`) is refused by name, with the fix.
#[cfg(feature = "python")]
#[test]
fn a_free_threaded_interpreter_is_refused_by_name() {
    let project = Project::new("freethreaded");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", SIMPLE_APP);
    let interpreter = executable(&project, "pyt/bin/python3.14t");
    let outcome = run_dev(
        &project,
        &[(
            "BLASTIT_PYTHON",
            interpreter.to_str().expect("a UTF-8 interpreter path"),
        )],
    );
    outcome.refused_with("a free-threaded interpreter must refuse the boot");
    let expected = format!(
        "blastit dev: CPython 3.14t at {} is free-threaded and cannot serve WSGI views; reinstall \
         it without --free-threaded (uv python install cpython-3.14)",
        interpreter.display()
    );
    assert!(
        outcome.stderr.contains(&expected),
        "the refusal is the documented sentence `{expected}`:\n{}",
        outcome.stderr
    );
}

/// The `3.<minor>` number of a dotted minor (`"3.14"` -> `14`).
#[cfg(feature = "python")]
fn minor_number(minor: &str) -> u32 {
    minor
        .rsplit('.')
        .next()
        .and_then(|digits| digits.parse().ok())
        .expect("a dotted minor")
}

/// §2: a prefix with no shared `libpython` is refused with the sentence that
/// names the minor and **every** remedy (Debian/Ubuntu, Alpine, Fedora/RHEL, uv).
///
/// The minor is chosen from `blastit-py-sapi`'s own search list, so a host that
/// ships neither library still gets the refusal and a host that ships *both*
/// supported minors skips loudly instead of failing a boot that would find one.
#[cfg(feature = "python")]
#[test]
fn a_prefix_without_a_shared_libpython_is_told_every_remedy() {
    let Some(minor) = ["3.14", "3.13"]
        .into_iter()
        .find(|minor| !blastit_py_sapi::discovery::system_has_libpython(minor_number(minor)))
    else {
        eprintln!(
            "SKIP a_prefix_without_a_shared_libpython_is_told_every_remedy: this host ships a \
             shared libpython for 3.14 and 3.13 in {:?}, so no prefix fixture can make the boot \
             refuse",
            blastit_py_sapi::discovery::system_libpython_directories()
        );
        return;
    };
    let project = Project::new("nolibpython");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", SIMPLE_APP);
    let prefix = project.path("prefix");
    let interpreter = executable(&project, &format!("prefix/bin/python{minor}"));
    project.write(
        ".venv/pyvenv.cfg",
        format!(
            "home = {}\nversion = {minor}.0\ninclude-system-site-packages = false\n",
            prefix.join("bin").display()
        ),
    );
    std::fs::create_dir_all(project.path(".venv/bin")).expect("the venv bin directory");
    std::os::unix::fs::symlink(&interpreter, project.path(".venv/bin/python3"))
        .expect("the venv interpreter");
    std::fs::create_dir_all(project.path(&format!(".venv/lib/python{minor}/site-packages")))
        .expect("the venv site-packages");

    let outcome = run_dev(&project, &[]);
    outcome.refused_with("a prefix with no libpython must refuse the boot");
    let head = format!(
        "blastit dev: CPython {minor} is installed at {}, but this host has no shared libpython \
         for CPython {minor}; install it",
        prefix.display()
    );
    assert!(
        outcome.stderr.contains(&head),
        "the refusal names the minor and the prefix `{head}`:\n{}",
        outcome.stderr
    );
    for remedy in [
        format!("apt install libpython{minor}"),
        "apk add python3".to_owned(),
        "dnf install python3-libs".to_owned(),
        format!("uv python install {minor}"),
    ] {
        assert!(
            outcome.stderr.contains(&remedy),
            "the refusal names the remedy `{remedy}`:\n{}",
            outcome.stderr
        );
    }
}

/// §2/§4: an unusable `BLASTIT_PYTHON` is refused with the variable, the value
/// and the fix — an **absolute** path (what CI and an agent set) and a relative
/// one a developer's shell spelled.
#[cfg(feature = "python")]
#[test]
fn an_unusable_blastit_python_names_the_variable_the_value_and_the_fix() {
    let project = Project::new("blastit-python");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", SIMPLE_APP);
    let venv = project.path(".venv");
    let absolute = project.path("missing/python3.14");
    let absolute_text = absolute.to_str().expect("a UTF-8 path").to_owned();
    for value in [
        // The absolute value the deleted suite used (`/definitely/not/a/python`),
        // inside the fixture so it names a path this test owns.
        absolute_text.as_str(),
        // A relative one: `dev` reads the shell it was started from.
        "bin/python3",
    ] {
        let outcome = run_dev(&project, &[("BLASTIT_PYTHON", value)]);
        outcome.refused_with(&format!("`BLASTIT_PYTHON={value}` must refuse the boot"));
        let expected = format!(
            "blastit dev: BLASTIT_PYTHON ({value}) is not an executable file; unset \
             BLASTIT_PYTHON, or point it at {}/bin/python3",
            venv.display()
        );
        assert!(
            outcome.stderr.contains(&expected),
            "the refusal is the documented sentence `{expected}`:\n{}",
            outcome.stderr
        );
    }
}

// ---------------------------------------------------------------------------
// §4: the venv activation rule.
// ---------------------------------------------------------------------------

/// A venv whose web app imports a module only its `site-packages` holds.
#[cfg(feature = "python")]
const VENV_APP: &str = r#"
from venv_only_mod import VALUE


def application(environ, start_response):
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [VALUE]
"#;

/// Writes `.venv` for `interpreter`: the `bin/python3` link, `pyvenv.cfg`'s
/// recorded `home`/`version`, and the minor's `site-packages` directory.
#[cfg(feature = "python")]
fn write_venv(project: &Project, interpreter: &Path, minor: &str) {
    std::fs::create_dir_all(project.path(".venv/bin")).expect("the venv bin directory");
    std::os::unix::fs::symlink(interpreter, project.path(".venv/bin/python3"))
        .expect("the venv interpreter");
    std::fs::create_dir_all(project.path(&format!(".venv/lib/python{minor}/site-packages")))
        .expect("the venv site-packages");
    let home = interpreter
        .parent()
        .expect("the interpreter's bin directory");
    project.write(
        ".venv/pyvenv.cfg",
        format!(
            "home = {}\nversion = {minor}.0\ninclude-system-site-packages = false\n",
            home.display()
        ),
    );
}

/// §4: stage 1 embeds the binary's own CPython, so the last hop of a project
/// venv is what restores its `site-packages`: this module exists nowhere else,
/// and a boot that never restored the path cannot answer.
#[cfg(feature = "python")]
#[test]
fn a_module_that_only_the_project_venv_provides_is_imported() {
    let minor = "3.14";
    let interpreter = python_for(minor);
    let project = Project::new("venv-site-packages");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", VENV_APP);
    write_venv(&project, &interpreter, minor);
    project.write(
        &format!(".venv/lib/python{minor}/site-packages/venv_only_mod.py"),
        b"VALUE = b\"hello from the venv site-packages\\n\"\n",
    );
    assert!(
        !project.path("venv_only_mod.py").exists(),
        "the module must live only inside the venv"
    );

    let mut dev = Dev::start(&project, &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/hello/");
    let response = dev.0.get("/hello/");
    assert_200("the venv-only module", &response);
    assert!(
        response.body.contains("hello from the venv site-packages"),
        "the project venv's site-packages is what the view imports from: {:?}\n{}",
        response.body,
        dev.0.text()
    );
}

/// §4: `$VIRTUAL_ENV` is consulted only when it is a venv, so an activated
/// directory that is not one (a stale activation) is **skipped**: the boot
/// serves from the project's `.venv` and its `runtime` line says so — never a
/// refusal.
#[cfg(feature = "python")]
#[test]
fn a_stale_activated_venv_is_skipped_and_the_boot_names_what_it_serves() {
    let minor = "3.14";
    let interpreter = python_for(minor);
    let project = Project::new("activated");
    project.write("Blastfile", SIMPLE_MANIFEST);
    project.write("app.py", VENV_APP);
    write_venv(&project, &interpreter, minor);
    project.write(
        &format!(".venv/lib/python{minor}/site-packages/venv_only_mod.py"),
        b"VALUE = b\"the project venv served this\\n\"\n",
    );
    // An activation whose directory has no `pyvenv.cfg`: not a venv, so it is
    // skipped and the project's own `.venv` decides the imports.
    let stale = project.path("elsewhere/venv");
    std::fs::create_dir_all(stale.join("bin")).expect("the stale activation");

    let mut dev = Dev::start(
        &project,
        &[("VIRTUAL_ENV", stale.to_str().expect("a UTF-8 path"))],
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/hello/");
    let response = dev.0.get("/hello/");
    assert_200("the project venv", &response);
    assert!(
        response.body.contains("the project venv served this"),
        "a stale activation must not change what the view imports: {:?}\n{}",
        response.body,
        dev.0.text()
    );

    // The boot's own statement about the environment it serves: the project's
    // venv, not the stale activation — the skip is on the record, and the
    // refusal never happened (`200` above).
    let log = dev.0.text();
    let venv = project.path(".venv");
    let served = format!("environment {}", venv.display());
    let runtime = log
        .lines()
        .find(|line| line.contains("runtime") && line.contains("python"))
        .unwrap_or_else(|| panic!("no `runtime` line:\n{log}"));
    assert!(
        runtime.contains(&served),
        "the runtime line names the venv the view imports from (`{served}`): {runtime}"
    );
    assert!(
        !runtime.contains("elsewhere"),
        "the skipped activation must not be named as serving: {runtime}"
    );
}

// ---------------------------------------------------------------------------
// §2/§4: a threaded WSGI route really answers concurrently.
// ---------------------------------------------------------------------------

/// How far apart the app's own per-request `t0` stamps may be for the four
/// requests to count as concurrent, in seconds.
#[cfg(feature = "python")]
const CONTENTION_SPAN: f64 = 0.100;

/// One WSGI view: one process, several requests at once.
#[cfg(feature = "python")]
const PARALLEL_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:app\"

view app regex '.*' {}
";

/// 200 ms in `time.sleep` (which releases the GIL), the last path segment back
/// as the request's own marker, and its own `t0`, so a leg that staged its
/// requests one after another cannot pass on a fast machine.
#[cfg(feature = "python")]
const PARALLEL_APP: &str = r#"
import time


def app(environ, start_response):
    t0 = time.monotonic()
    time.sleep(0.2)
    marker = environ.get("PATH_INFO", "/").rsplit("/", 1)[-1] or "root"
    body = ("wsgi:%s:%.6f" % (marker, t0)).encode()
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The marker and the request's own `t0` a WSGI answer carries
/// (`wsgi:<marker>:<t0>`).
#[cfg(feature = "python")]
fn wsgi_answer(response: &Response) -> Option<(&str, f64)> {
    let rest = response.body.split("wsgi:").nth(1)?;
    let mut fields = rest.split_whitespace().next()?.split(':');
    let marker = fields.next()?;
    let t0 = fields.next()?.parse().ok()?;
    Some((marker, t0))
}

/// Asserts one `200` per request, each carrying its **own** marker, and returns
/// the four `t0`s: a swapped or shared answer is a failure, not a coincidence.
#[cfg(feature = "python")]
fn assert_wsgi_burst(what: &str, responses: &[Response]) -> Vec<f64> {
    assert_eq!(
        responses.len(),
        views::WIDTH,
        "{what}: one response per request"
    );
    let mut starts = Vec::with_capacity(responses.len());
    for (index, response) in responses.iter().enumerate() {
        assert_200(what, response);
        let expected = format!("w{index}");
        let (marker, t0) = wsgi_answer(response).unwrap_or_else(|| {
            panic!(
                "{what}: request {index} carried no app body: {:?}",
                response.body
            )
        });
        assert_eq!(
            marker, expected,
            "{what}: request {index} answered for someone else: {:?}",
            response.body
        );
        starts.push(t0);
    }
    starts
}

/// §2/§4: four concurrent 200 ms requests to one WSGI route finish under 450 ms,
/// one process, against a serialized control measured in the same function —
/// and the app's own stamps prove they were in flight together.
#[cfg(feature = "python")]
#[test]
fn a_threaded_wsgi_route_serves_four_requests_at_once() {
    let project = Project::new("parallel");
    project.write("Blastfile", PARALLEL_MANIFEST);
    project.write("app.py", PARALLEL_APP);
    let interpreter = python_for("3.14");
    let python = interpreter.to_str().expect("a UTF-8 interpreter path");
    let paths: Vec<String> = (0..views::WIDTH)
        .map(|index| format!("/m/w{index}"))
        .collect();

    // The control: one live request slot, four requests one at a time.
    let mut control = Dev::start(
        &project,
        &[
            ("BLASTIT_PYTHON", python),
            ("BLASTIT_MAX_WORKERS", "1"),
            // The event stream is the oracle for "one process", so this boot
            // asks for the machine-readable form.
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
    );
    let control = StopOnDrop(&mut control);
    control.0.wait_event("ready");
    let started = Instant::now();
    let mut control_bodies = Vec::with_capacity(views::WIDTH);
    for path in &paths {
        control_bodies.push(control.0.get(path));
    }
    let control_wall = started.elapsed();
    assert_wsgi_burst("wsgi control", &control_bodies);
    assert!(
        control_wall >= views::CONTROL_FLOOR,
        "the serialized control took {control_wall:?}, below the {:?} floor the fixture's own \
         sleeps set",
        views::CONTROL_FLOOR
    );
    // Stopping the control hands the one-boot-at-a-time slot to the measured
    // leg: `StopOnDrop` calls `Dev::stop`, which releases it.
    drop(control);

    // The measured leg: a fresh boot at the scheduler's own width.
    let mut boot = Dev::start(
        &project,
        &[("BLASTIT_PYTHON", python), ("BLASTIT_LOG_FORMAT", "json")],
    );
    let boot = StopOnDrop(&mut boot);
    boot.0.wait_event("ready");
    assert!(
        boot.0.settled_spawned("app").is_empty(),
        "a lazy boot forks nothing before the first request"
    );
    let (wall, bodies) = burst(boot.0.client(), &paths);
    let starts = assert_wsgi_burst("wsgi parallel", &bodies);
    assert!(
        wall + views::CONTROL_MARGIN <= control_wall,
        "the parallel leg took {wall:?} against a serialized control of {control_wall:?}: it must \
         be at least {:?} faster (an absolute millisecond ceiling is deliberately not asserted: \
         it caps the host's speed, not the route's fairness)",
        views::CONTROL_MARGIN
    );
    let span = starts.iter().copied().fold(f64::MIN, f64::max)
        - starts.iter().copied().fold(f64::MAX, f64::min);
    assert!(
        span <= CONTENTION_SPAN,
        "the four requests entered the app within {:.1} ms of each other, more than the {:.0} ms \
         this leg allows: it would be measuring the client, not the route",
        span * 1000.0,
        CONTENTION_SPAN * 1000.0
    );
    let spawned = boot.0.settled_spawned("app");
    assert_eq!(
        spawned.len(),
        1,
        "a threaded WSGI route owns exactly one process however many requests it runs at once, \
         saw {spawned:?}"
    );
    println!(
        "wsgi: serialized control {control_wall:?} (BLASTIT_MAX_WORKERS=1), parallel {wall:?} \
         over {} process(es), four t0s spanning {:.1} ms",
        spawned.len(),
        span * 1000.0
    );
}
