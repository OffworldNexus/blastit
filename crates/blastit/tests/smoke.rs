//! Smoke tests: the real `blastit` binary, as root, over HTTP.
//!
//! Every test starts `cargo`'s built binary with the environment the platform
//! would give it (`PORT`, `BLASTIT_APP`, `BLASTIT_MANIFEST`, `PYTHONPATH`, a
//! per-test `BLASTIT_UID_BASE`), talks HTTP over a `TcpStream`, and checks both
//! the answer and the event stream on stdout.
//!
//! init has to be PID 1's uid — root — to bind the view sockets, drop
//! privileges and fork, so a test that is not root prints `skipped: needs root`
//! and passes. The `smoke (root)` CI job runs this file with `sudo`.
//!
//! The PostgreSQL checks only run when `BLASTIT_TEST_PG_URL` is set (the CI job
//! provides one as a service container). They speak the PostgreSQL wire
//! protocol by hand over a Unix socket, so the tests need no Python packages
//! beyond the standard library.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::Permissions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a test waits for the server to be ready.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a test waits for a process to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Counter for unique temporary directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Skips a test that cannot run without root, which is how `cargo test` behaves
/// for a developer who is not root.
fn needs_root(name: &str) -> bool {
    // SAFETY: `getuid` takes no arguments and cannot fail.
    if unsafe { libc::getuid() } == 0 {
        return true;
    }
    println!("{name}: skipped: needs root");
    false
}

