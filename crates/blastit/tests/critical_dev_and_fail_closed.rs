#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for `R17`, `R19..R22` and `R29` (`dev`): the
//! unprivileged single-uid supervisor, the `execve` secret barrier, the exact
//! banner, the `BLASTIT_DEV_EXPOSE` guard, the per-route socket directory, the
//! per-role seccomp `egress` decision, and `dev --learn`'s atomic write.
//!
//! Every case starts the real `blastit` binary as the calling user: no root, no
//! Docker, no `BLASTIT_TEST_PG_URL`. `PgUpstream` (in `common/pg_upstream.rs`)
//! is the real loopback PostgreSQL peer the dbproxy dials, on `127.0.0.1:0`.
//!
//! The negative twins are the ones the requirements name: unprivileged
//! production must exit `1` with no single-UID fallback, an ungranted route must
//! get `ENOENT` from its socket, an `egress = false` view must get
//! `EPERM | ENOSYS` from `socket(AF_INET)`, and `--host 0.0.0.0` without
//! `BLASTIT_DEV_EXPOSE=1` must exit `1` with an empty stdout.
// `MB.17`: every case boots `blastit dev` on a WSGI app — only with feature = "python" (ADR-0010 §4).
#![cfg(feature = "python")]

mod common;

use std::fs;
use std::io::{BufRead, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::pg_upstream::PgUpstream;

/// How long a case waits for the server to report `ready`.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// One gibibyte, for the synthetic memory trees.
const GIB: u64 = 1024 * 1024 * 1024;
/// How long a case waits for an event or a line.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a case waits for the process to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long one whole request/response exchange may take.
///
/// The budget covers the head, the declared body and the close that proves the
/// pairing, so a stuck or desynchronised exchange fails inside it instead of
/// handing a stale status to an assertion (`MB.21.4.8`).
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);
/// The most of a response head this client will read before calling it broken.
const MAX_RESPONSE_HEAD: usize = 64 * 1024;
/// The most of a response body this client will hold.
const MAX_RESPONSE_BODY: usize = 16 * 1024 * 1024;
/// How many stdout lines an exchange failure quotes back.
const DIAGNOSTIC_TRAIL: usize = 12;

/// How many times a launch is retried when another process takes the port
/// `free_port()` handed out before the child can `bind` it (`EADDRINUSE`).
///
/// `free_port()` cannot lease the socket to the child (`blastit dev` only
/// accepts a port number), so the race between dropping the probe listener and
/// the child's `bind` is unavoidable. Three attempts keep that race from
/// turning a leg about an import, a banner or `--learn` into a failure.
const PORT_ATTEMPTS: u32 = 3;

/// A counter for unique fixture directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The application every case serves.
///
/// It reports its own uid and pid (so the suite can prove a real, unprivileged
/// view served the request), its whole environment (so the suite can prove no
/// secret survived the `execve` barrier), the errno of a PostgreSQL connection
/// through the route's own socket, and the errno of `socket(AF_INET)` (so the
/// `egress` filters are measured, not inferred).
const APP: &str = r#"
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


def _ready(sock):
    while True:
        header = _read_exact(sock, 5)
        length = struct.unpack("!I", header[1:])[0]
        body = _read_exact(sock, length - 4)
        if header[:1] == b"E":
            raise RuntimeError("pg: " + body.decode("utf-8", "replace"))
        if header[:1] == b"Z":
            return


def pg(sql):
    url = os.environ["DATABASE_URL"]
    query = url.split("?", 1)[1] if "?" in url else ""
    params = {}
    for part in query.split("&"):
        if "=" in part:
            name, value = part.split("=", 1)
            params[name] = value
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
    _ready(sock)
    body = sql.encode() + b"\0"
    sock.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
    _ready(sock)
    sock.close()


def pg_state(sql):
    """The statement's outcome, with PostgreSQL's SQLSTATE when it was refused."""
    try:
        pg(sql)
        return {"ok": True}
    except RuntimeError as err:
        fields = {}
        for part in str(err).split("\x00"):
            if len(part) > 1:
                fields[part[0]] = part[1:]
        return {"ok": False, "sqlstate": fields.get("C"), "message": fields.get("M")}


def application(environ, start_response):
    path = environ.get("PATH_INFO", "/")
    try:
        if path.endswith("/inet"):
            try:
                probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                probe.close()
                payload = {"ok": True}
            except OSError as err:
                payload = {"errno": err.errno}
        elif path.endswith("/env") or path == "/":
            payload = {"pid": os.getpid(), "uid": os.getuid(),
                       "env": dict(os.environ)}
        elif path == "/a/1":
            pg("SELECT 111")
            payload = {"ok": True, "pid": os.getpid()}
        elif path == "/b":
            pg("SELECT 222")
            payload = {"ok": True, "pid": os.getpid()}
        elif path == "/a/2":
            pg("SELECT 333")
            payload = {"ok": True, "pid": os.getpid()}
        elif path.endswith("/state"):
            payload = pg_state("SELECT 1")
        elif path == "/literal":
            pg("SELECT 'alice'")
            payload = {"ok": True}
        elif path.startswith("/zzz/"):
            payload = {"ok": True, "pid": os.getpid()}
        else:
            try:
                pg("SELECT 1")
                payload = {"ok": True}
            except OSError as err:
                payload = {"errno": err.errno}
        body = json.dumps(payload).encode()
        status = "200 OK"
    except Exception as err:  # noqa: BLE001 - the body is the diagnosis
        body = json.dumps({"error": str(err)}).encode()
        status = "500 Internal Server Error"
    start_response(status, [("Content-Type", "application/json"),
                            ("Content-Length", str(len(body)))])
    return [body]
"#;

/// A running `blastit dev`, with its output collected as it arrives.
struct Dev {
    child: Child,
    port: u16,
    dir: PathBuf,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    /// The thread draining the child's stdout, kept so an exit can be reported
    /// with the whole transcript (the last lines are still in the pipe when
    /// `wait` returns).
    stdout_reader: Option<JoinHandle<()>>,
    /// The thread draining the child's stderr.
    stderr_reader: Option<JoinHandle<()>>,
}

/// Why a freshly spawned `blastit dev` never reached `ready`.
///
/// A launch fails for exactly three reasons, and only the first is a property
/// of the host's load: it is retried with a fresh port and never blamed on the
/// behaviour a case is testing.
enum LaunchFailure {
    /// Another process took the port `free_port()` handed out between the probe
    /// `bind` and the child's real `bind` (`EADDRINUSE`).
    LostThePort,
    /// The child exited before `ready` for any other reason.
    Exited(ExitStatus),
    /// The child stayed alive but never printed `ready` within
    /// [`READY_TIMEOUT`].
    Timeout,
}

impl Dev {
    /// Starts `blastit dev` in a fresh fixture directory.
    ///
    /// `env` is the *whole* child environment besides the loader/interpreter
    /// variables the embedded CPython needs; `env_clear` keeps the suite's own
    /// `BLASTIT_*` out of a case that is about the environment.
    ///
    /// The returned server has already printed `ready`; a launch that exits
    /// first fails here (with the child's own transcript) instead of after the
    /// full readiness timeout. When another process took the ephemeral port
    /// between `free_port()` and the child's `bind`, the launch is retried with
    /// a fresh port (`PORT_ATTEMPTS`) because that `EADDRINUSE` is a property
    /// of the host's load, not of the behaviour the case is testing.
    fn start(tag: &str, manifest: &str, env: &[(&str, &str)], arguments: &[&str]) -> Self {
        Self::start_with(tag, manifest, env, arguments, free_port)
    }

