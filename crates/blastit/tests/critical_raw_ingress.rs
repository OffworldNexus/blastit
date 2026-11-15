#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for `MB.3.53` (`ADR-0011`): `[route.ingress]
//! body = "raw"` without `allow_raw = true` is a **boot refusal on the
//! production path**, not the warning it was.
//!
//! Before this gate the acknowledgement gated nothing: `plan_body` streams a
//! `raw` body to the application's own parser whether or not `allow_raw` is
//! written (`blastit_ingress::plan_body`'s `StreamPassthrough`), and nothing at
//! boot read the key — so a production manifest with `body = "raw"` served
//! unvalidated bytes with only a lint (`blastit check`, which no preset runs)
//! objecting. The three environments now differ on purpose:
//!
//! * **production** (`blastit`, and stage 1's re-read of the same file)
//!   refuses, naming the route and the key, before a listener is bound;
//! * **`blastit dev`** keeps the loud `ManifestWarning::RawIngress` and serves —
//!   the local single-uid simulation;
//! * **`blastit check`** keeps the `raw_ingress` finding (`critical_check_engine`
//!   owns that half).
//!
//! # Why the refusal is measured through stage 1
//!
//! A `cargo test` process is unprivileged, and stage 0 reads its capability
//! sets before it reads the manifest (`stage0.rs`'s `blastit_sys::caps::detect`
//! is the first thing `run` does, and an unprivileged boot is refused there with
//! the `setcap` recipe — `critical_dev_and_fail_closed::unprivileged_production_has_no_single_uid_fallback`
//! pins exactly that). No unprivileged process can therefore reach a stage-0
//! gate that lives after the manifest parse, which is why every other
//! production-path gate in this tree is measured through `blastit dev` (the
//! shared rule) or through a library seam. This suite takes the strongest
//! reachable form instead: **stage 1 in production mode** (`--stage=1
//! --config-fd`, no `--dev-root`) is the real production process, the one that
//! binds and forks, and it re-reads the manifest from disk — so its refusal is
//! the fail-closed behaviour a deployment gets, proved by a real exit code on
//! the real binary. Stage 0's call site shares the same predicate
//! (`Manifest::raw_ingress_refusals`, unit-tested in `blastit-manifest`), so the
//! two stages cannot disagree about what is refused.
//!
//! # Why "nothing was bound" is measured as "no `ready`"
//!
//! `MB.3.54`: the three refusal cases used to assert `!listening(port)` — but
//! that probe runs *after* the child has exited, and a boot that bound a
//! listener and then exited frees the port again, so the assertion could not
//! fail on the exact state this suite exists to catch. `ready` is the ordered
//! signal instead: init emits it only after `bind_tcp` and the forks
//! (`stage1.rs`'s `emit_ready`), so a boot that announces it served, and a boot
//! that refuses at a gate never does. [`announced_ready`] is also asserted
//! *true* on the serving twins below, so the predicate is not one that can only
//! ever say "no".

use std::fs;
use std::io::Read;
use std::net::TcpListener;
#[cfg(feature = "python")]
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use blastit::config::LogFormat;

#[cfg(feature = "python")]
use std::io::Write;
#[cfg(feature = "python")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "python")]
use std::thread::JoinHandle;

/// How long a launched process may take before the case calls it wedged.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The port the accepted twin binds (and the refusal never binds).
const PORT: u16 = 0;

/// A counter for unique fixture directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The application the serving twin posts to: it answers with the request body
/// exactly as it received it, so "byte-for-byte intact" is measured rather than
/// inferred.
#[cfg(feature = "python")]
const APP: &str = r#"
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

/// A manifest whose only route takes a `raw` body, with the acknowledgement as
/// the caller wrote it (`allow_raw = true` or nothing at all).
fn raw_manifest(route: &str, acknowledgement: &str) -> String {
    format!(
        r#"version = 1
app = "app:app"

[[route]]
name = "{route}"
path = ['/upload']
methods = ["POST"]

[route.ingress]
body = "raw"
{acknowledgement}"#
    )
}

/// A manifest whose raw route is acknowledged — the shape a `body = "raw"`
/// deployment must now have.
fn accepted_manifest() -> String {
    raw_manifest("upload", "allow_raw = true\n")
}

