//! Protected critical suite: `dev --learn` must never drop a route's executor
//! block (`R20`, `R28`, `R29`; `docs/frameworks/php.md`,
//! `docs/design/static-and-spa.md`).
//!
//! This is the user-visible write path: `blastit dev --learn` records the
//! statements a request ran, and on `SIGTERM` hands the manifest to
//! [`blastit_manifest::edit::apply_edits`] and `write_atomic` through the
//! supervisor (`supervise.rs`, `dev_learn::LearnRun::finish`). The manifest is
//! the author's file, so a learned `[route.db]` grant must land **and** every
//! other line — the route's `[route.static]` block included — must be the byte
//! it was.
//!
//! The fixture is the documented shape (`docs/frameworks/php.md`,
//! `docs/frameworks/static-and-spa.md`): one WSGI route with
//! `[route.static] root = "public"` and one `[route.db] queries = ['SELECT 42']`
//! the author already granted, so the request's `SELECT 1` is ungranted and
//! `--learn` has to **extend** that list — a fixture with no `[route.db]` only
//! proves a table can be created. A route with a static block and an app view
//! is legal (`validate.rs`'s `check_static_declarations` returns early for a
//! route with a front controller, and a plain app forwards every non-file path
//! to the view), and needs no PHP binary.
//!
//! It pairs the promise with its twin: the learned query must be in the file
//! **as well as** the grant that was already there (the fix cannot be "make the
//! writer a no-op", nor "replace the list"), and the static block's lines must
//! be byte-identical (the bug was the rewriter rebuilding the route without the
//! block's keys).
//!
//! `dev --learn` itself needs the interpreter-backed dev build
//! (`features = "python"`; the standalone artifact refuses `--learn` with the
//! package sentence), exactly like `critical_dev_and_fail_closed.rs`. The
//! protected critical tier always builds with `--features python`
//! (`make rs-test-critical`), so this gate is a build-configuration match, not a
//! skipped case.
#![cfg(feature = "python")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs::{self, Permissions};
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blastit_manifest::{Grant, Manifest, ProxyName};
use common::mock_pg::MockPgServer;

/// How long the boot may take before the case gives up on it.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a signalled `blastit dev --learn` may take to finish its write.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// A launch whose port was stolen between `free_port` and `bind` is retried.
const PORT_ATTEMPTS: u32 = 3;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// The manifest the page promises for a static surface served by the app.
///
/// The `[route.static]` table is written as the author wrote it (a comment
/// between the route and the table), because the assertion below is on the raw
/// lines, not on the parsed model.
const ROUTES: &str = "\
[[route]]
name = \"sql\"
path = ['/sql']

# One statement is already granted, so the learned one has to be spliced onto
# this list: a new table would be a second `[route.db]`, and replacing the list
# would drop the grant the author wrote.
[route.db]
queries = ['SELECT 42']

# Built assets are served by the responder; every other path is the view's.
[route.static]
root = \"public\"
index = \"index.html\"
";

/// The page's manifest around [`ROUTES`]: one app and one proxy.
const MANIFEST_HEAD: &str = "\
version = 1
app = \"app:application\"

[proxy.db]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

";

/// A WSGI app that runs one ungranted statement and answers JSON.
///
/// The socket client is dbproxy's local endpoint, whose host/port ride in
/// `DATABASE_URL`'s query so the view never needs a credential (`R30`).
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
    try:
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

/// The `[route.static]` table's raw lines, header included.
fn static_block(document: &str) -> Vec<&str> {
    let lines: Vec<&str> = document.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim() == "[route.static]")
        .expect("the fixture declares a static block");
    let mut block: Vec<&str> = lines[start..]
        .iter()
        .take_while(|line| *line == &lines[start] || !line.trim().starts_with('['))
        .copied()
        .collect();
    // The blank line before the next table separates blocks, not content: the
    // learned `[route.db]` table is appended after it.
    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    block
}

/// How many times `header` appears as a table header.
///
/// A mention in prose does not count — a comment may legitimately name the
/// table — so only a line that *is* the header declares one.
fn table_headers(document: &str, header: &str) -> usize {
    document
        .lines()
        .filter(|line| line.trim() == header)
        .count()
}