    /// [`Dev::start`], with the port source injected so the retry can be driven
    /// deterministically from a test (the first attempt is handed a port that
    /// is already taken).
    fn start_with(
        tag: &str,
        manifest: &str,
        env: &[(&str, &str)],
        arguments: &[&str],
        mut ports: impl FnMut() -> u16,
    ) -> Self {
        let mut attempt = 1;
        loop {
            let dir = fixture(tag, manifest);
            let port = ports();
            let mut dev = Self::spawn(dir, port, env, arguments);
            match dev.settle() {
                Ok(()) => return dev,
                Err(LaunchFailure::LostThePort) if attempt < PORT_ATTEMPTS => {
                    eprintln!(
                        "blastit dev[{tag}]: port {port} was taken between free_port() and \
                         the child's bind (EADDRINUSE); retrying with a fresh port \
                         ({attempt}/{PORT_ATTEMPTS})"
                    );
                    attempt += 1;
                    // `dev` drops here: the child already exited and its
                    // fixture directory is removed.
                }
                Err(LaunchFailure::LostThePort) => panic!(
                    "blastit dev[{tag}] could not bind port {port} in {PORT_ATTEMPTS} \
                     attempts: another process kept taking the port between free_port() and \
                     the child's `bind` (EADDRINUSE). This is a harness isolation failure, not \
                     a `{tag}` behaviour failure.\nstdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
                Err(LaunchFailure::Exited(status)) => panic!(
                    "blastit dev[{tag}] exited {status} before `ready` on port {port}\
                     \nstdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
                Err(LaunchFailure::Timeout) => panic!(
                    "no stdout line contained `ready` within {READY_TIMEOUT:?} for \
                     blastit dev[{tag}] on port {port}\nstdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
            }
        }
    }

    /// Spawns one `blastit dev` attempt, with its output collected as it
    /// arrives.
    fn spawn(dir: PathBuf, port: u16, env: &[(&str, &str)], arguments: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .args(arguments)
            .env_clear()
            // The suite does not depend on the host's free memory or on how
            // many views this machine happens to fit: the sizing knobs are set
            // to values that cannot wedge a test on a loaded runner. They are
            // `BLASTIT_*`, so the `execve` barrier strips them again (which the
            // five-roles case checks).
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        copy_runtime_env(&mut command);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("the blastit binary starts");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stdout_reader = child
            .stdout
            .take()
            .map(|out| spawn_reader(out, Arc::clone(&stdout)));
        let stderr_reader = child
            .stderr
            .take()
            .map(|err| spawn_reader(err, Arc::clone(&stderr)));
        Self {
            child,
            port,
            dir,
            stdout,
            stderr,
            stdout_reader,
            stderr_reader,
        }
    }

    /// Waits for the child to print `ready`, returning early when it exits
    /// first so a lost port is classified before the readiness timeout.
    fn settle(&mut self) -> Result<(), LaunchFailure> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| is_ready_event(line)) {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait().expect("the child's exit status") {
                // `wait` returning does not mean the pipes are empty: the last
                // lines (including a failed `bind`) may still be on their way
                // to the sink, and they are what classifies the exit.
                self.drain_output();
                return Err(
                    if lost_the_port(self.port, &self.stdout_text(), &self.stderr_text()) {
                        LaunchFailure::LostThePort
                    } else {
                        LaunchFailure::Exited(status)
                    },
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(LaunchFailure::Timeout)
    }

    /// Waits (bounded) for the pipe readers to reach EOF after the child exits,
    /// and joins the ones that did.
    ///
    /// Only finished readers are joined: a lingering grandchild that inherited
    /// the write end must not wedge a case whose parent is already gone.
    fn drain_output(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let stdout_done = self
                .stdout_reader
                .as_ref()
                .is_none_or(JoinHandle::is_finished);
            let stderr_done = self
                .stderr_reader
                .as_ref()
                .is_none_or(JoinHandle::is_finished);
            if stdout_done && stderr_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        for slot in [&mut self.stdout_reader, &mut self.stderr_reader] {
            if slot.as_ref().is_some_and(JoinHandle::is_finished)
                && let Some(handle) = slot.take()
            {
                let _ = handle.join();
            }
        }
    }

    fn stdout_lines(&self) -> Vec<String> {
        self.stdout.lock().unwrap().clone()
    }

    fn stdout_text(&self) -> String {
        self.stdout_lines().join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().unwrap().join("\n")
    }

    /// Waits until a stdout line contains `needle`.
    fn wait_for_line(&self, needle: &str) {
        self.wait_for_event(&format!("contained `{needle}`"), |line| {
            line.contains(needle)
        });
    }

    /// Waits until a stdout line satisfies `predicate`.
    fn wait_for_event(&self, description: &str, predicate: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| predicate(line)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "no stdout line {description} within {READY_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// Waits for the `ready` event, in either log format.
    ///
    /// This must match the event itself, not any line containing the word:
    /// stage 1's `binding 127.0.0.1:<port>: Address al**ready** in use` is
    /// printed on stdout as an `app.output` event, so a substring match would
    /// declare a launch that never bound to be up — and the subsequent
    /// `connect` would then reach whichever process stole the port.
    fn wait_for_ready(&self) {
        self.wait_for_event("was the `ready` event", is_ready_event);
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the server never accepted a connection\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// The banner's three lines, found by its first line (§1.2).
    fn banner(&self) -> Vec<String> {
        self.wait_for_line("blastit dev · single-uid simulation");
        let lines = self.stdout_lines();
        let start = lines
            .iter()
            .position(|line| line.contains("blastit dev · single-uid simulation"))
            .unwrap_or(0);
        lines.into_iter().skip(start).take(3).collect()
    }

    /// One HTTP/1.1 request, answered by the response that belongs to it.
    ///
    /// Every exchange gets its own connection — one request, `Connection:
    /// close`, and the socket is dropped when the exchange ends — and the reader
    /// proves the pairing instead of trusting it: it parses the head on its own,
    /// reads exactly the body the head declares, and refuses a buffer it cannot
    /// prove complete (a short body, a deadline, a second response behind the
    /// first). A failure names this request, the bounded bytes read and the
    /// server's last lines, so a stale status can never reach an assertion
    /// (`MB.21.4.8`).
    fn request(&self, method: &str, target: &str, body: Option<(&str, &str)>) -> Response {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting for {method} {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        match exchange(stream, method, target, body, EXCHANGE_TIMEOUT) {
            Ok(response) => response,
            Err(diagnostic) => panic!(
                "{diagnostic}\nblastit dev stdout (last {DIAGNOSTIC_TRAIL} lines):\n{}",
                tail(&self.stdout_text(), DIAGNOSTIC_TRAIL)
            ),
        }
    }

    fn get_json(&self, target: &str) -> serde_json::Value {
        let response = self.request("GET", target, None);
        assert_eq!(
            response.status,
            200,
            "{target}: {}\nstdout:\n{}",
            response.body,
            self.stdout_text()
        );
        serde_json::from_str(&response.body)
            .unwrap_or_else(|err| panic!("{target}: {err}: {}", response.body))
    }

    /// The init process's children, as `/proc` sees them.
    fn children(&self) -> Vec<i32> {
        let path = format!(
            "/proc/{}/task/{}/children",
            self.child.id(),
            self.child.id()
        );
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => panic!("reading {path}: {err}\n{}", self.stdout_text()),
        };
        let mut pids: Vec<i32> = text
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect();
        pids.sort_unstable();
        pids
    }

    /// The ephemeral runtime root this run created, from `/tmp`.
    ///
    /// The suite clears `XDG_RUNTIME_DIR`, so the documented fallback applies and
    /// the name starts with `blastit-dev-<uid>-<init pid>-`.
    fn root(&self) -> PathBuf {
        let prefix = format!(
            "blastit-dev-{}-{}-",
            rustix::process::geteuid().as_raw(),
            self.child.id()
        );
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            let found = fs::read_dir("/tmp")
                .map(|entries| {
                    entries.flatten().map(|entry| entry.path()).find(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with(&prefix))
                    })
                })
                .ok()
                .flatten();
            if let Some(root) = found {
                return root;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no runtime root for pid {} appeared", self.child.id());
    }

    /// `SIGTERM`, then the exit status.
    fn terminate(&mut self) -> ExitStatus {
        // SAFETY: the pid came from `spawn`; `kill` takes two integers.
        let killed = unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        panic!(
            "blastit dev did not exit within {EXIT_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A parsed HTTP response.
#[derive(Debug)]
struct Response {
    status: u16,
    body: String,
}

/// How the head says the body is delimited.
enum Framing {
    /// `Content-Length: n`: exactly `n` bytes follow the head.
    Length(usize),
    /// No length at all: the body ends where the connection does.
    ///
    /// A chunked body takes this path too — the terminator is only provable
    /// against the close that follows it here, and this suite's own app always
    /// declares a length.
    UntilClose,
}

/// Why a read stopped before the exchange could be proven complete.
enum Fault {
    /// The peer closed the connection.
    Closed,
    /// The deadline passed with the exchange unfinished.
    Timeout,
    /// The socket failed.
    Io(String),
}

impl Fault {
    fn describe(&self) -> String {
        match self {
            Self::Closed => "the peer closed the connection".to_string(),
            Self::Timeout => "nothing arrived before the deadline".to_string(),
            Self::Io(err) => format!("reading the response failed: {err}"),
        }
    }
}

/// One request in flight: its own socket, one deadline, and the bytes read.
struct Exchange<'a> {
    stream: TcpStream,
    deadline: Instant,
    method: &'a str,
    target: &'a str,
    raw: Vec<u8>,
}

impl Exchange<'_> {
    /// The diagnostic one failed exchange panics with.
    ///
    /// It names the request, what was still missing, the bytes actually read
    /// (bounded and escaped) and the peer it read them from — never a whole
    /// frame dump.
    fn fail(&self, what: impl std::fmt::Display) -> String {
        let peer = self
            .stream
            .peer_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "an unknown peer".to_string());
        format!(
            "{} {}: {what}; {} response bytes read from {peer}: {}",
            self.method,
            self.target,
            self.raw.len(),
            preview(&self.raw)
        )
    }

    /// Appends whatever arrives next, or says why the exchange cannot continue.
    fn read_more(&mut self) -> Result<usize, Fault> {
        loop {
            let now = Instant::now();
            if now >= self.deadline {
                return Err(Fault::Timeout);
            }
            let remaining = (self.deadline - now).max(Duration::from_millis(1));
            self.stream
                .set_read_timeout(Some(remaining))
                .map_err(|err| Fault::Io(err.to_string()))?;
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(Fault::Closed),
                Ok(read) => {
                    self.raw.extend_from_slice(&chunk[..read]);
                    return Ok(read);
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Err(Fault::Timeout);
                }
                Err(err) => return Err(Fault::Io(err.to_string())),
            }
        }
    }
}

/// One request and the response that belongs to it (`MB.21.4.8`).
///
/// `stream` is this request's own connection: one request goes out, and it is
/// closed when the exchange ends, so nothing can be pipelined behind the
/// response and no body-carrying request can ever leave a connection to be
/// reused. The reader proves the pairing rather than trusting it — the head is
/// parsed on its own, the declared body is read exactly, and everything after
/// it must be the close — so a short, stale or desynchronised buffer is an
/// `Err` naming this request, never a status an assertion reads as the
/// server's answer.
fn exchange(
    mut stream: TcpStream,
    method: &str,
    target: &str,
    body: Option<(&str, &str)>,
    budget: Duration,
) -> Result<Response, String> {
    let (content_type, payload) = match body {
        Some((content_type, payload)) => (Some(content_type), payload),
        None => (None, ""),
    };
    let mut request =
        format!("{method} {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n");
    if let Some(content_type) = content_type {
        request.push_str(&format!("Content-Type: {content_type}\r\n"));
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    request.push_str("\r\n");
    request.push_str(payload);
    let written = stream
        .set_write_timeout(Some(budget))
        .and_then(|()| stream.write_all(request.as_bytes()));
    if let Err(err) = written {
        return Err(format!(
            "{method} {target}: sending {} request bytes failed: {err}",
            request.len()
        ));
    }
    let mut exchange = Exchange {
        stream,
        deadline: Instant::now() + budget,
        method,
        target,
        raw: Vec::new(),
    };

    // 1. The head, on its own: a status line is not a response.
    let head_end = loop {
        if let Some(at) = head_end_of(&exchange.raw) {
            break at;
        }
        if exchange.raw.len() > MAX_RESPONSE_HEAD {
            return Err(exchange.fail(format!("no complete head inside {MAX_RESPONSE_HEAD} bytes")));
        }
        exchange.read_more().map_err(|fault| {
            exchange.fail(format!("the head is incomplete: {}", fault.describe()))
        })?;
    };
    let (status, framing) = parse_head(&exchange.raw[..head_end])
        .map_err(|err| exchange.fail(format!("the head is unusable: {err}")))?;

    // 2. Exactly the body the head declared, or the close that declares it over.
    let body = match framing {
        Framing::Length(len) => {
            if len > MAX_RESPONSE_BODY {
                return Err(exchange.fail(format!(
                    "the head declares {len} body bytes, over {MAX_RESPONSE_BODY}"
                )));
            }
            while exchange.raw.len() - head_end < len {
                let read = exchange.raw.len() - head_end;
                exchange.read_more().map_err(|fault| {
                    exchange.fail(format!(
                        "{read} of {len} declared body bytes arrived, then {}",
                        fault.describe()
                    ))
                })?;
            }
            exchange.raw[head_end..head_end + len].to_vec()
        }
        Framing::UntilClose => {
            loop {
                let read = exchange.raw.len() - head_end;
                if read > MAX_RESPONSE_BODY {
                    return Err(
                        exchange.fail(format!("the body is over {MAX_RESPONSE_BODY} bytes"))
                    );
                }
                match exchange.read_more() {
                    Ok(_) => {}
                    // The close is what declares this body complete.
                    Err(Fault::Closed) => break,
                    Err(fault) => {
                        return Err(exchange.fail(format!(
                            "the body is unproven after {read} bytes: {}",
                            fault.describe()
                        )));
                    }
                }
            }
            exchange.raw[head_end..].to_vec()
        }
    };

    // 3. Nothing may follow the response this request was answered with.
    if let Framing::Length(len) = framing {
        let end = head_end + len;
        if exchange.raw.len() > end {
            let extra = exchange.raw.len() - end;
            return Err(exchange.fail(format!(
                "{extra} more bytes followed the declared body: the pairing is desynchronised"
            )));
        }
        match exchange.read_more() {
            Ok(read) => {
                return Err(exchange.fail(format!(
                    "{read} more bytes followed the declared body: the pairing is \
                     desynchronised"
                )));
            }
            Err(Fault::Closed) => {}
            Err(fault) => {
                return Err(exchange.fail(format!(
                    "the connection did not end after the declared body: {}",
                    fault.describe()
                )));
            }
        }
    }
    // The exchange is over: nothing of it is left to reuse.
    let _ = exchange.stream.shutdown(Shutdown::Both);
    Ok(Response {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Where the head ends, if its blank line has arrived.
fn head_end_of(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

/// The status and the body framing of one complete head.
fn parse_head(head: &[u8]) -> Result<(u16, Framing), String> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or_default();
    let code = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(format!("`{status_line}` is not an HTTP/1.x status line"));
    }
    if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("`{status_line}` has no three-digit status"));
    }
    let status: u16 = code
        .parse()
        .map_err(|err| format!("`{code}` is not a status: {err}"))?;

    let mut declared: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(format!("`{line}` is not a header"));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if declared.is_some() {
                return Err("the head declares a length twice".to_string());
            }
            declared = Some(
                value
                    .parse()
                    .map_err(|err| format!("`Content-Length: {value}` is not a length: {err}"))?,
            );
        }
        if name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked") {
            chunked = true;
        }
    }
    match (declared, chunked) {
        (Some(_), true) => Err("the head carries two body framings".to_string()),
        (Some(len), false) => Ok((status, Framing::Length(len))),
        (None, _) => Ok((status, Framing::UntilClose)),
    }
}

/// A bounded, printable view of the bytes read so far.
fn preview(raw: &[u8]) -> String {
    let mut text = String::new();
    for byte in raw.iter().take(200) {
        match byte {
            b'\r' => text.push_str("\\r"),
            b'\n' => text.push_str("\\n"),
            b'\\' => text.push_str("\\\\"),
            0x20..=0x7e => text.push(char::from(*byte)),
            _ => text.push_str(&format!("\\x{byte:02x}")),
        }
    }
    if raw.len() > 200 {
        text.push_str("...");
    }
    text
}

/// The last `count` lines of `text`, for a panic that stays readable.
fn tail(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// A fresh `0755` fixture directory holding `Blastfile` and `app.py`.
fn fixture(tag: &str, manifest: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "blastit-dev-{}-{tag}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("a readable fixture");
    fs::write(dir.join("Blastfile"), manifest).expect("write Blastfile");
    fs::write(dir.join("app.py"), APP).expect("write app.py");
    fs::set_permissions(dir.join("app.py"), fs::Permissions::from_mode(0o644))
        .expect("a readable app.py");
    dir
}

/// The loader and interpreter variables the embedded CPython needs.
///
/// The child is started with `env_clear` so a developer's `BLASTIT_*` cannot
/// reach a case about the environment; these two are what the binary itself
/// needs, exactly as `make check` sets them.
fn copy_runtime_env(command: &mut Command) {
    for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// Runs a `blastit dev` that is expected to exit, in a fresh fixture directory.
///
/// The caller gets the final attempt's pid, fixture directory and output. A
/// launch that lost the ephemeral port to another process between
/// `free_port()` and the child's `bind` is retried with a fresh port
/// (`PORT_ATTEMPTS`), so the case sees the failure it is about (here: the
/// `BLASTIT_APP` import) instead of an unrelated `EADDRINUSE`.
fn run_dev_to_exit(tag: &str, manifest: &str, env: &[(&str, &str)]) -> (u32, PathBuf, Output) {
    let mut attempt = 1;
    loop {
        let dir = fixture(tag, manifest);
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // The case measures a failure *after* stage 0 (the import, the
            // bind), so the host's free memory must not turn it into the
            // watermark boot refusal. A caller that is about the watermark
            // overrides this afterwards.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        copy_runtime_env(&mut command);
        for (name, value) in env {
            command.env(name, value);
        }
        let child = command.spawn().expect("blastit dev spawns");
        let pid = child.id();
        let output = child.wait_with_output().expect("blastit dev exits");
        let lost = lost_the_port(
            port,
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        );
        if lost && attempt < PORT_ATTEMPTS {
            eprintln!(
                "blastit dev[{tag}]: port {port} was taken between free_port() and the \
                 child's bind (EADDRINUSE); retrying with a fresh port ({attempt}/{PORT_ATTEMPTS})"
            );
            let _ = fs::remove_dir_all(&dir);
            attempt += 1;
            continue;
        }
        return (pid, dir, output);
    }
}

/// Reads a child's pipe into `sink`, line by line.
fn spawn_reader(pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(pipe);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&line)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();
                    if !text.is_empty() {
                        sink.lock().unwrap().push(text);
                    }
                }
            }
        }
    })
}