/// The PostgreSQL URL the CI job provides, if there is one.
fn test_pg_url() -> Option<String> {
    std::env::var("BLASTIT_TEST_PG_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

/// The password inside a `postgresql://user:password@host/db` URL.
fn password_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let before_host = rest.split('@').next()?;
    let (_, password) = before_host.split_once(':')?;
    if password.is_empty() {
        None
    } else {
        Some(password.to_string())
    }
}

/// A world-readable temporary directory of code and manifests.
struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// Creates `/tmp/blastit-smoke-<pid>-<tag>-<n>`, mode `0755` so the view
    /// uids can read the code (§7.2).
    fn new(tag: &str) -> Self {
        let mut path = PathBuf::from("/tmp");
        path.push(format!(
            "blastit-smoke-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        Self { path }
    }

    /// Writes a file the view uids may read (`0644`).
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        path
    }

    fn python_path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A running `blastit`, with its event stream collected as it goes.
struct Server {
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    _fixture: Fixture,
}

impl Server {
    /// Starts the binary and waits until it reports `ready` and answers HTTP.
    fn start(
        tag: &str,
        uid_base: u32,
        manifest: &str,
        app: &str,
        python: &[(&str, &str)],
        env: &[(&str, &str)],
    ) -> Self {
        let fixture = Fixture::new(tag);
        let manifest_path = fixture.write("blastit.toml", manifest);
        for (name, contents) in python {
            fixture.write(name, contents);
        }
        let port = free_port();

        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .env("PORT", port.to_string())
            .env("BLASTIT_APP", app)
            .env("BLASTIT_MANIFEST", &manifest_path)
            .env("PYTHONPATH", fixture.python_path())
            .env("BLASTIT_UID_BASE", uid_base.to_string())
            .env("BLASTIT_LOG_FORMAT", "json")
            .env("BLASTIT_LOG_LEVEL", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("the blastit binary starts");

        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, Arc::clone(&stdout));
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, Arc::clone(&stderr));
        }

        let mut server = Self {
            child,
            port,
            stdout,
            stderr,
            _fixture: fixture,
        };
        server.wait_for_ready();
        server
    }

    /// Waits for the `ready` event and for the port to answer.
    fn wait_for_ready(&mut self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self
                .stdout_lines()
                .iter()
                .any(|line| line.contains("\"ready\""))
            {
                return;
            }
            if let Some(status) = self.exited() {
                panic!(
                    "blastit exited with {status} before it was ready\nstdout:\n{}\nstderr:\n{}",
                    self.stdout_text(),
                    self.stderr_text()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "blastit was not ready within {READY_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// Every complete line stdout has produced so far.
    fn stdout_lines(&self) -> Vec<String> {
        self.stdout.lock().unwrap().clone()
    }

    fn stdout_text(&self) -> String {
        self.stdout_lines().join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().unwrap().join("\n")
    }

    /// Waits until stdout holds a line containing `needle`.
    fn wait_for_event(&self, needle: &str) -> bool {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| line.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// The exit status, if the process has already ended.
    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// One HTTP/1.1 request, answered in full.
    fn request(&self, method: &str, target: &str) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting to the server for {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let request = format!(
            "{method} {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        Response::parse(&raw)
    }

    /// `GET` with a body of assertions about the answer.
    fn get(&self, target: &str) -> Response {
        self.request("GET", target)
    }

    /// Sends a signal to the process.
    fn signal(&self, signal: i32) {
        // SAFETY: the pid came from `spawn`, so it is a live child of this
        // process and `kill` takes two integers.
        let killed = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
    }

    /// Waits for the process to exit, returning its status.
    fn wait_for_exit(&mut self) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A parsed HTTP response.
struct Response {
    status: u16,
    body: String,
}

impl Response {
    fn parse(raw: &[u8]) -> Self {
        let text = String::from_utf8_lossy(raw);
        let (head, body) = match text.split_once("\r\n\r\n") {
            Some((head, body)) => (head, body.to_string()),
            None => (text.as_ref(), String::new()),
        };
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or_else(|| panic!("no status line in {:?}", &text[..text.len().min(200)]));
        Self { status, body }
    }
}

/// Reads a child's pipe into a shared list, one line at a time.
fn spawn_reader(pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(pipe);
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => return,
                Ok(_) => {
                    sink.lock()
                        .unwrap()
                        .push(line.trim_end_matches('\n').to_string());
                }
                Err(_) => return,
            }
        }
    });
}

/// A port nothing is listening on yet, for the server to bind.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// The WSGI application every test that is not about the database uses.
///
/// It reports who served the request (uid, pid, route), prints one line so the
/// event stream carries an `app.output`, can exit the process in the middle of
/// a request, and can hold two concurrent requests on a barrier.
const APP: &str = r#"
import json
import os
import threading

BARRIER = threading.Barrier(2, timeout=5)


def respond(start_response, body):
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]


def application(environ, start_response):
    path = environ.get("PATH_INFO", "")
    if path == "/crash":
        os._exit(3)
    if path == "/parallel":
        BARRIER.wait()
        return respond(start_response, b"both-ran")
    if path == "/env":
        body = json.dumps({
            "home": os.environ.get("HOME"),
            "tmpdir": os.environ.get("TMPDIR"),
            "cwd": os.getcwd(),
            "blastit": sorted(k for k in os.environ if k.startswith("BLASTIT_")),
            "database_url": os.environ.get("DATABASE_URL"),
            "page_url": os.environ.get("PAGE_URL"),
            "uid": os.getuid(),
            "pid": os.getpid(),
            "gid": os.getgid(),
            "route": environ.get("blastit.route"),
            "script": environ.get("SCRIPT_NAME"),
        }).encode()
        return respond(start_response, body)
    body = ("uid=%d pid=%d path=%s" % (os.getuid(), os.getpid(), path)).encode()
    print("printed by the app", flush=True)
    return respond(start_response, body)
"#;

/// The PostgreSQL wire-protocol helper: no Python packages, by hand.
///
/// It connects to the route's own socket (`/proc/self/cwd/.s.PGSQL.<port>`),
/// sends a `StartupMessage` and one simple `Query`, and reports the reply's
/// message kinds, the first `DataRow` field, and the SQLSTATE of an
/// `ErrorResponse`.
const PG_APP: &str = r#"
import json
import os
import socket
import struct


def recv_exactly(sock, count):
    out = b""
    while len(out) < count:
        chunk = sock.recv(count - len(out))
        if not chunk:
            raise EOFError("the proxy closed the connection")
        out += chunk
    return out


def sqlstate(payload):
    index = 0
    while index < len(payload):
        kind = payload[index:index + 1]
        if kind == b"\x00":
            break
        end = payload.index(b"\x00", index + 1)
        if kind == b"C":
            return payload[index + 1:end].decode("latin-1")
        index = end + 1
    return "?"


def first_field(payload):
    count = struct.unpack("!H", payload[:2])[0]
    if count == 0:
        return ""
    size = struct.unpack("!i", payload[2:6])[0]
    if size < 0:
        return "NULL"
    return payload[6:6 + size].decode("latin-1")


def pg_query(sql, port):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(15)
    sock.connect("/proc/self/cwd/.s.PGSQL.%d" % port)
    params = b"user\x00blastit\x00database\x00blastit\x00\x00"
    body = struct.pack("!I", 196608) + params
    sock.sendall(struct.pack("!I", len(body) + 4) + body)
    # Startup: the standard reply, which ends at ReadyForQuery.
    while True:
        head = recv_exactly(sock, 5)
        payload = recv_exactly(sock, struct.unpack("!I", head[1:5])[0] - 4)
        if head[0:1] == b"Z":
            break
        if head[0:1] == b"E":
            return "startup-error " + sqlstate(payload)
    payload_sql = sql.encode("utf-8") + b"\x00"
    sock.sendall(b"Q" + struct.pack("!I", len(payload_sql) + 4) + payload_sql)
    # Read until the ReadyForQuery that closes *this* round trip, that is the
    # first one after the reply's own messages. dbproxy relays the upstream
    # session's own ReadyForQuery once, right after the startup reply, so the
    # first message can be a bare `Z`; stopping there would mistake it for the
    # answer. See the report on this step.
    kinds = []
    data = None
    content = False
    for _ in range(64):
        head = recv_exactly(sock, 5)
        payload = recv_exactly(sock, struct.unpack("!I", head[1:5])[0] - 4)
        kind = head[0:1]
        kinds.append(kind.decode("latin-1"))
        if kind in (b"T", b"D", b"C"):
            content = True
        if kind == b"D" and data is None:
            data = first_field(payload)
        elif kind == b"E":
            return "error %s kinds=%s" % (sqlstate(payload), "".join(kinds))
        elif kind == b"Z" and content:
            return "ok kinds=%s data=%s" % ("".join(kinds), data)
    return "incomplete kinds=%s" % "".join(kinds)


def application(environ, start_response):
    granted = pg_query("SELECT 1", 5432)
    denied = pg_query("SELECT 2", 5432)
    body = json.dumps({
        "granted": granted,
        "denied": denied,
        "database_url": os.environ.get("DATABASE_URL"),
        "uid": os.getuid(),
    }).encode()
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The manifest for the routing tests: two routes with different uids.
const TWO_ROUTES: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one']

[[route]]
name = "two"
path = ['/two/.*']
"#;

/// The manifest for the concurrency test: `/parallel` has to reach a route.
const PARALLEL_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/parallel']
"#;

/// The manifest for the crash test: `/crash` has to reach a route.
const CRASH_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/crash']
"#;

/// The manifest for the environment test: `/env` has to reach a route.
const ENV_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/env']
"#;

#[test]
fn routing_reaches_two_routes_and_each_route_has_its_own_process_and_uid() {
    if !needs_root("routing") {
        return;
    }
    let base = 40_000;
    let server = Server::start(
        "routing",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[],
    );

    let one = server.get("/one");
    assert_eq!(one.status, 200, "{}", server.stdout_text());
    assert!(
        one.body.contains(&format!("uid={} ", base + 1000)),
        "route one runs under B+1000: {}",
        one.body
    );
    let two = server.get("/two/anything");
    assert_eq!(two.status, 200);
    assert!(
        two.body.contains(&format!("uid={} ", base + 1001)),
        "route two runs under B+1001: {}",
        two.body
    );

    // Different processes, and the same one answers twice.
    let pid_of = |body: &str| {
        body.split_whitespace()
            .find_map(|field| field.strip_prefix("pid=").map(str::to_string))
            .unwrap()
    };
    assert_ne!(
        pid_of(&one.body),
        pid_of(&two.body),
        "one route, one process"
    );
    let again = server.get("/one");
    assert_eq!(pid_of(&again.body), pid_of(&one.body), "no respawn");

    // A catch-all-less manifest leaves unmatched paths to blastit.
    let missing = server.get("/nope");
    assert_eq!(missing.status, 404, "{}", server.stdout_text());

    // The health path is answered by the scheduler, without a view.
    let health = server.get("/_blastit/health");
    assert_eq!(health.status, 200);

    assert_eq!(server.get("/").status, 200, "the root route matches /");
}

#[test]
fn the_app_environment_has_no_blastit_variables_and_home_is_its_scratch_directory() {
    if !needs_root("environment") {
        return;
    }
    let base = 41_000;
    let server = Server::start(
        "environment",
        base,
        ENV_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("UNRELATED_VARIABLE", "kept")],
    );

    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();

    assert_eq!(
        value["blastit"].as_array().unwrap().len(),
        0,
        "no BLASTIT_* variable survives the rewrite (§5.3)"
    );
    assert_eq!(
        value["home"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000),
        "({})",
        server.stdout_text()
    );
    assert_eq!(
        value["tmpdir"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000)
    );
    assert_eq!(
        value["cwd"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000),
        "a view works in its own socket directory (§2.2)"
    );
    assert_eq!(value["route"].as_str().unwrap(), "one");
    assert_eq!(value["gid"].as_u64().unwrap(), u64::from(base + 1000));
}

