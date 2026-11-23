//! `TESTING_PLAN.md` §2 (applications & runtimes, static/SPA), §4 (`blastit
//! dev`: live reload, `--learn`, loopback safety, clean shutdown) and §6
//! (`body raw` fail-closed) — driven through the real binary.
//!
//! One harness for the whole file: a temporary project, a port from `28080+`, a
//! boot whose whole stdout/stderr is collected, deadline-based waits. Every leg
//! is a real `blastit dev` on loopback; the database legs dial a real PostgreSQL
//! v3 peer (`tests/common/pg_upstream.rs`). Nothing is `#[ignore]`d: a host with
//! no NTS `libphp` + shim, or no live-reload watch (Docker `overlayfs` answers
//! `EPERM`), skips its legs **loudly** with one `SKIP …` line.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every leg below needs at least one view runtime; a build with none has no
// test that reaches this harness, so the file is an empty suite there instead
// of a dead-code shell that `--no-default-features --all-targets` refuses.
#![cfg(any(feature = "python", feature = "php", feature = "static"))]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(any(feature = "python", feature = "php"))]
#[path = "common/pg_upstream.rs"]
mod pg_upstream;

/// The host's NTS `libphp` + shim, resolved without `blastit`.
#[cfg(feature = "php")]
#[path = "common/php_host.rs"]
mod php_host;

/// The HTTP client, request burst and calibration both concurrency legs read.
#[path = "common/views.rs"]
mod views;

use views::{Client, Response};

/// The static/SPA fixture and the two-backend seam the §2 static legs share.
#[cfg(feature = "static")]
#[path = "common/static_assets.rs"]
mod static_assets;

/// The temporary project root every fixture lives in.
#[path = "common/project.rs"]
mod project;

/// The `/proc` probes the shutdown leg reads.
#[path = "common/proc.rs"]
mod proc;

/// The two-view live-reload project and its app.
#[cfg(feature = "python")]
#[path = "common/reload.rs"]
mod reload;

use project::Project;
#[cfg(feature = "python")]
use reload::{RELOAD_APP, RELOAD_MANIFEST};

#[cfg(feature = "static")]
use static_assets::{MOUNTS_MANIFEST, SPA_INDEX, SPA_MANIFEST, write_mounts, write_spa};

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

/// How long a boot may take to leave after `SIGTERM`/`SIGINT`.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// What the port and the exit may take past [`STOP_TIMEOUT`].
const STOP_SLACK: Duration = Duration::from_secs(10);

/// The default command line: no live reload, so a case that is not about the
/// watcher reads the configuration it booted with.
const NO_RELOAD: &[&str] = &["--no-reload"];

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

/// `target/py-sapi`, where `make py-sapi` writes the per-minor CPython shims.
fn python_shim_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/py-sapi");
    dir.is_dir().then_some(dir)
}

/// The documented `sql.denied` line for `route`, in either log format.
fn denial_line<'a>(log: &'a str, needle: &str, route: &str) -> Option<&'a str> {
    let aligned = format!("  {route}  ");
    let json = format!("\"route\":\"{route}\"");
    log.lines().find(|line| {
        line.contains("sql.denied")
            && line.contains(needle)
            && (line.contains(&aligned) || line.contains(&json))
    })
}

/// Waits until the denial for `needle` on `route` is on stdout.
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
/// variables the embedded CPython needs, exactly as `make check` sets them.
///
/// `env_clear` is what a developer's shell is not, so nothing ambient can make a
/// probe and the boot disagree. `BLASTIT_MEM_HIGH_WATERMARK=100` keeps a loaded
/// host from turning a story about WSGI into the documented memory refusal.
fn dev_command(dir: &Path, port: u16, extra: &[(&str, &str)], flags: &[&str]) -> Command {
    let mut command = Command::new(BINARY);
    command
        .arg("dev")
        .args(flags)
        .arg("--port")
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

/// One boot at a time across this file: a wall-clock leg is only honest when
/// nothing else of this binary is forking.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `blastit dev` to an end and returns what it said.
fn run_dev(project: &Project, extra: &[(&str, &str)]) -> Outcome {
    run_dev_with(project, extra, NO_RELOAD)
}

/// [`run_dev`] with `flags` on the command line.
fn run_dev_with(project: &Project, extra: &[(&str, &str)], flags: &[&str]) -> Outcome {
    let _serial = boot_lock();
    let mut attempt = 1;
    loop {
        let port = free_port();
        let mut child = dev_command(&project.dir, port, extra, flags)
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
        // serve and another process held the port: retried with a fresh port.
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
    Ready,
    Waiting,
    LostThePort,
    Exited(ExitStatus),
}

/// One parsed response with its head (`ETag`, `Cache-Control`, `Content-Length`).
struct Raw {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Raw {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim())
        })
    }
}

/// One `GET`/`HEAD` with `headers`, or `None` while the boot does not answer.
fn raw_request(port: u16, method: &str, target: &str, headers: &[(&str, &str)]) -> Option<Raw> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(views::REQUEST_TIMEOUT)).ok()?;
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(Raw {
        status,
        head,
        body: raw[split + 4..].to_vec(),
    })
}

/// A running `blastit dev`, with both streams collected line by line.
struct Dev {
    dir: PathBuf,
    extra: Vec<(String, String)>,
    flags: Vec<String>,
    /// The boot's own process (it `execve`s into stage 1 in place, so the pid is
    /// stable).
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// Released by [`Dev::stop`], so the next boot in this file may start.
    #[allow(dead_code)]
    serial: Option<std::sync::MutexGuard<'static, ()>>,
}

impl Dev {
    /// Starts a boot in `project` with `extra` on top of an empty environment.
    fn start(project: &Project, extra: &[(&str, &str)]) -> Self {
        Self::start_with(project, extra, NO_RELOAD)
    }

    /// [`Dev::start`] with `flags` on the command line (live reload, `--learn`).
    fn start_with(project: &Project, extra: &[(&str, &str)], flags: &[&str]) -> Self {
        Self::spawn(
            project.dir.clone(),
            extra
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            flags.iter().map(|flag| (*flag).to_owned()).collect(),
            free_port(),
            boot_lock(),
        )
    }