/// A TCP port nothing is listening on right now.
///
/// The listener is dropped before the caller passes the number to `blastit
/// dev`, so this is a hint, not a reservation: another process can bind the
/// port in between. Every launch must therefore treat an `EADDRINUSE` on this
/// port as the harness losing a race (see [`PORT_ATTEMPTS`]) rather than as the
/// behaviour it is testing.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// Whether a stdout line is the `ready` event, in either log format.
///
/// A substring match is wrong: stage 1's failed bind prints
/// `Address already in use` on stdout (as an `app.output` event), which
/// contains the word `ready` and would make a dead launch look serving.
fn is_ready_event(line: &str) -> bool {
    line.contains("\"event\":\"ready\"")
        || line.contains("\"event\": \"ready\"")
        || line.split_whitespace().any(|word| word == "ready")
}

/// Whether a `blastit dev` that never served failed because another process
/// took the port `free_port()` handed out before the child could `bind` it.
///
/// Stage 1 reports the failure as `binding <host>:<port>: Address already in
/// use (os error 98)`, where the host is the `--host` that leg asked for
/// (`127.0.0.1` by default, `0.0.0.0` for the `EXPOSED` cases). That names a
/// harness isolation race, not the import, banner or `--learn` write the
/// surrounding case is about, so the launch retries with a fresh port.
fn lost_the_port(port: u16, stdout: &str, stderr: &str) -> bool {
    let address = format!(":{port}:");
    [stdout, stderr].iter().any(|text| {
        text.lines().any(|line| {
            line.contains("binding ")
                && line.contains(&address)
                && (line.contains("Address already in use")
                    || line.contains("os error 98")
                    || line.contains("EADDRINUSE"))
        })
    })
}

