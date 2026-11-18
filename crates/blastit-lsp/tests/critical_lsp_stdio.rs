//! Protected critical suite for the standalone `blastit-lsp` binary (`MB.35.4`).
//!
//! `blastit lsp` and `blastit-lsp` are **one** server (`docs/design/blastfile-lsp-zed.md`
//! §1.3, `ADR-0013`): the same `run()`, the same capability document, the same
//! diagnostic at the same `line:col`, the same exit code. `crates/blastit/tests/critical_lsp.rs`
//! keeps proving the delegation through `blastit lsp`; this file is the other half —
//! the portable artifact an editor on macOS or Windows actually runs, spawned from
//! `env!("CARGO_BIN_EXE_blastit-lsp")` with no `blastit` anywhere on its path.
//!
//! The session is the minimum that can tell the two binaries apart if they ever
//! drift: `initialize` and its capability document, one `didOpen` of a `query` typo
//! (`qery`) as a severity-1 diagnostic at the exact range, then `shutdown` + `exit`
//! and status `0`. It runs twice — once with a **cleared** environment and once
//! with `BLASTIT_*` poisoned — because "reads no environment" needs both halves:
//! a cleared one proves nothing is required, a poisoned one proves nothing is read
//! (§1.3 row 3, §8.3a's allowlist). The third case is the one documented argv
//! difference: the portable binary ignores an argument and exits `1` at end of
//! input, where `blastit lsp --bogus` exits `2` with a CLI error (§1.3 obligation 3).
//!
//! No case mutates this process's environment: each child gets its own pipes and
//! its own `env_clear()`/`env` pair.

// A test asserts by unwrapping: a missing frame or a malformed reply is a failure
// with a message, and `clippy::expect_used` is the wrong advice here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How long one frame may take to arrive, and how long a child may take to exit.
/// A session that hangs fails the case instead of hanging the suite.
const PATIENCE: Duration = Duration::from_secs(20);

/// The portable binary cargo built for this test run.
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blastit-lsp"))
}

/// The `query` typo: `qery` is not a directive, and the refusal names the word
/// it should have been. The error sits at 0-based line 4, characters 8–12 —
/// `qery`'s own span, not the block's. `concat!` rather than a `\` continuation:
/// a continuation strips the next line's indentation, and the indentation is the
/// column this case asserts.
const QUERY_TYPO: &str = concat!(
    "postgres $DATABASE_URL\n",
    "\n",
    "view v \"/x\" {\n",
    "    postgres {\n",
    "        qery 'SELECT 1'\n",
    "    }\n",
    "}\n",
);

/// `qery`'s exact 0-based start, and the end of its four-character word.
const TYPO_LINE: u64 = 4;
const TYPO_START: u64 = 8;
const TYPO_END: u64 = 12;

/// One live `blastit-lsp` session: its stdin, its framed stdout on a reader
/// thread, and its stderr for the verdict at the end.
struct Session {
    /// The server process.
    child: Child,
    /// Its stdin, `None` once the session sent `exit` and closed the pipe.
    stdin: Option<ChildStdin>,
    /// Frames read by the reader thread, in arrival order.
    frames: Receiver<Value>,
    /// Its stderr, read once it exited.
    stderr: Option<ChildStderr>,
}

