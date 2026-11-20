//! `TESTING_PLAN.md` §2 (applications & runtimes) and §4 (`blastit dev`) — the
//! embedded-PHP and Python WSGI halves, driven through the real binary.
//!
//! This file is one of three slices that share it: `.1` (the Python story),
//! `.2` (this one: PHP), `.3` (static assets, live reload, `--learn` and
//! shutdown). The harness at the top — a temporary project root, a port from
//! `28080+`, a boot whose whole stdout/stderr is collected, and deadline-based
//! waits — is the part every slice reuses, so a new story appends tests and
//! fixtures below it and never a fourth helper.
//!
//! The Python half re-proves what `MB.56.2.1`/`MB.56.2.2` inventoried and what
//! the deleted `critical_python_shim`, `python_activation_gate`,
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
//! The PHP half absorbs `critical_php_embed`, `critical_php_backend` and the PHP
//! half of `critical_parallel_views`, and re-proves §2's PHP story:
//!
//! 1. a PSR/Laravel-shaped front controller (`public/index.php` + a static mount
//!    under `public/`) where two views hold **distinct** PostgreSQL rights: a
//!    public read-only route, a write route, and a compromised public view
//!    refused `SELECT` on another table's columns and refused `INSERT` — both
//!    `500`, both with the documented `sql.denied … <view> db parse … = $?` line,
//!    and the upstream peer never contacted with the forbidden table;
//! 2. fault isolation: one request calling `exit()` and one hitting a fatal both
//!    recover — the pool keeps its single slot, a fresh pid serves the next
//!    request, and the crashed pid is gone from `/proc`;
//! 3. `php2shell` / CGI-argument-injection immunity, behaviourally over the real
//!    SAPI: a query string, headers and a form body never become `-d` flags,
//!    `auto_prepend_file` or `HTTP_PROXY`;
//! 4. entry pinning: an entry script rewritten in place is refused with the
//!    documented sentence until a restart;
//! 5. the pool really runs four requests at once (one process each) against the
//!    same serialized control the WSGI leg measures.
//!
//! Every leg is a real `blastit dev` process answering on loopback, and the
//! database legs dial a real PostgreSQL v3 peer (`tests/common/pg_upstream.rs`)
//! through the route's own dbproxy socket. Nothing is `#[ignore]`d: a host with
//! no usable NTS `libphp` + shim skips its PHP legs **loudly** (one `SKIP …`
//! line naming what to install) instead of asserting a refusal it cannot make,
//! because the refusal sentences themselves are §6's story, not §2's.
//!
//! Every leg maps to a `TESTING_PLAN.md` story: the grant, credential, fault and
//! concurrency legs to §2, the concurrency numbers to §4's diagnostics, and the
//! entry pin to §6's "tampered application entry files".

#![allow(clippy::unwrap_used, clippy::expect_used)]
// A build with neither runtime feature has no test that reaches this harness.
#![cfg_attr(not(any(feature = "python", feature = "php")), allow(dead_code))]

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(any(feature = "python", feature = "php"))]
#[path = "common/pg_upstream.rs"]
mod pg_upstream;

/// The independent host probe (`host_php`) and the discovery-order drift check
/// every PHP leg reads instead of the code under test.
#[cfg(feature = "php")]
#[path = "common/libphp_probes.rs"]
mod libphp_probes;

/// The host's NTS `libphp` + shim, resolved without `blastit`.
#[cfg(feature = "php")]
#[path = "common/php_host.rs"]
mod php_host;

/// The HTTP client, request burst and calibration both concurrency legs read
/// (`views::CONTROL_FLOOR`, `views::CONTROL_MARGIN`, `views::WIDTH`), kept once.
#[path = "common/views.rs"]
mod views;

use views::{Client, Response};