/// The effective uid of `pid`.
fn euid_of(pid: i32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// A manifest with one granted store and the views a case needs.
fn manifest(views: &str) -> String {
    format!(
        "app \"app\"\n\nentrypoint python \"app:application\"\n\npostgres $DATABASE_URL\n\n{views}"
    )
}

/// One `view <name> … { … }` block of a learned `Blastfile`, from just after
/// the name to the next `view`: its header's tail and its whole body.
///
/// A learned file is a `Blastfile`, so a grant belongs to the `view` it was
/// written under — the same document a developer reads and edits.
fn view_block<'a>(learned: &'a str, name: &str) -> &'a str {
    learned
        .split(&format!("\nview {name} "))
        .nth(1)
        .unwrap_or_default()
        .split("\nview ")
        .next()
        .unwrap_or_default()
}

/// The `DATABASE_URL` a case hands the child: a real-looking credential for a
/// `PgUpstream` that answers trust authentication, with TLS off because the
/// upstream is loopback.
fn database_url(server: &PgUpstream) -> String {
    format!(
        "postgresql://real-user:real-secret@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    )
}

/// A one-shot listener that answers one exchange with `script`, and its port.
///
/// Bound rather than probed, so the port is the one this exchange reaches: the
/// pairing under test must not be able to become a lost-port race.
fn answering_once(script: impl FnOnce(&mut TcpStream) + Send + 'static) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a listener on an ephemeral port");
    let port = listener
        .local_addr()
        .expect("the listener's address")
        .port();
    let handler = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one connection");
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .expect("a write timeout");
        script(&mut stream);
    });
    (port, handler)
}

/// Drives [`exchange`] against `script` and returns its answer.
fn ask_once(
    script: impl FnOnce(&mut TcpStream) + Send + 'static,
    budget: Duration,
) -> Result<Response, String> {
    let (port, handler) = answering_once(script);
    let answer = exchange(
        TcpStream::connect(("127.0.0.1", port)).expect("a connection to the listener"),
        "POST",
        "/env",
        Some(("text/plain", "hello")),
        budget,
    );
    handler.join().expect("the one-shot listener's thread");
    answer
}

/// The dev client answers only with the response it can prove belongs to it.
///
/// `MB.21.4.8`: the helper used to `read_to_end` and parse the first status line
/// of whatever arrived — `unwrap_or_default` swallowed the timeout or reset and
/// kept the bytes — so a response cut short, a second response behind the first
/// or a peer that never answered all handed a status to the assertion that the
/// server never sent. Every leg below is one of those shapes: the first is the
/// allowed one (a complete response split across writes is read in full), the
/// rest must fail with a diagnostic naming the request instead of answering.
#[test]
fn the_dev_client_only_answers_with_the_response_it_can_prove() {
    // Allowed: a complete head and its declared body, split across two writes.
    let complete = ask_once(
        |stream| {
            stream
                .write_all(
                    b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\
                      Connection: close\r\n\r\ncontent ",
                )
                .expect("the head and part of the body");
            std::thread::sleep(Duration::from_millis(20));
            stream
                .write_all(b"too large\n")
                .expect("the rest of the body");
            let _ = stream.shutdown(Shutdown::Write);
        },
        Duration::from_secs(10),
    )
    .expect("a complete response is read in full");
    assert_eq!(complete.status, 413);
    assert_eq!(complete.body, "content too large\n");

    // Denied: the peer closes one byte short of its declared body.
    let short = ask_once(
        |stream| {
            stream
                .write_all(b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\r\ncontent too large")
                .expect("a short body");
            let _ = stream.shutdown(Shutdown::Write);
        },
        Duration::from_secs(10),
    )
    .expect_err("a body the head declared and the peer did not send is not a response");
    assert!(short.contains("/env"), "the diagnostic names it: {short}");
    assert!(
        short.contains("17 of 18"),
        "the diagnostic counts the body it did not get: {short}"
    );

    // Denied: a second response follows the declared body, which is exactly what
    // a reused keep-alive connection looks like when a refusal is left unread.
    let desynchronised = ask_once(
        |stream| {
            stream
                .write_all(
                    b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\r\ncontent too large\n\
                      HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\n{\"ok\":true}",
                )
                .expect("two responses on one connection");
            let _ = stream.shutdown(Shutdown::Write);
        },
        Duration::from_secs(10),
    )
    .expect_err("a response with another one behind it cannot be paired");
    assert!(
        desynchronised.contains("more bytes followed"),
        "the diagnostic names the desynchronisation: {desynchronised}"
    );

    // Denied: the peer says nothing at all, so the deadline must fire — and fire
    // inside the budget, with the request named, instead of reading status 0.
    let started = Instant::now();
    let silent = ask_once(
        |_stream| std::thread::sleep(Duration::from_secs(1)),
        Duration::from_millis(300),
    )
    .expect_err("an exchange nobody answered is not an answer");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the deadline is a bound, not a hint: {:?}",
        started.elapsed()
    );
    assert!(
        silent.contains("before the deadline") && silent.contains("/env"),
        "the diagnostic names the deadline and the request: {silent}"
    );
}

#[test]
fn dev_boots_five_unprivileged_roles_and_strips_secrets() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let views = "\
view json POST \"/json\" {
    body json
}

view home regex '/.*' {
    postgres {
        query 'SELECT 1'
    }
}
";
    let mut dev = Dev::start(
        "five-roles",
        &manifest(views),
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_SECRET_MARKER", "must-not-reach-a-view"),
            ("STRIP_ME", "strip-me-too"),
            ("BLASTIT_ENV_STRIP", "STRIP_ME"),
        ],
        &[],
    );
    dev.wait_for_ready();

    // The banner's header (§1.2) plus one line per declared view, route count
    // in the third line, and the address a terminal can click. The helper reads
    // three lines, so the view table is asserted from the run's own stdout: a
    // header with no `  view  ` row behind it is not a banner.
    let banner = dev.banner();
    let stdout = dev.stdout_text();
    let header = stdout
        .lines()
        .position(|line| line.contains("blastit dev · single-uid simulation"))
        .unwrap_or_else(|| panic!("the banner header:\n{stdout}"));
    assert!(
        stdout
            .lines()
            .skip(header + 1)
            .any(|line| line.starts_with("  view  ")),
        "the header is followed by the view table:\n{stdout}"
    );
    assert_eq!(
        banner[0],
        format!(
            "  blastit dev · single-uid simulation · http://127.0.0.1:{}",
            dev.port
        )
    );
    assert_eq!(
        banner[1],
        "  Not a sandbox. Production `blastit` refuses to run like this."
    );
    assert_eq!(
        banner[2], "  http · scheduler · dbproxy · 2 routes",
        "the banner pluralises and counts the manifest's routes"
    );

    // The view table: one line per declared route, `view  NAME  METHODS
    // pattern`, in the same run's stdout — the banner names what is actually
    // served, route by route.
    let views: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("  view  "))
        .collect();
    assert_eq!(views.len(), 2, "one line per declared view:\n{stdout}");
    assert!(
        views
            .iter()
            .any(|line| line.trim() == "view  json  POST  /json"),
        "the json view's name, methods and pattern:\n{stdout}"
    );
    assert!(
        views.iter().any(|line| line.starts_with("  view  home  *")),
        "a view with no methods answers every method:\n{stdout}"
    );

    // `R22`: the boot event says which simulation this is.
    dev.wait_for_line("privilege=unprivileged-dev");

    // The application is really served, by a real view process.
    let env = dev.get_json("/env");
    let uid = rustix::process::geteuid().as_raw();
    assert_eq!(env["uid"].as_u64(), Some(u64::from(uid)), "{env}");
    assert_ne!(env["pid"].as_u64(), Some(u64::from(dev.child.id())));
    let app_env = env["env"].as_object().expect("the child's environment");
    let database = app_env
        .get("DATABASE_URL")
        .and_then(serde_json::Value::as_str)
        .expect("DATABASE_URL is rendered");
    assert!(
        !database.contains("real-secret"),
        "the real credential reached a view: {database}"
    );
    assert!(
        database.contains(".s.PGSQL") || database.contains("5432"),
        "DATABASE_URL points at the local endpoint: {database}"
    );
    assert!(
        app_env.get("BLASTIT_SECRET_MARKER").is_none(),
        "a BLASTIT_* variable reached a view: {env}"
    );
    assert!(
        app_env.get("STRIP_ME").is_none(),
        "a BLASTIT_ENV_STRIP name reached a view: {env}"
    );
    assert!(
        app_env.get("BLASTIT_ENV_STRIP").is_none(),
        "BLASTIT_ENV_STRIP itself reached a view: {env}"
    );

    // The granted route's SQL goes through dbproxy to the upstream.
    let sql = dev.get_json("/sql");
    assert_eq!(sql["ok"], true, "{sql}\n{}", dev.stdout_text());
    assert!(
        server
            .statements()
            .iter()
            .any(|statement| statement.contains("SELECT 1")),
        "the statement reached the upstream: {:?}",
        server.statements()
    );
    let startups = server.startups();
    assert!(
        startups.iter().any(|startup| startup.user == "real-user"),
        "dbproxy authenticated with the proxy's real user: {startups:?}"
    );

    // `R12`: a body on a route with no ingress is still `413` in dev.
    let rejected = dev.request("POST", "/env", Some(("text/plain", "hello")));
    assert_eq!(rejected.status, 413, "{}", dev.stdout_text());
    // `R14`: a declared `json` codec still rejects malformed JSON.
    let malformed = dev.request("POST", "/json", Some(("application/json", "{bad")));
    assert_eq!(malformed.status, 400, "{}", dev.stdout_text());

    // `R19`: init forked http, the scheduler, dbproxy and at least one view,
    // and every one of them runs as the caller.
    let children = dev.children();
    assert!(
        children.len() >= 4,
        "five roles means init plus four children, saw {children:?}\n{}",
        dev.stdout_text()
    );
    for pid in &children {
        assert_eq!(
            euid_of(*pid),
            Some(uid),
            "child {pid} does not run as uid {uid}"
        );
        if uid != 0 {
            assert_ne!(euid_of(*pid), Some(0), "child {pid} runs as root");
        }
    }

    let root = dev.root();
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
    assert!(
        !root.exists(),
        "the ephemeral root was not removed: {root:?}"
    );
}