impl Session {
    /// Starts `blastit-lsp` with the given environment **only**: `env_clear()`
    /// comes first, so a `BLASTIT_*` name this process happens to carry cannot
    /// reach the child unless the case says so on purpose.
    fn spawn(env: &[(&str, &str)]) -> Self {
        let mut command = binary();
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("spawn blastit-lsp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout: ChildStdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take();
        let (sender, frames) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(frame) = read_frame(&mut reader) {
                if sender.send(frame).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            frames,
            stderr,
        }
    }

    /// Writes one frame to the server, flushed so the server can answer.
    fn send(&mut self, value: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        let body = value.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write a frame");
        stdin.flush().expect("flush a frame");
    }

    /// The next frame, or a failure naming what the session was waiting for.
    fn recv(&self) -> Value {
        match self.frames.recv_timeout(PATIENCE) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => panic!("no frame within {PATIENCE:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("stdout closed before a frame"),
        }
    }

    /// `initialize` + `initialized`, returning the capability document.
    fn initialize(&mut self) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"capabilities": {}, "processId": Value::Null},
        }));
        let reply = self.recv();
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], 1, "the id is echoed verbatim: {reply}");
        assert!(reply.get("error").is_none(), "{reply}");
        self.send(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        reply["result"].clone()
    }

    /// `didOpen` and the `publishDiagnostics` notification it pushes.
    fn open(&mut self, uri: &str, text: &str) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": uri,
                "languageId": "blastfile",
                "version": 1,
                "text": text,
            }},
        }));
        let notification = self.recv();
        assert_eq!(
            notification["method"], "textDocument/publishDiagnostics",
            "{notification}"
        );
        assert_eq!(notification["params"]["uri"], uri, "{notification}");
        assert_eq!(notification["params"]["version"], 1, "{notification}");
        notification["params"]["diagnostics"].clone()
    }

    /// `shutdown` + `exit`, then the exit status and stderr.
    fn finish(mut self) -> (ExitStatus, String) {
        self.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null}));
        let reply = self.recv();
        assert_eq!(reply["id"], 2, "{reply}");
        assert_eq!(reply["result"], Value::Null, "{reply}");
        self.send(&json!({"jsonrpc": "2.0", "method": "exit", "params": null}));
        self.stdin.take();
        let status = wait_or_kill(&mut self.child);
        let mut stderr = String::new();
        if let Some(mut pipe) = self.stderr.take() {
            pipe.read_to_string(&mut stderr).expect("read stderr");
        }
        (status, stderr)
    }
}

/// One frame off the wire, or `None` at end of stream. A header block without a
/// `Content-Length`, or a body that is not UTF-8 JSON, is a failure: stdout
/// carries framed messages and nothing else.
fn read_frame(reader: &mut impl BufRead) -> Option<Value> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).expect("read a header line");
        if read == 0 {
            // End of stream: legal between frames, a failure inside one.
            assert!(length.is_none(), "a header block ended at end of stream");
            return None;
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').expect("a `Name: value` header");
        if name.eq_ignore_ascii_case("Content-Length") {
            length = Some(value.trim().parse().expect("a decimal Content-Length"));
        }
    }
    let length = length.expect("a Content-Length header");
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).expect("the declared body");
    let text = String::from_utf8(body).expect("a UTF-8 body");
    Some(serde_json::from_str(&text).expect("a JSON body"))
}