#[test]
fn dev_learn_writes_the_query_and_keeps_the_static_block_byte_for_byte() {
    let server = MockPgServer::start();
    let url = database_url(&server);
    let manifest = format!("{MANIFEST_HEAD}{ROUTES}");
    let expected_static = static_block(&manifest);

    let mut dev = Dev::start(
        "learn-executor",
        &manifest,
        &[("DATABASE_URL", &url)],
        &["--learn"],
    );
    dev.wait_for_ready();

    // The route's own executor answers: no file `public/sql` exists, so the
    // static responder falls through to the view, which runs the ungranted
    // `SELECT 1`. `--learn` allows it and records it.
    let response = dev.get("/sql");
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(
        response.json()["ok"],
        true,
        "the view reached dbproxy: {}",
        response.body
    );
    // The scheduler's `request` event names the routed request; wait for it
    // before SIGTERM, which is also when the supervisor stops reading.
    dev.wait_for_line("/sql");

    // The statement really crossed the wire through dbproxy's local endpoint:
    // the mock upstream saw the session and the query, so `--learn` is writing
    // an observation, not a proposal it made up.
    let deadline = Instant::now() + Duration::from_secs(10);
    while server.statements() != vec!["SELECT 1".to_string()] && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        server.statements(),
        vec!["SELECT 1".to_string()],
        "the view's statement reached the upstream"
    );
    assert!(
        !server.startups().is_empty(),
        "the view opened its session through dbproxy"
    );

    let status = dev.terminate();
    assert_eq!(
        status.code(),
        Some(0),
        "the learn write exits cleanly\nstdout:\n{}\nstderr:\n{}",
        dev.stdout_text(),
        dev.stderr_text()
    );

    let learned = fs::read_to_string(dev.manifest()).expect("the manifest is readable");
    // Direction the fix must keep: the statement landed, and the seed grant is
    // still there beside it.
    assert!(
        learned.contains("SELECT 1") && learned.contains("SELECT 42"),
        "the learned statement was written beside the seeded one:\n{learned}"
    );
    // Direction the bug broke: the static block is the author's bytes.
    assert_eq!(
        static_block(&learned),
        expected_static,
        "the [route.static] block changed:\n{learned}"
    );
    assert_eq!(
        table_headers(&learned, "[route.static]"),
        1,
        "a static block was duplicated:\n{learned}"
    );
    assert_eq!(
        table_headers(&learned, "[route.db]"),
        1,
        "the learned grant is one table:\n{learned}"
    );

    let (parsed, _warnings) =
        Manifest::parse_and_validate(&learned, 20_000).expect("the learned manifest boots");
    let route = parsed
        .routes()
        .iter()
        .find(|route| route.name() == "sql")
        .expect("the route is still declared");
    assert_eq!(
        route.static_policy().map(|policy| policy.root.as_str()),
        Some("public"),
        "the parsed route still serves its static root"
    );
    let queries = route
        .grant(&ProxyName::from("db"))
        .and_then(Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default();
    // The learned statement extends the author's list, in place: the patch is
    // a splice of one entry, not a rewritten table.
    assert_eq!(
        queries,
        vec!["SELECT 42".to_string(), "SELECT 1".to_string()],
        "the learned query extends the seeded grant"
    );
}