#[test]
fn one_route_banner_singularises() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view home regex '/.*' {}
";
    let mut dev = Dev::start(
        "one-route",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &[],
    );
    dev.wait_for_ready();
    let banner = dev.banner();
    assert_eq!(
        banner[2], "  http · scheduler · dbproxy · 1 route",
        "{banner:?}"
    );
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

#[test]
fn a_non_loopback_host_needs_the_acknowledgement() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {}\n";
    let manifest = manifest(routes);

    // Without `BLASTIT_DEV_EXPOSE=1`: exit `1`, nothing on stdout, and the fix
    // named on stderr (`S3`, `R22`).
    let dir = fixture("expose-refused", &manifest);
    let output = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--port", "18080", "--host", "0.0.0.0"])
        .env_clear()
        .env("DATABASE_URL", &url)
        .current_dir(&dir)
        .output()
        .expect("the binary runs");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "an exposure refusal writes nothing to stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("BLASTIT_DEV_EXPOSE"), "{stderr}");
    assert!(stderr.contains("0.0.0.0"), "{stderr}");

    // With it: the banner names the address and the uid, and a `WARN` event
    // names `<host>:<port>` and the uid as well.
    let mut dev = Dev::start(
        "exposed",
        &manifest,
        &[("DATABASE_URL", &url), ("BLASTIT_DEV_EXPOSE", "1")],
        &["--host", "0.0.0.0"],
    );
    dev.wait_for_ready();
    let banner = dev.banner();
    assert_eq!(
        banner[0],
        format!(
            "  blastit dev · single-uid simulation · 0.0.0.0:{} · uid {} (EXPOSED, no auth)",
            dev.port,
            rustix::process::geteuid().as_raw()
        )
    );
    dev.wait_for_line("EXPOSED");
    let transcript = dev.stdout_text();
    assert!(
        transcript.contains(&format!("0.0.0.0:{}", dev.port)),
        "the warning names host:port: {transcript}"
    );
    assert!(
        transcript.contains(&rustix::process::geteuid().as_raw().to_string()),
        "the warning names the uid: {transcript}"
    );
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

#[test]
fn a_memory_watermark_above_the_reading_serves_and_above_the_host_refuses_by_name() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n";

    // Allowed arm: a 100% watermark is above or equal to every reading, so the
    // boot serves. This is the only way to place the host *below* the
    // watermark; the default 85% cannot be raised by a test host.
    let mut dev = Dev::start(
        "mem-below",
        &manifest(routes),
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_MEM_HIGH_WATERMARK", "100"),
        ],
        &[],
    );
    dev.wait_for_ready();
    assert_eq!(
        dev.get_json("/")["uid"].as_u64(),
        Some(u64::from(rustix::process::geteuid().as_raw())),
        "the view served the first request:\n{}",
        dev.stdout_text()
    );
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());

    // Denied arm: a 0% watermark is below every real reading, so the boot has
    // to say so and exit instead of accepting requests that answer `503`.
    let (_, dir, output) = run_dev_to_exit(
        "mem-above",
        &manifest(routes),
        &[("DATABASE_URL", &url), ("BLASTIT_MEM_HIGH_WATERMARK", "0")],
    );
    let _ = fs::remove_dir_all(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a boot above the watermark fails:\nstdout:\n{}\nstderr:\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("BLASTIT_MEM_HIGH_WATERMARK"),
        "the failure names the setting: {stderr}"
    );
    assert!(
        stderr.contains("GiB") && stderr.contains('%'),
        "the failure names the current usage: {stderr}"
    );
    assert!(
        stderr.contains("503"),
        "the failure names the symptom it prevents: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "a startup error writes zero bytes on stdout (§1.1): {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn the_watermark_reading_leaves_out_the_cache_the_kernel_reclaims() {
    // 48 GiB of memory and 46 GiB of `memory.current`, of which 9 GiB is
    // reclaimable on demand: 8 GiB of page cache that is neither tmpfs nor
    // mapped (`file 40 − shmem 31 − file_mapped 1`) and 1 GiB of reclaimable
    // slab. The raw figure is at 95.8 %; the reading the watermark uses is at
    // 77.1 %.
    let dir = memory_tree(
        "cache",
        48,
        46,
        Some(
            "anon 6442450944\nfile 42949672960\nshmem 33285996544\n\
             file_mapped 1073741824\ninactive_file 6442450944\nactive_file 4332199936\n\
             slab_reclaimable 1073741824\nslab_unreclaimable 292206408\n",
        ),
    );
    let cache = blastit::mem_usage::read(&dir).expect("a readable tree");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(cache.total(), 48 * GIB);
    assert_eq!(cache.raw(), 46 * GIB, "memory.current");
    assert_eq!(
        cache.reclaimable(),
        9 * GIB,
        "(file − shmem − file_mapped) + slab_reclaimable"
    );
    assert_eq!(cache.used(), 37 * GIB, "46 − 8 − 1");
    assert!(
        cache.raw() as f64 / cache.total() as f64 * 100.0 > 95.0,
        "the raw figure is what made a healthy host refuse"
    );
    assert!(
        blastit::mem_usage::boot_refusal(&cache, 85.0).is_none(),
        "a host with 9 GiB reclaimable and 2 GiB of cache left serves: {cache:?}"
    );
    // tmpfs is not reclaimable without swap, so it stays counted: the same
    // tree without any page-cache margin is over the watermark.
    let dir = memory_tree(
        "tmpfs",
        48,
        46,
        Some("file 32212254720\nshmem 32212254720\nslab_reclaimable 0\n"),
    );
    let tmpfs = blastit::mem_usage::read(&dir).expect("a readable tree");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(tmpfs.reclaimable(), 0, "shmem is not page cache for this");
    assert!(blastit::mem_usage::boot_refusal(&tmpfs, 85.0).is_some());

    // The same host with none of the cache: 45.7 of 48 GiB is 95.2 %, and the
    // non-reclaimable reading refuses by name.
    let dir = memory_tree(
        "anon",
        48,
        46,
        Some("anon 42949672960\nshmem 0\ninactive_file 214748364\nslab_reclaimable 107374182\n"),
    );
    let anonymous = blastit::mem_usage::read(&dir).expect("a readable tree");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        anonymous.used(),
        46 * GIB - (214_748_364 + 107_374_182),
        "46 GiB less the 307.2 MiB the cgroup reports as reclaimable"
    );
    let message = blastit::mem_usage::boot_refusal(&anonymous, 85.0)
        .expect("a genuinely high reading refuses");
    assert!(message.contains("BLASTIT_MEM_HIGH_WATERMARK"), "{message}");
    assert!(message.contains("95.2%"), "the reading: {message}");
    assert!(message.contains("503"), "the symptom: {message}");

    // A cgroup that cannot report the split keeps the raw figure rather than
    // guessing what is reclaimable — and one with no cgroup at all keeps
    // `MemTotal − MemAvailable`, which already excludes the cache (subtracting
    // again would count it twice).
    let dir = memory_tree("nostat", 48, 46, None);
    let raw = blastit::mem_usage::read(&dir).expect("a readable tree");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!((raw.reclaimable(), raw.used()), (0, 46 * GIB));

    let dir = memory_tree("nocgroup", 48, 10, Some("inactive_file 6442450944\n"));
    fs::remove_file(dir.join("sys/fs/cgroup/memory.current")).expect("drop memory.current");
    fs::remove_file(dir.join("sys/fs/cgroup/memory.max")).expect("drop memory.max");
    let meminfo = blastit::mem_usage::read(&dir).expect("a readable tree");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        (meminfo.reclaimable(), meminfo.used()),
        (0, 10 * GIB),
        "MemTotal − MemAvailable (48 − 38 GiB), with the stale memory.stat \
         subtracted from nothing: MemAvailable already excludes it"
    );
}

/// A synthetic machine for [`blastit::mem_usage`]: `proc/meminfo`, a cgroup v2
/// directory, and optionally its `memory.stat`.
///
/// The watermark arithmetic is pinned against these figures, not against the
/// host that runs the suite: a runner with a warm page cache is exactly the
/// case in question, and a runner without one must not be able to make the
/// assertions vacuous.
fn memory_tree(tag: &str, total_gib: u64, current_gib: u64, stat: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "blastit-mem-{}-{tag}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("proc")).expect("a fixture proc");
    fs::create_dir_all(dir.join("sys/fs/cgroup")).expect("a fixture cgroup");
    fs::write(
        dir.join("proc/meminfo"),
        format!(
            "MemTotal:       {} kB\nMemAvailable:   {} kB\n",
            total_gib << 20,
            total_gib.saturating_sub(current_gib) << 20
        ),
    )
    .expect("write meminfo");
    fs::write(
        dir.join("sys/fs/cgroup/memory.max"),
        format!("{}\n", total_gib * GIB),
    )
    .expect("write memory.max");
    fs::write(
        dir.join("sys/fs/cgroup/memory.current"),
        format!("{}\n", current_gib * GIB),
    )
    .expect("write memory.current");
    if let Some(stat) = stat {
        fs::write(dir.join("sys/fs/cgroup/memory.stat"), stat).expect("write memory.stat");
    }
    dir
}

