//! Protected critical suite for `blastit lsp` (`MB.22.3`, R4–R34, S1–S11).
//!
//! Every case spawns the real binary's `lsp` subcommand over real pipes and
//! asserts the bytes, because the promises are about a stream: a frame's
//! declared length is UTF-8 bytes, a body is read by its length and never by a
//! newline, a malformed frame is answered and the loop continues, and stdout
//! carries framed messages and nothing else.
//!
//! The two halves of every bound are asserted: the 8 192-byte header block is
//! served **and** 8 193 is refused; a body one byte over `MAX_FRAME_BODY_BYTES`
//! is refused **and** a legal body is read; an unknown notification writes
//! nothing **and** the next request answers; a clean buffer formats to `[]`
//! **and** a broken one to `null`; a known word hovers **and** punctuation does
//! not.
//!
//! `LF`/`CRLF`, `😀` and `é` are deliberate fixtures: the only oracle for the
//! UTF-16 unit is (§4.5 P2) the non-BMP character, and the `# ✓ ok` fixture
//! proves the 0-based line and `col - 1` only.
//!
//! No case mutates the process environment or its working directory: every
//! child gets its own pipes, and the FIFO case creates its own path under the
//! temporary directory.

// A test asserts by unwrapping: a missing frame or a malformed reply is a
// failure with a message, and `clippy::expect_used` is the wrong advice here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How long a session may take to answer one frame, and how long a child may
/// take to exit. A watchdog kills the child past the exit budget, so a hung
/// server fails a test instead of hanging the suite.
const PATIENCE: Duration = Duration::from_secs(20);

/// The `blastit` binary cargo built for this test run.
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blastit"))
}

/// A child killed when a test panics, and a watchdog that kills one that hangs.
struct Watchdog {
    /// Set once the session ended, so the watchdog does not kill a dead pid.
    done: Arc<AtomicBool>,
}

impl Watchdog {
    /// Arms a kill for `pid` unless [`Watchdog::disarm`] is called first.
    fn arm(pid: u32) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::spawn(move || {
            let deadline = Instant::now() + PATIENCE;
            while Instant::now() < deadline {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // SAFETY: `kill` takes a pid and a signal number and touches no
            // memory of this process; the pid is the child this test spawned.
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        });
        Self { done }
    }

    /// Stops the watchdog: the session ended on its own.
    fn disarm(&self) {
        self.done.store(true, Ordering::Relaxed);
    }
}

/// One live `blastit lsp` session: its pipes, its watchdog and its status.
struct Session {
    /// The server process.
    child: Child,
    /// Its stdin, `None` once closed.
    stdin: Option<ChildStdin>,
    /// Its stdout, framed.
    stdout: BufReader<ChildStdout>,
    /// Its stderr, read once it exited.
    stderr: Option<ChildStderr>,
    /// The watchdog that kills a hung session.
    watchdog: Watchdog,
}

impl Session {
    /// Starts `blastit lsp` with a cleared environment (S3(b): no `BLASTIT_*`
    /// is needed) unless `env_clear` is false.
    fn spawn(env_clear: bool) -> Self {
        let mut command = binary();
        command
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if env_clear {
            command.env_clear();
        }
        let mut child = command.spawn().expect("spawn blastit lsp");
        let watchdog = Watchdog::arm(child.id());
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let stderr = child.stderr.take();
        Self {
            child,
            stdin: Some(stdin),
            stdout,
            stderr,
            watchdog,
        }
    }

    /// Writes one framed message.
    fn send(&mut self, value: &Value) {
        let body = serde_json::to_vec(value).expect("a message");
        self.send_body(&body);
    }

    /// Writes one frame around `body`.
    fn send_body(&mut self, body: &[u8]) {
        let stdin = self.stdin.as_mut().expect("an open stdin");
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        stdin.write_all(header.as_bytes()).expect("a header");
        stdin.write_all(body).expect("a body");
        stdin.flush().expect("a flush");
    }

    /// Writes a request and returns its id.
    fn request(&mut self, id: Value, method: &str, params: Value) -> Value {
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(&message);
        id
    }

    /// Writes a notification.
    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Reads one frame, returning its declared length and its body.
    fn frame(&mut self) -> Option<(usize, Vec<u8>)> {
        read_frame(&mut self.stdout)
    }

    /// Reads one frame as JSON.
    fn recv(&mut self) -> Option<Value> {
        let (_, body) = self.frame()?;
        Some(serde_json::from_slice(&body).expect("a JSON body"))
    }

    /// Reads one frame and asserts it is the capability document's parent.
    fn recv_result(&mut self) -> Value {
        let message = self.recv().expect("a reply");
        assert_eq!(message["jsonrpc"], "2.0");
        assert!(
            message.get("error").is_none(),
            "unexpected error: {message}"
        );
        message.get("result").cloned().expect("a result")
    }

    /// Reads one frame and asserts it is an error with `code`, returning the id.
    fn recv_error(&mut self, code: i32) -> Value {
        let message = self.recv().expect("a reply");
        assert_eq!(message["jsonrpc"], "2.0");
        let error = message.get("error").expect("an error");
        assert_eq!(error["code"], code, "{message}");
        // §2.4: a fixed ASCII sentence, never document text.
        let text = error["message"].as_str().expect("a message");
        assert!(text.is_ascii(), "{text}");
        message.get("id").cloned().unwrap_or(Value::Null)
    }

    /// Reads one `publishDiagnostics` and returns its `params`.
    fn publish(&mut self) -> Value {
        let message = self.recv().expect("a publish");
        assert_eq!(message["method"], "textDocument/publishDiagnostics");
        assert_eq!(message["jsonrpc"], "2.0");
        message.get("params").cloned().expect("publish params")
    }