// The burst and its assertion are used by the WSGI and PHP concurrency legs.
#[cfg(any(feature = "python", feature = "php"))]
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
/// for `route`, in either log format: the human stream aligns the route in its
/// own column (`  db  parse  `), the JSON one names it (`"route":"public"`).
fn denial_line<'a>(log: &'a str, needle: &str, route: &str) -> Option<&'a str> {
    let aligned = format!("  {route}  ");
    let json = format!("\"route\":\"{route}\"");
    log.lines().find(|line| {
        line.contains("sql.denied")
            && line.contains(needle)
            && (line.contains(&aligned) || line.contains(&json))
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

    /// The `request` event for `path`, waiting bounded for init's pipe.
    ///
    /// The event is written after the last body byte, so a snapshot taken at the
    /// instant the client is answered can be one tick early: this polls instead
    /// of reading the buffer once.
    fn request_event(&self, path: &str) -> serde_json::Value {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            let found = self
                .events()
                .into_iter()
                .find(|event| event["event"] == "request" && event["path"] == path);
            if let Some(event) = found {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "no `request` event for {path} on 127.0.0.1:{}:\n{}",
                self.port,
                self.text()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// The pids of the views this boot considers live, from its own
    /// `process.spawned` / `process.exited` / `process.retired` events.
    ///
    /// "The pool kept its slot" is this set: a crash that retired a worker and
    /// left nothing behind reads the same status as one that respawned, and only
    /// the live set tells them apart.
    fn live_view_pids(&self) -> Vec<u32> {
        let mut live: Vec<u32> = Vec::new();
        for event in self.events() {
            let pid = event["pid"].as_u64().unwrap_or(0) as u32;
            match event["event"].as_str().unwrap_or_default() {
                "process.spawned" => live.push(pid),
                "process.exited" | "process.retired" => live.retain(|live| *live != pid),
                _ => {}
            }
        }
        live.sort_unstable();
        live.dedup();
        live
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

// ---------------------------------------------------------------------------
// §2/§6: embedded PHP — the PSR front controller, per-view grants, faults,
// `php2shell`, and the entry pin. Absorbed from `critical_php_embed`,
// `critical_php_backend` and the PHP half of `critical_parallel_views`.
// ---------------------------------------------------------------------------

/// The host's NTS `libphp` + shim, or one loud `SKIP` line naming what a PHP
/// view needs.
///
/// A PHP leg cannot be graded on a host with no embed library: the boot's own
/// refusal is §6's story (pinned by `e2e_prod_refusals`), and asserting it here
/// would let a discovery regression pass as "this host has no PHP". The probe
/// decides, and the leg says so in one line rather than failing the run.
#[cfg(feature = "php")]
fn host_php_or_skip(what: &str) -> Option<php_host::HostPhp> {
    let host = php_host::host_php();
    if host.is_none() {
        eprintln!(
            "SKIP {what}: this host has no usable NTS libphp + shim (install the PHP embed \
             package and run `make php-sapi`); the served half needs the real pair"
        );
    }
    host
}

/// The host probe's table is the boot's discovery order: a copy that drifted
/// would walk a host the boot walks differently (`MB.3.62`).
#[cfg(feature = "php")]
#[test]
fn the_php_probe_table_matches_the_boot() {
    if let Err(drift) = libphp_probes::assert_matches(
        &php_host::PROBES,
        include_str!("../../blastit-view-php/src/discovery.rs"),
    ) {
        panic!("{drift}");
    }
}

/// Starts a PHP boot with the probe's own pair, the JSON event stream and
/// `extra` on top — the one place a PHP leg names `BLASTIT_LIBPHP`.
#[cfg(feature = "php")]
fn start_php(project: &Project, host: &php_host::HostPhp, extra: &[(&str, &str)]) -> Dev {
    let libphp = host.libphp.to_string_lossy().into_owned();
    let shim_dir = host
        .shim
        .parent()
        .expect("a shim directory")
        .to_string_lossy()
        .into_owned();
    let mut env: Vec<(String, String)> = vec![
        ("BLASTIT_LIBPHP".to_owned(), libphp),
        ("BLASTIT_PHP_SHIM_DIR".to_owned(), shim_dir),
        ("BLASTIT_LOG_FORMAT".to_owned(), "json".to_owned()),
    ];
    env.extend(
        extra
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    let pairs: Vec<(&str, &str)> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    Dev::start(project, &pairs)
}

/// The marker and the pid a PHP answer carries (`php:<marker>:<pid>`).
#[cfg(feature = "php")]
fn php_answer(response: &Response) -> Option<(&str, u32)> {
    let rest = response.body.split("php:").nth(1)?;
    let (marker, rest) = rest.split_once(':')?;
    Some((marker, rest.split_whitespace().next()?.parse().ok()?))
}

/// Asserts one `200` per response, each carrying its **own** marker, and returns
/// the pids that served them in request order: a swapped or shared answer fails.
#[cfg(feature = "php")]
fn assert_php_burst(what: &str, responses: &[Response]) -> Vec<u32> {
    assert_eq!(
        responses.len(),
        views::WIDTH,
        "{what}: one response per request"
    );
    let mut pids = Vec::with_capacity(responses.len());
    for (index, response) in responses.iter().enumerate() {
        assert_200(what, response);
        let (marker, pid) = php_answer(response).unwrap_or_else(|| {
            panic!(
                "{what}: request {index} carried no page body: {:?}",
                response.body
            )
        });
        let expected = format!("p{index}");
        assert_eq!(
            marker, expected,
            "{what}: request {index} answered for someone else: {:?}",
            response.body
        );
        pids.push(pid);
    }
    pids
}

/// The distinct pids of a burst, sorted.
#[cfg(feature = "php")]
fn worker_set(pids: &[u32]) -> Vec<u32> {
    let mut set = pids.to_vec();
    set.sort_unstable();
    set.dedup();
    set
}

/// Everything a PHP worker wrote to its own stderr, read back out of the JSON
/// `app.output` events (`logs::fail` emits each line as one event, with the
/// quotes of `route "app"` JSON-escaped — so the sentence is never greppable in
/// the encoded line).
#[cfg(feature = "php")]
fn worker_output(dev: &Dev) -> String {
    dev.events()
        .into_iter()
        .filter(|event| event["event"] == "app.output")
        .filter_map(|event| event["line"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `pid` still has a `/proc` entry: the external witness that a retired
/// worker is gone and not merely quiet.
#[cfg(feature = "php")]
fn in_proc(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// A PSR-style project: a front controller under `public/`, a static mount of
/// the same directory, and two views with **distinct** PostgreSQL rights.
///
/// The store is the URL form; `a_php_split_store_renders_the_laravel_slots`
/// covers `docs/frameworks/php/laravel.md`'s split `DB_*` form. The
/// Laravel/Symfony `$_SERVER` shape itself is `e2e/php2shell.py`'s story.
#[cfg(feature = "php")]
const PHP_GRANT_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

postgres $DATABASE_URL

static \"/\" {
    root \"public\"
}

view public GET \"/read\", \"/read/*\" {
    postgres {
        select products(id, name)
    }
}

view admin POST \"/admin/write\" {
    postgres {
        select products(id, name)
        insert products(id, name)
    }
}
";

/// The static asset the mount answers without a `libphp` request.
#[cfg(feature = "php")]
const PHP_ASSET: &str = "body{color:#0b0}\n";

/// The front controller: a PSR-style router over a real PostgreSQL v3 client on
/// the route's own dbproxy socket.
///
/// The wire client is deliberate, not an accident: this host and every
/// `e2e/php-sapi` leg image ship no `pdo_pgsql`, so a PDO leg here would be
/// unexecuted code (`RESULT.md` records the measurement). What the fixture does
/// keep is the shape the grants are learned and matched in: every statement is a
/// **prepared** statement with `$1` binds and never an inlined literal, so the
/// proxy sees `$?` and never a value.
#[cfg(feature = "php")]
const PHP_GRANT_APP: &str = r#"<?php
function pg_params() {
    $url = (string) getenv('DATABASE_URL');
    $query = str_contains($url, '?') ? substr($url, strpos($url, '?') + 1) : '';
    $params = [];
    foreach (explode('&', $query) as $part) {
        if (!str_contains($part, '=')) { continue; }
        [$name, $value] = explode('=', $part, 2);
        $params[$name] = urldecode($value);
    }
    return [$params['host'] ?? '/proc/self/cwd', $params['port'] ?? '5432'];
}

function pg_read($sock, $count) {
    $body = '';
    while (strlen($body) < $count) {
        $chunk = fread($sock, $count - strlen($body));
        if ($chunk === false || $chunk === '') { throw new RuntimeException('pg: closed'); }
        $body .= $chunk;
    }
    return $body;
}

function pg_until_ready($sock) {
    while (true) {
        $tag = pg_read($sock, 1);
        $length = unpack('N', pg_read($sock, 4))[1];
        $body = pg_read($sock, $length - 4);
        if ($tag === 'E') { throw new RuntimeException('pg: ' . $body); }
        if ($tag === 'Z') { return; }
    }
}

function pg_connect() {
    [$host, $port] = pg_params();
    $sock = stream_socket_client('unix://' . $host . '/.s.PGSQL.' . $port, $errno, $errstr, 10);
    if ($sock === false) { throw new RuntimeException('pg: ' . $errstr); }
    $startup = pack('N', 196608) . "user\0blastit\0database\0app\0\0";
    fwrite($sock, pack('N', strlen($startup) + 4) . $startup);
    pg_until_ready($sock);
    return $sock;
}

function pg($sql, ...$values) {
    $sock = pg_connect();
    try {
        $body = "\0" . $sql . "\0" . pack('n', 0);
        fwrite($sock, 'P' . pack('N', strlen($body) + 4) . $body);
        $body = "\0\0" . pack('nn', 0, count($values));
        foreach ($values as $value) { $body .= pack('N', strlen($value)) . $value; }
        $body .= pack('n', 0);
        fwrite($sock, 'B' . pack('N', strlen($body) + 4) . $body);
        fwrite($sock, 'D' . pack('N', 6) . "P\0");
        $body = "\0" . pack('N', 0);
        fwrite($sock, 'E' . pack('N', strlen($body) + 4) . $body);
        fwrite($sock, 'S' . pack('N', 4));
        pg_until_ready($sock);
    } finally {
        fclose($sock);
    }
}

$segment = basename((string) ($_SERVER['PATH_INFO'] ?? '/'));
$status = 200;
try {
    if ($segment === 'read') {
        pg('SELECT id, name FROM products WHERE id = $1', '7');
        $payload = ['ok' => true];
    } elseif ($segment === 'orders') {
        pg('SELECT id, name FROM orders WHERE id = $1', '7');
        $payload = ['ok' => true];
    } elseif ($segment === 'columns') {
        pg('SELECT id, name, cost FROM products WHERE id = $1', '7');
        $payload = ['ok' => true];
    } elseif ($segment === 'insert') {
        pg('INSERT INTO orders (id, name) VALUES ($1, $2)', '7', 'leak');
        $payload = ['ok' => true];
    } elseif ($segment === 'update') {
        pg('UPDATE products SET name = $1 WHERE id = $2', 'leak', '7');
        $payload = ['ok' => true];
    } elseif ($segment === 'write') {
        pg('INSERT INTO products (id, name) VALUES ($1, $2)', '7', 'widget');
        $payload = ['ok' => true];
    } elseif ($segment === 'env') {
        $payload = ['url' => (string) getenv('DATABASE_URL')];
    } else {
        $payload = ['app' => 'blastit-php-ok'];
    }
} catch (Throwable $err) {
    $status = 500;
    $payload = ['error' => $err->getMessage()];
}
http_response_code($status);
header('Content-Type: application/json');
echo json_encode($payload);
"#;

/// §2: the PSR front controller serves its static mount, and each view runs only
/// its own PostgreSQL grants — a compromised public view is refused on another
/// table's columns and on `INSERT`, and never reaches the upstream with either.
#[cfg(feature = "php")]
#[test]
fn a_multi_view_php_front_controller_serves_assets_and_only_its_view_grants() {
    use pg_upstream::PgUpstream;

    let Some(host) = host_php_or_skip("PHP per-view grants") else {
        return;
    };
    let server = PgUpstream::start();
    let secret = format!("php-secret-{}", std::process::id());
    let url = format!(
        "postgresql://real-user:{secret}@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new("php-grants");
    project.write("Blastfile", PHP_GRANT_MANIFEST);
    project.write("public/index.php", PHP_GRANT_APP);
    project.write("public/assets/app.css", PHP_ASSET);

    let mut dev = start_php(&project, &host, &[("DATABASE_URL", &url)]);
    let dev = StopOnDrop(&mut dev);

    // The static mount answers from disk: waiting on the asset is also the
    // readiness wait, and a `libphp` request would be the bug it proves against.
    dev.0.wait_ready("/assets/app.css");
    let asset = dev.0.get("/assets/app.css");
    assert_200("the static mount", &asset);
    assert!(
        asset.body.contains("body{color:#0b0}"),
        "the mount must answer the file's own bytes: {:?}\n{}",
        asset.body,
        dev.0.text()
    );
    // A `.php` source under the static root is never served (`docs/frameworks/
    // php.md`: sources are `404`), and neither static answer cost a worker.
    let source = dev.0.get("/index.php");
    assert_eq!(
        source.status,
        404,
        "a source file under the mount's root must be refused: {:?}\n{}",
        source.body,
        dev.0.text()
    );
    // The other two PHP source spellings the static responder must never stream.
    for name in ["/index.phtml", "/index.phar"] {
        assert_eq!(
            dev.0.get(name).status,
            404,
            "{name} is a PHP source and must be refused:\n{}",
            dev.0.text()
        );
    }
    assert!(
        dev.0.spawned_pids("public").is_empty() && dev.0.spawned_pids("admin").is_empty(),
        "a static answer must not fork a PHP worker:\n{}",
        dev.0.text()
    );

    // The grant: the public view's prepared read and the admin view's insert.
    let read = dev.0.get("/read");
    assert_200("the public read", &read);
    assert!(
        read.body.contains("\"ok\":true"),
        "the granted read must succeed: {:?}\n{}",
        read.body,
        dev.0.text()
    );
    let write = dev.0.request("POST", "/admin/write");
    assert_200("the admin write", &write);
    assert!(
        write.body.contains("\"ok\":true"),
        "the granted write must succeed: {:?}\n{}",
        write.body,
        dev.0.text()
    );

    // Zero credentials (§2): the rendered URL reaches the worker, the real one
    // does not.
    let env = dev.0.get("/read/env");
    assert_200("the environment probe", &env);
    assert!(
        !env.body.contains(&secret),
        "the view can read the real database password: {:?}",
        env.body
    );
    assert!(
        !env.body.contains("real-user"),
        "the proxy's real user reached the view: {:?}",
        env.body
    );

    // A compromised public view: an ungranted statement, an ungranted column, an
    // ungranted `INSERT` and an ungranted `UPDATE` are refused at runtime, each a
    // `500` with the documented normalised line naming this view and the proxy.
    for (path, needle, what) in [
        (
            "/read/orders",
            "FROM orders WHERE id = $?",
            "another table's columns",
        ),
        (
            "/read/columns",
            "cost FROM products WHERE id = $?",
            "an ungranted column",
        ),
        (
            "/read/insert",
            "INSERT INTO orders",
            "an ungranted `INSERT`",
        ),
        ("/read/update", "UPDATE products", "an ungranted `UPDATE`"),
    ] {
        let refused = dev.0.get(path);
        assert_eq!(
            refused.status,
            500,
            "{what} must fail the request: {:?}\n{}",
            refused.body,
            dev.0.text()
        );
        // The SQLSTATE reaches the app in its error text: that is a proxy→app
        // passthrough oracle (the page echoes `$err->getMessage()`), not a
        // sentence any page promises — the `sql.denied` line below is the
        // documented one.
        assert!(
            refused.body.contains("42501") || refused.body.contains("denied"),
            "{what} is refused by the proxy, not by the fixture: {:?}",
            refused.body
        );
        // `sql.denied …  public  db  parse  … = $?` (or the JSON stream's
        // `"route":"public" … "proxy":"db","kind":"parse"`): the view, the proxy
        // and the statement normalised with `$?` — never a parameter value.
        let line = wait_for_denial(dev.0, needle, "public");
        assert!(
            line.contains("\"proxy\":\"db\"") && line.contains("\"kind\":\"parse\""),
            "the denial names the proxy and the parse kind: {line}"
        );
        assert!(
            !line.contains("= 7") && !line.contains("'widget'"),
            "a parameter value reached the log: {line}"
        );
    }

    // The denied view keeps serving its own grant and the sibling view is
    // untouched (`docs/local.md`: "every other view is untouched").
    assert_200("the denied view", &dev.0.get("/read"));
    assert_200("the sibling view", &dev.0.request("POST", "/admin/write"));

    // The proxy really reached a database — the granted statements forwarded,
    // the denied ones never sent — and authenticated as the operator's user.
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
        "the admin view's insert reached the upstream: {statements:?}"
    );
    // The public view contributed **none**: each forbidden statement carries a
    // table or verb only it names (`orders`, `cost`, `UPDATE`), so its absence is
    // the assertion and no literal count depends on the test's own request order.
    assert!(
        !statements
            .iter()
            .any(|statement| statement.contains("orders")
                || statement.contains("cost")
                || statement.contains("UPDATE")),
        "a statement only the public view sent reached the upstream: {statements:?}"
    );
    assert!(
        server
            .startups()
            .iter()
            .any(|startup| startup.user == "real-user"),
        "dbproxy authenticated with the proxy's own credential: {:?}",
        server.startups()
    );

    // `served: HTTP/1.1 200 OK` plus the library path is the evidence line
    // `make rs-test-php-image` and `e2e/php-sapi/run-two-package.sh` grep: a leg
    // that skipped prints no `served:` line, so it can never be a witness.
    println!(
        "served: HTTP/1.1 200 OK (libphp {}) — the mount answered the asset and refused the \
         source, `public` ran only its `$1` read, `admin` its insert, four denials were logged \
         normalised, and no credential reached a worker",
        host.libphp.display()
    );
}

/// The fault-isolation route: `prefork` warms the pool's one slot, so a crash has
/// a worker to retire and the next request has a slot to be served by.
#[cfg(feature = "php")]
const PHP_FAULT_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    prefork
}
";

/// The fault page: `/crash-exit` and `/crash-fatal` bail out of their own
/// request; everything else answers. One page for both legs, because a contained
/// bailout must not require rewriting the pinned entry between requests.
///
/// The busy loop is deliberate: `CLOCK_PROCESS_CPUTIME_ID` reports whole ticks,
/// and a script that returns immediately can measure `0`.
#[cfg(feature = "php")]
const PHP_FAULT_APP: &str = r#"<?php
$segment = basename((string) ($_SERVER['PATH_INFO'] ?? '/'));
header('Content-Type: text/plain');
if ($segment === 'crash-exit') {
    for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
    exit(0);
}
if ($segment === 'crash-fatal') {
    for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
    trigger_error('blastit fatal', E_USER_ERROR);
}
echo "blastit-php-ok\n";
"#;

/// §2: `exit()` and a fatal are contained in their own request — a `bailed`
/// outcome, never a `200`, a `502` or a `view_died` — the pool keeps its one
/// slot, and the next request is served by a fresh worker.
#[cfg(feature = "php")]
#[test]
fn a_php_exit_and_a_fatal_recover_without_dropping_the_pool() {
    let Some(host) = host_php_or_skip("PHP fault isolation") else {
        return;
    };
    let project = Project::new("php-faults");
    project.write("Blastfile", PHP_FAULT_MANIFEST);
    project.write("public/index.php", PHP_FAULT_APP);
    // One request slot, named explicitly: the pool's size is the premise this leg
    // reads, so it must not be whatever the default happens to be.
    let mut dev = start_php(&project, &host, &[("BLASTIT_MAX_WORKERS", "1")]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    let warmed = dev.0.settled_spawned("app");
    assert_eq!(
        warmed.len(),
        1,
        "`prefork` warms exactly the pool's one slot: {warmed:?}"
    );
    let mut respawned: Vec<(u32, u32)> = Vec::new();

    for (path, what) in [("/crash-exit", "exit()"), ("/crash-fatal", "a fatal")] {
        // The pool's one live slot *before* the crash: the pid to retire, read
        // from init's own spawn/exit events rather than from the request event.
        let before = dev.0.live_view_pids();
        assert_eq!(
            before.len(),
            1,
            "{what}: one live pool slot before the crash: {before:?}"
        );
        let crashed_pid = before[0];
        let crashed = dev.0.get(path);
        assert!(
            crashed.status >= 500,
            "{what}: a contained bailout is never a `2xx`: {:?}\n{}",
            crashed.body,
            dev.0.text()
        );
        assert!(
            !crashed.body.contains("view_died"),
            "{what}: a contained bailout leaves the worker alive, so it is not `view_died`: {:?}",
            crashed.body
        );
        let event = dev.0.request_event(path);
        assert_eq!(
            event["status"], crashed.status,
            "{what}: the event agrees with the wire: {event}"
        );
        assert!(
            event["outcome"] == "bailed",
            "{what}: a handler that never returned is reported as `bailed`: {event}"
        );
        // The slot still serves, and it is one slot: the bailed pid is gone from
        // `/proc` and exactly one worker of this route is live. Which event names
        // its retirement (`exited` for a clean self-retire, `retired` when the
        // supervisor reaps it) is the scheduler's business — the live set is what
        // "the pool kept its slot" means.
        let after = dev.0.get("/");
        assert_eq!(
            after.status,
            200,
            "{what}: the next request must be served: {:?}\n{}",
            after.body,
            dev.0.text()
        );
        assert!(
            after.body.contains("blastit-php-ok"),
            "{what}: the served page is the pinned one: {:?}",
            after.body
        );
        let deadline = Instant::now() + BOOT_TIMEOUT;
        while in_proc(crashed_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !in_proc(crashed_pid),
            "{what}: the crashed worker {crashed_pid} is still in `/proc`, so the pool never \
             retired it:\n{}",
            dev.0.text()
        );
        let live = dev.0.live_view_pids();
        assert_eq!(
            live.len(),
            1,
            "{what}: the pool keeps exactly its one slot, live {live:?}:\n{}",
            dev.0.text()
        );
        assert_ne!(
            live[0], crashed_pid,
            "{what}: the slot must be a fresh worker: {live:?}"
        );
        respawned.push((crashed_pid, live[0]));
    }

    let spawned = dev.0.settled_spawned("app").len();
    assert!(
        spawned >= 3,
        "`prefork` plus one worker per crash is at least three spawns, saw {spawned}:\n{}",
        dev.0.text()
    );
    // `retired pid … respawned pid …` is the second evidence line CI greps: only a
    // leg that really crashed and really grew the slot back prints it.
    for (retired, fresh) in &respawned {
        println!(
            "php faults: retired pid {retired} respawned pid {fresh} after a contained bailout"
        );
    }
    println!(
        "php faults: two contained bailouts, each answered `5xx` as `bailed`, and the next \
         request served"
    );
}

/// The `php2shell`/CGI route: a GET endpoint and a `form` POST endpoint, so a
/// body that reaches `$_POST` can be told apart from one that reaches PHP's
/// interpreter settings.
#[cfg(feature = "php")]
const PHP_INJECTION_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app {
    GET \"/*\"
    POST \"/*\" {
        body form
    }
}
";

/// What the front controller reports: the interpreter settings a CGI argument
/// injection would rewrite, the proxy variables, and the form value it received.
///
/// `allow_url_include` is deliberately absent: PHP 8.0 removed the setting, so
/// `ini_get` on it is falsy on every supported minor whatever a payload does.
#[cfg(feature = "php")]
const PHP_INJECTION_APP: &str = r#"<?php
$post = $_POST;
echo json_encode([
    'argv' => $_SERVER['argv'] ?? [],
    'query' => (string) ($_SERVER['QUERY_STRING'] ?? ''),
    'proxy' => (string) getenv('HTTP_PROXY') . (string) getenv('http_proxy'),
    'auto_prepend_file' => (string) ini_get('auto_prepend_file'),
    'auto_append_file' => (string) ini_get('auto_append_file'),
    'post_x' => isset($post['x']) ? $post['x'] : 'absent',
]);
"#;

/// §2: `php2shell`'s class is request data reaching PHP's interpreter, and the
/// embedded SAPI has no CGI argument surface at all — so a query string, headers
/// and a form body stay data over the real SAPI.
#[cfg(feature = "php")]
#[test]
fn a_php_request_cannot_become_interpreter_arguments_or_a_proxy() {
    let Some(host) = host_php_or_skip("PHP php2shell immunity") else {
        return;
    };
    let project = Project::new("php2shell");
    project.write("Blastfile", PHP_INJECTION_MANIFEST);
    project.write("public/index.php", PHP_INJECTION_APP);
    // The canary a prepend would run: the payload names this **existing** file, so
    // a hit prints its marker before the page's own output.
    project.write("public/prelude.php", "<?php echo \"PREPENDED-RAN\\n\";\n");
    let prelude = project.path("public/prelude.php").display().to_string();
    let mut dev = start_php(&project, &host, &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");

    // The CVE-2024-4577 shape (`%AD` is a soft hyphen on Windows code pages, the
    // byte that made a CGI query string into `-d` flags), a plain `-d` string,
    // and the proxy/header vectors. Both `-d` vectors point `auto_prepend_file` at
    // a file that exists, so "it never ran" is a witness and not a tautology.
    let vectors: Vec<(String, &str)> = vec![
        (format!("/?%ADd+-d+auto_prepend_file%3D{prelude}"), ""),
        (format!("/?-d+auto_prepend_file%3D{prelude}"), ""),
        (
            "/".to_owned(),
            "Proxy: http://127.0.0.1:1\r\nProxy-Authorization: Basic Zm9vOmJhcg==",
        ),
    ];
    for (target, extra) in &vectors {
        let headers: Vec<(&str, &str)> = if extra.is_empty() {
            Vec::new()
        } else {
            extra
                .split("\r\n")
                .filter_map(|line| line.split_once(": "))
                .collect()
        };
        let response = views::request_with(dev.0.port, "GET", target, &headers, None)
            .unwrap_or_else(|| panic!("no answer to GET {target}:\n{}", dev.0.text()));
        assert_200(&format!("GET {target}"), &response);
        assert_injection_free(&response.body, target, dev.0);
    }

    // The same payload in a form body: it must arrive as `$_POST` data and never
    // as an interpreter setting (this is also the `form` codec's proof that a
    // buffered body reaches the front controller).
    let form = format!("x=-d+auto_prepend_file%3D{prelude}");
    let response = views::request_with(
        dev.0.port,
        "POST",
        "/post",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        Some(&form),
    )
    .unwrap_or_else(|| panic!("no answer to the form post:\n{}", dev.0.text()));
    assert_200("the form post", &response);
    assert!(
        response.body.contains("-d auto_prepend_file="),
        "the body must reach `$_POST` as data: {:?}",
        response.body
    );
    assert_injection_free(&response.body, "the form body", dev.0);
    println!("php2shell: query string, headers and a form body stayed data over the real SAPI");
}

/// Asserts one `php2shell` answer: no `-d` argument surface, no proxy variable,
/// and the planted prepend file never ran.
#[cfg(feature = "php")]
fn assert_injection_free(body: &str, what: &str, dev: &Dev) {
    for (field, expected) in [
        ("\"argv\":[]", "the SAPI passes no argv"),
        ("\"proxy\":\"\"", "no `HTTP_PROXY` reaches the worker"),
        ("\"auto_prepend_file\":\"\"", "no `auto_prepend_file`"),
        ("\"auto_append_file\":\"\"", "no `auto_append_file`"),
    ] {
        assert!(
            body.contains(field),
            "{what}: {expected} ({field} missing): {body:?}\n{}",
            dev.text()
        );
    }
    assert!(
        !body.contains("PREPENDED-RAN"),
        "{what}: the payload named a file that exists and it ran: {body:?}"
    );
}

/// The entry-pin route: a `regex` view, so every path reaches the one page.
#[cfg(feature = "php")]
const PHP_PLAIN_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

/// The pinned front controller's body.
#[cfg(feature = "php")]
const PHP_PLAIN_APP: &str = "\
<?php
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
";

/// §6: an entry script rewritten **in place** (the same inode: `cp`, `tar -x`, a
/// deploy script) keeps its size but moves `st_ctime`, so the boot's record is
/// gone — the route refuses with the documented sentence until a restart records
/// what is there now.
///
/// The decoy is the same length as the page it replaces, so `st_size` alone
/// cannot catch it and `st_ctime` is what must. The `O_PATH` descriptor that
/// makes a freed inode number un-reusable has its own witness in
/// `blastit-view-php`'s unit tests.
#[cfg(feature = "php")]
#[test]
fn a_php_entry_rewritten_in_place_is_refused_until_a_restart() {
    use std::os::unix::fs::MetadataExt;

    let Some(host) = host_php_or_skip("PHP entry pin") else {
        return;
    };
    let project = Project::new("php-entry-pin");
    project.write("Blastfile", PHP_PLAIN_MANIFEST);
    let entry = project.write("public/index.php", PHP_PLAIN_APP);
    let pinned = std::fs::metadata(&entry).expect("the entry's metadata");
    let mut dev = start_php(&project, &host, &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/users/42");
    let served = dev.0.get("/users/42");
    assert_200("an untouched entry", &served);
    assert!(
        served.body.contains("blastit-php-ok"),
        "an untouched tree serves its own entry: {:?}",
        served.body
    );

    // The rewrite: the **same** inode, the same size, another script. `cp` and
    // `tar -x` write through the existing file; nothing is renamed.
    let original = std::fs::read(&entry).expect("the recorded page");
    let mut decoy = String::from("<?php echo \"IN-PLACE-DECOY-RAN\\n\"; //");
    while decoy.len() < original.len() {
        decoy.push('x');
    }
    assert_eq!(
        decoy.len(),
        original.len(),
        "the decoy must be the same length, or st_size alone would catch it"
    );
    // Cross the kernel's coarse timestamp tick before rewriting: `st_ctime`
    // advances one tick at a time, so a same-size rewrite inside the tick the
    // boot stamped would be invisible to any metadata record.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&entry, &decoy).expect("rewriting the entry in place");
    let rewritten = std::fs::metadata(&entry).expect("the rewritten entry's metadata");
    assert_eq!(
        (rewritten.dev(), rewritten.ino()),
        (pinned.dev(), pinned.ino()),
        "a write in place must keep the inode"
    );
    assert_eq!(
        rewritten.len(),
        pinned.len(),
        "and it must keep st_size, so only st_ctime can catch it"
    );
    // The premise the refusal rests on, stated rather than inferred from a `500`.
    assert_ne!(
        (rewritten.ctime(), rewritten.ctime_nsec()),
        (pinned.ctime(), pinned.ctime_nsec()),
        "a write in place must move st_ctime, or this leg measures the clock tick"
    );

    let refused = dev.0.get("/users/42");
    assert!(
        refused.status >= 500,
        "an entry rewritten in place must be refused, not served: {:?}\n{}",
        refused.body,
        dev.0.text()
    );
    assert!(
        !refused.body.contains("IN-PLACE-DECOY-RAN"),
        "the in-place decoy ran: the pin compared identity only, not size/ctime: {:?}",
        refused.body
    );
    assert!(
        !refused.body.contains("blastit-php-ok"),
        "the refusal answered with the app's own body: {:?}",
        refused.body
    );
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        let logged = worker_output(dev.0);
        if logged.contains(&entry.display().to_string())
            && logged.contains("changed since this boot recorded it")
            && logged.contains("restart blastit to run the file as it is now")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the worker never named the entry that moved and the fix: {logged:?}\n{}",
            dev.0.text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // The fix the sentence names: put the page back (in place) and restart, so
    // the boot records what is there now. That is the deploy/restore path.
    std::fs::write(&entry, PHP_PLAIN_APP).expect("restoring the page in place");
    dev.0.stop();
    let mut fresh = start_php(&project, &host, &[]);
    let fresh = StopOnDrop(&mut fresh);
    fresh.0.wait_ready("/users/42");
    let again = fresh.0.get("/users/42");
    assert_200("the restored page", &again);
    assert!(
        again.body.contains("blastit-php-ok"),
        "a restart must serve the file as it is now: {:?}\n{}",
        again.body,
        fresh.0.text()
    );
    println!(
        "php entry pin: served, refused in place with the documented sentence, and served again \
         after a restart"
    );
}

/// The concurrency route: `prefork` warms a worker, so the measured window has
/// nothing cold inside it.
#[cfg(feature = "php")]
const PHP_PARALLEL_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    prefork
}
";

/// 200 ms in `usleep`, then this request's own marker and the pid that served it
/// — so a swapped response and a shared process are both detectable from the
/// body alone.
#[cfg(feature = "php")]
const PHP_PARALLEL_APP: &str = r#"<?php
$marker = $_GET['m'] ?? 'none';
usleep(200000);
header('Content-Type: text/plain');
echo 'php:' . $marker . ':' . getmypid() . "\n";
"#;

/// §2/§4: four concurrent 200 ms requests to one PHP route finish under the
/// serialized control's own floor, one process each — and the warm workers are
/// reused, so no fork, `dlopen` or `php_module_startup` hides inside the window.
#[cfg(feature = "php")]
#[test]
fn a_php_pool_serves_four_requests_at_once() {
    let Some(host) = host_php_or_skip("PHP concurrency") else {
        return;
    };
    let project = Project::new("php-parallel");
    project.write("Blastfile", PHP_PARALLEL_MANIFEST);
    project.write("public/index.php", PHP_PARALLEL_APP);
    let paths: Vec<String> = (0..views::WIDTH)
        .map(|index| format!("/php?m=p{index}"))
        .collect();

    // The control: one live request slot, four requests one at a time.
    let mut control = start_php(&project, &host, &[("BLASTIT_MAX_WORKERS", "1")]);
    let control = StopOnDrop(&mut control);
    control.0.wait_event("ready");
    let started = Instant::now();
    let mut control_bodies = Vec::with_capacity(views::WIDTH);
    for path in &paths {
        control_bodies.push(control.0.get(path));
    }
    let control_wall = started.elapsed();
    let control_pids = assert_php_burst("php control", &control_bodies);
    assert_eq!(
        worker_set(&control_pids).len(),
        1,
        "with BLASTIT_MAX_WORKERS=1 one worker serves all four serially, saw {control_pids:?}"
    );
    assert!(
        control_wall >= views::CONTROL_FLOOR,
        "the serialized control took {control_wall:?}, below the {:?} floor the fixture's own \
         sleeps set — the parallel leg would prove nothing",
        views::CONTROL_FLOOR
    );
    // Stopping the control hands the one-boot-at-a-time slot to the measured
    // leg: `StopOnDrop` calls `Dev::stop`, which releases it.
    drop(control);

    // The measured boot: four workers, one of them warm.
    let mut boot = start_php(
        &project,
        &host,
        &[
            ("BLASTIT_MAX_WORKERS", "4"),
            ("BLASTIT_MAX_VIEW_PROCESSES", "8"),
        ],
    );
    let boot = StopOnDrop(&mut boot);
    boot.0.wait_event("ready");

    // The warm-up burst: every worker's `dlopen` + `php_module_startup` is paid
    // here, not inside the measured window.
    let (_warm_wall, warm_bodies) = burst(boot.0.client(), &paths);
    let warm_workers = worker_set(&assert_php_burst("php warm-up", &warm_bodies));
    assert_eq!(
        warm_workers.len(),
        views::WIDTH,
        "the warm-up burst is four wide: it must have used four processes, saw {warm_workers:?}"
    );
    let spawned_before = boot.0.settled_spawned("app");

    let (wall, bodies) = burst(boot.0.client(), &paths);
    let pids = assert_php_burst("php measured", &bodies);
    assert_eq!(
        worker_set(&pids),
        warm_workers,
        "the measured window must reuse the four warm workers {warm_workers:?}, saw {pids:?}"
    );
    assert_eq!(
        boot.0.settled_spawned("app"),
        spawned_before,
        "no `process.spawned` may appear inside the measured window"
    );
    assert!(
        wall + views::CONTROL_MARGIN <= control_wall,
        "the parallel leg took {wall:?} against a serialized control of {control_wall:?}: it must \
         be at least {:?} faster (an absolute millisecond ceiling is deliberately not asserted: \
         it caps the host, not the pool)",
        views::CONTROL_MARGIN
    );
    println!(
        "php: serialized control {control_wall:?} (BLASTIT_MAX_WORKERS=1), parallel {wall:?} \
         over {} warm process(es)",
        spawned_before.len()
    );
}

/// §6 sizing: `BLASTIT_MAX_VIEW_PROCESSES` really bounds one view's live
/// processes — four concurrent 200 ms requests under a cap of two run on exactly
/// two processes, in two waves, and no fifth worker is hidden by the fan-in.
#[cfg(feature = "php")]
#[test]
fn a_php_view_process_cap_bounds_the_pool() {
    let Some(host) = host_php_or_skip("PHP view-process cap") else {
        return;
    };
    let project = Project::new("php-view-cap");
    project.write("Blastfile", PHP_PARALLEL_MANIFEST);
    project.write("public/index.php", PHP_PARALLEL_APP);
    let paths: Vec<String> = (0..views::WIDTH)
        .map(|index| format!("/php?m=p{index}"))
        .collect();
    let mut dev = start_php(
        &project,
        &host,
        &[
            ("BLASTIT_MAX_WORKERS", "4"),
            ("BLASTIT_MAX_VIEW_PROCESSES", "2"),
        ],
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_event("ready");

    let (wall, bodies) = burst(dev.0.client(), &paths);
    let workers = worker_set(&assert_php_burst("php cap", &bodies));
    assert_eq!(
        workers.len(),
        2,
        "BLASTIT_MAX_VIEW_PROCESSES=2 must bound the pool to two processes, saw {workers:?}"
    );
    assert_eq!(
        dev.0.settled_spawned("app"),
        workers,
        "init must have forked exactly the two workers that answered, no more"
    );
    // The fixture's own sleep makes the cap visible in time as well as in pids:
    // two processes cannot answer four 200 ms requests in one wave.
    assert!(
        wall >= Duration::from_millis(400),
        "four 200 ms requests over a two-process cap took {wall:?}, inside one wave"
    );
    println!(
        "php cap: BLASTIT_MAX_VIEW_PROCESSES=2 served four concurrent requests over {workers:?} \
         in {wall:?}"
    );
}

/// `docs/frameworks/php/laravel.md`'s split store: the framework reads `DB_*`,
/// not a URL, so the slots are what has to render.
#[cfg(feature = "php")]
const PHP_SPLIT_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

postgres {
    host $DB_HOST
    port $DB_PORT
    user $DB_USERNAME
    password $DB_PASSWORD
    database $DB_DATABASE
}

view app GET \"/read\" {
    postgres {
        select products(id, name)
    }
}
";

/// A Laravel-shaped reader: it parses what the store rendered — no URL, no
/// `pdo_pgsql` — and asks the proxy one granted, unparameterized statement.
#[cfg(feature = "php")]
const PHP_SPLIT_APP: &str = r#"<?php
function slot($name) { return (string) getenv($name); }
function drain($sock, &$payload) {
    while (($tag = fread($sock, 1)) !== false && $tag !== '') {
        $length = unpack('N', fread($sock, 4))[1];
        $body = $length > 4 ? fread($sock, $length - 4) : '';
        if ($tag === 'E') { $payload['error'] = $body; }
        if ($tag === 'Z') { return; }
        $payload['tags'] .= $tag;
    }
}
$payload = [
    'host' => slot('DB_HOST'),
    'port' => slot('DB_PORT'),
    'database' => slot('DB_DATABASE'),
    'user' => slot('DB_USERNAME'),
    'password' => slot('DB_PASSWORD'),
];
$sock = @stream_socket_client(
    'unix://' . slot('DB_HOST') . '/.s.PGSQL.' . slot('DB_PORT'), $errno, $errstr, 10);
if ($sock === false) {
    $payload['error'] = $errstr;
} else {
    $startup = pack('N', 196608) . "user\0" . slot('DB_USERNAME')
        . "\0database\0" . slot('DB_DATABASE') . "\0\0";
    fwrite($sock, pack('N', strlen($startup) + 4) . $startup);
    $payload['tags'] = '';
    drain($sock, $payload);
    $payload['tags'] = '';
    $sql = 'SELECT id, name FROM products';
    fwrite($sock, 'Q' . pack('N', strlen($sql) + 5) . $sql . "\0");
    drain($sock, $payload);
    $payload['query_ok'] = str_contains($payload['tags'], 'T')
        && str_contains($payload['tags'], 'C') ? 'yes' : 'no';
    fclose($sock);
}
header('Content-Type: application/json');
echo json_encode($payload);
"#;

/// §2: the split `DB_*` store renders the local endpoint and a scoped credential,
/// and one granted statement runs through those slots — the runtime witness for
/// `docs/frameworks/php/laravel.md`'s store block.
#[cfg(feature = "php")]
#[test]
fn a_php_split_store_renders_the_laravel_slots() {
    use pg_upstream::PgUpstream;

    let Some(host) = host_php_or_skip("PHP split store") else {
        return;
    };
    let server = PgUpstream::start();
    let secret = format!("split-secret-{}", std::process::id());
    let project = Project::new("php-split-store");
    project.write("Blastfile", PHP_SPLIT_MANIFEST);
    project.write("public/index.php", PHP_SPLIT_APP);
    let port = server.port().to_string();
    let mut dev = start_php(
        &project,
        &host,
        &[
            ("DB_HOST", "127.0.0.1"),
            ("DB_PORT", &port),
            ("DB_USERNAME", "real-user"),
            ("DB_PASSWORD", &secret),
            ("DB_DATABASE", "shop"),
        ],
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/read");

    let answer = dev.0.get("/read");
    assert_200("the split-store read", &answer);
    // The slots a framework reads: the socket directory, the proxy's port, the
    // configured database — and no operator credential (json_encode escapes `/`).
    assert!(
        answer.body.contains("\"host\":\"\\/proc\\/self\\/cwd\""),
        "`DB_HOST` is the route's socket directory: {:?}\n{}",
        answer.body,
        dev.0.text()
    );
    assert!(
        answer.body.contains("\"database\":\"shop\""),
        "`DB_DATABASE` is the store's database: {:?}",
        answer.body
    );
    assert!(
        !answer.body.contains(&secret) && !answer.body.contains("real-user"),
        "the real credential reached the view: {:?}",
        answer.body
    );
    assert!(
        answer.body.contains("\"query_ok\":\"yes\"") && !answer.body.contains("\"error\""),
        "the granted statement ran through the rendered slots: {:?}\n{}",
        answer.body,
        dev.0.text()
    );
    // It ran on the upstream this fixture declared, so the slots were the route's.
    assert!(
        server
            .statements()
            .iter()
            .any(|statement| statement.contains("FROM products")),
        "the split-store statement reached the upstream: {:?}",
        server.statements()
    );
    println!(
        "php split store: DB_HOST=/proc/self/cwd, DB_DATABASE=shop, a scoped credential, and one \
         granted statement through the rendered slots"
    );
}