#[test]
fn a_host_whose_page_cache_is_above_the_default_still_boots_and_serves() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n";

    // The documented default, named explicitly so the leg does not follow the
    // shipped constant: on this host `memory.current` is far above 85 % while
    // the reading the watermark uses is below it, so a boot that counted the
    // page cache would refuse here and the view below would never be forked.
    // The served `uid` is the proof that it read the non-reclaimable figure.
    let reading =
        blastit::mem_usage::read(std::path::Path::new("/")).expect("this host's memory reads");
    let raw_percent = reading.raw() as f64 / reading.total() as f64 * 100.0;

    let mut dev = Dev::start(
        "mem-cache",
        &manifest(routes),
        &[("DATABASE_URL", &url), ("BLASTIT_MEM_HIGH_WATERMARK", "85")],
        &[],
    );
    dev.wait_for_ready();
    assert_eq!(
        dev.get_json("/")["uid"].as_u64(),
        Some(u64::from(rustix::process::geteuid().as_raw())),
        "a view served the first request at the 85% default: the reading is \
         {:.1}% of {:.1} GiB, and `memory.current` alone is {raw_percent:.1}%:\n{}",
        reading.percent(),
        reading.total() as f64 / GIB as f64,
        dev.stdout_text()
    );
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

#[test]
fn the_dev_memory_advisory_is_one_line_and_only_above_the_default() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n";

    // A 100% watermark always boots, so the transcript shows exactly what the
    // boot says about the reading the watermark *default* is compared with.
    let reading =
        blastit::mem_usage::read(std::path::Path::new("/")).expect("this host's memory reads");
    let mut dev = Dev::start(
        "mem-advisory",
        &manifest(routes),
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_MEM_HIGH_WATERMARK", "100"),
        ],
        &[],
    );
    dev.wait_for_ready();
    let transcript = dev.stdout_text();
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());

    let advised = transcript
        .lines()
        .filter(|line| line.contains("WARN") && line.contains("BLASTIT_MEM_HIGH_WATERMARK"))
        .count();
    match blastit::mem_usage::advisory(&reading, 100.0) {
        Some(message) => {
            assert_eq!(advised, 1, "one WARN line, not {advised}:\n{transcript}");
            assert!(
                transcript.contains(&message),
                "the WARN names the setting, the reading and the cache it left out:\n{transcript}"
            );
        }
        None => assert_eq!(
            advised,
            0,
            "a boot at or below the 85% default says nothing about the watermark \
             (measured {:.1}%):\n{transcript}",
            reading.percent()
        ),
    }
}

#[test]
fn unprivileged_production_has_no_single_uid_fallback() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {}\n";
    let dir = fixture("production", &manifest(routes));
    let output = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .env_clear()
        .env("DATABASE_URL", &url)
        .current_dir(&dir)
        .output()
        .expect("the binary runs");
    let _ = fs::remove_dir_all(&dir);
    // `R17`: production fails closed. The suite runs unprivileged, so the
    // capability detection is what refuses.
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("blastit dev"),
        "the refusal names the local command: {combined}"
    );
    assert!(
        !combined.contains(r#""privilege":"unprivileged-dev""#),
        "production never becomes dev: {combined}"
    );
    assert!(
        !combined.contains(r#""event":"ready""#),
        "production never served: {combined}"
    );
}

#[test]
fn only_a_granted_route_gets_a_pg_socket() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view granted regex '/granted/.*' {
    postgres {
        query 'SELECT 1'
    }
}

view ungranted regex '/ungranted/.*' {}
";
    let mut dev = Dev::start("sockets", &manifest(routes), &[("DATABASE_URL", &url)], &[]);
    dev.wait_for_ready();

    // The granted route reaches PostgreSQL.
    assert_eq!(dev.get_json("/granted/sql")["ok"], true);
    // The ungranted one has no socket at all: a connection gets `ENOENT`.
    let refused = dev.get_json("/ungranted/sql");
    assert_eq!(
        refused["errno"].as_i64(),
        Some(i64::from(libc::ENOENT)),
        "{refused}\n{}",
        dev.stdout_text()
    );

    let root = dev.root();
    let granted = root.join("views/0/.s.PGSQL.5432");
    let ungranted = root.join("views/1/.s.PGSQL.5432");
    let metadata = fs::metadata(&granted).expect("the granted route's socket");
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        0o600,
        "the socket is 0600"
    );
    let route_dir = fs::metadata(root.join("views/1")).expect("the route directory");
    assert_eq!(
        route_dir.permissions().mode() & 0o777,
        0o700,
        "each route directory is private"
    );
    assert_eq!(
        fs::metadata(&ungranted).err().map(|err| err.kind()),
        Some(std::io::ErrorKind::NotFound),
        "an ungranted route must have no socket: {ungranted:?}"
    );

    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

#[test]
fn an_empty_grant_gets_a_denying_listener() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view locked regex '/locked/.*' {
    postgres {}
}

view open regex '/open/.*' {
    postgres {
        query 'SELECT 1'
    }
}
";
    let mut dev = Dev::start(
        "denying-listener",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &[],
    );
    dev.wait_for_ready();

    // `docs/local.md`: "`queries = []` gives it a listener that denies every
    // statement". The app connects and reaches PostgreSQL's permission error,
    // not a missing socket.
    let denied = dev.get_json("/locked/state");
    assert_eq!(
        denied["sqlstate"].as_str(),
        Some("42501"),
        "an empty grant denies with SQLSTATE 42501: {denied}\n{}",
        dev.stdout_text()
    );
    // Allowed twin: the same statement passes on the granted route.
    assert_eq!(
        dev.get_json("/open/state")["ok"],
        true,
        "the granted route still passes `SELECT 1`:\n{}",
        dev.stdout_text()
    );

    let root = dev.root();
    let locked = root.join("views/0/.s.PGSQL.5432");
    let metadata = fs::metadata(&locked).expect("the empty grant's socket exists");
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        0o600,
        "the denying listener is 0600"
    );

    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

#[test]
fn an_egress_false_view_cannot_open_inet() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view closed regex '/closed/.*' {}

view open regex '/open/.*' {
    egress any
}
";
    let mut dev = Dev::start("egress", &manifest(routes), &[("DATABASE_URL", &url)], &[]);
    dev.wait_for_ready();

    let closed = dev.get_json("/closed/inet");
    let errno = closed["errno"].as_i64().unwrap_or(0);
    assert!(
        errno == i64::from(libc::EPERM) || errno == i64::from(libc::ENOSYS),
        "an egress = false view must get EPERM or ENOSYS, got {closed}\n{}",
        dev.stdout_text()
    );
    let open = dev.get_json("/open/inet");
    assert_eq!(
        open["ok"],
        true,
        "an egress = true view may create AF_INET sockets: {open}\n{}",
        dev.stdout_text()
    );

    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
}

/// Every `/tmp/blastit-dev-<uid>-<pid>-*` entry left behind for `pid`.
fn roots_for_pid(pid: u32) -> Vec<PathBuf> {
    let prefix = format!("blastit-dev-{}-{pid}-", rustix::process::geteuid().as_raw());
    fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect()
}

/// Asserts that `value` and every nested JSON object inside it carry no `port`
/// key (`R22`, `ARCHITECTURE.md` §8.1).
fn assert_no_port_key(value: &serde_json::Value, raw_line: &str) {
    match value {
        serde_json::Value::Object(map) => {
            assert!(
                !map.contains_key("port"),
                "a log event must never carry a `port` key: {raw_line}"
            );
            for child in map.values() {
                assert_no_port_key(child, raw_line);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_no_port_key(item, raw_line);
            }
        }
        _ => {}
    }
}

#[test]
fn fatal_startup_removes_the_ephemeral_dev_root() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view home regex '/.*' {
    postgres {
        query 'SELECT 1'
    }
}
";
    let manifest_text = manifest(routes);

    // 1. Post-adopt `fail` path: occupied `--port` fails `bind_tcp` in stage 1
    // after stage 0 created the root and handed it across `execve` (`R20`).
    let occupied = TcpListener::bind(("127.0.0.1", 0)).expect("bind occupied port");
    let occupied_port = occupied.local_addr().expect("bound port").port();
    let dir = fixture("occupied-port", &manifest_text);
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &occupied_port.to_string()])
        .env_clear()
        .env("DATABASE_URL", &url)
        // This leg is about stage 1's `bind_tcp` failure, so it has to reach
        // stage 1 on a host whose memory would otherwise refuse the boot.
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    copy_runtime_env(&mut command);
    let child = command.spawn().expect("blastit dev spawns");
    let pid = child.id();
    let output = child.wait_with_output().expect("blastit dev exits");
    drop(occupied);
    let _ = fs::remove_dir_all(&dir);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "occupied port must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("privilege=unprivileged-dev"),
        "stage 0 created the root and emitted boot before stage 1 failed:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!("binding 127.0.0.1:{occupied_port}")),
        "stage 1 must report the deliberately occupied bind address \
         `binding 127.0.0.1:{occupied_port}: Address already in use` (this leg \
         holds the listener itself, so a different failure would not be the \
         `bind_tcp` path it tests):\n{stdout}\n{stderr}"
    );
    let leaked = roots_for_pid(pid);
    assert!(
        leaked.is_empty(),
        "occupied-port failure leaked runtime root(s) for pid {pid}: {leaked:?}"
    );

    // 2. Post-adopt `python_fail` path: a broken `BLASTIT_APP` fails inside
    // CPython import after `http` and `scheduler` were forked (`R20`).
    //
    // The launch retries a lost ephemeral port (`run_dev_to_exit`), so the
    // output is the failed import — never an unrelated `EADDRINUSE`.
    let (pid, dir, output) = run_dev_to_exit(
        "bad-import",
        &manifest_text,
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_APP", "missing_app_module:application"),
        ],
    );
    let _ = fs::remove_dir_all(&dir);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "the failed import must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("BLASTIT_APP (missing_app_module:application)"),
        "stage 1 must report the failed `BLASTIT_APP` import — this leg tests the \
         python_fail path after `http`/`scheduler` were forked, not `bind_tcp`; an \
         `Address already in use` here would mean the port was taken even after the \
         retry, not that the import was never attempted:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let leaked = roots_for_pid(pid);
    assert!(
        leaked.is_empty(),
        "python_fail leaked runtime root(s) for pid {pid}: {leaked:?}"
    );
}

