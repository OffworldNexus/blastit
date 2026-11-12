#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for `R17`, `R19..R22` and `R29` (`dev`): the
//! unprivileged single-uid supervisor, the `execve` secret barrier, the exact
//! banner, the `BLASTIT_DEV_EXPOSE` guard, the per-route socket directory, the
//! per-role seccomp `egress` decision, and `dev --learn`'s atomic write.
//!
//! Every case starts the real `blastit` binary as the calling user: no root, no
//! Docker, no `BLASTIT_TEST_PG_URL`. `MockPgServer` (in `common/mock_pg.rs`)
//! stands in for PostgreSQL on `127.0.0.1:0`.
//!
//! The negative twins are the ones the requirements name: unprivileged
//! production must exit `1` with no single-UID fallback, an ungranted route must
//! get `ENOENT` from its socket, an `egress = false` view must get
//! `EPERM | ENOSYS` from `socket(AF_INET)`, and `--host 0.0.0.0` without
//! `BLASTIT_DEV_EXPOSE=1` must exit `1` with an empty stdout.

mod common;

use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::mock_pg::MockPgServer;

/// How long a case waits for the server to report `ready`.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a case waits for an event or a line.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a case waits for the process to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

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
}

impl Dev {
    /// Starts `blastit dev` in a fresh fixture directory.
    ///
    /// `env` is the *whole* child environment besides the loader/interpreter
    /// variables the embedded CPython needs; `env_clear` keeps the suite's own
    /// `BLASTIT_*` out of a case that is about the environment.
    fn start(tag: &str, manifest: &str, env: &[(&str, &str)], arguments: &[&str]) -> Self {
        let dir = fixture(tag, manifest);
        let port = free_port();
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
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, Arc::clone(&stdout));
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, Arc::clone(&stderr));
        }
        Self {
            child,
            port,
            dir,
            stdout,
            stderr,
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
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| line.contains(needle)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "no stdout line contained `{needle}` within {READY_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// Waits for the `ready` event, in either log format.
    fn wait_for_ready(&self) {
        self.wait_for_line("ready");
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

    /// One HTTP/1.1 request answered in full.
    fn request(&self, method: &str, target: &str, body: Option<(&str, &str)>) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting for {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
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
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap_or_default();
        Response::parse(&raw)
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
struct Response {
    status: u16,
    body: String,
}

impl Response {
    fn parse(raw: &[u8]) -> Self {
        let text = String::from_utf8_lossy(raw);
        let status = text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = text
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();
        Self { status, body }
    }
}

/// A fresh `0755` fixture directory holding `blastit.toml` and `app.py`.
fn fixture(tag: &str, manifest: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "blastit-dev-{}-{tag}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("a readable fixture");
    fs::write(dir.join("blastit.toml"), manifest).expect("write blastit.toml");
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

/// Reads a child's pipe into `sink`, line by line.
fn spawn_reader(pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) {
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
    });
}

/// A TCP port nothing is listening on right now.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// The effective uid of `pid`.
fn euid_of(pid: i32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// A manifest with one granted proxy and the routes a case needs.
fn manifest(routes: &str) -> String {
    format!(
        "version = 1\napp = \"app:application\"\n\n\
         [proxy.db]\nprotocol = \"postgresql\"\ninput = {{ url = \"DATABASE_URL\" }}\n\n\
         {routes}"
    )
}

/// The `DATABASE_URL` a case hands the child: a real-looking credential for a
/// `MockPgServer` that never sees it, with TLS off because the mock is local.
fn database_url(server: &MockPgServer) -> String {
    format!(
        "postgresql://real-user:real-secret@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    )
}

#[test]
fn dev_boots_five_unprivileged_roles_and_strips_secrets() {
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"json\"
path = ['/json']
[route.ingress]
body = \"json\"

[[route]]
name = \"home\"
path = ['/.*']
[route.db]
queries = [\"SELECT 1\"]
";
    let mut dev = Dev::start(
        "five-roles",
        &manifest(routes),
        &[
            ("DATABASE_URL", &url),
            ("BLASTIT_SECRET_MARKER", "must-not-reach-a-view"),
            ("STRIP_ME", "strip-me-too"),
            ("BLASTIT_ENV_STRIP", "STRIP_ME"),
        ],
        &[],
    );
    dev.wait_for_ready();

    // The exact 3-line banner (§1.2), route count in the third line.
    let banner = dev.banner();
    assert_eq!(banner.len(), 3, "{banner:?}\n{}", dev.stdout_text());
    assert_eq!(
        banner[0],
        format!(
            "  blastit dev · single-uid simulation · 127.0.0.1:{}",
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

    // The granted route's SQL goes through dbproxy to the mock upstream.
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
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"home\"
path = ['/.*']
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
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "[[route]]\nname = \"home\"\npath = ['/.*']\n";
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
fn unprivileged_production_has_no_single_uid_fallback() {
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "[[route]]\nname = \"home\"\npath = ['/.*']\n";
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
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"granted\"
path = ['/granted/.*']
[route.db]
queries = [\"SELECT 1\"]

[[route]]
name = \"ungranted\"
path = ['/ungranted/.*']
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
fn an_egress_false_view_cannot_open_inet() {
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"closed\"
path = ['/closed/.*']

[[route]]
name = \"open\"
path = ['/open/.*']
egress = true
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
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"home\"
path = ['/.*']
[route.db]
queries = [\"SELECT 1\"]
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
        "stage 1 reported the occupied bind address:\n{stdout}"
    );
    let leaked = roots_for_pid(pid);
    assert!(
        leaked.is_empty(),
        "occupied-port failure leaked runtime root(s) for pid {pid}: {leaked:?}"
    );

    // 2. Post-adopt `python_fail` path: a broken `BLASTIT_APP` fails inside
    // CPython import after `http` and `scheduler` were forked (`R20`).
    let dir = fixture("bad-import", &manifest_text);
    let free = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &free.to_string()])
        .env_clear()
        .env("DATABASE_URL", &url)
        .env("BLASTIT_APP", "missing_app_module:application")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    copy_runtime_env(&mut command);
    let child = command.spawn().expect("blastit dev spawns");
    let pid = child.id();
    let output = child.wait_with_output().expect("blastit dev exits");
    let _ = fs::remove_dir_all(&dir);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "failed import must exit 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("BLASTIT_APP (missing_app_module:application)"),
        "stage 1 reported the failed BLASTIT_APP import:\n{stdout}"
    );
    let leaked = roots_for_pid(pid);
    assert!(
        leaked.is_empty(),
        "python_fail leaked runtime root(s) for pid {pid}: {leaked:?}"
    );
}

#[test]
fn json_mode_emits_privilege_and_never_a_port_key() {
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"home\"
path = ['/.*']
[route.db]
queries = [\"SELECT 1\"]
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
    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"home\"
path = ['/']
[route.db]
queries = [\"SELECT 42\"]

[[route]]
name = \"json\"
path = ['/json']
[route.ingress]
body = \"json\"

[[route]]
name = \"sql\"
path = ['/sql']
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

    let manifest_path = dev.dir.join("blastit.toml");
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
        learned.contains("name = \"extra\""),
        "the unrouted route was proposed:\n{learned}"
    );
    assert!(
        learned.contains("\"/extra/[0-9]+\"") || learned.contains("'/extra/[0-9]+'"),
        "the numeric segment became a pattern:\n{learned}"
    );
    assert!(
        learned.contains("SELECT 1"),
        "the routed statement was written:\n{learned}"
    );
    let sql_section = learned
        .split("name = \"sql\"")
        .nth(1)
        .unwrap_or_default()
        .split("[[route]]")
        .next()
        .unwrap_or_default();
    assert!(
        sql_section.contains("SELECT 1"),
        "the statement belongs to the route that ran it:\n{learned}"
    );
    assert!(
        !sql_section.contains("ingress"),
        "`--learn` never synthesises `[route.ingress]` from a rejected 413:\n{learned}"
    );
    // The learn fallback's own view is route `home`; its observations belong to
    // an unrouted request, so they are not written as a grant for `home`.
    let home = learned.split("name = \"home\"").nth(1).unwrap_or_default();
    let home = home.split("[[route]]").next().unwrap_or_default();
    assert!(
        !home.contains("SELECT 1"),
        "an unrouted request's statement must not become a route grant:\n{learned}"
    );
    dev.wait_for_line("learn: ");
    assert!(
        dev.stdout_text()
            .contains("learn: 1 routes, 1 queries added"),
        "the summary was printed:\n{}",
        dev.stdout_text()
    );
}

#[test]
fn learn_attributes_queries_per_route_across_concurrent_live_views() {
    use blastit_manifest::{Grant, Manifest, ProxyName};

    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"a\"
path = ['/a/.*']

[[route]]
name = \"b\"
path = ['/b']
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
    let manifest_path = dev.dir.join("blastit.toml");
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
        Manifest::parse_and_validate(&learned, 20_000).expect("the learned manifest parses");
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

    let server = MockPgServer::start();
    let url = database_url(&server);
    let routes = "\
[[route]]
name = \"a\"
path = ['/a/.*']

[[route]]
name = \"b\"
path = ['/b']
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

    let manifest_path = dev.dir.join("blastit.toml");
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
        Manifest::parse_and_validate(&learned, 20_000).expect("the learned manifest parses");
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
