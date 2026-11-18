//! The DX-invariance rows the epic promised not to change, asserted in process
//! (`MB.36.7`, design `view-crates-architecture.md` §1 rows 1 and 3, R14).
//!
//! The replay in `docs/testing/AUDIT-MB.36.md` compares two captures of every
//! documented command. This file is the part of that promise that must keep
//! running on every `cargo test`, without a capture and without Docker — the
//! shape a future refactor is most likely to break silently:
//!
//! * **row 1, `blastit dev` on a static site** — one file answers `200` with its
//!   exact bytes and an `ETag` of the documented hex shape, a missing path is a
//!   `404`, and a traversal attempt is a `400` (never a `403`, never a served
//!   file). The mount is the whole manifest: no `view`, no app, no interpreter.
//! * **row 3, `blastit dev` on a WSGI app** — `GET` is the app's own status and
//!   body, a `POST` with a JSON body reaches the app with its bytes intact, and a
//!   `POST` to a view that declares **no** body is a `413` *before* the app runs.
//!   Row 3 needs the `python` capability, so it is compiled with it; `make check`
//!   and `make rs-test-critical` both pass `--features python`, which is the
//!   configuration every shipped wheel is built in.
//!
//! Row 4 (`--no-reload`) is the boot-line capture and row 7 a build-level one;
//! row 5's JSON shape is `critical_check_schema`'s. Nothing here boots production
//! or needs a privilege: `blastit dev` is the unprivileged simulation, and the
//! suite's children get an explicit environment and working directory.
//!
//! Every case pairs the promise with its negative: `200` beside `404` beside
//! `400`, and the accepted body beside the `413` that must never reach the app.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long a `blastit dev` gets to answer its first request.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a signalled `blastit dev` gets to leave.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

/// The static fixture's one file; its bytes are asserted exactly.
const INDEX_BYTES: &[u8] = b"<!doctype html><h1>dx-invariance</h1>\n";

/// Row 1's manifest: `docs/frameworks/static-and-spa.md`'s smallest site.
const STATIC_ONLY: &str = "\
static \"/\" {
    root \"public\"
}
";

#[cfg(feature = "python")]
/// Row 3's manifest: `docs/frameworks/python/index.md`'s shape — one
/// `entrypoint`, one body-taking view, one view that declares no body.
const WSGI: &str = "\
app \"dx\"

entrypoint python \"app:application\"

view echo POST \"/echo\" {
    body json
}

view bare POST \"/bare\" {}

view home GET \"/\" {}
";

#[cfg(feature = "python")]
/// The fixture app: it answers every `GET` with one marker body and reports how
/// many body bytes it read on `/echo`, so a request that bypassed the ingress
/// codec is visible in the response rather than inferred from a log line.
const APP: &str = r#"def application(environ, start_response):
    length = int(environ.get("CONTENT_LENGTH") or 0)
    body = environ["wsgi.input"].read(length) if length else b""
    if environ.get("PATH_INFO") == "/echo":
        start_response("200 OK", [("Content-Type", "application/json")])
        return [b'{"received":' + str(len(body)).encode() + b"}"]
    start_response("200 OK", [("Content-Type", "text/plain")])
    return [b"wsgi-fixture"]
"#;

/// A fixture directory, removed on drop.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blastit-mb367-dx-{}-{tag}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("a fresh fixture directory");
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the file's directory");
        }
        fs::write(path, contents).expect("write a fixture file");
    }

    #[cfg(feature = "python")]
    fn write_both(&self) {
        self.write("Blastfile", WSGI);
        self.write("app.py", APP);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// One HTTP/1.1 response, decoded from either framing.
struct Response {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    }
}

/// A `blastit dev` child, stopped on drop however the case ends.
struct Dev {
    child: Child,
    port: u16,
}

impl Dev {
    /// Boots `blastit dev` in `dir`, proves it answers, or panics with its output.
    fn boot(dir: &Path) -> Self {
        let mut last: Option<String> = None;
        for _ in 0..3 {
            let mut dev = Self::spawn(dir);
            if dev.answered() {
                return dev;
            }
            last = Some(dev.stop());
        }
        panic!(
            "`blastit dev` never answered `GET /` within {READY_TIMEOUT:?}:\n{}",
            last.unwrap_or_default()
        );
    }