    /// Opens a document and returns the diagnostics its publish carried.
    fn open(&mut self, uri: &str, text: &str, version: i64) -> Vec<Value> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "blastfile", "version": version, "text": text}}),
        );
        let params = self.publish();
        assert_eq!(params["uri"], uri);
        params["diagnostics"]
            .as_array()
            .expect("a diagnostics list")
            .clone()
    }

    /// Initializes the session, asserting the reply is a result.
    fn initialize(&mut self) {
        self.request(json!(1), "initialize", json!({}));
        let _ = self.recv_result();
    }

    /// Closes stdin, waits for the exit status and returns it with stderr.
    fn finish(mut self) -> (ExitStatus, String) {
        self.stdin = None;
        let status = self.wait();
        let stderr = self.stderr_text();
        self.watchdog.disarm();
        (status, stderr)
    }

    /// Waits for the child within [`PATIENCE`], killing it on overrun.
    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                panic!("the server did not exit within {PATIENCE:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Reads the child's stderr once it exited.
    fn stderr_text(&mut self) -> String {
        let Some(mut stderr) = self.stderr.take() else {
            return String::new();
        };
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    }

    /// Reads every byte of stdout still unread, to EOF.
    ///
    /// After `exit` (or after the server gave up on a frame) this returns the
    /// bytes a stray write would have produced, which is how "stdout is frames
    /// only" is asserted on the closed side of a session rather than only on the
    /// frames a test happened to read.
    fn drain_stdout(&mut self) -> Vec<u8> {
        let mut rest = Vec::new();
        let _ = self.stdout.read_to_end(&mut rest);
        rest
    }

    /// The server's pid, for the `/proc` child check (S2).
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Every thread id in `/proc/<pid>/task`, for S2's children check.
    fn task_ids(&self) -> Vec<String> {
        let directory = format!("/proc/{}/task", self.pid());
        std::fs::read_dir(directory)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for Session {
    /// A failing assertion must not leak a child process.
    fn drop(&mut self) {
        self.watchdog.disarm();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One frame from `reader`: the declared length and the body read by it.
///
/// The header line is parsed as the literal `Content-Length: ` prefix: a server
/// that wrote anything else would make the first assertion of every test fail.
fn read_frame(reader: &mut impl BufRead) -> Option<(usize, Vec<u8>)> {
    let mut declared: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).ok()?;
        if read == 0 {
            return None;
        }
        if line == "\r\n" {
            break;
        }
        let value = line
            .strip_prefix("Content-Length: ")
            .unwrap_or_else(|| panic!("the frame header is the exact line, got {line:?}"))
            .trim_end()
            .parse::<usize>()
            .expect("a decimal length");
        assert!(declared.is_none(), "one Content-Length per frame");
        declared = Some(value);
    }
    let declared = declared.expect("a Content-Length");
    let mut body = vec![0_u8; declared];
    reader.read_exact(&mut body).expect("a whole body");
    Some((declared, body))
}

/// The one fixture the position tests share: a comment with `✓` (three UTF-8
/// bytes, one UTF-16 unit) then a `view` whose `body` has a bare `=`.
const P1: &str = "# ✓ ok\nview v \"/x\" {\n  body =\n}\n";

/// The only unit oracle: `😀` is one `char` and two UTF-16 units, so a `col - 1`
/// count is 19 and a byte count is 22; the right answer is 20.
const P2: &str = "view v \"/😀\" { body = }\n";

/// The `=` sentence, as the CLI prints it and as the editor must show it.
const EQUALS: &str = "`=` is not Blastfile syntax; write the value after the keyword: `body json`";

/// A clean, canonical document: no diagnostic, and `blastit fmt` would change
/// nothing, so a hover and a formatting request on it answer without a refusal.
const CLEAN: &str =
    "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        transactions\n    }\n}\n";

// ---------------------------------------------------------------------------
// 1. Framing (R4, R7, R10, R13, S6, S7)
// ---------------------------------------------------------------------------

/// R6: the capability document of §2.3, field by field, and none of the
/// capabilities this server does not offer.
#[test]
fn handshake_publishes_the_capability_document() {
    let mut session = Session::spawn(true);
    let result = session.request(json!(1), "initialize", json!({}));
    let message = session.recv().expect("a reply");
    assert_eq!(message["id"], result);
    let result = message.get("result").expect("a result");
    let capabilities = &result["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["textDocumentSync"]["openClose"], true);
    assert_eq!(capabilities["textDocumentSync"]["change"], 1);
    assert_eq!(capabilities["textDocumentSync"]["save"], false);
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(capabilities["hoverProvider"], true);
    for absent in [
        "definitionProvider",
        "diagnosticProvider",
        "completionProvider",
        "documentSymbolProvider",
    ] {
        assert!(
            capabilities.get(absent).is_none(),
            "{absent} must not be advertised"
        );
    }
    assert_eq!(result["serverInfo"]["name"], "blastit");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1), "EOF without shutdown is 1");
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// R4: the declared length is the body's **UTF-8 byte** length — the `uri`
/// carries `å` (two bytes, one `char`), so a `char` count would desynchronise
/// the client at once.
#[test]
fn frame_length_is_utf8_bytes() {
    let mut session = Session::spawn(true);
    session.initialize();
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": "file:///tmp/Blåstfile",
            "languageId": "blastfile",
            "version": 0,
            "text": "view v \"/x\" {\n  body =\n}\n",
        }}),
    );
    let (declared, body) = session.frame().expect("a publish");
    assert_eq!(declared, body.len());
    let text = std::str::from_utf8(&body).expect("UTF-8");
    assert!(text.contains("Blåstfile"));
    assert_ne!(body.len(), text.chars().count(), "the å is two bytes");
    let parsed: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(parsed["params"]["uri"], "file:///tmp/Blåstfile");
    let (_, stderr) = session.finish();
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// R5: ids are echoed verbatim, an id-less message is a notification and gets no
/// reply, and an explicit `"id": null` is answered `-32600` with a `null` id.
#[test]
fn ids_echo_verbatim() {
    let mut session = Session::spawn(true);
    session.initialize();

    session.request(json!(7), "window/workDoneProgress/create", json!({}));
    let reply = session.recv().expect("a reply");
    assert_eq!(reply["id"], 7);
    assert!(reply["id"].is_number());

    session.request(json!("abc"), "window/workDoneProgress/create", json!({}));
    let reply = session.recv().expect("a reply");
    assert_eq!(reply["id"], "abc");
    assert!(reply["id"].is_string());

    // An id-less known notification: nothing on stdout, and the next request
    // still answers — so "no reply" is proved by the frame that follows.
    session.notify("initialized", json!({}));
    session.request(json!(9), "window/workDoneProgress/create", json!({}));
    let reply = session.recv().expect("a reply");
    assert_eq!(reply["id"], 9);

    session.send(&json!({"jsonrpc": "2.0", "id": null, "method": "initialize", "params": {}}));
    let id = session.recv_error(-32600);
    assert_eq!(id, Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R7 and R13: the codes of §2.4 that are reachable, each with its own trigger.
#[test]
fn error_codes_are_the_table() {
    let mut session = Session::spawn(true);

    // -32002: any request before `initialize`.
    session.request(json!(1), "textDocument/hover", json!({}));
    assert_eq!(session.recv_error(-32002), 1);

    // A notification before `initialize` is still never answered.
    session.notify("textDocument/didOpen", json!({}));
    session.request(json!(2), "initialize", json!({}));
    let result = session.recv().expect("a reply");
    assert_eq!(result["id"], 2);
    assert!(result.get("result").is_some());

    // -32600: `initialize` twice.
    session.request(json!(3), "initialize", json!({}));
    assert_eq!(session.recv_error(-32600), 3);

    // -32600: `jsonrpc` is not "2.0".
    session.send(&json!({"jsonrpc": "1.0", "id": 4, "method": "shutdown"}));
    assert_eq!(session.recv_error(-32600), 4);

    // -32601: an unknown method, and a `$/…` **request**.
    session.request(json!(5), "textDocument/rename", json!({}));
    assert_eq!(session.recv_error(-32601), 5);
    session.request(json!(6), "$/something", json!({}));
    assert_eq!(session.recv_error(-32601), 6);

    // -32602: `hover` without a numeric `position`, `formatting` without `uri`.
    session.request(
        json!(7),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///x"}}),
    );
    assert_eq!(session.recv_error(-32602), 7);
    session.request(json!(8), "textDocument/formatting", json!({}));
    assert_eq!(session.recv_error(-32602), 8);

    // `shutdown` answers `null`, and every later request is `-32600`.
    session.request(json!(9), "shutdown", json!({}));
    assert_eq!(session.recv_result(), Value::Null);
    session.request(json!(10), "window/workDoneProgress/create", json!({}));
    assert_eq!(session.recv_error(-32600), 10);

    session.notify("exit", json!({}));
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(0), "shutdown then exit is 0");
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// R7: `window/workDoneProgress/create` is a request, so it must be answered —
/// a request with no reply hangs a client.
#[test]
fn work_done_progress_is_answered() {
    let mut session = Session::spawn(true);
    session.initialize();
    session.request(
        json!(2),
        "window/workDoneProgress/create",
        json!({"token": "t"}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R7: an unknown notification writes nothing at all, and the session continues.
#[test]
fn unknown_notification_is_dropped() {
    let mut session = Session::spawn(true);
    session.initialize();
    session.notify("$/setTrace", json!({"value": "messages"}));
    session.notify("$/cancelRequest", json!({"id": 1}));
    session.notify("workspace/didChangeConfiguration", json!({"settings": {}}));
    session.notify("some/unknownNotification", json!({"x": 1}));
    session.request(json!(3), "window/workDoneProgress/create", json!({}));
    // If any of the four notifications had been answered, the frame after this
    // request would be that reply instead of `shutdown`'s.
    let reply = session.recv().expect("a reply");
    assert_eq!(reply["id"], 3);
    session.request(json!(4), "shutdown", json!({}));
    let reply = session.recv().expect("a reply");
    assert_eq!(reply["id"], 4);
    assert_eq!(reply["result"], Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(0));
}

/// R7: a body that is not UTF-8, not JSON, or not an object gets its own code,
/// and the server keeps serving — the length was known, so the next frame's
/// offset is too.
#[test]
fn malformed_body_answers_the_right_code_and_serves() {
    let mut session = Session::spawn(true);
    session.initialize();

    // Not JSON at all.
    session.send_body(b"{");
    assert_eq!(session.recv_error(-32700), Value::Null);

    // Not UTF-8 (`0xff` never appears in valid UTF-8).
    session.send_body(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\"}");
    assert_eq!(session.recv_error(-32700), Value::Null);

    // JSON, but not an object.
    session.send_body(b"[]");
    assert_eq!(session.recv_error(-32600), Value::Null);

    // A number is JSON too, and is no more an object.
    session.send_body(b"3");
    assert_eq!(session.recv_error(-32600), Value::Null);

    // Still serving: a well-formed request answers.
    session.request(json!(2), "window/workDoneProgress/create", json!({}));
    assert_eq!(session.recv_result(), Value::Null);
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// R12, S6: a body over the cap is refused **before it is read or allocated**,
/// so the answer precedes any attempt to consume the bytes — even when the
/// client never sends them.
#[test]
fn oversized_body_is_refused_before_reading() {
    let started = Instant::now();
    let mut session = Session::spawn(true);
    {
        // The first and only frame is a header only, over the cap: nothing of a
        // body is written, so an answer proves the declared length was refused
        // before any read or allocation.
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"Content-Length: 33619969\r\n\r\n")
            .expect("a header");
        stdin.flush().expect("a flush");
    }
    let (declared, body) = session.frame().expect("the refusal");
    assert_eq!(declared, body.len());
    let refusal: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(refusal["error"]["code"], -32700);
    assert_eq!(refusal["id"], Value::Null);
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.contains("the body is too long"), "{stderr:?}");
    assert!(started.elapsed() < Duration::from_secs(10));

    // The same with a value that cannot even be a `usize` body.
    let started = Instant::now();
    let mut session = Session::spawn(true);
    {
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"Content-Length: 4294967295\r\n\r\n")
            .expect("a header");
        stdin.flush().expect("a flush");
    }
    let (_, body) = session.frame().expect("the refusal");
    let refusal: Value = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(refusal["error"]["code"], -32700);
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(!stderr.is_empty(), "the reason goes to stderr");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// R10: the header cap includes the terminator, so 8 192 bytes are served and
/// 8 193 are refused.
#[test]
fn header_cap_boundaries() {
    // A body of `{}` is not a request, but the frame itself is readable: the
    // reply is -32600, which proves the header was accepted.
    let body = b"{}";
    let mut at_cap = Vec::new();
    at_cap.extend_from_slice(b"Content-Length: 2\r\n");
    // One padding header line to land the block on exactly 8 192 bytes.
    let used = at_cap.len() + 2; // the blank line that ends the block
    let padding = 8_192 - used - "X-Pad: \r\n".len();
    at_cap.extend_from_slice(format!("X-Pad: {}\r\n", "0".repeat(padding)).as_bytes());
    at_cap.extend_from_slice(b"\r\n");
    assert_eq!(at_cap.len(), 8_192);
    at_cap.extend_from_slice(body);

    let mut session = Session::spawn(true);
    {
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin.write_all(&at_cap).expect("a maximal header");
        stdin.flush().expect("a flush");
    }
    assert_eq!(session.recv_error(-32600), Value::Null);
    session.request(json!(2), "initialize", json!({}));
    assert!(session.recv().expect("a reply").get("result").is_some());
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));

    // One byte more, with no terminator yet: refused, answered, exit 1.
    let mut over_cap = Vec::new();
    over_cap.extend_from_slice(b"Content-Length: 2\r\n");
    let used = over_cap.len() + 2;
    let padding = 8_193 - used - "X-Pad: \r\n".len();
    over_cap.extend_from_slice(format!("X-Pad: {}\r\n", "0".repeat(padding)).as_bytes());
    over_cap.extend_from_slice(b"\r\n");
    assert_eq!(over_cap.len(), 8_193);
    let mut session = Session::spawn(true);
    {
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin.write_all(&over_cap).expect("an over-long header");
        stdin.flush().expect("a flush");
    }
    assert_eq!(session.recv_error(-32700), Value::Null);
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(
        stderr.contains("the header block is too long"),
        "{stderr:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. Sync, positions and diagnostics (R14–R24, S2, S4)
// ---------------------------------------------------------------------------

/// R14, R19, R20: the `=` diagnostic is at the exact 0-based line, its UTF-16
/// unit comes from the `😀` fixture, and its message is the CLI's sentence with
/// no location prefix and no `<path>:`.
#[test]
fn diagnostics_carry_severity_source_and_message() {
    let mut session = Session::spawn(true);
    session.initialize();
    // P1 proves the line and `col - 1`: the `=` is at 1-based 3:8.
    let diagnostics = session.open("file:///p1", P1, 1);
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["severity"], 1);
    assert_eq!(diagnostic["source"], "blastit");
    assert!(
        diagnostic.get("code").is_none(),
        "no `code` key at all, not null"
    );
    assert!(diagnostic.get("tags").is_none());
    assert!(diagnostic.get("relatedInformation").is_none());
    assert_eq!(diagnostic["message"], EQUALS);
    assert_eq!(diagnostic["range"]["start"]["line"], 2);
    assert_eq!(diagnostic["range"]["start"]["character"], 7);
    assert_eq!(diagnostic["range"]["end"]["character"], 8);

    // P2 is the only unit oracle: `😀` is one char, two units, so 20 not 19.
    let diagnostics = session.open("file:///p2", P2, 1);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 0);
    assert_eq!(diagnostics[0]["range"]["start"]["character"], 20);

    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// R14, P2: the only unit oracle. `😀` is one `char` and two UTF-16 units, so
/// the `=` at 0-based `character` 20 is 19 by `col - 1` and 22 by a byte count.
#[test]
fn positions_are_utf16() {
    let mut session = Session::spawn(true);
    session.initialize();
    let diagnostics = session.open("file:///utf16", P2, 0);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 0);
    assert_eq!(diagnostics[0]["range"]["start"]["character"], 20);
    assert_eq!(diagnostics[0]["range"]["end"]["line"], 0);
    assert_eq!(diagnostics[0]["range"]["end"]["character"], 21);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R14, R15: `# ✓ ok` proves the 0-based line and `col - 1`; a document over
/// `MAX_SOURCE_BYTES` proves `Position::NONE` is `(0,0)-(0,0)`.
#[test]
fn positions_line_split_and_none() {
    let mut session = Session::spawn(true);
    session.initialize();
    // The line is 2 (0-based) and the column is the `col - 1` of 8, not the
    // three UTF-8 bytes of `✓` on a line the error is not even on.
    let diagnostics = session.open("file:///lines", P1, 0);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 2);
    assert_eq!(diagnostics[0]["range"]["start"]["character"], 7);

    // One byte over the parser's cap: refused from the length, at `(0,0)`.
    let oversized = format!("#{}", "x".repeat(16_777_216));
    assert_eq!(oversized.len(), 16_777_217);
    let diagnostics = session.open("file:///huge", &oversized, 0);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["severity"], 1);
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    assert_eq!(
        diagnostics[0]["range"]["end"],
        json!({"line": 0, "character": 0})
    );
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .expect("a message")
            .contains("limited to 16777216 bytes"),
        "{}",
        diagnostics[0]["message"]
    );
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R16: the squiggle covers the word, and punctuation covers exactly one unit.
#[test]
fn squiggle_covers_the_word_or_one_unit() {
    let mut session = Session::spawn(true);
    session.initialize();
    let diagnostics = session.open("file:///word", "viewx \"x\"\n", 0);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    assert_eq!(
        diagnostics[0]["range"]["end"],
        json!({"line": 0, "character": 5})
    );
    let diagnostics = session.open("file:///equals", P1, 0);
    assert_eq!(
        diagnostics[0]["range"]["end"]["character"],
        diagnostics[0]["range"]["start"]["character"]
            .as_u64()
            .expect("a unit")
            + 1
    );
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R22, D7: seven warnings on seven declarations, each on the line the fixture
/// writes its word on, and each sentence the boot's own.
#[test]
fn warning_sites_are_their_declarations() {
    let fixture_a = "entrypoint php \"public/index.php\"\n\n\
                     postgres $DATABASE_URL\n\
                     postgres analytics $ANALYTICS_URL\n";
    let fixture_b = "postgres $DATABASE_URL\n\n\
                     trait session {\n    postgres { transactions }\n}\n\n\
                     view open \"/a\" {\n    body raw\n}\n\n\
                     view wide \"/b\" {\n    postgres unrestricted\n}\n\n\
                     view outbound \"/c\" {\n    egress any\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let a = session.open("file:///a", fixture_a, 0);
    let b = session.open("file:///b", fixture_b, 0);

    // The seven fragments, the line each one's word is on, and the sentence.
    let expectations: [(&str, u64, &str); 7] = [
        (
            "is reached by no view",
            0,
            "entrypoint php \"public/index.php\" is reached by no view: declare a `view` that forwards to it",
        ),
        (
            "store \"db\" has no grant",
            2,
            "store \"db\" has no grant in any view or trait: it is unusable",
        ),
        (
            "store \"analytics\" has no grant",
            3,
            "store \"analytics\" has no grant in any view or trait: it is unusable",
        ),
        (
            "is attached to no view",
            2,
            "trait \"session\" is attached to no view and no global traits: it grants nothing",
        ),
        (
            "passes the request body through unvalidated",
            7,
            "view \"open\": body raw passes the request body through unvalidated",
        ),
        (
            "may run any statement",
            11,
            "view \"wide\" may run any statement on store \"db\" (unrestricted)",
        ),
        (
            "egress any opens any outbound address",
            15,
            "view \"outbound\": egress any opens any outbound address and port",
        ),
    ];
    let all: Vec<Value> = a.iter().chain(b.iter()).cloned().collect();
    let mut sites = BTreeSet::new();
    for (fragment, line, sentence) in expectations {
        let matching: Vec<&Value> = all
            .iter()
            .filter(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(fragment))
            })
            .collect();
        assert_eq!(matching.len(), 1, "exactly one diagnostic for {fragment:?}");
        let diagnostic = matching[0];
        assert_eq!(diagnostic["severity"], 2, "{fragment:?}");
        assert_eq!(diagnostic["source"], "blastit", "{fragment:?}");
        assert_eq!(diagnostic["message"], sentence, "{fragment:?}");
        assert_eq!(
            diagnostic["range"]["start"]["line"], line,
            "{fragment:?} is on its own line"
        );
        // The boot's sentence carries no `line:col` of its own.
        let position = diagnostic["range"]["start"]["line"]
            .as_u64()
            .expect("a line")
            .to_string();
        assert!(
            !sentence.contains(&format!(":{position}")),
            "the message must not carry a position"
        );
        sites.insert((
            diagnostic["range"]["start"]["line"]
                .as_u64()
                .expect("a line"),
            diagnostic["range"]["start"]["character"]
                .as_u64()
                .expect("a unit"),
        ));
    }
    assert_eq!(sites.len(), 7, "seven distinct sites, not one");
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R21, D8: a warning's `Display` is the boot's sentence byte for byte — no
/// `line:col` was added to any variant, and the editor shows the same words the
/// boot's log line carries.
#[test]
fn warning_display_is_unchanged() {
    let fixture_a = "entrypoint php \"public/index.php\"\n\n\
                     postgres $DATABASE_URL\n\
                     postgres analytics $ANALYTICS_URL\n";
    let fixture_b = "postgres $DATABASE_URL\n\n\
                     trait session {\n    postgres { transactions }\n}\n\n\
                     view open \"/a\" {\n    body raw\n}\n\n\
                     view wide \"/b\" {\n    postgres unrestricted\n}\n\n\
                     view outbound \"/c\" {\n    egress any\n}\n";
    let expected = [
        "entrypoint php \"public/index.php\" is reached by no view: declare a `view` that forwards to it",
        "store \"db\" has no grant in any view or trait: it is unusable",
        "store \"analytics\" has no grant in any view or trait: it is unusable",
        "trait \"session\" is attached to no view and no global traits: it grants nothing",
        "view \"open\": body raw passes the request body through unvalidated",
        "view \"wide\" may run any statement on store \"db\" (unrestricted)",
        "view \"outbound\": egress any opens any outbound address and port",
    ];
    let mut session = Session::spawn(true);
    session.initialize();
    let mut messages: Vec<String> = session
        .open("file:///d8a", fixture_a, 0)
        .iter()
        .chain(session.open("file:///d8b", fixture_b, 0).iter())
        .map(|diagnostic| {
            diagnostic["message"]
                .as_str()
                .expect("a message")
                .to_owned()
        })
        .collect();
    messages.sort();
    let mut expected: Vec<String> = expected
        .iter()
        .map(|sentence| (*sentence).to_owned())
        .collect();
    expected.sort();
    assert_eq!(messages, expected);
    for message in &messages {
        // No `line:col` shape anywhere: the position is the range, not the text.
        let bytes = message.as_bytes();
        assert!(
            !bytes.windows(3).any(|window| {
                window[0].is_ascii_digit() && window[1] == b':' && window[2].is_ascii_digit()
            }),
            "{message}"
        );
    }
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R24, D6: 600 refusals publish 512 diagnostics, and the last is the count of
/// what was left out — not 600 squiggles on a 600-line file.
#[test]
fn diagnostics_are_capped() {
    let mut text = String::from("postgres $DATABASE_URL\n\n");
    for index in 0..600 {
        text.push_str(&format!(
            "view v{index} \"/x{index}\" {{\n    postgres nosuchstore {{ select x }}\n}}\n"
        ));
    }
    let mut session = Session::spawn(true);
    session.initialize();
    let diagnostics = session.open("file:///cap", &text, 0);
    assert_eq!(diagnostics.len(), 512);
    for diagnostic in &diagnostics[..511] {
        assert_eq!(diagnostic["severity"], 1);
        assert_eq!(
            diagnostic["range"]["start"],
            json!({"line": 0, "character": 0}),
            "a `Location`-carrying refusal has no site"
        );
    }
    let last = &diagnostics[511];
    assert_eq!(last["severity"], 2);
    assert_eq!(
        last["message"],
        "89 more problems; run `blastit check` for the full list"
    );
    assert_eq!(last["range"]["start"], json!({"line": 0, "character": 0}));
    // The session is still alive after a capped publish.
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///cap"}, "position": {"line": 0, "character": 0}}),
    );
    assert!(session.recv().expect("a reply").get("result").is_some());
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R8: the 33rd open document gets the visible notice, and the 32nd still
/// answers a request.
#[test]
fn open_documents_are_capped() {
    let clean = CLEAN;
    let mut session = Session::spawn(true);
    session.initialize();
    for index in 0..32 {
        let diagnostics = session.open(&format!("file:///doc{index}"), clean, index);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    let diagnostics = session.open("file:///doc32", clean, 32);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["severity"], 2);
    assert_eq!(
        diagnostics[0]["message"],
        "blastit lsp tracks at most 32 open Blastfiles; close one"
    );
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    // The 32nd document is still there.
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///doc31"}, "position": {"line": 2, "character": 0}}),
    );
    let result = session.recv_result();
    assert!(result["contents"]["value"].as_str().is_some());
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R8: an oversized document is not stored — a later request for it answers
/// `null`, and the notice carries no `version`.
#[test]
fn oversized_document_is_not_stored() {
    let oversized = format!("#{}", "x".repeat(16_777_216));
    let mut session = Session::spawn(true);
    session.initialize();
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": "file:///huge",
            "languageId": "blastfile",
            "version": 3,
            "text": oversized,
        }}),
    );
    let params = session.publish();
    assert!(
        params.get("version").is_none(),
        "a session notice carries no version: {params}"
    );
    let diagnostics = params["diagnostics"].as_array().expect("a list").clone();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///huge"}, "position": {"line": 0, "character": 1}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R8: a `didChange` for an unknown `uri` never inserts, so the map cannot grow