#[test]
fn json_mode_emits_privilege_and_never_a_port_key() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view home regex '/.*' {
    postgres {
        query 'SELECT 1'
    }
}
";
    let mut dev = Dev::start(
        "json-no-port",
        &manifest(routes),
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_LOG_FORMAT", "json"),
            ("BLASTIT_DEV_EXPOSE", "1"),
        ],
        &["--host", "0.0.0.0"],
    );
    dev.wait_for_ready();
    assert_eq!(dev.get_json("/sql")["ok"], true);
    dev.wait_for_line("\"path\":\"/sql\"");

    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());

    let lines = dev.stdout_lines();
    assert!(!lines.is_empty(), "JSON mode emitted events");
    let mut saw_stage0_boot = false;
    let mut saw_preload_boot = false;
    let mut saw_ready = false;
    for line in &lines {
        let event: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|err| panic!("every stdout line in json mode is JSON ({err}): {line}"));
        assert!(event.is_object(), "each event line is an object: {line}");
        assert_no_port_key(&event, line);
        match (
            event.get("event").and_then(serde_json::Value::as_str),
            event.get("stage").and_then(serde_json::Value::as_str),
        ) {
            (Some("boot"), Some("stage0")) => {
                assert_eq!(event["privilege"], "unprivileged-dev", "{line}");
                saw_stage0_boot = true;
            }
            (Some("boot"), Some("preload")) => {
                assert_eq!(event["privilege"], "unprivileged-dev", "{line}");
                assert_eq!(event["build"], env!("CARGO_PKG_VERSION"), "{line}");
                saw_preload_boot = true;
            }
            (Some("ready"), Some("serving")) => {
                saw_ready = true;
            }
            _ => {}
        }
    }
    assert!(
        saw_stage0_boot,
        "missing stage0 boot:\n{}",
        dev.stdout_text()
    );
    assert!(
        saw_preload_boot,
        "missing preload boot:\n{}",
        dev.stdout_text()
    );
    assert!(saw_ready, "missing ready event:\n{}", dev.stdout_text());
}

#[test]
fn learn_writes_routes_and_queries_on_sigterm() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view home \"/\" {
    postgres {
        query 'SELECT 42'
    }
}

view json \"/json\" {
    body json
}

view sql \"/sql\" {}
";
    let mut dev = Dev::start(
        "learn",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &["--learn"],
    );
    dev.wait_for_ready();

    // `R12` and `R14` stay enforced under `--learn`: an undeclared body is
    // still rejected with `413` (never learned into `[route.ingress]`), and a
    // declared `json` codec still rejects malformed JSON with `400`.
    let undeclared_body = dev.request("POST", "/sql", Some(("text/plain", "hello")));
    assert_eq!(
        undeclared_body.status,
        413,
        "undeclared body must stay 413 under --learn:\n{}",
        dev.stdout_text()
    );
    let bad_json = dev.request("POST", "/json", Some(("application/json", "{bad")));
    assert_eq!(
        bad_json.status,
        400,
        "malformed JSON on a declared json route must stay 400 under --learn:\n{}",
        dev.stdout_text()
    );

    // A declared route that is not granted: `--learn` allows and records it.
    assert_eq!(dev.get_json("/sql")["ok"], true, "{}", dev.stdout_text());
    // An unrouted request proposes a `[[route]]`.
    assert_eq!(
        dev.get_json("/extra/7")["ok"],
        true,
        "{}",
        dev.stdout_text()
    );
    // The scheduler's `request` event is what names an unrouted request; wait
    // for both to reach init before `SIGTERM`, which is also when the
    // supervisor stops reading.
    dev.wait_for_line("/extra/7");
    dev.wait_for_line("/sql");

    let manifest_path = dev.dir.join("Blastfile");
    let status = dev.terminate();
    assert_eq!(
        status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        dev.stdout_text(),
        dev.stderr_text()
    );
    let learned = fs::read_to_string(&manifest_path).expect("the manifest is readable");
    assert!(
        learned.contains("view extra "),
        "the unrouted view was proposed:\n{learned}"
    );
    assert!(
        learned.contains("\"/extra/{id:int}\""),
        "the numeric segment became a pattern placeholder:\n{learned}"
    );
    assert!(
        learned.contains("SELECT 1"),
        "the routed statement was written:\n{learned}"
    );
    let sql_view = view_block(&learned, "sql");
    assert!(
        sql_view.contains("SELECT 1"),
        "the statement belongs to the view that ran it:\n{learned}"
    );
    assert!(
        !sql_view.contains("body"),
        "`--learn` never synthesises a `body` codec from a rejected 413:\n{learned}"
    );
    // The learn fallback's own view is `home`; its observations belong to an
    // unrouted request, so they are not written as a grant for `home`.
    let home = view_block(&learned, "home");
    assert!(
        !home.contains("SELECT 1"),
        "an unrouted request's statement must not become a view grant:\n{learned}"
    );
    // One pass writes the newly discovered view *and* its statement (the
    // route's own socket is `_learn_fallback`, and with one proposed view the
    // attribution is exact), so no second `dev --learn` is needed.
    let extra = view_block(&learned, "extra");
    assert!(
        extra.contains("SELECT 1"),
        "the view this pass discovered keeps the statement it ran:\n{learned}"
    );
    dev.wait_for_line("learn: ");
    // Each count is spelled in its own number (`1 route`, `2 routes`), and
    // `docs/local.md` shows `learn: 1 route, 1 query added` — so this pin is
    // the singular-and-plural sentence, not the old invariant `routes`.
    assert!(
        dev.stdout_text()
            .contains("learn: 1 route, 2 queries added"),
        "the summary counts the routed and the discovered statement:\n{}",
        dev.stdout_text()
    );
}

#[test]
fn learn_refuses_a_literal_on_a_live_request() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view home \"/\" {
    postgres {
        query 'SELECT 1'
    }
}

view literal \"/literal\" {}
";
    let mut dev = Dev::start(
        "learn-literal",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &["--learn"],
    );
    dev.wait_for_ready();

    // The literal-bearing statement is allowed in learn mode (R26) so the app
    // finishes, then refused instead of written (R25).
    let response = dev.request("GET", "/literal", None);
    assert_eq!(
        response.status,
        200,
        "the literal statement ran under --learn: {}\n{}",
        response.body,
        dev.stdout_text()
    );
    let literal: serde_json::Value = serde_json::from_str(&response.body)
        .unwrap_or_else(|err| panic!("{err}: {}", response.body));
    assert_eq!(
        literal["ok"],
        true,
        "the literal statement ran under --learn: {literal}\n{}",
        dev.stdout_text()
    );
    // A request on another declared route is unaffected by the refusal.
    assert_eq!(
        dev.get_json("/")["uid"].as_u64(),
        Some(u64::from(rustix::process::geteuid().as_raw())),
        "{}",
        dev.stdout_text()
    );
    dev.wait_for_line("/literal");

    let manifest_path = dev.dir.join("Blastfile");
    let status = dev.terminate();
    assert_eq!(
        status.code(),
        Some(1),
        "a refused statement is exit 1:\nstdout:\n{}\nstderr:\n{}",
        dev.stdout_text(),
        dev.stderr_text()
    );
    let stderr = dev.stderr_text();
    assert!(
        stderr.contains("learn: refused"),
        "the refusal is reported: {stderr}"
    );
    assert!(
        !stderr.contains("alice"),
        "a refused literal never reaches a stream: {stderr}"
    );
    let learned = fs::read_to_string(&manifest_path).expect("the manifest is readable");
    assert!(
        !learned.contains("alice"),
        "a refused literal never reaches the manifest:\n{learned}"
    );
    assert!(
        !learned.contains("SELECT 'alice'"),
        "a literal-bearing statement is never granted:\n{learned}"
    );
}