    fn spawn(dir: &Path) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // The host's memory must not turn a DX case into a capacity refusal;
            // both names are `BLASTIT_*`, so the `execve` barrier strips them.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let child = command.spawn().expect("the blastit binary starts");
        Self { child, port }
    }

    /// Whether the same live process answers `GET /` within the deadline.
    fn answered(&mut self) -> bool {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(response) = self.get("/")
                && response.status == 200
            {
                return true;
            }
            if self
                .child
                .try_wait()
                .expect("the child's exit status")
                .is_some()
            {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn get(&self, target: &str) -> Option<Response> {
        self.request("GET", target, &[], None)
    }

    fn request(
        &self,
        method: &str,
        target: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Option<Response> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .ok()?;
        let mut head = format!(
            "{method} {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\
             Content-Length: {}\r\n",
            body.len()
        );
        if let Some(value) = content_type {
            head.push_str(&format!("Content-Type: {value}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).ok()?;
        stream.write_all(body).ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).ok()?;
        Some(parse_response(&raw))
    }

    /// `SIGTERM`, then a bounded wait; `SIGKILL` if the child did not leave.
    ///
    /// Takes `&mut self` so it can run both on the success path and from the
    /// boot retry loop, and leaves the drop guard intact for a panicking case.
    fn stop(&mut self) -> String {
        // SAFETY: the pid came from `spawn`; `kill` takes two integers.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + EXIT_TIMEOUT;
        let mut left = false;
        while Instant::now() < deadline {
            if self
                .child
                .try_wait()
                .expect("the child's exit status")
                .is_some()
            {
                left = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !left {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let mut stdout = String::new();
        if let Some(pipe) = self.child.stdout.take() {
            let mut pipe = pipe;
            let _ = pipe.read_to_string(&mut stdout);
        }
        let mut stderr = String::new();
        if let Some(pipe) = self.child.stderr.take() {
            let mut pipe = pipe;
            let _ = pipe.read_to_string(&mut stderr);
        }
        format!("stdout:\n{stdout}\nstderr:\n{stderr}")
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_response(raw: &[u8]) -> Response {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a complete HTTP head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut body = raw[split + 4..].to_vec();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("an HTTP status line");
    let response = Response {
        status,
        head,
        body: Vec::new(),
    };
    if response
        .header("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        body = decode_chunked(&body);
    }
    Response { body, ..response }
}

/// Decodes a chunked body: `len(hex) CRLF bytes CRLF`, ended by a `0` chunk.
fn decode_chunked(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = raw;
    loop {
        let Some(end) = rest.windows(2).position(|window| window == b"\r\n") else {
            return out;
        };
        let size_text = String::from_utf8_lossy(&rest[..end]);
        let size = usize::from_str_radix(size_text.trim(), 16).unwrap_or(0);
        rest = &rest[end + 2..];
        if size == 0 || rest.len() < size {
            return out;
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or_default();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// The documented `ETag` shape (design §1): hex, a dash, hex, in quotes.
fn is_etag_shape(value: &str) -> bool {
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'));
    inner.is_some_and(|inner| {
        inner.split_once('-').is_some_and(|(first, second)| {
            let hex = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_hexdigit());
            hex(first) && hex(second)
        })
    })
}

// ---------------------------------------------------------------------------
// Row 1 — `blastit dev` on a static-only manifest.
// ---------------------------------------------------------------------------

#[test]
fn row_1_static_dev_serves_the_file_and_refuses_a_traversal() {
    let fixture = Fixture::new("static");
    fixture.write("Blastfile", STATIC_ONLY);
    fixture.write("public/index.html", INDEX_BYTES);

    let mut dev = Dev::boot(fixture.path());

    let index = dev.get("/index.html").expect("the server answers");
    assert_eq!(index.status, 200);
    assert_eq!(
        index.body, INDEX_BYTES,
        "a static answer is the file's bytes, not a rendering"
    );
    assert_eq!(
        index.header("content-length"),
        Some(INDEX_BYTES.len().to_string().as_str()),
        "the length is the file's, so a truncated body cannot pass"
    );
    let etag = index
        .header("etag")
        .expect("a static answer carries an ETag");
    assert!(
        is_etag_shape(etag),
        "the ETag keeps the documented `\"<hex>-<hex>\"` shape, got {etag}"
    );
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8"),
        "the documented type for a `.html` file"
    );

    let missing = dev.get("/missing").expect("the server answers");
    assert_eq!(missing.status, 404, "a path with no file is not a 200");
    assert_eq!(missing.body, b"not found\n");

    let traversal = dev.get("/..%2fetc").expect("the server answers");
    assert_eq!(
        traversal.status, 400,
        "a traversal through a static mount is refused, never served"
    );
    assert_eq!(traversal.body, b"bad request\n");

    // The documented banner: an all-static manifest binds no route and needs no
    // app, so the third line counts zero of them and never says `BLASTIT_APP`.
    let output = dev.stop();
    assert!(
        output.contains("http · scheduler · dbproxy · 0 routes"),
        "the documented banner is missing from {output}"
    );
    assert!(
        !output.contains("BLASTIT_APP"),
        "an all-static manifest needs no app: {output}"
    );
}

// ---------------------------------------------------------------------------
// Row 3 — `blastit dev` on a WSGI app (needs the `python` capability).
// ---------------------------------------------------------------------------

#[cfg(feature = "python")]
#[test]
fn row_3_wsgi_dev_serves_the_app_and_refuses_a_body_with_no_codec() {
    let fixture = Fixture::new("wsgi");
    fixture.write_both();

    let mut dev = Dev::boot(fixture.path());

    let home = dev.get("/").expect("the server answers");
    assert_eq!(
        home.status, 200,
        "the app's own status is what a reader gets"
    );
    assert_eq!(home.body, b"wsgi-fixture");
    assert_eq!(home.header("content-type"), Some("text/plain"));

    let body = b"{\"n\":1}";
    let echo = dev
        .request("POST", "/echo", body, Some("application/json"))
        .expect("the server answers");
    assert_eq!(echo.status, 200);
    assert_eq!(
        echo.body,
        format!("{{\"received\":{}}}", body.len()).as_bytes(),
        "the app reads the request body the client sent"
    );

    // The negative twin: the view declares no body, so the ingress refuses the
    // request with `413` and the app never runs (any app answer would be 200).
    let refused = dev
        .request("POST", "/bare", body, Some("application/json"))
        .expect("the server answers");
    assert_eq!(
        refused.status, 413,
        "a body on a view with no `body` is a 413, not the app's answer"
    );
    assert_ne!(
        refused.body, b"wsgi-fixture",
        "the 413 must come from the ingress, never from the app"
    );

    let output = dev.stop();
    assert!(
        output.contains("http · scheduler · dbproxy · 3 routes"),
        "the banner counts the three views: {output}"
    );
}