    /// One spawn at `port`: the process, both readers, and the one-boot lock.
    fn spawn(
        dir: PathBuf,
        extra: Vec<(String, String)>,
        flags: Vec<String>,
        port: u16,
        serial: std::sync::MutexGuard<'static, ()>,
    ) -> Self {
        let pairs: Vec<(&str, &str)> = extra
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let names: Vec<&str> = flags.iter().map(String::as_str).collect();
        let mut child = dev_command(&dir, port, &pairs, &names)
            .spawn()
            .expect("spawn `blastit dev`");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::with_capacity(2);
        let stdout_pipe = child.stdout.take().expect("the boot's stdout");
        let collected = Arc::clone(&stdout);
        readers.push(std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout_pipe).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("a stream buffer").push(line);
            }
        }));
        let stderr_pipe = child.stderr.take().expect("the boot's stderr");
        let collected = Arc::clone(&stderr);
        readers.push(std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr_pipe).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("a stream buffer").push(line);
            }
        }));
        Self {
            dir,
            extra,
            flags,
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
        let flags = std::mem::take(&mut self.flags);
        *self = Self::spawn(dir, extra, flags, port, serial);
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
    /// One raw request whose head must be read (`ETag`, `304`, `HEAD`).
    fn raw(&self, method: &str, target: &str, headers: &[(&str, &str)]) -> Raw {
        raw_request(self.port, method, target, headers).unwrap_or_else(|| {
            panic!(
                "no answer to {method} {target} on 127.0.0.1:{}:\n{}",
                self.port,
                self.text()
            )
        })
    }

    #[cfg(feature = "python")]
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

    /// Every stdout line parsed as an event.
    fn events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lock()
            .expect("the stdout buffer")
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// Every `name` event, in order.
    fn events_named(&self, name: &str) -> Vec<serde_json::Value> {
        self.events()
            .into_iter()
            .filter(|event| event["event"] == name)
            .collect()
    }

    /// Waits bounded for the first `name` event naming `needle` in its `files`,
    /// `path` or `message` field.
    fn wait_event_mentioning(&self, name: &str, needle: &str) -> serde_json::Value {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(event) = self.events_named(name).into_iter().find(|event| {
                event["files"].as_array().is_some_and(|files| {
                    files
                        .iter()
                        .any(|file| file.as_str().is_some_and(|file| file.contains(needle)))
                }) || event["path"].as_str() == Some(needle)
                    || event["message"]
                        .as_str()
                        .is_some_and(|message| message.contains(needle))
            }) {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "no `{name}` event naming {needle}:\n{}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
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

    /// The spawned pid set once init's event stream has stopped growing.
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

    /// The pids of the views this boot considers live, from its own events.
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

    /// The child's command line, from `/proc` (stage 1's, after the `execve`).
    fn cmdline(&self) -> String {
        proc::cmdline(self.child.id())
    }

    /// The `--dev-root` stage 1 was given.
    fn dev_root(&self) -> PathBuf {
        proc::dev_root(self.child.id())
    }

    /// Every process init forked, recursively, with its `starttime`.
    fn descendants(&self) -> Vec<(i32, u64)> {
        proc::descendants(self.child.id())
    }

    /// `signal` to the pid this test started — init, after the `execve`.
    fn signal(&self, signal: i32) {
        // SAFETY: the pid came from `spawn`, so it is this test's own child.
        let killed = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
    }

    /// Waits the readers to end of file (the process is gone).
    fn join_readers(&mut self) {
        for reader in std::mem::take(&mut self.readers) {
            let _ = reader.join();
        }
    }

    /// Waits bounded for the boot to exit and reaps it.
    fn wait_exit(&mut self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            if let Some(status) = self.child.try_wait().expect("the boot's state") {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `SIGTERM`, a bounded wait for exit `0`, then `SIGKILL`; returns the status.
    fn terminate(&mut self) -> Option<ExitStatus> {
        if self.child.try_wait().expect("the boot's state").is_none() {
            self.signal(libc::SIGTERM);
            if self.wait_exit(Instant::now() + STOP_TIMEOUT).is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        let status = self.child.wait().ok();
        self.join_readers();
        self.serial = None;
        status
    }

    /// Stops the boot, leaving a fresh one possible in this file.
    fn stop(&mut self) {
        let _ = self.terminate();
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

/// The app: a minimal PostgreSQL v3 client over the route's own dbproxy socket.
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
        elif path.endswith("/deny_column"):
            pg(DENIED_COLUMN, "7")
            payload = {"ok": True}
        elif path.endswith("/literal"):
            pg("SELECT 'alice' FROM products")
            payload = {"ok": True}
        else:
            pg(READ, "7")
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

/// One minor's leg of the multi-view grant story.
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

    // Zero credentials: the answer is the worker's whole environment.
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
            database.contains("host=") && !database.contains("real-user"),
            "route {route} was handed the local endpoint: {database}"
        );
        assert!(
            answer["env"]["BLASTIT_PYTHON"].is_null(),
            "a BLASTIT_* variable reached route {route}: {answer}"
        );
    }

    // The trait's grants answer for **both** views: neither declares one.
    for (method, path) in [
        ("GET", "/a/read"),
        ("GET", "/a/write"),
        ("POST", "/b/read"),
        ("POST", "/b/write"),
    ] {
        let response = dev.0.request(method, path);
        assert_eq!(
            response.status,
            200,
            "{method} {path} runs through the trait's grant: {}\n{}",
            response.body,
            dev.0.text()
        );
    }

    // An ungranted statement and an ungranted column are refused at runtime **in
    // each view**, and the request that asked for one is a server error.
    for (view, prefix, method) in [("read", "/a", "GET"), ("write", "/b", "POST")] {
        for (path, needle) in [
            ("deny_statement", "FROM orders WHERE id = $?"),
            ("deny_column", "cost FROM products WHERE id = $?"),
        ] {
            let response = dev.0.request(method, &format!("{prefix}/{path}"));
            assert_eq!(
                response.status,
                500,
                "{needle} in view {view} must fail the request: {}\n{}",
                response.body,
                dev.0.text()
            );
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

    // The refused view still serves its grant and the sibling is untouched.
    assert_eq!(dev.0.get("/a/read").status, 200, "{}", dev.0.text());
    assert_eq!(
        dev.0.request("POST", "/b/read").status,
        200,
        "{}",
        dev.0.text()
    );

    // The proxy really reached a database — granted statements forwarded, denied
    // ones never sent — and it authenticated with the operator's own user.
    let statements = server.statements();
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
}

// ---------------------------------------------------------------------------
// §2/§4: the host-interpreter rules, verbatim.
// ---------------------------------------------------------------------------

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

/// Writes an executable regular file: stage 0 reads names and versions, never
/// executes the interpreter it refuses.
#[cfg(feature = "python")]
fn executable(project: &Project, relative: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = project.write(relative, b"");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("an executable fixture");
    path
}

/// §2: an unusable interpreter is refused by its own sentence — an unsupported
/// minor, a free-threaded build, and every value of `BLASTIT_PYTHON` a shell can
/// spell (absolute and relative) are three boots, one contract.
#[cfg(feature = "python")]
#[test]
fn an_unusable_interpreter_is_refused_with_its_documented_sentence() {
    let project = Project::new("py-refusals");
    project.write("Blastfile", PARALLEL_MANIFEST);
    project.write("app.py", PARALLEL_APP);

    let old = executable(&project, "py312/bin/python3.12");
    let outcome = run_dev(
        &project,
        &[("BLASTIT_PYTHON", old.to_str().expect("a UTF-8 path"))],
    );
    outcome.refused_with("an unsupported minor must refuse the boot");
    for expected in [
        "blastit dev: CPython 3.12 at",
        "is not supported for WSGI views",
        "CPython 3.13, 3.14, 3.15 and 3.16 are supported",
        "run uv python install 3.16 && uv venv --python 3.16",
    ] {
        assert!(
            outcome.stderr.contains(expected),
            "the refusal names `{expected}`:\n{}",
            outcome.stderr
        );
    }

    let free = executable(&project, "pyt/bin/python3.14t");
    let outcome = run_dev(
        &project,
        &[("BLASTIT_PYTHON", free.to_str().expect("a UTF-8 path"))],
    );
    outcome.refused_with("a free-threaded interpreter must refuse the boot");
    for expected in [
        "CPython 3.14t at",
        "is free-threaded and cannot serve WSGI views",
        "uv python install cpython-3.16",
    ] {
        assert!(
            outcome.stderr.contains(expected),
            "the refusal names `{expected}`:\n{}",
            outcome.stderr
        );
    }

    let venv = project.path(".venv");
    let missing = project.path("missing/python3.14");
    for value in [missing.to_str().expect("a UTF-8 path"), "bin/python3"] {
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

/// §2/§4: two WSGI views of one project import the module the **project venv**
/// provides, and a view route really answers four requests at once.
#[cfg(feature = "python")]
const PARALLEL_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:app\"

view app regex '.*' {}
";

/// 200 ms in `time.sleep` (which releases the GIL), the last path segment back
/// as the request's own marker, and its own `t0`.
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

/// How far apart the app's own per-request `t0` stamps may be, in seconds.
#[cfg(feature = "python")]
const CONTENTION_SPAN: f64 = 0.100;

/// The marker and the request's own `t0` a WSGI answer carries.
#[cfg(feature = "python")]
fn wsgi_answer(response: &Response) -> Option<(&str, f64)> {
    let rest = response.body.split("wsgi:").nth(1)?;
    let mut fields = rest.split_whitespace().next()?.split(':');
    Some((fields.next()?, fields.next()?.parse().ok()?))
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
        let (marker, t0) = wsgi_answer(response)
            .unwrap_or_else(|| panic!("{what}: request {index} carried no app body"));
        assert_eq!(
            marker,
            format!("w{index}"),
            "{what}: request {index} answered for someone else: {:?}",
            response.body
        );
        starts.push(t0);
    }
    starts
}

/// §2/§4: four concurrent 200 ms requests to one WSGI route finish well under
/// the serialized control measured in the same function, one process, and the
/// app's own stamps prove they were in flight together.
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
         be at least {:?} faster (an absolute millisecond ceiling is deliberately not asserted)",
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
}

// ---------------------------------------------------------------------------
// §2/§6: embedded PHP — the PSR front controller, per-view grants, faults,
// `php2shell`, and the entry pin.
// ---------------------------------------------------------------------------

/// The host's NTS `libphp` + shim, or one loud `SKIP` line naming what a PHP
/// view needs.
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

/// Starts a PHP boot with the probe's own pair and the JSON event stream — the
#[cfg(feature = "php")]
fn start_php(
    project: &Project,
    host: &php_host::HostPhp,
    extra: &[(&str, &str)],
    flags: &[&str],
) -> Dev {
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
    Dev::start_with(project, &pairs, flags)
}

/// A PSR-style project: a front controller under `public/`, a static mount of
/// the same directory, and two views with **distinct** PostgreSQL rights.
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
/// the route's own dbproxy socket, with `$1` binds and never an inlined literal.
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

function pg($sql, ...$values) {
    [$host, $port] = pg_params();
    $sock = stream_socket_client('unix://' . $host . '/.s.PGSQL.' . $port, $errno, $errstr, 10);
    if ($sock === false) { throw new RuntimeException('pg: ' . $errstr); }
    try {
        $startup = pack('N', 196608) . "user\0blastit\0database\0app\0\0";
        fwrite($sock, pack('N', strlen($startup) + 4) . $startup);
        pg_until_ready($sock);
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

    let mut dev = start_php(&project, &host, &[("DATABASE_URL", &url)], NO_RELOAD);
    let dev = StopOnDrop(&mut dev);

    // The static mount answers from disk: waiting on the asset is also the
    // readiness wait, and a `libphp` request would be the bug it proves against.
    dev.0.wait_ready("/assets/app.css");
    let asset = dev.0.get("/assets/app.css");
    assert_200("the static mount", &asset);
    assert!(
        asset.body.contains("body{color:#0b0}"),
        "the mount answers the file's own bytes: {:?}\n{}",
        asset.body,
        dev.0.text()
    );
    for name in ["/index.php", "/index.phtml", "/index.phar"] {
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

    let read = dev.0.get("/read");
    assert_200("the public read", &read);
    let write = dev.0.request("POST", "/admin/write");
    assert_200("the admin write", &write);
    assert!(
        read.body.contains("\"ok\":true") && write.body.contains("\"ok\":true"),
        "the granted read and insert must succeed:\n{}",
        dev.0.text()
    );

    // Zero credentials (§2): the rendered URL reaches the worker, the real one
    // does not.
    let env = dev.0.get("/read/env");
    assert_200("the environment probe", &env);
    assert!(
        !env.body.contains(&secret) && !env.body.contains("real-user"),
        "a real credential reached the view: {:?}",
        env.body
    );

    // A compromised public view: four ungranted statements are refused at
    // runtime, each a `500` with the documented normalised line.
    for (path, needle) in [
        ("/read/orders", "FROM orders WHERE id = $?"),
        ("/read/columns", "cost FROM products WHERE id = $?"),
        ("/read/insert", "INSERT INTO orders"),
        ("/read/update", "UPDATE products"),
    ] {
        let refused = dev.0.get(path);
        assert_eq!(
            refused.status,
            500,
            "{needle} must fail the request: {:?}\n{}",
            refused.body,
            dev.0.text()
        );
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

    // The denied view keeps serving its own grant and the sibling is untouched.
    assert_200("the denied view", &dev.0.get("/read"));
    assert_200("the sibling view", &dev.0.request("POST", "/admin/write"));

    let statements = server.statements();
    assert!(
        statements
            .iter()
            .any(|statement| statement.contains("INSERT INTO products")),
        "the admin view's insert reached the upstream: {statements:?}\n{}",
        dev.0.text()
    );
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
    // `make rs-test-php-image` greps: a leg that skipped prints no `served:`.
    println!(
        "served: HTTP/1.1 200 OK (libphp {}) — the mount answered the asset and refused the \
         sources, `public` ran only its `$1` read, `admin` its insert, four denials were logged \
         normalised, and no credential reached a worker",
        host.libphp.display()
    );
}

/// The fault-isolation route: `prefork` warms the pool's one slot.
#[cfg(feature = "php")]
const PHP_FAULT_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    prefork
}
";

/// The fault page: `/crash-exit` and `/crash-fatal` bail out of their own
/// request. The busy loop is deliberate: `CLOCK_PROCESS_CPUTIME_ID` reports whole
/// ticks, and a script that returns immediately can measure `0`.
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
if (isset($_GET['m'])) { usleep(200000); }
echo 'blastit-php-ok php:' . ($_GET['m'] ?? 'none') . ':' . getmypid() . "\n";
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
    let mut dev = start_php(&project, &host, &[("BLASTIT_MAX_WORKERS", "1")], NO_RELOAD);
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
        let before = dev.0.live_view_pids();
        assert_eq!(before.len(), 1, "{what}: one live slot: {before:?}");
        let crashed_pid = before[0];
        let crashed = dev.0.get(path);
        assert!(
            crashed.status >= 500 && !crashed.body.contains("view_died"),
            "{what}: a contained bailout is a `5xx`, never `view_died`: {:?}\n{}",
            crashed.body,
            dev.0.text()
        );
        let event = dev.0.wait_event_mentioning("request", path);
        assert_eq!(
            event["status"], crashed.status,
            "{what}: the event agrees with the wire: {event}"
        );
        assert!(
            event["outcome"] == "bailed",
            "{what}: a handler that never returned is `bailed`: {event}"
        );
        let after = dev.0.get("/");
        assert_eq!(
            after.status,
            200,
            "{what}: the next request must be served: {:?}\n{}",
            after.body,
            dev.0.text()
        );
        let deadline = Instant::now() + BOOT_TIMEOUT;
        while proc::in_proc(crashed_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !proc::in_proc(crashed_pid),
            "{what}: the crashed worker {crashed_pid} is still in `/proc`:\n{}",
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
            "{what}: the slot must be fresh: {live:?}"
        );
        respawned.push((crashed_pid, live[0]));
    }

    let spawned = dev.0.settled_spawned("app").len();
    assert!(
        spawned >= 3,
        "`prefork` plus one worker per crash is at least three spawns, saw {spawned}:\n{}",
        dev.0.text()
    );
    for (retired, fresh) in &respawned {
        eprintln!("php faults: retired pid {retired} respawned pid {fresh}");
    }
}

/// The pid and marker a `PHP_FAULT_APP` answer carries (`php:<marker>:<pid>`),
/// asserted per request so a shared or swapped answer cannot pass.
#[cfg(feature = "php")]
fn php_pids(what: &str, responses: &[Response]) -> Vec<u32> {
    assert_eq!(
        responses.len(),
        views::WIDTH,
        "{what}: one response per request"
    );
    responses
        .iter()
        .enumerate()
        .map(|(index, response)| {
            assert_200(what, response);
            let rest = response
                .body
                .split("php:")
                .nth(1)
                .unwrap_or_else(|| panic!("{what}: request {index} carried no pid"));
            let (marker, rest) = rest.split_once(':').expect("a marker and a pid");
            assert_eq!(
                marker,
                format!("p{index}"),
                "{what}: request {index} answered for someone else"
            );
            rest.split_whitespace()
                .next()
                .and_then(|pid| pid.parse().ok())
                .unwrap_or_else(|| panic!("{what}: no pid in {:?}", response.body))
        })
        .collect()
}

/// §6 sizing: `BLASTIT_MAX_VIEW_PROCESSES` really bounds one view's live
/// processes — four concurrent 200 ms requests under a cap of two run on exactly
/// two processes, in two waves, and `init` forked no third worker.
#[cfg(feature = "php")]
#[test]
fn the_view_process_cap_bounds_one_views_pool() {
    let Some(host) = host_php_or_skip("PHP view-process cap") else {
        return;
    };
    let project = Project::new("php-view-cap");
    project.write("Blastfile", PHP_FAULT_MANIFEST);
    project.write("public/index.php", PHP_FAULT_APP);
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
        NO_RELOAD,
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_event("ready");

    let (wall, bodies) = burst(dev.0.client(), &paths);
    let pids = php_pids("php cap", &bodies);
    let mut workers = pids.clone();
    workers.sort_unstable();
    workers.dedup();
    assert_eq!(
        workers.len(),
        2,
        "BLASTIT_MAX_VIEW_PROCESSES=2 must bound the pool to two processes, saw {workers:?}"
    );
    assert_eq!(
        dev.0.settled_spawned("app"),
        workers,
        "init forked exactly the two workers that answered, no more"
    );
    // The fixture's own sleep makes the cap visible in time as well as in pids:
    // two processes cannot answer four 200 ms requests in one wave.
    assert!(
        wall >= Duration::from_millis(400),
        "four 200 ms requests over a two-process cap took {wall:?}, inside one wave"
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
/// `allow_url_include` is absent: PHP 8.0 removed the setting.
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
    // The canary a prepend would run: the payload names this **existing** file.
    project.write("public/prelude.php", "<?php echo \"PREPENDED-RAN\\n\";\n");
    let prelude = project.path("public/prelude.php").display().to_string();
    let mut dev = start_php(&project, &host, &[], NO_RELOAD);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");

    // The CVE-2024-4577 shape (`%AD` is a soft hyphen on Windows code pages, the
    // byte that made a CGI query string into `-d` flags), a plain `-d` string,
    // and the proxy/header vectors. Both `-d` vectors name a file that exists, so
    // "it never ran" is a witness and not a tautology.
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
    // as an interpreter setting.
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
    project.write("Blastfile", PARALLEL_MANIFEST);
    project.write("app.py", PARALLEL_APP);
    let prefix = project.path("prefix");
    // A venv whose recorded home is a prefix with no `lib/libpython3.X.so`: the
    // refusal is stage 0's, before any view is forked.
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

/// §6: an entry script rewritten **in place** (the same inode, the same size:
/// `cp`, `tar -x`, a deploy script) moves `st_ctime`, so the boot's record is
/// gone — the route refuses with the documented sentence until a restart.
#[cfg(feature = "php")]
#[test]
fn a_php_entry_rewritten_in_place_is_refused_until_a_restart() {
    use std::os::unix::fs::MetadataExt;

    let Some(host) = host_php_or_skip("PHP entry pin") else {
        return;
    };
    let project = Project::new("php-entry-pin");
    project.write("Blastfile", PHP_FAULT_MANIFEST);
    let entry = project.write("public/index.php", PHP_FAULT_APP);
    let pinned = std::fs::metadata(&entry).expect("the entry's metadata");
    let mut dev = start_php(&project, &host, &[], NO_RELOAD);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/users/42");
    let served = dev.0.get("/users/42");
    assert_200("an untouched entry", &served);
    assert!(
        served.body.contains("blastit-php-ok"),
        "an untouched tree serves its own entry: {:?}",
        served.body
    );

    // The rewrite: the **same** inode, the same size, another script.
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
    // Cross the kernel's coarse timestamp tick before rewriting.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&entry, &decoy).expect("rewriting the entry in place");
    let rewritten = std::fs::metadata(&entry).expect("the rewritten entry's metadata");
    assert_eq!(
        (rewritten.dev(), rewritten.ino()),
        (pinned.dev(), pinned.ino()),
        "a write in place must keep the inode"
    );
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
        !refused.body.contains("IN-PLACE-DECOY-RAN") && !refused.body.contains("blastit-php-ok"),
        "the in-place decoy ran, or the refusal answered with the app's own body: {:?}",
        refused.body
    );
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        let logged = dev.0.text();
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

    // The fix the sentence names: put the page back (in place) and restart.
    std::fs::write(&entry, PHP_FAULT_APP).expect("restoring the page in place");
    dev.0.stop();
    let mut fresh = start_php(&project, &host, &[], NO_RELOAD);
    let fresh = StopOnDrop(&mut fresh);
    fresh.0.wait_ready("/users/42");
    assert_200("the restored page", &fresh.0.get("/users/42"));
}

// ---------------------------------------------------------------------------
// §2: a standalone SPA — files, ETag/304, SPA history, and the confinement that
// blocks traversal, symlinks, dotfiles and source leaks, on a host with neither
// Python nor PHP reachable.
// ---------------------------------------------------------------------------

/// §2: a standalone SPA serves its files with the documented cache and
/// conditional answers, a deep link from the shell, and every confined refusal.
#[cfg(feature = "static")]
#[test]
fn a_standalone_spa_serves_immutable_assets_and_confines_every_refusal() {
    let project = Project::new("spa");
    project.write("Blastfile", SPA_MANIFEST);
    write_spa(&project.dir);

    // No interpreter is reachable: `PATH` names a directory that does not exist.
    let mut dev = Dev::start(&project, &[("PATH", "/nonexistent")]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    let banner = dev.0.text();
    assert!(
        banner.contains(&format!("http://127.0.0.1:{}", dev.0.port)),
        "the banner names a clickable address: {banner}"
    );
    assert!(
        banner.contains("0 routes"),
        "a `static` mount is not a route: {banner}"
    );

    // The exact file: its bytes, the year-long immutable cache, and an `ETag`.
    let bundle = dev.0.raw("GET", "/assets/app.9f3c.js", &[]);
    assert_eq!(bundle.status, 200, "the bundle: {}", dev.0.text());
    assert_eq!(
        bundle.header("cache-control"),
        Some("public, max-age=31536000, immutable"),
        "a fingerprint earns the year: {:?}",
        bundle.head
    );
    let etag = bundle.header("etag").expect("a strong ETag").to_owned();
    assert!(
        etag.starts_with('"') && etag.ends_with('"') && etag.len() > 2,
        "the ETag is a quoted strong validator: {etag}"
    );

    // `HEAD` answers the `GET` head with no body, and `If-None-Match` is the
    // `304` with the validator repeated and no body.
    let cached = dev.0.raw(
        "GET",
        "/assets/app.9f3c.js",
        &[("If-None-Match", etag.as_str())],
    );
    assert_eq!(
        cached.status,
        304,
        "a matching ETag revalidates: {}",
        dev.0.text()
    );
    assert!(cached.body.is_empty(), "a `304` carries no body");
    assert_eq!(
        cached.header("etag"),
        Some(etag.as_str()),
        "the `304` repeats the validator: {:?}",
        cached.head
    );

    // `HEAD` wherever `GET` is (`docs/reference/blastfile.md`): the asset
    // answers the same `Content-Length` and `ETag` with no body, and the SPA
    // fallback answers a `HEAD` too.
    let head = dev.0.raw("HEAD", "/assets/app.9f3c.js", &[]);
    assert_eq!(head.status, 200, "HEAD on the bundle: {}", dev.0.text());
    assert!(head.body.is_empty(), "HEAD carries no body");
    assert_eq!(
        head.header("content-length"),
        bundle.header("content-length"),
        "HEAD promises the GET's length"
    );
    assert_eq!(
        head.header("etag"),
        Some(etag.as_str()),
        "HEAD repeats the ETag"
    );
    let head_link = dev.0.raw("HEAD", "/dashboard/settings", &[]);
    assert_eq!(head_link.status, 200, "HEAD on the shell: {}", dev.0.text());
    assert!(head_link.body.is_empty(), "a HEAD fallback carries no body");

    // SPA history: a deep link nothing names answers the shell, and the shell is
    // `no-cache` — a hashed URL is never matched against the cache rule.
    for deep in ["/dashboard/settings", "/orders/42"] {
        let answer = dev.0.raw("GET", deep, &[]);
        assert_eq!(answer.status, 200, "deep link {deep}: {}", dev.0.text());
        assert!(
            String::from_utf8_lossy(&answer.body).contains("blastit-spa-shell"),
            "deep link {deep} answers the SPA shell: {:?}",
            String::from_utf8_lossy(&answer.body)
        );
        assert_eq!(
            answer.header("cache-control"),
            Some("no-cache"),
            "the fallback shell revalidates: {:?}",
            answer.head
        );
    }
    let root = dev.0.raw("GET", "/", &[]);
    assert_eq!(root.status, 200, "/: {}", dev.0.text());
    assert_eq!(
        root.header("cache-control"),
        Some("no-cache"),
        "a directory index revalidates: {:?}",
        root.head
    );

    // Only an exact file the pattern matches earns the year: the deeper chunk
    // does (`"/assets/*"` crosses `/`), the original does not.
    let chunk = dev.0.raw("GET", "/assets/chunks/vendor.abc.js", &[]);
    assert_eq!(
        chunk.header("cache-control"),
        Some("public, max-age=31536000, immutable"),
        "a quoted pattern crosses `/`: {:?}",
        chunk.head
    );
    let report = dev.0.raw("GET", "/report.txt", &[]);
    assert_eq!(report.status, 200, "the original: {}", dev.0.text());
    assert!(
        !report
            .header("cache-control")
            .unwrap_or_default()
            .contains("immutable"),
        "a file the pattern does not match is not year-cached: {:?}",
        report.head
    );

    // A mount's `fallback` answers only `GET`/`HEAD`: another method is refused.
    let refused_method = dev.0.raw("POST", "/dashboard/settings", &[]);
    assert!(
        matches!(refused_method.status, 404 | 405),
        "a POST to a fallback path is refused: {}",
        dev.0.text()
    );

    // The `.well-known` exemption is one segment deep: an ACME token is served,
    // `.well-known/.env` is a dotfile like any other.
    let token = dev
        .0
        .raw("GET", "/.well-known/acme-challenge/token-42", &[]);
    assert_eq!(token.status, 200, "an ACME token: {}", dev.0.text());
    let hidden_env = dev.0.raw("GET", "/.well-known/.env", &[]);
    assert_eq!(
        hidden_env.status,
        404,
        "`.well-known/.env` is still a dotfile: {}",
        dev.0.text()
    );
    let published = dev.0.raw("GET", "/.well-known/security.txt", &[]);
    assert_eq!(
        published.status,
        200,
        "`.well-known` is the one dotfile a mount publishes: {}",
        dev.0.text()
    );
    assert!(
        String::from_utf8_lossy(&published.body).contains("ops@example.test"),
        "the published document is its own bytes"
    );

    // Every refusal is final with its **documented** status; no body may carry a
    // planted canary.
    let hostname = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
    let canaries = [
        "postgres://leak",
        "well-known-leak",
        "source-leak",
        hostname.trim(),
    ];
    for (target, status, what) in [
        ("/../etc/passwd", 400, "a `..` segment"),
        ("/.env", 404, "a dotfile"),
        ("/redirect.js", 404, "a symlink out of the root"),
        ("/console.php", 404, "a `.php` source"),
    ] {
        let refused = dev.0.raw("GET", target, &[]);
        let body = String::from_utf8_lossy(&refused.body).into_owned();
        assert_eq!(
            refused.status,
            status,
            "{what} must be `{status}`, not `{}`: {body}\n{}",
            refused.status,
            dev.0.text()
        );
        assert!(
            !body.contains("blastit-spa-shell"),
            "{what} answered with the SPA fallback"
        );
        for canary in canaries.iter().filter(|canary| !canary.is_empty()) {
            assert!(!body.contains(canary), "{what} leaked `{canary}`: {body}");
        }
    }
}

/// §2/§6: both documented static backends — `openat2` and the per-segment `Walk`
/// — serve the same bytes and make the same refusals, through the
/// `blastit-view-static::StaticRoot::open_with` seam.
#[cfg(feature = "static")]
#[test]
fn both_static_backends_serve_the_same_bytes_and_the_same_refusals() {
    let project = Project::new("static-backends");
    write_spa(&project.dir);
    static_assets::assert_parity(&project.dir);
}

// ---------------------------------------------------------------------------
// §4: live reload — a `.py` save, a `Blastfile` save that does not parse, the
// interpreter's own preflight sentence, and a `.php` save.
// ---------------------------------------------------------------------------

/// `true` (and one `SKIP` line) when the boot could not watch for saves — a
/// Docker `overlayfs` answers `EPERM` — so a reload leg never grades the host.
fn reload_unavailable(dev: &Dev, what: &str) -> bool {
    let unavailable = dev
        .events()
        .iter()
        .any(|event| event["event"] == "dev.reload.unavailable");
    if unavailable {
        eprintln!(
            "SKIP {what}: this host cannot watch for saves (`dev.reload.unavailable`; Docker's \
             overlayfs answers EPERM), so no live reload runs here"
        );
    }
    unavailable
}

/// §4: a `Blastfile` or preloaded module that does not parse prints the
/// interpreter's own sentence and keeps the last valid configuration serving; a
/// valid save then reloads once and is served — and a request already in flight
/// when the save lands is answered instead of dropped.
#[cfg(feature = "python")]
#[test]
fn a_broken_save_keeps_serving_and_a_good_one_reloads_without_dropping_a_request() {
    let project = Project::new("reload");
    project.write("Blastfile", RELOAD_MANIFEST);
    project.write("app.py", RELOAD_APP);
    project.write("models.py", "VERSION = 1\n");
    let mut dev = Dev::start_with(&project, &[("BLASTIT_LOG_FORMAT", "json")], &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    if reload_unavailable(dev.0, "live reload of a broken save") {
        return;
    }
    assert!(
        dev.0.get("/").body.contains("reload:1"),
        "the boot serves the preloaded module:\n{}",
        dev.0.text()
    );

    // A half-written module: the parent compiles it (never imports it), warns
    // with the file's own sentence, and retires nothing.
    std::fs::write(project.path("models.py"), "VERSION = (\n").expect("the broken module");
    let warning = dev.0.wait_event_mentioning("manifest.warning", "models.py");
    let message = warning["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("SyntaxError") && message.contains("models.py"),
        "the interpreter's own preflight sentence names the file and the error: {message}"
    );
    assert!(
        dev.0.events_named("dev.reload").is_empty(),
        "a broken module reloads nothing:\n{}",
        dev.0.text()
    );
    assert!(
        dev.0.get("/").body.contains("reload:1"),
        "the last valid configuration keeps serving:\n{}",
        dev.0.text()
    );

    // A `Blastfile` that does not parse: one warning naming the file and its
    // position, and the topology keeps answering.
    std::fs::write(
        project.path("Blastfile"),
        "app \"app\"\nentrypoint python \"app:app\"\n\nview app regex '.*' {\n",
    )
    .expect("the invalid manifest");
    let warning = dev.0.wait_event_mentioning("manifest.warning", "Blastfile");
    let message = warning["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Blastfile") && message.contains(':'),
        "one sentence naming the file and its position: {message}"
    );
    assert!(
        dev.0.events_named("dev.reload").is_empty(),
        "an invalid manifest must not reload anything"
    );
    assert!(
        dev.0.get("/").body.contains("reload:1"),
        "the previous topology keeps serving the invalid save:\n{}",
        dev.0.text()
    );

    // The in-flight request: it sleeps inside the app, so the save below lands
    // while it is being served.
    let client = dev.0.client();
    let slow = std::thread::spawn(move || client.get("/slow"));
    std::thread::sleep(Duration::from_millis(150));
    std::fs::write(project.path("Blastfile"), RELOAD_MANIFEST).expect("the valid manifest");
    std::fs::write(project.path("models.py"), "VERSION = 2\n").expect("the fixed module");
    let reload = dev.0.wait_event_mentioning("dev.reload", "models.py");
    let routes: Vec<&str> = reload["routes"]
        .as_array()
        .expect("a routes list")
        .iter()
        .filter_map(|route| route.as_str())
        .collect();
    assert_eq!(
        routes,
        ["app", "other"],
        "the reload names every view, never a subset: {reload}"
    );
    assert_eq!(reload["reason"], "blastfile", "{reload}");
    let files: Vec<&str> = reload["files"]
        .as_array()
        .expect("a files list")
        .iter()
        .filter_map(|file| file.as_str())
        .collect();
    assert!(
        !files.is_empty() && files.iter().all(|file| file.starts_with('/')),
        "every changed path is absolute: {files:?}"
    );
    let answered = slow.join().expect("the in-flight request thread");
    assert_eq!(
        answered.status,
        200,
        "a save must not drop a request already in flight: {:?}\n{}",
        answered.body,
        dev.0.text()
    );
    // The cycle stops the scheduler and re-execs the image, so a request during
    // it may be accepted and not answered: poll with `try_request`.
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        if dev
            .0
            .try_request("GET", "/")
            .is_some_and(|response| response.body.contains("reload:2"))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the replacement image never served the new module:\n{}",
            dev.0.text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    // A module save alone is the `loaded` reason, and one save is one reload.
    std::fs::write(project.path("models.py"), "VERSION = 3\n").expect("the module save");
    let deadline = Instant::now() + BOOT_TIMEOUT;
    while dev.0.events_named("dev.reload").len() < 2 {
        assert!(
            Instant::now() < deadline,
            "the module save never reloaded:\n{}",
            dev.0.text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let reloads = dev.0.events_named("dev.reload");
    assert_eq!(reloads.len(), 2, "one save is exactly one reload");
    assert_eq!(reloads[1]["reason"], "loaded", "{}", reloads[1]);
}

/// §4: editing a `.php` file the view had already loaded reloads the route and
/// the next request serves the new source.
#[cfg(feature = "php")]
#[test]
fn a_php_save_reloads_and_the_next_request_serves_the_new_source() {
    let Some(host) = host_php_or_skip("PHP live reload") else {
        return;
    };
    let project = Project::new("reload-php");
    project.write(
        "Blastfile",
        "entrypoint php \"public/index.php\"\n\nview app regex '.*' {}\n",
    );
    project.write(
        "public/index.php",
        "<?php require __DIR__ . '/lib.php'; echo \"php-reload:\" . MARKER . \"\\n\";\n",
    );
    project.write("public/lib.php", "<?php const MARKER = 1;\n");
    let mut dev = start_php(&project, &host, &[], &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    if reload_unavailable(dev.0, "live reload of a `.php` save") {
        return;
    }
    assert!(
        dev.0.get("/").body.contains("php-reload:1"),
        "the boot serves the included file:\n{}",
        dev.0.text()
    );

    std::fs::write(project.path("public/lib.php"), "<?php const MARKER = 2;\n").expect("the save");
    dev.0.wait_event_mentioning("dev.reload", "lib.php");
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        if dev.0.get("/").body.contains("php-reload:2") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the reloaded route never served the new source:\n{}",
            dev.0.text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    println!("reload: a `.php` save reloaded the route and the new source is served");
}

// ---------------------------------------------------------------------------
// §4: `blastit dev --learn` and loopback safety.
// ---------------------------------------------------------------------------

/// The one-pass project: a routed view with no grant, and an unrouted path that
/// `--learn` has to propose.
#[cfg(feature = "python")]
const LEARN_MANIFEST: &str = "\
# The shop front end: one route, and `--learn` fills in the rest.
app \"app\"
entrypoint python \"app:application\"

postgres $DATABASE_URL

view shop GET \"/shop/*\" {}
";

/// A started `--learn` boot, ready to be exercised.
#[cfg(feature = "python")]
fn learn_dev(project: &Project, url: &str) -> Dev {
    let interpreter = python_for("3.14");
    Dev::start_with(
        project,
        &[
            ("DATABASE_URL", url),
            (
                "BLASTIT_PYTHON",
                interpreter.to_str().expect("a UTF-8 interpreter path"),
            ),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        &["--learn"],
    )
}

/// §4: one `--learn` pass records the missing view and the parameterized grant,
/// preserves the author's comment, prints the exact `learn:` line, and never
/// proposes a view for a vulnerability scanner's probe.
#[cfg(feature = "python")]
#[test]
fn a_learn_pass_records_views_and_parameterized_grants_and_ignores_probes() {
    use pg_upstream::PgUpstream;

    let server = PgUpstream::start();
    let url = format!(
        "postgresql://real-user:s3cr3t@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new("learn");
    let manifest = project.write("Blastfile", LEARN_MANIFEST);
    project.write("app.py", GRANT_APP);
    let mut dev = learn_dev(&project, &url);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/shop/read");

    // A routed request whose statement the file never granted, and an unrouted
    // path `--learn` has to turn into a view.
    assert_eq!(dev.0.json_request("GET", "/shop/read")["ok"], true);
    assert_eq!(dev.0.json_request("GET", "/cart/7")["ok"], true);
    // The scan: a dotfile, a VCS path and a traversal are never proposals.
    for probe in ["/.env", "/.git/config", "/../../etc/passwd"] {
        let _ = dev.0.try_request("GET", probe);
    }
    // The learn tally lives in init, so wait for the request event first.
    dev.0.wait_event_mentioning("request", "/cart/7");

    let status = dev.0.terminate().expect("the learn run exits");
    assert_eq!(
        status.code(),
        Some(0),
        "a clean learn pass exits 0:\n{}",
        dev.0.text()
    );
    let learned = std::fs::read_to_string(&manifest).expect("the learned manifest");
    for expected in [
        "view cart ",
        "\"/cart/{id:int}\"",
        "SELECT id, name FROM products WHERE id = $?",
    ] {
        assert!(
            learned.contains(expected),
            "the learned file is missing `{expected}`:\n{learned}"
        );
    }
    assert!(
        learned.contains("# The shop front end: one route"),
        "the author's comment is preserved:\n{learned}"
    );
    for forbidden in [".env", ".git", "etc/passwd", "= 7", "'7'", "s3cr3t"] {
        assert!(
            !learned.contains(forbidden),
            "`{forbidden}` must never reach the file:\n{learned}"
        );
    }
    // The exact human line, with the absolute path and both counts in their own
    // number (`docs/local.md`).
    let absolute = std::fs::canonicalize(&manifest).expect("the manifest's absolute path");
    let expected = format!("learn: 1 route, 1 query added → {}", absolute.display());
    assert!(
        dev.0.text().contains(&expected),
        "the summary line is `{expected}`:\n{}",
        dev.0.text()
    );
}

/// §4: a statement that inlines a literal is refused instead of written — the app
/// still answers, and neither the manifest nor the log ever carries the literal.
#[cfg(feature = "python")]
#[test]
fn learn_refuses_a_literal_and_never_writes_it() {
    use pg_upstream::PgUpstream;

    let server = PgUpstream::start();
    let url = format!(
        "postgresql://real-user:s3cr3t@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new("learn-literal");
    let manifest = project.write(
        "Blastfile",
        "app \"app\"\nentrypoint python \"app:application\"\n\npostgres $DATABASE_URL\n\n\
         view literal GET \"/literal\" {}\n",
    );
    project.write("app.py", GRANT_APP);
    let mut dev = learn_dev(&project, &url);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/literal");
    assert_eq!(
        dev.0.json_request("GET", "/literal")["ok"],
        true,
        "the literal statement ran under --learn:\n{}",
        dev.0.text()
    );
    dev.0.wait_event_mentioning("request", "/literal");
    let status = dev.0.terminate().expect("the learn run exits");
    assert_eq!(
        status.code(),
        Some(1),
        "a refused statement is exit 1:\n{}",
        dev.0.text()
    );
    let stderr = dev.0.stderr.lock().expect("the stderr buffer").join("\n");
    assert!(
        stderr.contains("learn: refused"),
        "the refusal is reported: {stderr}"
    );
    assert!(
        !stderr.contains("alice"),
        "a refused literal never reaches a stream: {stderr}"
    );
    let learned = std::fs::read_to_string(&manifest).expect("the manifest is readable");
    assert!(
        !learned.contains("alice") && !learned.contains("SELECT 'alice'"),
        "a literal-bearing statement is never granted:\n{learned}"
    );
}

/// The app the two `--learn` attribution legs serve: each path runs one fixed
/// statement over the route's own socket, and the unrouted paths run none (or
/// the fallback view's `SELECT 1`), so attribution can be read back per route.
#[cfg(feature = "python")]
const LEARN_ATTRIBUTION_APP: &str = r#"
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


def pg(sql):
    sock = _session()
    try:
        body = sql.encode() + b"\0"
        sock.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
        _until_ready(sock)
    finally:
        sock.close()


def application(environ, start_response):
    path = environ.get("PATH_INFO", "/")
    try:
        if path == "/a/1":
            pg("SELECT 111")
        elif path == "/b":
            pg("SELECT 222")
        elif path == "/a/2":
            pg("SELECT 333")
        elif path.startswith("/extra/"):
            pg("SELECT 1")
        payload = {"ok": True, "pid": os.getpid()}
        status = "200 OK"
    except Exception as err:  # noqa: BLE001 - the body is the diagnosis
        payload = {"error": str(err)}
        status = "500 Internal Server Error"
    body = json.dumps(payload).encode()
    start_response(status, [("Content-Type", "application/json"),
                            ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The two routes both attribution legs declare, and nothing else.
#[cfg(feature = "python")]
const LEARN_ATTRIBUTION_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:application\"

postgres $DATABASE_URL

view a regex '/a/.*' {}

view b \"/b\" {}
";

/// §4/§5: two declared routes run under one `--learn` pass, and every statement
/// is attributed to the route whose own view executed it — route `a` keeps both
/// of its statements after route `b`'s view has spawned, and never receives
/// `b`'s.
#[cfg(feature = "python")]
#[test]
fn learn_attributes_queries_per_route_across_concurrent_live_views() {
    use blastit_manifest::{Grant, Manifest, ProxyName};
    use pg_upstream::PgUpstream;

    let server = PgUpstream::start();
    let url = format!(
        "postgresql://real-user:s3cr3t@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new("learn-multi-view");
    let manifest = project.write("Blastfile", LEARN_ATTRIBUTION_MANIFEST);
    project.write("app.py", LEARN_ATTRIBUTION_APP);
    let mut dev = learn_dev(&project, &url);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/a/1");

    // Route `a` spawns its view and runs `SELECT 111`.
    let a1 = dev.0.json_request("GET", "/a/1");
    assert_eq!(a1["ok"], true, "{a1}\n{}", dev.0.text());
    let pid_a = a1["pid"].as_i64().expect("route a view pid");
    // Route `b` spawns a second view while `a`'s stays live.
    let b = dev.0.json_request("GET", "/b");
    assert_eq!(b["ok"], true, "{b}\n{}", dev.0.text());
    let pid_b = b["pid"].as_i64().expect("route b view pid");
    assert_ne!(pid_a, pid_b, "each route runs in its own view process");
    for (name, pid) in [("a", pid_a), ("b", pid_b)] {
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "route {name}'s view ({pid}) must still be live"
        );
    }
    // Route `a` runs `SELECT 333` on its still-live view: attribution comes from
    // `a`'s per-route listener and never drifts onto `b`.
    let a2 = dev.0.json_request("GET", "/a/2");
    assert_eq!(a2["ok"], true, "{a2}\n{}", dev.0.text());
    assert_eq!(
        a2["pid"].as_i64(),
        Some(pid_a),
        "the warm view of route a served /a/2 while route b was live"
    );
    dev.0.wait_event_mentioning("request", "/a/2");

    let status = dev.0.terminate().expect("the learn run exits");
    assert_eq!(
        status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        dev.0.text(),
        dev.0.stderr.lock().expect("the stderr buffer").join("\n")
    );

    let learned = std::fs::read_to_string(&manifest).expect("the manifest is readable");
    let (parsed, _) =
        Manifest::parse_blastfile(&learned, 20_000).expect("the learned manifest parses");
    let proxy = ProxyName::new("db");
    let queries = |index: usize| -> Vec<String> {
        parsed
            .effective_grants(index)
            .get(&proxy)
            .and_then(Grant::as_postgresql)
            .unwrap_or_else(|| panic!("route {index} received no postgresql grant:\n{learned}"))
            .queries()
            .to_vec()
    };
    assert_eq!(
        queries(0),
        vec!["SELECT 111", "SELECT 333"],
        "route a keeps both of its statements after route b spawned:\n{learned}"
    );
    assert_eq!(
        queries(1),
        vec!["SELECT 222"],
        "route b never receives route a's statements:\n{learned}"
    );
    assert!(
        dev.0.text().contains("learn: 0 routes, 3 queries added"),
        "the summary counted all three queries:\n{}",
        dev.0.text()
    );
}

/// §4/§5: the first declared route keeps **all** its statements when unrouted
/// requests exist; the fallback view's `SELECT 1` belongs to the inferred view,
/// never to a declared one.
#[cfg(feature = "python")]
#[test]
fn first_declared_route_keeps_all_queries_when_unrouted_requests_exist() {
    use blastit_manifest::{Grant, Manifest, ProxyName};
    use pg_upstream::PgUpstream;

    let server = PgUpstream::start();
    let url = format!(
        "postgresql://real-user:s3cr3t@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    );
    let project = Project::new("learn-first-route");
    let manifest = project.write("Blastfile", LEARN_ATTRIBUTION_MANIFEST);
    project.write("app.py", LEARN_ATTRIBUTION_APP);
    let mut dev = learn_dev(&project, &url);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/a/1");

    // a → 111, unrouted → no SQL, b → 222, a again → 333, unrouted fallback → 1.
    for path in ["/a/1", "/zzz/9", "/b", "/a/2", "/extra/7"] {
        let answer = dev.0.json_request("GET", path);
        assert_eq!(answer["ok"], true, "{path}: {answer}\n{}", dev.0.text());
    }
    for path in ["/zzz/9", "/extra/7", "/a/2"] {
        dev.0.wait_event_mentioning("request", path);
    }

    let status = dev.0.terminate().expect("the learn run exits");
    assert_eq!(
        status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        dev.0.text(),
        dev.0.stderr.lock().expect("the stderr buffer").join("\n")
    );

    let learned = std::fs::read_to_string(&manifest).expect("the manifest is readable");
    let (parsed, _) =
        Manifest::parse_blastfile(&learned, 20_000).expect("the learned manifest parses");
    let route_names: Vec<&str> = parsed.routes().iter().map(|route| route.name()).collect();
    assert_eq!(
        route_names,
        vec!["a", "b", "extra", "zzz"],
        "only the truly-unrouted requests contribute inferred routes:\n{learned}"
    );
    let proxy = ProxyName::new("db");
    let queries = |index: usize| -> Vec<String> {
        parsed
            .effective_grants(index)
            .get(&proxy)
            .and_then(Grant::as_postgresql)
            .unwrap_or_else(|| panic!("route {index} received no postgresql grant:\n{learned}"))
            .queries()
            .to_vec()
    };
    assert_eq!(
        queries(0),
        vec!["SELECT 111", "SELECT 333"],
        "the first declared route keeps all of its statements, and none of the fallback's:\n{learned}"
    );
    assert_eq!(
        queries(1),
        vec!["SELECT 222"],
        "route b keeps its exact statement:\n{learned}"
    );
    assert!(
        dev.0.text().contains("learn: 2 routes, 3 queries added"),
        "the summary reports two inferred routes and all three routed queries:\n{}",
        dev.0.text()
    );
}

/// §4: loopback by default — a non-loopback `--host` is refused with the variable
/// to set, and with it the boot serves and says it is exposed.
#[cfg(feature = "static")]
#[test]
fn a_non_loopback_host_needs_the_exposure_acknowledgement() {
    let project = Project::new("expose");
    project.write("Blastfile", SPA_MANIFEST);
    project.write("site/index.html", SPA_INDEX);

    let refused = run_dev_with(&project, &[], &["--no-reload", "--host", "0.0.0.0"]);
    refused.refused_with("a non-loopback host must be refused");
    for expected in [
        "`--host 0.0.0.0` exposes `blastit dev` beyond loopback",
        "The simulation has no authentication",
        "Set BLASTIT_DEV_EXPOSE=1 if that is what you want",
    ] {
        assert!(
            refused.stderr.contains(expected),
            "the refusal names `{expected}`:\n{}",
            refused.stderr
        );
    }

    let mut dev = Dev::start_with(
        &project,
        &[("BLASTIT_DEV_EXPOSE", "1")],
        &["--no-reload", "--host", "0.0.0.0"],
    );
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    let text = dev.0.text();
    let uid = rustix::process::geteuid().as_raw();
    assert!(
        text.contains(&format!("0.0.0.0:{}", dev.0.port))
            && text.contains(&format!("uid {uid} (EXPOSED, no auth)"))
            && text.contains("there is no authentication")
            && text.contains("(BLASTIT_DEV_EXPOSE)"),
        "an exposed boot names the address, the operator's uid and the variable: {text}"
    );
}

// ---------------------------------------------------------------------------
// §2/§4: prefix mounts, a missing root, the banner, `--no-reload`, and
// `check --learn --dry-run`.
// ---------------------------------------------------------------------------

/// §2: a `static` mount serves under its prefix with the prefix stripped, and
/// the longest matching prefix answers (`docs/reference/blastfile.md`).
#[cfg(feature = "static")]
#[test]
fn a_prefix_mount_serves_under_its_prefix_and_the_longest_one_wins() {
    let project = Project::new("mounts");
    project.write("Blastfile", MOUNTS_MANIFEST);
    write_mounts(&project.dir);
    let mut dev = Dev::start(&project, &[("PATH", "/nonexistent")]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/static/app.js");
    let shallow = dev.0.get("/static/app.js");
    assert_200("the prefix mount", &shallow);
    assert!(shallow.body.contains("shallow mount"), "{:?}", shallow.body);
    let deep = dev.0.get("/static/deep/app.js");
    assert_200("the deeper mount", &deep);
    assert!(deep.body.contains("deeper mount"), "{:?}", deep.body);
}

/// §2: a missing static root stops the boot before a listener binds.
#[test]
fn a_missing_static_root_stops_the_boot_before_it_binds() {
    let project = Project::new("missing-root");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    let outcome = run_dev(&project, &[]);
    outcome.refused_with("a missing static root must refuse the boot");
    for expected in [
        "Blastfile",
        "mount \"/\" (index 0)",
        "static root \"public\"",
        "no such file or directory",
    ] {
        assert!(
            outcome.stderr.contains(expected),
            "the refusal names `{expected}`:\n{}",
            outcome.stderr
        );
    }
}

/// §4: the banner lists one row per view and per mount (`docs/local.md`).
#[cfg(all(feature = "python", feature = "static"))]
#[test]
fn the_banner_names_every_view_and_mount() {
    let project = Project::new("banner");
    project.write(
        "Blastfile",
        "app \"app\"\nentrypoint python \"app:app\"\n\nstatic \"/site\" {\n    root \"site\"\n}\n\n\
         view hello GET \"/hello/*\" {}\n",
    );
    project.write("app.py", PARALLEL_APP);
    project.write("site/index.html", SPA_INDEX);
    let mut dev = Dev::start(&project, &[]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/site/index.html");
    let text = dev.0.text();
    let row = |kind: &str| {
        text.lines()
            .find(|line| line.split_whitespace().next() == Some(kind))
            .map(|line| {
                line.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| panic!("no `{kind}` row in:\n{text}"))
    };
    // A mount's `fallback` answers `GET`/`HEAD` only: everything else is the
    // documented `405` naming both (`ADR-0007`).
    let refused_method = dev.0.raw("POST", "/hello/x", &[]);
    assert_eq!(refused_method.status, 405, "{}", dev.0.text());
    assert_eq!(
        refused_method.header("allow"),
        Some("GET, HEAD"),
        "the gate names the two methods it answers: {:?}",
        refused_method.head
    );
    assert_eq!(row("view"), ["view", "hello", "GET,", "HEAD", "/hello/.*"]);
    assert_eq!(row("static"), ["static", "/site", "GET,", "HEAD", "site"]);
}

/// §4: `--no-reload` watches nothing, so a save and a worker-cache write change
/// neither the served body nor the event stream (`docs/reference/cli.md`).
#[cfg(feature = "python")]
#[test]
fn a_no_reload_boot_watches_nothing_and_a_cache_write_never_reloads() {
    let project = Project::new("reload-off");
    project.write("Blastfile", RELOAD_MANIFEST);
    project.write("app.py", RELOAD_APP);
    project.write("models.py", "VERSION = 1\n");
    let mut dev = Dev::start(&project, &[("BLASTIT_LOG_FORMAT", "json")]);
    let dev = StopOnDrop(&mut dev);
    dev.0.wait_ready("/");
    assert!(
        !dev.0.text().contains("\"event\":\"reload\""),
        "`--no-reload` starts no watcher:\n{}",
        dev.0.text()
    );
    std::fs::write(project.path("models.py"), "VERSION = 2\n").expect("the save");
    std::fs::create_dir_all(project.path("__pycache__")).expect("a cache directory");
    std::fs::write(project.path("__pycache__/app.cpython-314.pyc"), b"cache")
        .expect("a cache write");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(
        dev.0.get("/").body.contains("reload:1"),
        "an unwatched save changes nothing:\n{}",
        dev.0.text()
    );
    assert!(dev.0.events_named("dev.reload").is_empty());
}

// ---------------------------------------------------------------------------
// §4/R20: clean shutdown — `SIGTERM` and `SIGINT`, exit `0`, port closed, no
// surviving descendant and the dev root removed.
// ---------------------------------------------------------------------------

/// §4: signalling a serving `blastit dev` reaps its whole tree and removes the
/// runtime root, for both interrupt signals.
///
/// The pid a caller starts **is** init after the in-place `execve`, so the test
/// signals the pid it spawned and looks for survivors by identity
/// (pid + `starttime`), never by a command-line pattern.
#[cfg(feature = "static")]
#[test]
fn sigterm_and_sigint_reap_the_tree_close_the_port_and_remove_the_root() {
    let project = Project::new("shutdown");
    project.write("Blastfile", SPA_MANIFEST);
    write_spa(&project.dir);

    for signal in [libc::SIGTERM, libc::SIGINT] {
        let mut dev = Dev::start(&project, &[]);
        dev.wait_ready("/");
        assert_eq!(dev.get("/sub/page.html").status, 200);
        // The pid of the launch is stage 1 — the whole trap, asserted.
        assert!(
            dev.cmdline().contains("--stage=1"),
            "the launched pid is stage 1: {:?}",
            dev.cmdline()
        );
        let root = dev.dev_root();
        assert!(root.is_dir(), "the run's --dev-root exists while it serves");
        let descendants = dev.descendants();
        assert!(
            !descendants.is_empty(),
            "init forked its components:\n{}",
            dev.text()
        );
        let port = dev.port;

        dev.signal(signal);
        let status = dev
            .wait_exit(Instant::now() + STOP_TIMEOUT + STOP_SLACK)
            .unwrap_or_else(|| {
                panic!(
                    "blastit dev did not exit within {STOP_TIMEOUT:?} + {STOP_SLACK:?}\n{}",
                    dev.text()
                )
            });
        assert_eq!(
            status.code(),
            Some(0),
            "a drained dev boot exits 0, not {status:?}\n{}",
            dev.text()
        );
        // The listener is closed, not merely unused.
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err(),
            "127.0.0.1:{port} still accepts after the boot exited"
        );
        // No descendant of that tree survived: `starttime` tells a survivor from
        // a recycled pid.
        for (pid, started) in &descendants {
            assert_ne!(
                proc::starttime(*pid),
                Some(*started),
                "pid {pid} of the dev tree survived signal {signal}\n{}",
                dev.text()
            );
        }
        // The ephemeral root is gone (`R20`).
        assert!(
            !root.exists(),
            "the --dev-root survived the exit: {root:?}\n{}",
            dev.text()
        );
        dev.join_readers();
        dev.serial = None;
    }
}

// ---------------------------------------------------------------------------
// §6: `body raw` without `unvalidated` is refused on the production path, and
// `dev` serves it with the loud warning.
// ---------------------------------------------------------------------------

/// §10.2's production sentence, verbatim.
#[cfg(feature = "python")]
const RAW_REFUSAL: &str = "view \"upload\": body raw hands the request body to the application \
                            unvalidated, and the production path refuses it without the \
                            acknowledgement; write `body raw unvalidated` in that view, or \
                            declare the codec it takes (json, form, none)";

/// §13.2's `RawIngress` warning, verbatim.
#[cfg(feature = "python")]
const RAW_WARNING: &str = "view \"upload\": body raw passes the request body through unvalidated";

/// The app the serving twin posts to: it answers with the request body exactly as
/// it received it, so "byte-for-byte intact" is measured rather than inferred.
#[cfg(feature = "python")]
const RAW_APP: &str = r#"
def app(environ, start_response):
    length = int(environ.get("CONTENT_LENGTH") or 0)
    body = environ["wsgi.input"].read(length) if length else b""
    start_response(
        "200 OK",
        [("Content-Type", "application/octet-stream"),
         ("Content-Length", str(len(body)))],
    )
    return [body]
"#;

/// A manifest whose only view takes a `raw` body, with the acknowledgement as the
/// caller wrote it (` unvalidated` or nothing at all).
#[cfg(feature = "python")]
fn raw_manifest(acknowledgement: &str) -> String {
    format!(
        "app \"app\"\nentrypoint python \"app:app\"\n\nview upload POST \"/upload\" {{\n    \
         body raw{acknowledgement}\n}}\n"
    )
}

/// §6: production stage 1 (the process that binds and forks) refuses a `body raw`
/// route with no acknowledgement — in both log formats, naming the view and the
/// node, and never announcing `ready`.
///
/// An unprivileged `cargo test` never reaches a stage-0 gate, so
/// `--stage=1 --config-fd` (no `--dev-root`) is the strongest reachable form of
/// the production path: it re-reads the manifest from disk.
#[cfg(feature = "python")]
#[test]
fn a_raw_route_without_the_acknowledgement_is_refused_on_the_production_path() {
    use std::os::fd::AsRawFd;

    let document = |dir: &Path, port: u16, format: blastit::config::LogFormat| {
        let config = blastit::config::Config {
            manifest: dir.join("Blastfile"),
            manifest_dir: dir.to_path_buf(),
            port,
            server_port: port,
            log_format: format,
            ..blastit::config::Config::default()
        };
        serde_json::to_vec(&config.to_json()).expect("the configuration document")
    };

    for (format, tag) in [
        (blastit::config::LogFormat::Json, "json"),
        (blastit::config::LogFormat::Pretty, "pretty"),
    ] {
        let _serial = boot_lock();
        let dir = Project::new(&format!("raw-{tag}"));
        dir.write("Blastfile", raw_manifest(""));
        let port = free_port();
        let path = dir.path("stage1-config.json");
        std::fs::write(&path, document(&dir.dir, port, format)).expect("the document");
        let file = std::fs::File::open(&path).expect("the document file");
        rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty()).expect("clear FD_CLOEXEC");
        let fd = file.as_raw_fd();
        let mut command = Command::new(BINARY);
        command
            .arg("--stage=1")
            .arg(format!("--config-fd={fd}"))
            .env_clear()
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().expect("the blastit binary starts");
        drop(file);
        let deadline = Instant::now() + BOOT_TIMEOUT;
        let status = loop {
            match child.try_wait().expect("the boot's state") {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    break child.wait().expect("the killed stage 1");
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        // A refusal reaches the emitter's descriptor as one sentence: JSON wraps
        // it in an `app.output` event (with the quotes escaped), pretty writes it
        // as plain text. Both are read back, never grepped in the encoded line.
        let events: Vec<String> = stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["event"] == "app.output")
            .filter_map(|event| event["line"].as_str().map(ToOwned::to_owned))
            .collect();
        let refusal = format!("{stdout}\n{}", events.join("\n"));
        assert_eq!(
            status.code(),
            Some(1),
            "a refused boot exits 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            refusal.contains(RAW_REFUSAL),
            "the refusal is §10.2's sentence: {refusal}"
        );
        assert!(
            refusal.contains("view \"upload\"") && refusal.contains("body raw unvalidated"),
            "the refusal names the view and the node that fixes it: {refusal}"
        );
        assert!(
            stderr.is_empty(),
            "stage 1 writes the refusal on the emitter's descriptor, not fd 2: {stderr}"
        );
        assert!(
            !refusal.lines().any(|line| line
                .split_whitespace()
                .any(|word| word == "\"event\":\"ready\"")),
            "a refused boot never announces `ready`: {stdout}"
        );
    }
}

/// §6/§4: `dev` is the environment that serves the same file — with the loud
/// warning — and the body reaches the application byte for byte.
#[cfg(feature = "python")]
#[test]
fn dev_serves_a_raw_route_byte_for_byte_and_still_warns() {
    // A payload every other codec would refuse: a duplicate key and trailing
    // bytes.
    const PAYLOAD: &str = "{\"a\":1,\"a\":2} trailing";
    for acknowledgement in ["", " unvalidated"] {
        let project = Project::new("raw-dev");
        project.write("Blastfile", raw_manifest(acknowledgement));
        project.write("app.py", RAW_APP);
        let mut dev = Dev::start(&project, &[]);
        let dev = StopOnDrop(&mut dev);
        dev.0.wait_ready("/upload");
        let response = views::request_with(
            dev.0.port,
            "POST",
            "/upload",
            &[("Content-Type", "application/octet-stream")],
            Some(PAYLOAD),
        )
        .unwrap_or_else(|| panic!("no answer to the raw post:\n{}", dev.0.text()));
        assert_200("the raw post", &response);
        assert_eq!(
            response.body, PAYLOAD,
            "the raw body reaches the application and the client unchanged"
        );
        assert!(
            dev.0.text().contains(RAW_WARNING),
            "dev keeps the loud warning (`{acknowledgement:?}`):\n{}",
            dev.0.text()
        );
        assert!(
            !dev.0.text().contains("production path refuses"),
            "the boot refusal never fires in dev:\n{}",
            dev.0.text()
        );
    }
}