/// past 32 — a fresh `didOpen` still gets the notice afterwards.
#[test]
fn did_change_for_unknown_uri_does_not_grow_the_map() {
    let clean = CLEAN;
    let mut session = Session::spawn(true);
    session.initialize();
    for index in 0..40 {
        session.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": format!("file:///ghost{index}"), "version": 1},
                "contentChanges": [{"text": clean}],
            }),
        );
        let diagnostics = session.publish();
        assert_eq!(
            diagnostics["diagnostics"][0]["message"],
            "blastit lsp has no open document for this file; reopen it"
        );
    }
    for index in 0..32 {
        let diagnostics = session.open(&format!("file:///doc{index}"), clean, 0);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    let diagnostics = session.open("file:///doc32", clean, 0);
    assert_eq!(
        diagnostics.len(),
        1,
        "the map holds 32 entries, not 32 + 40"
    );
    assert_eq!(
        diagnostics[0]["message"],
        "blastit lsp tracks at most 32 open Blastfiles; close one"
    );
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R8: an incremental `didChange` is refused visibly, and the old text's
/// diagnostics are gone with it.
#[test]
fn incremental_change_is_refused_visibly() {
    let mut session = Session::spawn(true);
    session.initialize();
    let diagnostics = session.open("file:///inc", P1, 1);
    assert_eq!(diagnostics.len(), 1);
    session.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": "file:///inc", "version": 2},
            "contentChanges": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "text": "x"}],
        }),
    );
    let params = session.publish();
    assert!(params.get("version").is_none());
    let diagnostics = params["diagnostics"].as_array().expect("a list").clone();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["severity"], 2);
    assert_eq!(
        diagnostics[0]["message"],
        "incremental sync is not supported; reopen the file"
    );
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///inc"}, "position": {"line": 2, "character": 7}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R18, R23: `didChange` replaces the whole text and echoes the new version;
/// `didClose` publishes `[]` with no `version`.
#[test]
fn did_change_replaces_and_did_close_clears() {
    let mut session = Session::spawn(true);
    session.initialize();
    let diagnostics = session.open("file:///change", P1, 4);
    assert_eq!(diagnostics.len(), 1);
    session.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": "file:///change", "version": 9},
            "contentChanges": [{"text": "view v \"/x\" {\n    body json\n}\n"}],
        }),
    );
    let params = session.publish();
    assert_eq!(params["version"], 9);
    assert_eq!(params["diagnostics"], json!([]));
    session.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": "file:///change"}}),
    );
    let params = session.publish();
    assert_eq!(params["uri"], "file:///change");
    assert_eq!(params["diagnostics"], json!([]));
    assert!(params.get("version").is_none(), "{params}");
    // The document is gone: a hover for it is `null`, not an error.
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///change"}, "position": {"line": 0, "character": 0}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// 3. Formatting (R25–R27)
// ---------------------------------------------------------------------------