/// A manifest with the same route and no acknowledgement: the boot refusal.
fn refused_manifest() -> String {
    raw_manifest("upload", "")
}

/// A fresh fixture directory holding `blastit.toml` (and `app.py` when the
/// serving twin needs it).
fn fixture(tag: &str, manifest: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "blastit-raw-{}-{tag}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a fixture directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("a readable fixture");
    fs::write(dir.join("blastit.toml"), manifest).expect("write blastit.toml");
    #[cfg(feature = "python")]
    {
        fs::write(dir.join("app.py"), APP).expect("write app.py");
        fs::set_permissions(dir.join("app.py"), fs::Permissions::from_mode(0o644))
            .expect("a readable app.py");
    }
    dir
}

/// The loader and interpreter variables the embedded CPython needs.
///
/// The children are started with `env_clear` so the suite's own `BLASTIT_*`
/// cannot reach a case about the manifest; these are what the binary needs,
/// exactly as `make check` sets them.
fn copy_runtime_env(command: &mut Command) {
    for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// An ephemeral port nothing holds.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", PORT))
        .expect("a probe listener")
        .local_addr()
        .expect("the probe address")
        .port()
}

/// The configuration document stage 0 would hand stage 1: the same struct,
/// serialized the same way, with the manifest's **path** and no digest of what
/// stage 0 approved — which is why stage 1 must re-apply the gate.
///
/// `format` is the whole point of the last case: the refusal's *stream* is the
/// log format's decision, so both spellings are asserted rather than assumed.
fn config_document(dir: &Path, port: u16, format: LogFormat) -> Vec<u8> {
    let config = blastit::config::Config {
        manifest: dir.join("blastit.toml"),
        manifest_dir: dir.to_path_buf(),
        port,
        server_port: port,
        log_format: format,
        ..blastit::config::Config::default()
    };
    serde_json::to_vec(&config.to_json()).expect("the configuration document")
}

/// Runs `blastit --stage=1 --config-fd=N` — production, because no `--dev-root`
/// is passed — and returns its exit code with both streams.
///
/// Bounded, because the interesting failure is the *absent* refusal: a stage 1
/// that got past the gate binds and serves, and `wait_with_output` would hang
/// instead of reporting it.
fn stage_one(dir: &Path, document: &[u8]) -> (Option<i32>, String, String) {
    let path = dir.join("stage1-config.json");
    fs::write(&path, document).expect("write the configuration document");
    let file = fs::File::open(&path).expect("open the configuration document");
    let fd = file.as_raw_fd();
    // The child has to inherit this descriptor at this number, exactly as stage
    // 0 hands stage 1 its pipe.
    rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty()).expect("clear FD_CLOEXEC");
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .arg("--stage=1")
        .arg(format!("--config-fd={fd}"))
        .env_clear()
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    copy_runtime_env(&mut command);
    let child = command.spawn().expect("the blastit binary starts");
    drop(file);
    finish(child)
}