/// Waits for the child, killing it past [`PATIENCE`] so a hang is a failure.
fn wait_or_kill(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = child.try_wait().expect("poll the child") {
            return status;
        }
        if Instant::now() > deadline {
            child.kill().expect("kill a hung child");
            panic!("the child did not exit within {PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The whole session's assertions, shared by the cleared and the poisoned
/// environment so the two runs can only differ in the environment they were
/// given.
fn assert_one_session(
    capabilities: &Value,
    diagnostics: &Value,
    status: &ExitStatus,
    stderr: &str,
) {
    // §2.3 byte for byte: `codeActionProvider` is the boolean `true` (an object
    // would advertise kinds), `save` is `false`, and `version` is this crate's.
    assert_eq!(
        *capabilities,
        json!({
            "capabilities": {
                "positionEncoding": "utf-16",
                "textDocumentSync": {"openClose": true, "change": 1, "save": false},
                "documentFormattingProvider": true,
                "hoverProvider": true,
                "codeActionProvider": true,
            },
            "serverInfo": {"name": "blastit", "version": env!("CARGO_PKG_VERSION")},
        })
    );
    assert_eq!(
        capabilities["capabilities"]["codeActionProvider"],
        Value::Bool(true),
        "the boolean, not a kind list"
    );

    // §5.2: one severity-1 diagnostic, at the typo's own span, with the CLI's
    // sentence and no `code`/`tags`/`relatedInformation`.
    let list = diagnostics.as_array().expect("an array of diagnostics");
    assert_eq!(list.len(), 1, "{diagnostics}");
    let diagnostic = &list[0];
    assert_eq!(diagnostic["severity"], 1, "{diagnostic}");
    assert_eq!(diagnostic["source"], "blastit", "{diagnostic}");
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": TYPO_LINE, "character": TYPO_START}),
        "{diagnostic}"
    );
    assert_eq!(
        diagnostic["range"]["end"],
        json!({"line": TYPO_LINE, "character": TYPO_END}),
        "{diagnostic}"
    );
    let message = diagnostic["message"].as_str().expect("a message");
    assert!(
        message.starts_with("`qery` is not a directive"),
        "{message}"
    );
    assert!(
        message.contains("and `query`"),
        "the fix is named: {message}"
    );
    assert!(diagnostic.get("code").is_none(), "{diagnostic}");
    assert!(diagnostic.get("tags").is_none(), "{diagnostic}");

    // §3.4: `0` after `shutdown`, and nothing on stderr.
    assert_eq!(status.code(), Some(0), "{stderr:?}");
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// §14.2: the portable binary speaks the same session as `blastit lsp`, and a
/// cleared environment is enough — the server requires no `BLASTIT_*` name.
#[test]
fn the_standalone_binary_speaks_the_same_session() {
    let mut session = Session::spawn(&[]);
    let capabilities = session.initialize();
    let diagnostics = session.open("file:///typo", QUERY_TYPO);
    let (status, stderr) = session.finish();
    assert_one_session(&capabilities, &diagnostics, &status, &stderr);
}

/// §1.3 row 3 / §8.3a: the server **reads** no environment either. A `BLASTIT_*`
/// poisoning that would move a manifest, a working directory or an interpreter
/// changes no byte of the session — same capability document, same diagnostic,
/// same status.
#[test]
fn a_poisoned_environment_changes_nothing() {
    let mut session = Session::spawn(&[
        ("BLASTIT_APP", "blastit.poisoned:app"),
        ("BLASTIT_CONFIG", "/nonexistent/poisoned/Blastfile"),
        ("BLASTIT_ROOT", "/nonexistent/poisoned"),
        (
            "BLASTIT_DATABASE_URL",
            "postgres://poisoned@127.0.0.1/poisoned",
        ),
        ("BLASTIT_LOG", "trace"),
    ]);
    let capabilities = session.initialize();
    let diagnostics = session.open("file:///typo", QUERY_TYPO);
    let (status, stderr) = session.finish();
    assert_one_session(&capabilities, &diagnostics, &status, &stderr);
}

/// §1.3 obligation 3: the one documented argv difference. The portable binary
/// ignores an argument — it reads stdin and exits `1` at end of input, where
/// `blastit lsp --bogus` exits `2` with a CLI error. Nothing is written to
/// either stream.
#[test]
fn an_argument_is_ignored_and_end_of_input_exits_one() {
    let mut child = binary()
        .env_clear()
        .arg("--bogus")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn blastit-lsp --bogus");
    // End of input at once: a server that had parsed argv would have exited
    // `2` before opening the pipe.
    drop(child.stdin.take());
    let status = wait_or_kill(&mut child);
    assert_eq!(status.code(), Some(1), "EOF without `shutdown` is `1`");
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout).expect("read stdout");
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).expect("read stderr");
    }
    assert!(stdout.is_empty(), "stdout is the protocol: {stdout:?}");
    assert!(
        stderr.is_empty(),
        "the argument was not a CLI error: {stderr:?}"
    );
}