/// R25, R26: a canonical buffer is `[]` — a no-op edit would mark the buffer
/// dirty and fight `format_on_save`.
#[test]
fn formatting_equal_is_empty() {
    let canonical = "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///canonical", canonical, 0);
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///canonical"}, "options": {"tabSize": 2, "insertSpaces": true}}),
    );
    assert_eq!(session.recv_result(), json!([]));
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R26, F2: a mis-indented buffer is exactly one edit, ending at
/// `max_point_utf16` of the **old** text, whose `newText` is `fmt`'s output.
#[test]
fn formatting_diff_is_one_edit() {
    let canonical = "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n";
    let misindented = "app \"shop\"\n\nview v \"/x\" {\n  body json\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///misindented", misindented, 0);
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///misindented"}, "options": {}}),
    );
    let edits = session.recv_result();
    let edits = edits.as_array().expect("a list");
    assert_eq!(edits.len(), 1, "one edit, one undo step");
    let edit = &edits[0];
    assert_eq!(edit["range"]["start"], json!({"line": 0, "character": 0}));
    // The edit replaces the old region: five `\n` in the input, so end is (5,0).
    assert_eq!(edit["range"]["end"], json!({"line": 5, "character": 0}));
    assert_eq!(edit["newText"], canonical);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R25, F4: a broken buffer is `null`, never a JSON-RPC error — an error reply
/// makes an editor report a server failure on every save — and the session is
/// alive after declining.
#[test]
fn formatting_broken_is_null() {
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///broken", P1, 0);
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///broken"}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    // ...and the very next request answers.
    session.request(
        json!(3),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///broken"}, "position": {"line": 2, "character": 2}}),
    );
    assert!(session.recv().expect("a reply").get("result").is_some());
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R27, F5: a CRLF buffer is normalised to LF in one save, and the next request
/// on the formatted text is `[]` — the fixed point is reached once.
#[test]
fn formatting_normalises_crlf_once() {
    let crlf = "app \"shop\"\r\n\r\nview v \"/x\" {\r\n    body json\r\n}\r\n";
    let lf = "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///crlf", crlf, 0);
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///crlf"}}),
    );
    let edits = session.recv_result();
    let edits = edits.as_array().expect("a list");
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let new_text = edit["newText"].as_str().expect("a newText");
    assert!(!new_text.contains('\r'), "the endings are normalised");
    assert_eq!(new_text, lf);
    // Five `\n` in the CRLF input.
    assert_eq!(edit["range"]["end"], json!({"line": 5, "character": 0}));

    // Second request, on the formatted text: nothing left to do.
    let _ = session.open("file:///crlf2", new_text, 1);
    session.request(
        json!(3),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///crlf2"}}),
    );
    assert_eq!(session.recv_result(), json!([]));
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// 4. Hover (R30–R34)
// ---------------------------------------------------------------------------