#[test]
fn learn_attributes_queries_per_route_across_concurrent_live_views() {
    use blastit_manifest::{Grant, Manifest, ProxyName};

    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view a regex '/a/.*' {}

view b \"/b\" {}
";
    let mut dev = Dev::start(
        "learn-multi-view",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &["--learn"],
    );
    dev.wait_for_ready();

    // 1. Route `a` spawns its view and runs `SELECT 111`.
    let a1 = dev.get_json("/a/1");
    assert_eq!(a1["ok"], true, "{a1}\n{}", dev.stdout_text());
    let pid_a = a1["pid"].as_i64().expect("route a view pid") as i32;

    // 2. Route `b` spawns a second view while route `a`'s view stays live, and
    // runs `SELECT 222`.
    let b = dev.get_json("/b");
    assert_eq!(b["ok"], true, "{b}\n{}", dev.stdout_text());
    let pid_b = b["pid"].as_i64().expect("route b view pid") as i32;
    assert_ne!(pid_a, pid_b, "each route runs in its own view process");
    let live = dev.children();
    assert!(
        live.contains(&pid_a) && live.contains(&pid_b),
        "both views are alive at once: pid_a={pid_a}, pid_b={pid_b}, children={live:?}"
    );

    // 3. Route `a` runs `SELECT 333` *after* route `b`'s view started, on its
    // still-live view. Attribution must come from `a`'s per-route listener and
    // never drift onto `b` (`R27`).
    let a2 = dev.get_json("/a/2");
    assert_eq!(a2["ok"], true, "{a2}\n{}", dev.stdout_text());
    assert_eq!(
        a2["pid"].as_i64(),
        Some(i64::from(pid_a)),
        "the warm view of route a served /a/2 while route b was live"
    );

    dev.wait_for_line("/a/2");
    let manifest_path = dev.dir.join("Blastfile");
    let status = dev.terminate();
    assert_eq!(
        status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        dev.stdout_text(),
        dev.stderr_text()
    );

    let learned = fs::read_to_string(&manifest_path).expect("the manifest is readable");
    let (parsed, _) =
        Manifest::parse_blastfile(&learned, 20_000).expect("the learned manifest parses");
    let proxy = ProxyName::new("db");
    let queries_a: Vec<String> = parsed
        .effective_grants(0)
        .get(&proxy)
        .and_then(Grant::as_postgresql)
        .expect("route a received a postgresql grant")
        .queries()
        .to_vec();
    let queries_b: Vec<String> = parsed
        .effective_grants(1)
        .get(&proxy)
        .and_then(Grant::as_postgresql)
        .expect("route b received a postgresql grant")
        .queries()
        .to_vec();

    assert_eq!(
        queries_a,
        vec!["SELECT 111", "SELECT 333"],
        "route a keeps both of its statements even after route b spawned:\n{learned}"
    );
    assert_eq!(
        queries_b,
        vec!["SELECT 222"],
        "route b never receives route a's statements:\n{learned}"
    );
    dev.wait_for_line("learn: ");
    assert!(
        dev.stdout_text()
            .contains("learn: 0 routes, 3 queries added"),
        "the summary counted all three queries:\n{}",
        dev.stdout_text()
    );
}

#[test]
fn first_declared_route_keeps_all_queries_when_unrouted_requests_exist() {
    use blastit_manifest::{Grant, Manifest, ProxyName};

    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "\
view a regex '/a/.*' {}

view b \"/b\" {}
";
    let mut dev = Dev::start(
        "learn-first-route-with-unrouted",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &["--learn"],
    );
    dev.wait_for_ready();

    // Exact reviewer reproduction + unrouted SQL check:
    // 1. First declared route `a` runs `SELECT 111`.
    // 2. Unrouted `/zzz/9` runs no SQL (proposes `zzz` `/zzz/[0-9]+`).
    // 3. Second declared route `b` runs `SELECT 222`.
    // 4. First declared route `a` runs `SELECT 333`.
    // 5. Unrouted `/extra/7` runs `SELECT 1` on the fallback view (proposes
    //    `extra` `/extra/[0-9]+`, must not leak `SELECT 1` onto `a` or `b`).
    assert_eq!(dev.get_json("/a/1")["ok"], true, "{}", dev.stdout_text());
    assert_eq!(dev.get_json("/zzz/9")["ok"], true, "{}", dev.stdout_text());
    assert_eq!(dev.get_json("/b")["ok"], true, "{}", dev.stdout_text());
    assert_eq!(dev.get_json("/a/2")["ok"], true, "{}", dev.stdout_text());
    assert_eq!(
        dev.get_json("/extra/7")["ok"],
        true,
        "{}",
        dev.stdout_text()
    );

    dev.wait_for_line("/zzz/9");
    dev.wait_for_line("/extra/7");
    dev.wait_for_line("/a/2");

    let manifest_path = dev.dir.join("Blastfile");
    let status = dev.terminate();
    assert_eq!(
        status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        dev.stdout_text(),
        dev.stderr_text()
    );

    let learned = fs::read_to_string(&manifest_path).expect("the manifest is readable");
    let (parsed, _) =
        Manifest::parse_blastfile(&learned, 20_000).expect("the learned manifest parses");
    let route_names: Vec<&str> = parsed.routes().iter().map(|r| r.name()).collect();
    assert_eq!(
        route_names,
        vec!["a", "b", "extra", "zzz"],
        "only the truly-unrouted requests contribute inferred routes:\n{learned}"
    );

    let proxy = ProxyName::new("db");
    let queries_a: Vec<String> = parsed
        .effective_grants(0)
        .get(&proxy)
        .and_then(Grant::as_postgresql)
        .expect("first declared route `a` must keep its postgresql grant")
        .queries()
        .to_vec();
    let queries_b: Vec<String> = parsed
        .effective_grants(1)
        .get(&proxy)
        .and_then(Grant::as_postgresql)
        .expect("route `b` received its postgresql grant")
        .queries()
        .to_vec();

    assert_eq!(
        queries_a,
        vec!["SELECT 111", "SELECT 333"],
        "first declared route `a` keeps ALL of its statements and none of the fallback view's:\n{learned}"
    );
    assert_eq!(
        queries_b,
        vec!["SELECT 222"],
        "route `b` keeps its exact statement:\n{learned}"
    );
    dev.wait_for_line("learn: ");
    assert!(
        dev.stdout_text()
            .contains("learn: 2 routes, 3 queries added"),
        "summary reports 2 inferred routes and all 3 routed queries:\n{}",
        dev.stdout_text()
    );
}

#[test]
fn only_a_lost_bind_is_classified_as_a_lost_port() {
    // Stage 1's failed bind is printed on stdout as an `app.output` event and
    // contains the word `already`, which a substring match for `ready` used to
    // accept: a launch that never bound looked serving.
    let bind_line = "21:07:36.914 ERROR app.output  stderr  \
                     binding 127.0.0.1:39411: Address already in use (os error 98)";
    assert!(!is_ready_event(bind_line));
    assert!(is_ready_event(
        "21:07:36.899 INFO  ready        serving  routes=3"
    ));
    assert!(is_ready_event(
        "{\"ts\":\"2026-11-12T21:07:36Z\",\"event\":\"ready\",\"stage\":\"serving\"}"
    ));

    // The exact stage-1 line an `EADDRINUSE` on the probe port produces, in
    // both the human and the JSON log format.
    assert!(lost_the_port(
        39411,
        "{\"event\":\"fail\",\"msg\":\"binding 127.0.0.1:39411: Address already in use (os error 98)\"}",
        ""
    ));
    assert!(lost_the_port(
        39411,
        "",
        "binding 127.0.0.1:39411: Address already in use (os error 98)"
    ));
    // The `EXPOSED` cases pass `--host 0.0.0.0`, so the host in the message is
    // not always `127.0.0.1`; the port is what identifies the lost race.
    assert!(lost_the_port(
        34833,
        "{\"line\":\"binding 0.0.0.0:34833: Address already in use (os error 98)\"}",
        ""
    ));
    // A different port's `EADDRINUSE`, a different bind error, and the failure
    // this suite's python_fail leg is about must all stay visible: only a race
    // on the exact port that was handed out is retried.
    assert!(!lost_the_port(
        39411,
        "binding 127.0.0.1:39412: Address already in use (os error 98)",
        ""
    ));
    assert!(!lost_the_port(
        39411,
        "binding 127.0.0.1:39411: Permission denied (os error 13)",
        ""
    ));
    assert!(!lost_the_port(
        39411,
        "BLASTIT_APP (missing_app_module:application)",
        ""
    ));
}

#[test]
fn a_launch_that_loses_its_port_retries_with_a_fresh_one() {
    let server = PgUpstream::start();
    let url = database_url(&server);
    let routes = "view home regex '/.*' {}\n";

    // This listener stands in for the unrelated process that wins the race
    // between `free_port()` and the child's `bind`.
    let held = TcpListener::bind(("127.0.0.1", 0)).expect("a held port");
    let held_port = held.local_addr().expect("a bound address").port();
    // The first attempt is handed the held port; every later attempt (the
    // retry, and any further loss under load) gets a genuinely free one.
    let mut first = Some(held_port);
    let ports = move || first.take().unwrap_or_else(free_port);

    // The first attempt (the held port) must fail with stage 1's `EADDRINUSE`
    // and be retried; the second (a real free port) must serve.
    let mut dev = Dev::start_with(
        "lost-port",
        &manifest(routes),
        &[("DATABASE_URL", &url)],
        &[],
        ports,
    );
    assert_ne!(
        dev.port, held_port,
        "the launch moved to a fresh port after the first bind failed"
    );
    dev.wait_for_ready();
    assert_eq!(
        dev.get_json("/env")["uid"].as_u64(),
        Some(u64::from(rustix::process::geteuid().as_raw())),
        "the retried launch serves for real"
    );
    let status = dev.terminate();
    assert_eq!(status.code(), Some(0), "{}", dev.stderr_text());
    drop(held);
}