#[test]
fn a_declared_proxy_boots_dbproxy_and_replaces_the_credential_with_a_local_endpoint() {
    if !needs_root("proxy") {
        return;
    }
    let base = 42_000;
    let password = "super-secret-not-for-the-app";
    let url =
        format!("postgresql://doadmin:{password}@db.internal:25060/defaultdb?sslmode=require");
    let manifest = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

[[route]]
name = "one"
path = ['/.*']
[route.db]
queries = ["SELECT 1"]
"#;
    let server = Server::start(
        "proxy",
        base,
        manifest,
        "app:application",
        &[("app.py", APP)],
        &[
            ("DATABASE_URL", &url),
            ("DATABASE_CA_CERT", "-----BEGIN CERTIFICATE-----"),
        ],
    );

    // dbproxy was forked, waited for, and the boot got past it.
    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let database_url = value["database_url"].as_str().expect("DATABASE_URL is set");
    assert!(
        database_url.contains("/proc/self/cwd") || database_url.contains("%2Fproc%2Fself%2Fcwd"),
        "the app is pointed at its own socket: {database_url}"
    );
    assert!(
        !database_url.contains(password),
        "the rendered URL carries no credential: {database_url}"
    );
    assert_eq!(value["blastit"].as_array().unwrap().len(), 0);

    // The listeners init bound belong to the view's uid, mode 0600 (§2.2).
    let socket = PathBuf::from(format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1000));
    let meta = std::fs::metadata(&socket).expect("the pg socket exists");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    use std::os::unix::fs::MetadataExt;
    assert_eq!(meta.uid(), base + 1000, "owned by the view uid");

    // No secret reaches the log: the password is in dbproxy's configuration
    // pipe, not in an event (§4).
    assert!(
        !server.stdout_text().contains(password),
        "the password reached stdout:\n{}",
        server.stdout_text()
    );
    assert!(!server.stderr_text().contains(password));
}