/// R30, R31: a directive word returns the table's markdown over its own span,
/// and a name returns `null` because a name is not a directive.
#[test]
fn hover_finds_the_word() {
    let text = "view my_view \"/x\" {\n    postgres $DATABASE_URL\n    postgres unrestricted\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///hover", text, 0);
    // `unrestricted` on line 2 (0-based), starting at character 13.
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///hover"}, "position": {"line": 2, "character": 16}}),
    );
    let hover = session.recv_result();
    let value = hover["contents"]["value"].as_str().expect("markdown");
    assert!(value.contains("every statement"), "{value}");
    assert!(value.contains("warns at boot"), "{value}");
    assert_eq!(hover["contents"]["kind"], "markdown");
    assert_eq!(hover["range"]["start"], json!({"line": 2, "character": 13}));
    assert_eq!(hover["range"]["end"], json!({"line": 2, "character": 25}));

    // The `view` keyword at line 0, character 0.
    session.request(
        json!(3),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///hover"}, "position": {"line": 0, "character": 0}}),
    );
    let hover = session.recv_result();
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("markdown")
            .contains("one sandbox")
    );
    assert_eq!(hover["range"]["end"], json!({"line": 0, "character": 4}));

    // `my_view` is a name: no directive, so `null`.
    session.request(
        json!(4),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///hover"}, "position": {"line": 0, "character": 6}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R32: punctuation, whitespace and an unopened document are `null`, never an
/// error.
#[test]
fn hover_unknown_is_null() {
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///punct", "view v \"/x\" {\n    body = json\n}\n", 0);
    for character in [9u64, 10] {
        session.request(
            json!(character),
            "textDocument/hover",
            json!({"textDocument": {"uri": "file:///punct"}, "position": {"line": 1, "character": character}}),
        );
        assert_eq!(session.recv_result(), Value::Null, "character {character}");
    }
    session.request(
        json!(20),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///not-open"}, "position": {"line": 0, "character": 0}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R30, H3, S2's neighbour: `$IDENT` gets the one `$NAME` paragraph, with the
/// name interpolated and never its value.
#[test]
fn hover_env_var_paragraph() {
    let text = "postgres $DATABASE_URL\n\npostgres analytics $ANALYTICS_URL\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///env", text, 0);
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///env"}, "position": {"line": 2, "character": 20}}),
    );
    let hover = session.recv_result();
    let value = hover["contents"]["value"].as_str().expect("markdown");
    assert!(value.contains("$ANALYTICS_URL"), "{value}");
    assert!(value.contains("stays in the environment"), "{value}");
    assert!(!value.contains("postgres://"), "never the value: {value}");
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R17, H5: the inverse conversion — UTF-16 `character` 6 in `"😀 view"` is the
/// `w`, so the reply is the `view` sentence; 20 in the `😀` fixture is the `=`,
/// so the reply is `null`. `character as usize` gives `null` for both.
#[test]
fn hover_inverse_utf16() {
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///emoji", "😀 view", 0);
    session.request(
        json!(2),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///emoji"}, "position": {"line": 0, "character": 6}}),
    );
    let hover = session.recv_result();
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("markdown")
            .contains("one sandbox"),
        "{hover}"
    );
    assert_eq!(hover["range"]["start"], json!({"line": 0, "character": 3}));
    assert_eq!(hover["range"]["end"], json!({"line": 0, "character": 7}));

    let _ = session.open("file:///emoji2", P2, 0);
    session.request(
        json!(3),
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///emoji2"}, "position": {"line": 0, "character": 20}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// R33, R34: definition is declined with `null` and is not advertised — a probe
/// of an unadvertised capability is not a protocol violation.
#[test]
fn definition_is_declined_and_unadvertised() {
    let mut session = Session::spawn(true);
    session.request(json!(1), "initialize", json!({}));
    let result = session.recv_result();
    assert!(result["capabilities"].get("definitionProvider").is_none());
    session.request(
        json!(2),
        "textDocument/definition",
        json!({"textDocument": {"uri": "file:///x"}, "position": {"line": 0, "character": 0}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// 4a. The Quick Fix and the two SQL tokens (R60–R62, `MB.34.5`)
// ---------------------------------------------------------------------------

/// The page's blockquote paragraph that starts with `first`, with its wrapped
/// `> ` lines joined by one space.
///
/// The two SQL hovers must be the reference page's own words (§7.6, R68), so
/// the critical suite compares the reply against the page rather than against a
/// third copy of the sentence: a page edit the editor did not follow is red
/// here, which is the drift signal the design asks for.
fn page_paragraph(first: &str) -> String {
    let page = include_str!("../../../docs/reference/blastfile.md");
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in page.lines() {
        match line.strip_prefix("> ") {
            Some(text) => {
                let joined = current.get_or_insert_with(String::new);
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(text);
            }
            None => {
                if let Some(joined) = current.take() {
                    paragraphs.push(joined);
                }
            }
        }
    }
    if let Some(joined) = current {
        paragraphs.push(joined);
    }
    paragraphs
        .into_iter()
        .find(|paragraph| paragraph.starts_with(first))
        .unwrap_or_else(|| panic!("the page has no blockquote starting {first:?}"))
}

/// The byte range of the string interior that begins after `open`: from the
/// first character inside the opening quote to the closing quote (§7A.3).
fn interior(text: &str, open: &str) -> (usize, usize) {
    let start = text.find(open).expect("the opening quote") + open.len();
    let quote = open.chars().last().expect("a quote");
    let end = start + text[start..].find(quote).expect("the closing quote");
    (start, end)
}

/// The LSP point of a byte offset, computed here rather than by the server, so
/// a byte/`char`/UTF-16 mix-up is red.
fn point_at(text: &str, byte: usize) -> Value {
    let line_start = text[..byte].rfind('\n').map_or(0, |at| at + 1);
    let line = text[..line_start].matches('\n').count();
    let character: usize = text[line_start..byte].chars().map(char::len_utf16).sum();
    json!({"line": line, "character": character})
}

/// Applies one `TextEdit` to `text` by its two byte offsets.
fn apply(text: &str, start: usize, end: usize, new_text: &str) -> String {
    format!("{}{}{}", &text[..start], new_text, &text[end..])
}

/// The request body for one `codeAction` caret.
fn caret(uri: &str, line: u64, character: u64) -> Value {
    json!({
        "textDocument": {"uri": uri},
        "range": {
            "start": {"line": line, "character": character},
            "end": {"line": line, "character": character},
        },
    })
}

/// C1, R62: `codeActionProvider` is advertised, it is the boolean `true` (an
/// object would advertise kinds), and it is the only key this amendment adds —
/// no `codeActionKinds`, no `codeAction/resolve`.
#[test]
fn code_action_provider_is_advertised_and_alone() {
    let mut session = Session::spawn(true);
    session.request(json!(1), "initialize", json!({}));
    let result = session.recv_result();
    let capabilities = result["capabilities"].as_object().expect("capabilities");
    assert_eq!(capabilities.get("codeActionProvider"), Some(&json!(true)));
    let mut keys: Vec<&str> = capabilities.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "codeActionProvider",
            "documentFormattingProvider",
            "hoverProvider",
            "positionEncoding",
            "textDocumentSync",
        ],
        "codeActionProvider is the only key the Quick Fix adds"
    );
    let document = result.to_string();
    assert!(
        !document.contains("codeActionKinds") && !document.contains("resolveProvider"),
        "{document}"
    );
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// C2, R62: a psql paste (`$1`) gets exactly one action — the exact title and
/// kind, one uri, one `TextEdit` over the **whole interior** — and applying it
/// re-parses clean, so the squiggle goes.
#[test]
fn a_literal_query_gets_one_quick_fix() {
    let text = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        \
                query \"SELECT id FROM products WHERE email = $1\"\n    }\n}\n";
    let (start, end) = interior(text, "query \"");
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///quickfix", text, 0);
    session.request(
        json!(2),
        "textDocument/codeAction",
        caret("file:///quickfix", 4, 20),
    );
    let result = session.recv_result();
    let actions = result.as_array().expect("an array").clone();
    assert_eq!(actions.len(), 1, "exactly one action: {result}");
    let action = &actions[0];
    assert_eq!(action["title"], "Parameterize SQL for Blastfile ($?)");
    assert_eq!(action["kind"], "quickfix");
    // No second opinion: no `diagnostics`, no `isPreferred`, no `command`.
    for absent in ["diagnostics", "isPreferred", "disabled", "command", "data"] {
        assert!(action.get(absent).is_none(), "{absent} must not be emitted");
    }
    let changes = action["edit"]["changes"].as_object().expect("changes");
    assert_eq!(changes.len(), 1, "one document, never a second authority");
    let edits = changes["file:///quickfix"].as_array().expect("edits");
    assert_eq!(edits.len(), 1, "one undo step, one edit: {result}");
    let edit = &edits[0];
    assert_eq!(edit["range"]["start"], point_at(text, start));
    assert_eq!(edit["range"]["end"], point_at(text, end));
    let new_text = edit["newText"].as_str().expect("newText");
    assert!(new_text.contains("$?"), "{new_text}");
    assert!(!new_text.contains("$1"), "{new_text}");
    assert_ne!(
        new_text,
        &text[start..end],
        "a real change, not an identity edit"
    );

    let applied = apply(text, start, end, new_text);
    assert!(
        blastit_manifest::blastfile::check(&applied, blastit_manifest::UidLayout::DEFAULT_BASE)
            .is_ok(),
        "the applied edit must re-check clean: {applied}"
    );
    let _ = session.open("file:///applied", &applied, 1);
    session.request(
        json!(3),
        "textDocument/codeAction",
        caret("file:///applied", 4, 20),
    );
    assert_eq!(session.recv_result(), json!([]), "nothing left to fix");
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// One `query`'s document, so a `check` of an applied edit answers for that
/// statement alone (`MB.34.5`'s C3/C4 fixtures).
fn with_query(statement: &str) -> String {
    format!(
        "postgres $DATABASE_URL\n\nview v \"/x\" {{\n    postgres {{\n        {statement}\n    }}\n}}\n"
    )
}

/// C3, R62: both spellings are edited. A `"…"` interior must come back escaped
/// and still legal, and a `'…'` interior takes the rewrite verbatim; each range
/// is the whole interior, so the second `$?` of the `'…'` statement is inside
/// the edit rather than a literal-only patch.
#[test]
fn both_spellings_are_parameterized() {
    let mut session = Session::spawn(true);
    session.initialize();

    let cases = [
        (
            "file:///double",
            "query \"SELECT id FROM t WHERE email = 'a@b.com'\"",
            "SELECT id FROM t WHERE email = $?",
            false,
        ),
        (
            "file:///single",
            "query 'UPDATE t SET a = $1 WHERE b = $2'",
            "UPDATE t SET a = $? WHERE b = $?",
            true,
        ),
    ];
    for (uri, statement, expected, single) in cases {
        let text = with_query(statement);
        let open = if single { "query '" } else { "query \"" };
        let (start, end) = interior(&text, open);
        let _ = session.open(uri, &text, 0);
        session.request(json!(7), "textDocument/codeAction", caret(uri, 4, 20));
        let result = session.recv_result();
        let actions = result.as_array().expect("an array");
        assert_eq!(actions.len(), 1, "{statement}: {result}");
        let edit = &actions[0]["edit"]["changes"][uri]
            .as_array()
            .expect("edits")[0];
        assert_eq!(edit["range"]["start"], point_at(&text, start), "{uri}");
        assert_eq!(edit["range"]["end"], point_at(&text, end), "{uri}");
        assert_eq!(edit["newText"], expected, "{uri}");
        // The property is asserted on the server's own bytes, not on the local
        // `expected`: a rewrite that kept the value or the numbering reds here.
        let new_text = edit["newText"].as_str().expect("newText");
        if single {
            assert!(
                !new_text.contains("$1") && !new_text.contains("$2"),
                "{new_text}"
            );
        } else {
            assert!(!new_text.contains('@'), "the value is gone: {new_text}");
        }
        let applied = apply(&text, start, end, new_text);
        assert!(
            blastit_manifest::blastfile::check(&applied, blastit_manifest::UidLayout::DEFAULT_BASE)
                .is_ok(),
            "{uri} must re-check clean: {applied}"
        );
    }
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// C4, R62: nothing is offered where nothing can change — a clean `query`, the
/// protocol's own `SET TIME ZONE 'UTC'` (which keeps its literal, ADR-0003),
/// and a redacted `…`.
#[test]
fn nothing_is_offered_where_nothing_can_change() {
    let mut session = Session::spawn(true);
    session.initialize();
    let cases = [
        ("file:///clean", "query 'SELECT id FROM t'"),
        ("file:///setup", "query \"SET TIME ZONE 'UTC'\""),
        ("file:///redacted", "query 'SELECT … FROM'"),
    ];
    for (uri, statement) in cases {
        let _ = session.open(uri, &with_query(statement), 0);
        session.request(json!(uri), "textDocument/codeAction", caret(uri, 4, 20));
        assert_eq!(session.recv_result(), json!([]), "{statement}");
    }
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// C5, R62: `context.diagnostics` is never the source of truth. An empty set
/// and a fabricated one get the same one action, because the server re-derives
/// the finding from its own tracked text.
#[test]
fn context_diagnostics_is_never_trusted() {
    let text = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        \
                query \"SELECT id FROM t WHERE email = $1\"\n    }\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///ctx", text, 0);
    let mut answers = Vec::new();
    for context in [
        json!({"diagnostics": []}),
        json!({"diagnostics": [{
            "range": {"start": {"line": 9, "character": 9}, "end": {"line": 9, "character": 9}},
            "severity": 1,
            "message": "a fabricated finding the client made up",
        }],
        "only": ["quickfix"]}),
        json!({}),
    ] {
        let mut params = caret("file:///ctx", 4, 20);
        params["context"] = context;
        session.request(json!(5), "textDocument/codeAction", params);
        answers.push(session.recv_result());
    }
    assert_eq!(answers[0].as_array().map(Vec::len), Some(1), "{answers:?}");
    assert_eq!(
        answers[0], answers[1],
        "the fabricated set changed the answer"
    );
    assert_eq!(answers[0], answers[2], "the missing set changed the answer");
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// C6, R62: the range test is per interior, not per line or per document. On
/// one file holding a comment, a `body none`, a `regex '…'` **and** a query,
/// only the query's range offers anything.
#[test]
fn a_range_off_every_query_offers_nothing() {
    let text = "# a comment with $1 in it\npostgres $DATABASE_URL\n\nview v \"/x\" {\n    \
                body none\n    postgres {\n        \
                query \"SELECT id FROM products WHERE email = $1\"\n    }\n}\n\n\
                view r regex '/v[0-9]+/items' {\n    body none\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///ranges", text, 0);
    for (label, line, character) in [
        ("a comment holding $1", 0, 17),
        ("`body none`", 4, 5),
        ("`regex '…'`", 10, 16),
        ("the closing quote of the query", 6, 55),
    ] {
        session.request(
            json!(label),
            "textDocument/codeAction",
            caret("file:///ranges", line, character),
        );
        assert_eq!(session.recv_result(), json!([]), "{label}");
    }
    session.request(
        json!(9),
        "textDocument/codeAction",
        caret("file:///ranges", 6, 20),
    );
    assert_eq!(
        session.recv_result().as_array().map(Vec::len),
        Some(1),
        "the query's own range still offers the fix"
    );
    // A document this session does not track has nothing to offer — and it is
    // `[]`, not an error, so the client is never left hanging (§7A.1).
    session.request(
        json!(10),
        "textDocument/codeAction",
        caret("file:///not-open", 6, 20),
    );
    assert_eq!(session.recv_result(), json!([]));
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// P7, C7, R60: the edit's two ends are UTF-16 points, not `char` or byte
/// counts. The interior holds `'😀'` (one `char`, two UTF-16 units, four bytes)
/// and a tab, so the closing quote is character **70** — a `char` count gives
/// 69 and a byte count 72 — and the file above it starts with a surrogate pair,
/// so a document-wide byte count cannot answer either.
#[test]
fn the_edit_range_is_utf16_across_a_surrogate_pair() {
    let text = "# 😀 a surrogate pair above the query\npostgres $DATABASE_URL\n\n\
                view v \"/x\" {\n    postgres {\n        \
                query \"SELECT id FROM products WHERE tag = '😀'\tAND email = $1\"\n    }\n}\n";
    let (start, end) = interior(text, "query \"");
    assert_eq!(point_at(text, end), json!({"line": 5, "character": 70}));
    assert_eq!(point_at(text, start), json!({"line": 5, "character": 15}));

    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///pair", text, 0);
    session.request(
        json!(2),
        "textDocument/codeAction",
        caret("file:///pair", 5, 20),
    );
    let result = session.recv_result();
    let edit = &result.as_array().expect("an array")[0]["edit"]["changes"]["file:///pair"]
        .as_array()
        .expect("edits")[0];
    assert_eq!(edit["range"]["start"], point_at(text, start));
    assert_eq!(edit["range"]["end"], point_at(text, end));
    // The rewrite as the server returns it: both values became `$?`, and the
    // tab `normalize` folds is a single space by the time the edit exists.
    assert_eq!(
        edit["newText"],
        "SELECT id FROM products WHERE tag = $? AND email = $?"
    );
    let new_text = edit["newText"].as_str().expect("newText");
    let applied = apply(text, start, end, new_text);
    assert!(
        blastit_manifest::blastfile::check(&applied, blastit_manifest::UidLayout::DEFAULT_BASE)
            .is_ok(),
        "the applied edit must re-check clean: {applied}"
    );
    assert!(!new_text.contains("$1"), "{new_text}");
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// C8, R62: unusable params are `-32602` — never a panic and never a silent
/// `[]` — and the malformed request does not end the session.
#[test]
fn code_action_params_are_validated() {
    let text = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        \
                query \"SELECT id FROM t WHERE email = $1\"\n    }\n}\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///params", text, 0);

    session.request(
        json!(2),
        "textDocument/codeAction",
        json!({"textDocument": {"uri": "file:///params"}}),
    );
    assert_eq!(session.recv_error(-32602), json!(2));

    session.request(
        json!(3),
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": "file:///params"},
            "range": {
                "start": {"line": 4, "character": "twenty"},
                "end": {"line": 4, "character": 20},
            },
        }),
    );
    assert_eq!(session.recv_error(-32602), json!(3));

    // No `textDocument` at all is unusable params too.
    session.request(
        json!(4),
        "textDocument/codeAction",
        json!({"range": {"start": {"line": 4, "character": 20}, "end": {"line": 4, "character": 20}}}),
    );
    assert_eq!(session.recv_error(-32602), json!(4));

    // The session still answers, and the well-formed request still works.
    session.request(
        json!(5),
        "textDocument/codeAction",
        caret("file:///params", 4, 20),
    );
    assert_eq!(
        session.recv_result().as_array().map(Vec::len),
        Some(1),
        "a malformed request must not poison the session"
    );
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// H7, R61: inside a `query`, `$?` answers on its `$`, on its `?` and on the
/// character just after it, byte-identically, with a range over exactly the two
/// source characters — and the sentence is the reference page's own blockquote.
#[test]
fn hover_explains_the_placeholder() {
    let text = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        \
                query 'SELECT id FROM t WHERE price > $? AND id IN ($?, ...)'\n    }\n}\n";
    let line = text.lines().nth(4).expect("the query line");
    let first = line.find("$?").expect("the token");
    let expected = page_paragraph("`$?` is one bind placeholder");
    assert!(expected.contains("IN ($?, ...)"), "{expected}");

    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///token", text, 0);
    let mut answers = Vec::new();
    for offset in 0..=2u64 {
        session.request(
            json!(offset),
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///token"},
                "position": {"line": 4, "character": first as u64 + offset},
            }),
        );
        answers.push(session.recv_result());
    }
    for answer in &answers {
        assert_eq!(answer["contents"]["kind"], "markdown");
        assert_eq!(answer["contents"]["value"], expected.as_str());
        assert_eq!(
            answer["range"]["start"],
            json!({"line": 4, "character": 46})
        );
        assert_eq!(answer["range"]["end"], json!({"line": 4, "character": 48}));
    }
    assert_eq!(answers[0], answers[1], "the `?` is part of the token");
    assert_eq!(
        answers[0], answers[2],
        "so is the character just after it, which here is a space inside the string"
    );

    // §7.6's predicate is `start <= byte < end`, so the closing quote itself is
    // not inside the string: on a statement that *ends* with `$?`, hovering the
    // quote (and the character after it) answers `null`, never the token.
    let trailing = with_query("query 'SELECT id FROM t WHERE price > $?'");
    let (_, quote) = interior(&trailing, "query '");
    let quote_line = trailing[..quote].matches('\n').count();
    let line_start = trailing[..quote].rfind('\n').map_or(0, |at| at + 1);
    let quote_character = quote - line_start;
    let _ = session.open("file:///trailing", &trailing, 2);
    for (label, character) in [
        ("the closing quote", quote_character),
        ("just after the closing quote", quote_character + 1),
    ] {
        session.request(
            json!(label),
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///trailing"},
                "position": {"line": quote_line, "character": character},
            }),
        );
        assert_eq!(session.recv_result(), Value::Null, "{label}");
    }
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// H8, R61: `...` answers on all three dots with a range over the three, and
/// neither token answers outside a `query` — a `$?` in a comment or a `regex`
/// gets `null`, because a hover is not a second grammar.
#[test]
fn hover_explains_the_variadic_marker() {
    let text = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        \
                query 'SELECT id FROM t WHERE id IN ($?, ...)'\n    }\n}\n";
    let line = text.lines().nth(4).expect("the query line");
    let dots = line.find("...").expect("the token");
    let expected = page_paragraph("`...` is Blastit's variadic marker");
    assert!(expected.contains("blastit check"), "{expected}");

    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///variadic", text, 0);
    for offset in 0..3u64 {
        session.request(
            json!(offset),
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///variadic"},
                "position": {"line": 4, "character": dots as u64 + offset},
            }),
        );
        let answer = session.recv_result();
        assert_eq!(answer["contents"]["value"], expected.as_str(), "{offset}");
        assert_eq!(
            answer["range"]["start"],
            json!({"line": 4, "character": dots as u64})
        );
        assert_eq!(
            answer["range"]["end"],
            json!({"line": 4, "character": dots as u64 + 3})
        );
    }
    // Just after the last dot is not the token.
    session.request(
        json!(9),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": "file:///variadic"},
            "position": {"line": 4, "character": dots as u64 + 3},
        }),
    );
    assert_eq!(session.recv_result(), Value::Null);

    let outside = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    \
                   body raw # $? and ... live in a comment\n    postgres {\n        \
                   query \"SELECT id FROM t WHERE email = $1\"\n    }\n}\n\n\
                   view r regex '$?' {\n    body none\n}\n";
    let _ = session.open("file:///outside", outside, 1);
    for (label, line, character) in [
        ("`$?` in a comment", 3, 15),
        ("`...` in a comment", 3, 21),
        ("the `...`'s trailing position", 3, 22),
        ("`regex '$?'`", 9, 14),
    ] {
        session.request(
            json!(label),
            "textDocument/hover",
            json!({
                "textDocument": {"uri": "file:///outside"},
                "position": {"line": line, "character": character},
            }),
        );
        assert_eq!(session.recv_result(), Value::Null, "{label}");
    }
    // The `$DATABASE_URL` paragraph still answers: the word table is untouched.
    session.request(
        json!(20),
        "textDocument/hover",
        json!({
            "textDocument": {"uri": "file:///outside"},
            "position": {"line": 0, "character": 12},
        }),
    );
    let env = session.recv_result();
    assert_eq!(env["contents"]["kind"], "markdown");
    assert!(
        env["contents"]["value"]
            .as_str()
            .expect("markdown")
            .contains("$DATABASE_URL"),
        "{env}"
    );
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// 5. Hygiene, security and the CLI surface (R1, R3, R3a, S1–S11)
// ---------------------------------------------------------------------------