/// The `DATABASE_URL` the child gets: a real-looking credential for a
/// `MockPgServer` that never sees it, with TLS off because the mock is local.
fn database_url(server: &MockPgServer) -> String {
    format!(
        "postgresql://real-user:real-secret@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    )
}

/// A running `blastit dev --learn`, with its output collected as it arrives.
struct Dev {
    child: Option<Child>,
    port: u16,
    dir: PathBuf,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
}

impl Dev {
    /// Starts `blastit dev` in a fresh fixture and waits for `ready`.
    fn start(tag: &str, manifest: &str, env: &[(&str, &str)], arguments: &[&str]) -> Self {
        let mut attempt = 1;
        loop {
            let dir = fixture(tag, manifest);
            let port = free_port();
            let mut dev = Self::spawn(dir, port, env, arguments);
            match dev.settle() {
                Ok(()) => return dev,
                Err(LaunchFailure::LostThePort) if attempt < PORT_ATTEMPTS => {
                    eprintln!("blastit dev[{tag}]: port {port} was taken; retrying");
                    attempt += 1;
                }
                Err(LaunchFailure::LostThePort) => panic!(
                    "blastit dev[{tag}] could not bind a port in {PORT_ATTEMPTS} attempts\n\
                     stdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
                Err(LaunchFailure::Exited(status)) => panic!(
                    "blastit dev[{tag}] exited {status} before `ready`\nstdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
                Err(LaunchFailure::Timeout) => panic!(
                    "blastit dev[{tag}] never printed `ready` on port {port}\nstdout:\n{}\n\
                     stderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                ),
            }
        }
    }

    fn spawn(dir: PathBuf, port: u16, env: &[(&str, &str)], arguments: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .args(arguments)
            .env_clear()
            // The host's free memory and process budget must not turn a case
            // about the manifest write into a boot refusal.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
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
            child: Some(child),
            port,
            dir,
            stdout,
            stderr,
        }
    }

    /// Waits for the `ready` event, or reports why the launch ended first.
    fn settle(&mut self) -> Result<(), LaunchFailure> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| is_ready_event(line)) {
                return Ok(());
            }
            if let Some(status) = self
                .child
                .as_mut()
                .and_then(|child| child.try_wait().expect("the child's exit status"))
            {
                std::thread::sleep(Duration::from_millis(50));
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

    fn manifest(&self) -> PathBuf {
        self.dir.join("blastit.toml")
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

    fn wait_for_ready(&self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| is_ready_event(line))
                && TcpStream::connect(("127.0.0.1", self.port)).is_ok()
            {
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
            "no stdout line contained `{needle}` within {READY_TIMEOUT:?}\nstdout:\n{}\n\
             stderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// One HTTP/1.1 GET answered in full.
    fn get(&self, target: &str) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|err| panic!("connecting for {target}: {err}\n{}", self.stdout_text()));
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let request =
            format!("GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap_or_default();
        Response::parse(&raw)
    }

    /// `SIGTERM` (the documented stop, which is when `--learn` writes), then
    /// the exit status.
    fn terminate(&mut self) -> std::process::ExitStatus {
        let child = self.child.as_mut().expect("the child is still running");
        // SAFETY: the pid came from `spawn`; `kill` takes two integers.
        let killed = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        assert_eq!(killed, 0, "signalling pid {}", child.id());
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                // Keep the pipe readers' last lines: the write's stderr is the
                // diagnosis when the learn refuses.
                std::thread::sleep(Duration::from_millis(50));
                self.child = None;
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        panic!(
            "blastit dev did not exit within {EXIT_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Why a freshly spawned `blastit dev` never reached `ready`.
enum LaunchFailure {
    LostThePort,
    Exited(std::process::ExitStatus),
    Timeout,
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

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|err| panic!("{err}: the response is not JSON: {}", self.body))
    }
}

/// A fresh `0755` fixture directory: the manifest, `app.py`, and the static
/// root the responder audits at boot.
fn fixture(tag: &str, manifest: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "blastit-mb321-{}-{tag}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("public")).expect("a static root");
    fs::set_permissions(&dir, Permissions::from_mode(0o755)).expect("a readable fixture");
    fs::write(dir.join("blastit.toml"), manifest).expect("write blastit.toml");
    fs::write(dir.join("app.py"), APP).expect("write app.py");
    fs::write(dir.join("public/index.html"), "<!doctype html>").expect("write the index");
    fs::set_permissions(dir.join("app.py"), Permissions::from_mode(0o644))
        .expect("a readable app.py");
    dir
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

/// A TCP port nothing is listening on right now (a hint, not a reservation).
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// Whether a stdout line is the `ready` event, in either log format.
///
/// A substring match would also hit stage 1's failed-bind line (it contains
/// the word `ready` inside `Address already in use`), so the event is matched.
fn is_ready_event(line: &str) -> bool {
    line.contains("\"event\":\"ready\"")
        || line.contains("\"event\": \"ready\"")
        || line.split_whitespace().any(|word| word == "ready")
}

/// Whether a launch that never served lost the port to another process.
fn lost_the_port(port: u16, stdout: &str, stderr: &str) -> bool {
    let address = format!(":{port}:");
    [stdout, stderr].iter().any(|text| {
        text.lines().any(|line| {
            line.contains("binding ")
                && line.contains(&address)
                && (line.contains("Address already in use") || line.contains("os error 98"))
        })
    })
}