#[test]
fn a_view_that_crashes_gives_502_and_the_next_request_respawns_it() {
    if !needs_root("crash") {
        return;
    }
    let base = 43_000;
    let server = Server::start(
        "crash",
        base,
        CRASH_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );

    let first = server.get("/one");
    assert_eq!(first.status, 200);

    let crash = server.get("/crash");
    assert_eq!(crash.status, 502, "{}", server.stdout_text());
    assert!(server.wait_for_event("\"event\":\"process.exited\""));

    // The scheduler asks for a new process, and the route works again.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut recovered = false;
    while Instant::now() < deadline {
        let response = server.get("/one");
        if response.status == 200 {
            recovered = true;
            break;
        }
        assert_eq!(response.status, 502, "{}", server.stdout_text());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(recovered, "the route respawned\n{}", server.stdout_text());
}

#[test]
fn concurrent_requests_to_one_route_run_in_parallel_threads() {
    if !needs_root("parallel") {
        return;
    }
    let base = 44_000;
    let server = Server::start(
        "parallel",
        base,
        PARALLEL_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    // Warm the view up so the fork does not race the barrier.
    assert_eq!(server.get("/one").status, 200);

    let port = server.port;
    let handle = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream
            .write_all(b"GET /parallel HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        Response::parse(&raw)
    });
    let other = server.get("/parallel");
    let first = handle.join().unwrap();

    assert_eq!(other.status, 200, "{}", server.stdout_text());
    assert_eq!(first.status, 200, "{}", server.stdout_text());
    assert_eq!(other.body, "both-ran");
    assert_eq!(first.body, "both-ran");
}

#[test]
fn stdout_is_one_event_per_line_and_covers_the_lifecycle() {
    if !needs_root("events") {
        return;
    }
    let base = 45_000;
    let server = Server::start(
        "events",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    assert_eq!(server.get("/one").status, 200);
    assert!(server.wait_for_event("\"event\":\"app.output\""));

    let lines = server.stdout_lines();
    assert!(lines.len() >= 5, "{}", lines.join("\n"));
    let mut events = Vec::new();
    for line in &lines {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|err| panic!("{err}: {line}"));
        assert_eq!(value["ts"].as_str().map(str::len), Some(24), "{line}");
        assert!(value["level"].is_string(), "{line}");
        assert!(value["component"].is_string(), "{line}");
        events.push(value["event"].as_str().unwrap().to_string());
    }
    for expected in ["boot", "ready", "request", "process.spawned", "app.output"] {
        assert!(events.iter().any(|event| event == expected), "{events:?}");
    }
    // Every line is at most 2048 bytes, which is what §8.1 promises.
    for line in &lines {
        assert!(line.len() <= 2048, "{} bytes: {}", line.len(), line);
    }
}

#[test]
fn sigterm_drains_and_exits_zero_within_the_graceful_timeout() {
    if !needs_root("sigterm") {
        return;
    }
    let base = 46_000;
    let mut server = Server::start(
        "sigterm",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_GRACEFUL_TIMEOUT", "2s")],
    );
    assert_eq!(server.get("/one").status, 200);

    server.signal(libc::SIGTERM);
    let status = server
        .wait_for_exit()
        .expect("the server exits within the graceful timeout");
    assert_eq!(status.code(), Some(0), "{}", server.stdout_text());
    assert!(
        server.wait_for_event("\"stage\":\"stopped\""),
        "{}",
        server.stdout_text()
    );
}

#[test]
fn a_granted_query_is_forwarded_and_an_ungranted_one_is_denied() {
    if !needs_root("postgres") {
        return;
    }
    let Some(url) = test_pg_url() else {
        println!("postgres: skipped: BLASTIT_TEST_PG_URL is not set");
        return;
    };
    let base = 47_000;
    let manifest = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "data"
path = ['/query']
[route.db]
queries = ["SELECT 1"]

[[route]]
name = "other"
path = ['/other']
"#;
    let server = Server::start(
        "postgres",
        base,
        manifest,
        "app:application",
        &[("app.py", PG_APP)],
        &[("DATABASE_URL", &url)],
    );

    let response = server.get("/query");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let granted = value["granted"]
        .as_str()
        .unwrap_or_else(|| panic!("no `granted` in {}", response.body));
    assert!(
        granted.contains("data=1"),
        "SELECT 1 returns a DataRow: {granted}"
    );
    assert!(
        granted.contains("TDC"),
        "SELECT 1 is answered with RowDescription/DataRow/CommandComplete: {granted}"
    );
    let denied = value["denied"]
        .as_str()
        .unwrap_or_else(|| panic!("no `denied` in {}", response.body));
    assert!(
        denied.starts_with("error 42501"),
        "SELECT 2 is refused with 42501: {denied}"
    );

    // The denial is an event (§8.1)…
    assert!(server.wait_for_event("\"event\":\"sql.denied\""));
    assert!(server.stdout_text().contains("\"sqlstate\":\"42501\""));

    // …the app's URL carries no password, and points at the local socket…
    let database_url = value["database_url"]
        .as_str()
        .unwrap_or_else(|| panic!("no `database_url` in {}", response.body));
    let password = password_of(&url).expect("the test URL has a password");
    assert!(
        !database_url.contains(&password),
        "no password in the app's URL: {database_url}"
    );
    assert!(
        database_url.contains("proc") && database_url.contains("5432"),
        "the app is pointed at its own socket: {database_url}"
    );
    // …and the real password is nowhere in the log.
    assert!(
        !server.stdout_text().contains(&password),
        "the password reached stdout:\n{}",
        server.stdout_text()
    );

    // Another view's uid cannot reach this route's socket (§2.2).
    let socket = format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1000);
    let errno = connect_as(base + 1001, &socket);
    assert_eq!(
        errno,
        Some(libc::EACCES),
        "another uid gets EACCES, not {errno:?}"
    );

    // The route without a grant has no socket at all.
    assert!(
        !PathBuf::from(format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1001)).exists(),
        "a route with no grant gets no listener"
    );
}