/// R1: the subcommand is reachable from both installations, `--help` prints
/// `usage: blastit lsp` first, and a bad argument is a CLI error (`2`).
#[test]
fn lsp_subcommand_is_reachable() {
    let help = binary().args(["lsp", "--help"]).output().expect("run");
    assert_eq!(help.status.code(), Some(0));
    let stdout = String::from_utf8(help.stdout).expect("UTF-8");
    assert_eq!(
        stdout.lines().next(),
        Some("usage: blastit lsp"),
        "{stdout:?}"
    );
    assert!(help.stderr.is_empty(), "{:?}", help.stderr);

    let bogus = binary().args(["lsp", "--bogus"]).output().expect("run");
    assert_eq!(bogus.status.code(), Some(2));
    assert!(bogus.stdout.is_empty(), "nothing on stdout");
    assert!(
        String::from_utf8(bogus.stderr)
            .expect("UTF-8")
            .starts_with("blastit lsp: ")
    );
}

/// S3: the `Lsp` arm checks the process holds no capability **before**
/// `lsp::run()`, and `lsp.rs` reads no environment, path, socket or config.
#[test]
fn the_lsp_arm_drops_capabilities() {
    let cli = include_str!("../src/cli.rs");
    let capability = cli.find("hold_none()").expect("the capability check");
    let run = cli.find("lsp::run()").expect("the run");
    assert!(
        capability < run,
        "the check must precede the session in cli.rs"
    );
}