/// Waits (bounded) for `child` to exit, kills it if it did not, and returns the
/// exit code with everything it wrote.
fn finish(mut child: Child) -> (Option<i32>, String, String) {
    let deadline = Instant::now() + TIMEOUT;
    let mut code = None;
    while Instant::now() < deadline {
        match child.try_wait().expect("the child's exit status") {
            Some(status) => {
                code = status.code();
                break;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    if code.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    (code, stdout, stderr)
}

/// Every line of `stdout` that parses as an event, in order.
fn events(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect()
}

/// The `app.output` events on `stdout`, parsed — one per line of a refusal.
///
/// Asserting on the event rather than on the joined sentence is what makes the
/// *fields* assertable: the emitter writes one descriptor in both formats and
/// records the originating stream as a field (`MB.3.53`), so `stream` is
/// `stderr` for a refusal even though the bytes are on stdout.
fn app_output_events(stdout: &str) -> Vec<serde_json::Value> {
    events(stdout)
        .into_iter()
        .filter(|event| event.get("event").and_then(|kind| kind.as_str()) == Some("app.output"))
        .collect()
}

/// The `line` fields of every `app.output` event on `stdout`, joined — what a
/// reader of the log (or `blastit check --json`) would see.
fn app_output(stdout: &str) -> String {
    app_output_events(stdout)
        .iter()
        .filter_map(|event| {
            event
                .get("line")
                .and_then(|line| line.as_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the boot announced `ready` — the **ordered** signal, not a port
/// probe (`MB.3.54`).
///
/// Init emits `ready` after `bind_tcp` and the forks, so it is the boot
/// answering for itself: a boot that refuses at a gate never writes it, whereas
/// `!listening(port)` measured after the child exited is also satisfied by a
/// boot that bound and *then* exited (`MB.3.48`'s `ready`-partition —
/// `critical_php_embed`'s `Boot::await_boot` — is the same rule, and the same
/// reason a foreign listener on an ephemeral port cannot be trusted).
///
/// Both formats are read: JSON records it as `"event":"ready"`, and pretty
/// spells it in the event-name column (`… INFO  ready       …`) with no JSON at
/// all. Matching the *word* there is precise enough — the name is its own
/// token in that column and the sentences asserted here contain no bare
/// `ready`.
fn announced_ready(stdout: &str) -> bool {
    events(stdout)
        .iter()
        .any(|event| event.get("event").and_then(|kind| kind.as_str()) == Some("ready"))
        || stdout
            .lines()
            .any(|line| line.split_whitespace().any(|word| word == "ready"))
}

/// **The refusal.** A production stage 1 refuses a manifest whose raw route has
/// no acknowledgement: exit `1`, the sentence on stdout as an `app.output` event
/// (the JSON default, `"stream":"stderr"` — the stream field is written into the
/// event, not into the descriptor), naming **both** the route and the key, and
/// no `ready` — the ordered signal, because init emits it only after the bind.
///
/// The pretty-format twin below asserts the *other* stream, so neither case can
/// pass by grepping the descriptor its format never writes to.
#[test]
fn production_stage_one_refuses_raw_ingress_without_allow_raw() {
    let dir = fixture("production-refusal", &refused_manifest());
    let port = free_port();
    let (code, stdout, stderr) = stage_one(&dir, &config_document(&dir, port, LogFormat::Json));
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(
        code,
        Some(1),
        "a refused boot exits 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let refusal = app_output(&stdout);
    assert!(
        refusal.contains("route \"upload\""),
        "the refusal names the route: {refusal}\nstdout:\n{stdout}"
    );
    assert!(
        refusal.contains("allow_raw = true"),
        "the refusal names the key that fixes it: {refusal}\nstdout:\n{stdout}"
    );
    assert!(
        refusal.contains("body = \"raw\""),
        "the refusal names what was declared: {refusal}"
    );
    assert!(
        refusal.contains("[route.ingress]"),
        "the refusal names the table to edit: {refusal}"
    );
    // `MB.3.54` (item 2): the *event's* `stream` field, parsed — the descriptor
    // is stdout in both formats, so `stderr.is_empty()` below could hold with
    // this field flipped. The event is found by its sentence so the assertion
    // cannot pass on some other `app.output` line.
    let event = app_output_events(&stdout)
        .into_iter()
        .find(|event| {
            event
                .get("line")
                .and_then(|line| line.as_str())
                .is_some_and(|line| line.contains("allow_raw = true"))
        })
        .unwrap_or_else(|| panic!("the refusal is an `app.output` event naming the key: {stdout}"));
    assert_eq!(
        event["level"], "error",
        "a refusal is an error-level event: {event}"
    );
    assert_eq!(
        event["stream"], "stderr",
        "the refusal records the stream it came from as a field: {event}"
    );
    assert!(
        stderr.is_empty(),
        "JSON events go to the emitter's descriptor, so stderr stays empty: {stderr}"
    );
    assert!(
        !announced_ready(&stdout),
        "a refused boot never announces `ready` — it is emitted only after the bind: {stdout}"
    );
}

/// The same refusal under `BLASTIT_LOG_FORMAT=pretty`: the sentence is **plain
/// text**, and it is still on stdout — `Emitter::stdout` writes one descriptor
/// in both formats and records the originating stream as a field (`stderr`, the
/// `app.output` column above), so a case that greps fd 2 for a stage-1 refusal
/// finds nothing in either format. `blastit dev`'s pre-banner refusals are the
/// ones that are raw text on stderr, which is why this suite never reuses a
/// `stderr` assertion from another.
#[test]
fn the_pretty_refusal_is_plain_text_naming_the_same_route() {
    let dir = fixture("production-pretty", &refused_manifest());
    let port = free_port();
    let (code, stdout, stderr) = stage_one(&dir, &config_document(&dir, port, LogFormat::Pretty));
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(
        code,
        Some(1),
        "a refused boot exits 1\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("route \"upload\"") && stdout.contains("allow_raw = true"),
        "pretty writes the sentence as text on the emitter's descriptor: {stdout}"
    );
    assert!(
        !stdout.contains("\"event\""),
        "pretty is not JSON: {stdout}"
    );
    assert!(
        stderr.is_empty(),
        "the stream field names stderr; the descriptor is still stdout: {stderr}"
    );
    assert!(
        !announced_ready(&stdout),
        "a refused boot never announces `ready`, in either format: {stdout}"
    );
}

/// **Stage 1's re-read.** `config` carries the manifest's path and nothing that
/// records what stage 0 approved, so a file swapped between the two stages
/// reaches stage 1 as an unvalidated declaration. The edited file is refused
/// with the same sentence, and the boot never announces `ready`.
///
/// This is the shape `docs/design/learn-single-pass.md` §7.2 makes the second
/// checkpoint necessary for; the document is built for the acknowledged file,
/// and only the file on disk changes.
#[test]
fn production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it() {
    let dir = fixture("toctou", &accepted_manifest());
    let port = free_port();
    // What stage 0 read and approved, and what it handed on: a path, not a
    // digest — the premise of the case.
    let document = config_document(&dir, port, LogFormat::Json);
    let document_text = String::from_utf8_lossy(&document).into_owned();
    assert!(
        document_text.contains(&*dir.join("blastit.toml").to_string_lossy()),
        "the document carries the manifest's path: {document_text}"
    );
    assert!(
        !document_text.contains("allow_raw"),
        "the document carries no record of what stage 0 approved: {document_text}"
    );
    // The edit, between the stages: the acknowledgement is gone.
    fs::write(dir.join("blastit.toml"), refused_manifest()).expect("rewrite blastit.toml");

    let (code, stdout, stderr) = stage_one(&dir, &document);
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(
        code,
        Some(1),
        "the re-read refuses the edited manifest\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let refusal = app_output(&stdout);
    assert!(
        refusal.contains("route \"upload\"") && refusal.contains("allow_raw = true"),
        "the re-read names the route and the key: {refusal}\nstdout:\n{stdout}"
    );
    assert!(
        !announced_ready(&stdout),
        "the edited manifest never reaches a serving boot on {port}: {stdout}"
    );
}

/// Running `blastit dev`, with its output collected as it arrives.
#[cfg(feature = "python")]
struct Dev {
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
}

#[cfg(feature = "python")]
impl Dev {
    /// Starts `blastit dev` in a fresh fixture and waits until it serves.
    fn start(tag: &str, manifest: &str) -> Self {
        let dir = fixture(tag, manifest);
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        copy_runtime_env(&mut command);
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
        let mut dev = Self {
            child,
            port,
            stdout,
            stderr,
            stdout_reader,
            stderr_reader,
        };
        // The ordered signal, not the port (`MB.3.54`): `ready` is emitted after
        // the bind and the forks, so waiting for it is what makes the absence
        // assertions in the refusal cases mean "this boot never served". It also
        // keeps this predicate honest — a serving boot must make it true.
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if announced_ready(&dev.stdout_text()) {
                return dev;
            }
            if let Some(status) = dev.child.try_wait().expect("the dev exit status") {
                panic!(
                    "blastit dev[{tag}] exited {status} before `ready`\nstdout:\n{}\nstderr:\n{}",
                    dev.stdout_text(),
                    dev.stderr_text()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "blastit dev[{tag}] never served on {port}\nstdout:\n{}\nstderr:\n{}",
            dev.stdout_text(),
            dev.stderr_text()
        );
    }

    fn stdout_text(&self) -> String {
        self.stdout.lock().unwrap().join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().unwrap().join("\n")
    }

    /// One `POST` whose body is exactly `payload`, answered with the response
    /// body bytes and nothing else.
    fn post(&self, target: &str, payload: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting for {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        stream
            .set_read_timeout(Some(TIMEOUT))
            .expect("a read timeout");
        let mut request = format!(
            "POST {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\
             Content-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        )
        .into_bytes();
        request.extend_from_slice(payload);
        stream.write_all(&request).expect("send the request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap_or_default();
        response_body(&raw)
    }
}

#[cfg(feature = "python")]
impl Drop for Dev {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for slot in [&mut self.stdout_reader, &mut self.stderr_reader] {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
}

/// Drains a child's pipe into `sink`, one line at a time.
#[cfg(feature = "python")]
fn spawn_reader(pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) -> JoinHandle<()> {
    use std::io::BufRead;

    std::thread::spawn(move || {
        for line in std::io::BufReader::new(pipe).lines() {
            match line {
                Ok(line) => sink.lock().unwrap().push(line),
                Err(_) => break,
            }
        }
    })
}

/// The body bytes of an HTTP/1.1 response, chunked or framed by
/// `Content-Length` — the coded form is decoded, never compared as framing.
#[cfg(feature = "python")]
fn response_body(raw: &[u8]) -> Vec<u8> {
    let Some(split) = raw.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("no header terminator in: {}", String::from_utf8_lossy(raw));
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let body = &raw[split + 4..];
    if !head.contains("transfer-encoding: chunked") {
        return body.to_vec();
    }
    let mut decoded = Vec::new();
    let mut rest = body;
    while let Some(end) = rest.windows(2).position(|window| window == b"\r\n") {
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&rest[..end])
                .split(';')
                .next()
                .unwrap_or("0")
                .trim(),
            16,
        )
        .expect("a chunk size");
        rest = &rest[end + 2..];
        if size == 0 {
            break;
        }
        decoded.extend_from_slice(&rest[..size.min(rest.len())]);
        rest = &rest[(size + 2).min(rest.len())..];
    }
    decoded
}

/// **The accepted twin.** The same route with `allow_raw = true` boots and
/// serves the body exactly as it arrived — so the gate is a gate, not a blanket
/// refusal — and `dev` still says the body goes through unvalidated (the
/// warning half of the rule is not deleted to make the refusal pass).
///
/// The payload is chosen so the other codecs would refuse it: `json` rejects a
/// duplicate key, trailing bytes and a NUL, and none of the three survive
/// re-encoding. `dev` is the environment that serves it here because a
/// production boot cannot run unprivileged (see the module comment); the
/// serving code path below it — `plan_body`'s `StreamPassthrough` and the
/// scheduler's body frames — is the same one production uses.
#[cfg(feature = "python")]
#[test]
fn allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns() {
    const PAYLOAD: &[u8] = b"{\"a\":1,\"a\":2}\r\n\0\xff\xfe trailing bytes \x01 not json";
    let dev = Dev::start("accepted", &accepted_manifest());
    let served = dev.post("/upload", PAYLOAD);
    let stdout = dev.stdout_text();
    drop(dev);

    assert_eq!(
        served,
        PAYLOAD,
        "the raw body reaches the application and the client unchanged; served: {:?}\nstdout:\n{}",
        String::from_utf8_lossy(&served),
        stdout
    );
    assert!(
        stdout.contains("passes the request body through unvalidated"),
        "dev keeps the loud warning even with the acknowledgement: {stdout}"
    );
    assert!(
        !stdout.contains("allow_raw = true"),
        "the boot refusal never fires in dev: {stdout}"
    );
}

/// **The same file, the other environments.** `blastit dev` serves a raw route
/// with no acknowledgement (its warning, not the boot refusal) — which is what
/// makes the production refusal a property of the deployment path rather than a
/// blanket ban on the key.
#[cfg(feature = "python")]
#[test]
fn dev_serves_the_unacknowledged_manifest_that_production_refuses() {
    const PAYLOAD: &[u8] = b"no-acknowledgement";
    let dev = Dev::start("dev-unacknowledged", &refused_manifest());
    let served = dev.post("/upload", PAYLOAD);
    let stdout = dev.stdout_text();
    drop(dev);

    assert_eq!(
        served, PAYLOAD,
        "dev serves the unacknowledged raw route\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("passes the request body through unvalidated"),
        "dev warns about the route it serves: {stdout}"
    );
}