/// Forks a child that drops to `uid` and tries to `connect()` to `path`.
///
/// Returns the `errno` of the failed connection, or `None` when it succeeded.
/// The child only makes raw syscalls: it is forked from a test harness that has
/// threads, so anything that allocates could deadlock.
fn connect_as(uid: u32, path: &str) -> Option<i32> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_bytes();
    assert!(bytes.len() < address.sun_path.len(), "the path fits");
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t;

    // SAFETY: the child only calls `setresgid`, `setresuid`, `socket`,
    // `connect` and `_exit`, all of which are async-signal-safe, and it reads
    // the `address` on the parent's stack, which the fork copied.
    let fork = unsafe { blastit_sys::fork_unchecked() }.expect("fork");
    match fork {
        blastit_sys::Fork::Child => {
            if blastit_sys::drop_privileges(uid).is_err() {
                unsafe { libc::_exit(9) }
            }
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                unsafe { libc::_exit(8) }
            }
            let connected = unsafe {
                libc::connect(
                    fd,
                    std::ptr::from_ref(&address).cast::<libc::sockaddr>(),
                    length,
                )
            };
            if connected == 0 {
                unsafe { libc::_exit(1) }
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            unsafe { libc::_exit(errno.min(120) + 10) }
        }
        blastit_sys::Fork::Parent(pid) => {
            let mut status = 0;
            // SAFETY: `waitpid` writes one status word into `status`.
            let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) };
            assert_eq!(waited, pid.as_raw(), "the child was reaped");
            let code = (status >> 8) & 0xff;
            match code {
                0 => None,
                1 => Some(0),
                other if other >= 10 => Some(other - 10),
                other => panic!("the helper child failed with {other}"),
            }
        }
    }
}