/// R3, S1, S11: `lsp.rs` names no filesystem, process, environment, privilege
/// or config module, and names the manifest's one reader and writer.
#[test]
fn the_lsp_module_is_a_text_tool() {
    // §1's entry is `blastit-lsp/src/lib.rs` and its hover data lives beside it
    // as `blastit-lsp/src/hover.rs`: both are this module, so both carry the list.
    let sources = [
        include_str!("../../blastit-lsp/src/lib.rs"),
        include_str!("../../blastit-lsp/src/hover.rs"),
    ];
    for source in sources {
        for forbidden in [
            "std::fs",
            "std::path",
            "std::process",
            "std::os::unix::net",
            "libc",
            "rustix",
            "blastit_sys",
            "crate::stage0",
            "crate::stage1",
            "crate::host",
            "crate::config",
            "std::env",
            "include!",
            "fn lex",
            "fn parse",
            "fn lower",
        ] {
            assert!(
                !source.contains(forbidden),
                "lsp must not name {forbidden:?}"
            );
        }
    }
    let source = sources[0];
    assert!(source.contains("blastfile::check"));
    assert!(source.contains("blastfile::fmt"));
    assert!(source.contains("set_hook"));
    assert!(source.contains("catch_unwind"));
    assert!(source.contains("internal error"));
}

/// R3a: the wildcard arm is gone from `position()`, and the three families map
/// where §4.2 says — a positioned syntax error at its own site, a
/// `Location`-carrying validation error and a whole-file refusal at `(0,0)`.
#[test]
fn no_position_of_a_validation_error_is_invented() {
    let error_source = include_str!("../../blastit-manifest/src/error.rs");
    assert!(
        !error_source.contains("_ => Position::NONE"),
        "position() must list every variant"
    );

    let mut session = Session::spawn(true);
    session.initialize();
    // A positioned syntax error: `=` at 1-based 2:8.
    let syntax = session.open("file:///syntax", "view v \"/x\" {\n  body =\n}\n", 0);
    assert_eq!(
        syntax[0]["range"]["start"],
        json!({"line": 1, "character": 7})
    );
    // A `Location`-carrying validation refusal: `(0,0)`, the documented cost.
    let validation = session.open(
        "file:///validation",
        "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres nosuchstore { select x }\n}\n",
        0,
    );
    assert_eq!(
        validation[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    assert!(
        validation[0]["message"]
            .as_str()
            .expect("a message")
            .contains("grant on unknown proxy")
    );
    // A whole-file refusal: the parser's cap, at `(0,0)`.
    let oversized = format!("#{}", "x".repeat(16_777_216));
    let whole = session.open("file:///whole", &oversized, 0);
    assert_eq!(
        whole[0]["range"]["start"],
        json!({"line": 0, "character": 0})
    );
    assert_eq!(whole[0]["severity"], 1);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
}

/// S2: a document is parsed, never executed — the `query "…"` string that
/// grants a program-invoking statement is a diagnostic, the server spawns no
/// child, and stderr stays empty.
#[test]
fn a_document_is_never_executed() {
    let text = "postgres $DATABASE_URL\n\n\
                view v \"/x\" {\n\
                    postgres {\n\
                        query \"COPY x FROM PROGRAM 'sh'\"\n\
                    }\n\
                }\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let children_before = children_of(session.pid(), &session.task_ids());
    assert!(children_before.is_empty(), "{children_before:?}");
    let diagnostics = session.open("file:///exec", text, 0);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["severity"], 1);
    assert_eq!(diagnostics[0]["source"], "blastit");
    let message = diagnostics[0]["message"].as_str().expect("a message");
    assert!(message.contains("inlines a quoted literal"), "{message}");
    assert!(!message.contains("COPY"), "the entry is never printed");
    let children_after = children_of(session.pid(), &session.task_ids());
    assert!(children_after.is_empty(), "{children_after:?}");
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// S4: a `uri` is an opaque string — a document whose `uri` names a FIFO with no
/// writer answers at once, so the path was never opened.
#[test]
fn no_path_from_a_document_is_opened() {
    let path = std::env::temp_dir().join(format!("blastit-lsp-fifo-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let c_path =
        std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("a path without a NUL");
    // SAFETY: `mkfifo` takes a NUL-terminated path and a mode.
    let created = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(created, 0, "mkfifo {}", path.display());
    let uri = format!("file://{}", path.display());

    let started = Instant::now();
    let mut session = Session::spawn(true);
    session.initialize();
    let clean = CLEAN;
    let diagnostics = session.open(&uri, clean, 0);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}}),
    );
    assert_eq!(session.recv_result(), json!([]));
    assert!(started.elapsed() < Duration::from_secs(5));
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));
    let _ = std::fs::remove_file(&path);
}

/// S7: a whole session's stdout is frames and nothing else, and every declared
/// length is the UTF-8 byte length of its body.
#[test]
fn stdout_is_frames_only() {
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///frames", P1, 0);
    session.request(
        json!(2),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///frames"}}),
    );
    assert_eq!(session.recv_result(), Value::Null);
    session.request(json!(3), "shutdown", json!({}));
    let _ = session.recv_result();
    session.notify("exit", json!({}));
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// S7 positive control: `drain_stdout` really returns the bytes a session left
/// unread, so the `assert!(rest.is_empty())` of the framing tests is not a
/// vacuous read of an empty pipe.
#[test]
fn drain_stdout_sees_an_unread_frame() {
    let mut session = Session::spawn(true);
    session.initialize();
    // Two replies nobody reads.
    session.request(json!(2), "window/workDoneProgress/create", json!({}));
    session.request(json!(3), "shutdown", json!({}));
    session.notify("exit", json!({}));
    let rest = session.drain_stdout();
    let text = String::from_utf8(rest).expect("UTF-8");
    assert_eq!(
        text.matches("Content-Length: ").count(),
        2,
        "two unread replies, byte for byte: {text:?}"
    );
    assert!(text.contains("\"id\":2"), "{text:?}");
    assert!(text.contains("\"id\":3"), "{text:?}");
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(0));
}

/// S8: no document text reaches stderr, in either of the two sessions that can
/// write it — the SQL fixture and the truncated one — and every stderr line is
/// one of §11.3's four sentences.
#[test]
fn stderr_never_holds_document_text() {
    let allowed = [
        "blastit lsp: the input ended inside a frame",
        "blastit lsp: the header block is too long",
        "blastit lsp: the body is too long",
        "blastit lsp: internal error",
    ];
    let text = "postgres $DATABASE_URL\n\n\
                view v \"/x\" {\n\
                    postgres {\n\
                        query \"SELECT secret_token FROM api_keys\"\n\
                    }\n\
                }\n";
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open("file:///secret", text, 0);
    session.request(json!(2), "shutdown", json!({}));
    let _ = session.recv_result();
    session.notify("exit", json!({}));
    // Nothing at all follows `exit`: draining stdout to EOF is what catches a
    // stray write the test did not happen to read.
    let rest = session.drain_stdout();
    assert!(
        rest.is_empty(),
        "stdout after `exit` held {:?}",
        String::from_utf8_lossy(&rest)
    );
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(0), "shutdown then exit is 0");
    for needle in ["secret_token", "SELECT", "api_keys", "file:///secret"] {
        assert!(!stderr.contains(needle), "{stderr:?} holds {needle}");
    }
    assert!(stderr.is_empty(), "{stderr:?}");

    // A truncated frame does write the first sentence — and nothing else. The
    // header is complete and declares 100 bytes; only ten arrive before EOF, so
    // stdin is closed to make the stream end inside the frame.
    let mut session = Session::spawn(true);
    {
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"Content-Length: 100\r\n\r\n{\"jsonrpc\"")
            .expect("a truncated frame");
        stdin.flush().expect("a flush");
    }
    session.stdin = None;
    // The one answer the reader can give, then EOF and nothing more.
    assert_eq!(session.recv_error(-32700), Value::Null);
    let rest = session.drain_stdout();
    assert!(
        rest.is_empty(),
        "stdout after the refusal held {:?}",
        String::from_utf8_lossy(&rest)
    );
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(1));
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{stderr:?}");
    assert!(allowed.contains(&lines[0]), "{stderr:?}");
    assert!(lines[0].contains("the input ended inside a frame"));
}

/// R13, S10: every ending's exit status — `0` only after `shutdown`, `1` for
/// EOF and for a stream that cannot be framed, `2` for a bad argument.
#[test]
fn exit_codes() {
    // clean: shutdown, then EOF, is 0.
    let mut session = Session::spawn(true);
    session.initialize();
    session.request(json!(2), "shutdown", json!({}));
    let _ = session.recv_result();
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(0));

    // exit without shutdown is 1.
    let mut session = Session::spawn(true);
    session.initialize();
    session.notify("exit", json!({}));
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));

    // EOF mid-session is 1.
    let mut session = Session::spawn(true);
    session.initialize();
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));

    // A stream that cannot be framed is 1. The junk header is the first frame,
    // so the `-32700` read here is its own refusal and not a leftover reply.
    let mut session = Session::spawn(true);
    {
        let stdin = session.stdin.as_mut().expect("stdin");
        stdin
            .write_all(b"Not-A-Length: 3\r\n\r\n")
            .expect("a junk header");
        stdin.flush().expect("a flush");
    }
    assert_eq!(session.recv_error(-32700), Value::Null);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));

    // EOF with nothing at all is 1: a server started and dropped.
    let session = Session::spawn(true);
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(1));

    // A bad argument is a CLI error, not a session.
    let output = binary().args(["lsp", "serve"]).output().expect("run");
    assert_eq!(output.status.code(), Some(2));
}

/// S5: 2 000 framed bodies of every shape the client can send leave the loop
/// alive, with every reply one well-formed frame.
#[test]
fn fuzz_shaped_bodies_do_not_kill_the_loop() {
    let mut session = Session::spawn(true);
    session.initialize();
    let _ = session.open(
        "file:///fuzz",
        "postgres $DATABASE_URL\n\nview v \"/x\" {\n    body = json\n}\n",
        0,
    );
    let methods = [
        "initialize",
        "textDocument/hover",
        "textDocument/formatting",
        "textDocument/didOpen",
        "textDocument/definition",
        "$/cancelRequest",
        "fuzz/unknown",
    ];
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for index in 0..2_000_u64 {
        let pick = next();
        let body: Vec<u8> = match pick % 6 {
            0 => b"{\"jsonrpc\":\"2.0\"".to_vec(),
            1 => b"not json at all".to_vec(),
            2 => b"[1,2,3]".to_vec(),
            3 => vec![b'{', 0xff, b'}'],
            4 => b"null".to_vec(),
            _ => {
                let id = match index % 3 {
                    0 => json!(index),
                    1 => json!(format!("id-{index}")),
                    _ => Value::Null,
                };
                let method = methods[(pick as usize) % methods.len()];
                let params = if pick % 2 == 0 {
                    json!({"textDocument": {"uri": "file:///fuzz"}, "position": {"line": 0, "character": 0}})
                } else {
                    json!({})
                };
                serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                }))
                .expect("a body")
            }
        };
        session.send_body(&body);
        let (declared, reply) = session.frame().expect("one reply per request");
        assert_eq!(declared, reply.len());
        let parsed: Value = serde_json::from_slice(&reply).expect("a JSON reply");
        assert_eq!(parsed["jsonrpc"], "2.0", "{parsed}");
        assert!(
            parsed.get("result").is_some() || parsed.get("error").is_some(),
            "exactly one of result and error: {parsed}"
        );
    }
    // Still serving: a final request answers with a real result.
    let _ = session.open("file:///fuzz", CLEAN, 1);
    session.request(
        json!(1),
        "textDocument/formatting",
        json!({"textDocument": {"uri": "file:///fuzz"}}),
    );
    assert_eq!(session.recv_result(), json!([]));
    session.request(json!(2), "shutdown", json!({}));
    let _ = session.recv_result();
    let (status, _) = session.finish();
    assert_eq!(status.code(), Some(0));
}

/// S3(b): no environment is needed — the handshake and one diagnostic complete
/// with the child's environment cleared, so no `BLASTIT_*` is read.
#[test]
fn no_environment_is_needed() {
    let mut session = Session::spawn(true);
    session.request(json!(1), "initialize", json!({}));
    let result = session.recv_result();
    assert_eq!(result["serverInfo"]["name"], "blastit");
    let diagnostics = session.open("file:///envless", P1, 0);
    assert_eq!(diagnostics.len(), 1);
    session.request(json!(2), "shutdown", json!({}));
    let _ = session.recv_result();
    let (status, stderr) = session.finish();
    assert_eq!(status.code(), Some(0));
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// Every child of every task of `pid`, read from `/proc`: the only oracle for
/// "this server spawned nothing" that is about *its* table.
fn children_of(pid: u32, tasks: &[String]) -> Vec<String> {
    assert!(
        !tasks.is_empty(),
        "the server process has at least one task to read"
    );
    let mut children = Vec::new();
    let mut readable = 0_usize;
    for task in tasks {
        let path = PathBuf::from(format!("/proc/{pid}/task/{task}/children"));
        // Loud, never skipped: a half that silently proves nothing is worse than
        // no half. A kernel without `CONFIG_PROC_CHILDREN` fails here instead of
        // letting "spawns no child" pass vacuously.
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "{} must be readable for the no-child check: {error}",
                path.display()
            )
        });
        readable = readable.saturating_add(1);
        children.extend(text.split_whitespace().map(str::to_owned));
    }
    assert!(readable > 0, "at least one /proc children file was read");
    children
}
