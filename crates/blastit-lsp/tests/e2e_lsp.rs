//! The `blastit-lsp` suite (`TESTING_PLAN.md` §7), driven through the **real
//! binary** cargo built for this run: diagnostics, hovers, formatting, the
//! literal-to-`$?` quick fix in both spellings, and the argv surface — with
//! `LLVM_PROFILE_FILE` forwarded past `env_clear()` for `make coverage`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(20);

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blastit-lsp"))
}

fn fresh_command() -> Command {
    let mut command = binary();
    command.env_clear();
    if let Ok(file) = std::env::var("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", file);
    }
    command
}

const TYPO: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        qery 'SELECT 1'\n    }\n}\n";
const TYPO_MESSAGE: &str = "`qery` is not a directive here; a grant block takes \
     `transactions`, `functions`, `select`, `insert`, `update`, `delete` and `query`";
const CANONICAL: &str = "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n";
const MISINDENTED: &str = "app \"shop\"\n\nview v \"/x\" {\n  body json\n}\n";
const CLEAN: &str = "view v \"/x\" {\n    body json\n}\n";
const NON_BMP: &str = "view v \"/😀\" { body = }\n";
const TWO_ERRORS: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        query \"SELECT 'a'\"\n        query \"SELECT 'b'\"\n    }\n}\n";
const PARSE_ERROR: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        query 'SELCT id FROM t'\n    }\n}\n";
const SQL_DOC: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        query 'SELECT id FROM t WHERE price > $? AND id IN ($?, ...)'\n    }\n}\n";
const SQL_PLACEHOLDER: &str = "`$?` is one bind placeholder: the app sends one value \
     for it, and the wire sees `$1, $2, …` in the order the placeholders appear. \
     `IN ($?, ...)` binds any number of values with one grant — the list length is \
     not part of the grant.";
const SQL_VARIADIC: &str = "`...` is Blastit's variadic marker, not SQL: it is legal \
     only as `IN ($?, ...)` (any number of values) or after a `VALUES` row, \
     `($?, $?), ...` (any number of rows). `blastit check` refuses it anywhere else.";
const DOUBLE: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        query \"SELECT id FROM products WHERE email = $1\"\n    }\n}\n";
const SINGLE: &str = "postgres $DATABASE_URL\n\nview v \"/x\" {\n    postgres {\n        query 'UPDATE t SET a = $1 WHERE b = $2'\n    }\n}\n";
const DOUBLE_FIX: &str = "SELECT id FROM products WHERE email = $?";
const SINGLE_FIX: &str = "UPDATE t SET a = $? WHERE b = $?";

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<Value>,
    stderr: Option<ChildStderr>,
    next_id: u64,
}

impl Session {
    fn spawn(env: &[(&str, &str)]) -> Self {
        let mut command = fresh_command();
        command
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
            next_id: 2,
        }
    }

    fn send(&mut self, value: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        let body = value.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write a frame");
        stdin.flush().expect("flush a frame");
    }

    fn recv(&self) -> Value {
        match self.frames.recv_timeout(PATIENCE) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => panic!("no frame within {PATIENCE:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("stdout closed before a frame"),
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// One request: its `result`, with the id echoed and no `error` — a JSON-RPC
    /// error on `formatting` is exactly the failure the contract forbids.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id = id.saturating_add(1);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.recv();
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], id, "the id is echoed verbatim: {reply}");
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn initialize(&mut self) -> Value {
        let result = self.request(
            "initialize",
            json!({"capabilities": {}, "processId": Value::Null}),
        );
        self.notify("initialized", json!({}));
        result
    }

    fn publish(&self) -> Value {
        let notification = self.recv();
        assert_eq!(
            notification["method"], "textDocument/publishDiagnostics",
            "{notification}"
        );
        notification["params"].clone()
    }

    fn open(&mut self, uri: &str, text: &str, version: u64) -> Value {
        let document = json!({
            "uri": uri, "languageId": "blastfile", "version": version, "text": text,
        });
        self.notify("textDocument/didOpen", json!({"textDocument": document}));
        let published = self.publish();
        assert_eq!(published["uri"], uri, "{published}");
        published
    }

    fn change(&mut self, uri: &str, text: &str, version: u64) -> Value {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": text}],
            }),
        );
        let published = self.publish();
        assert_eq!(published["uri"], uri, "{published}");
        published
    }

    fn close(&mut self, uri: &str) -> Value {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
        self.publish()
    }

    fn hover(&mut self, uri: &str, position: (u64, u64)) -> Value {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": position.0, "character": position.1},
            }),
        )
    }

    fn formatting(&mut self, uri: &str) -> Value {
        self.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4}}),
        )
    }

    /// `textDocument/codeAction` in the shape an editor sends: `context` is a
    /// required member (and `only` rides inside it).
    fn code_action(&mut self, uri: &str, position: (u64, u64), context: Value) -> Value {
        let (line, character) = position;
        self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": line, "character": character},
                    "end": {"line": line, "character": character},
                },
                "context": context,
            }),
        )
    }

    /// `shutdown` + `exit`: `0` after the handshake.
    fn finish(mut self) {
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "shutdown", "params": null}));
        let reply = self.recv();
        assert_eq!(reply["id"], id, "{reply}");
        assert_eq!(reply["result"], Value::Null, "{reply}");
        self.notify("exit", Value::Null);
        self.stdin.take();
        let status = wait_or_kill(&mut self.child);
        let mut stderr = String::new();
        if let Some(mut pipe) = self.stderr.take() {
            pipe.read_to_string(&mut stderr).expect("read stderr");
        }
        assert_eq!(status.code(), Some(0), "{stderr:?}");
        assert!(stderr.is_empty(), "{stderr:?}");
    }
}

fn read_frame(reader: &mut impl BufRead) -> Option<Value> {
    let mut length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header).expect("a header line") == 0 {
            assert!(length.is_none(), "a header block ended at end of stream");
            return None;
        }
        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').expect("a `Name: value` header");
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

fn assert_capabilities(result: &Value) {
    assert_eq!(
        *result,
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
}

/// §5.2: one severity-1 diagnostic at the typo's own span, the CLI's sentence.
fn assert_typo(published: &Value) {
    let list = published["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(list.len(), 1, "{published}");
    let diagnostic = &list[0];
    assert_eq!(diagnostic["severity"], 1, "{diagnostic}");
    assert_eq!(diagnostic["source"], "blastit", "{diagnostic}");
    let range = &diagnostic["range"];
    assert_eq!(range["start"], json!({"line": 4, "character": 8}));
    assert_eq!(range["end"], json!({"line": 4, "character": 12}));
    assert_eq!(diagnostic["message"], TYPO_MESSAGE, "{diagnostic}");
    assert!(diagnostic.get("code").is_none(), "{diagnostic}");
    assert!(diagnostic.get("tags").is_none(), "{diagnostic}");
}

fn line_point(text: &str, line: usize, needle: &str) -> (u64, u64) {
    let line_text = text.lines().nth(line).expect("the fixture line");
    let at = line_text.find(needle).expect("the needle on that line");
    (line as u64, at as u64)
}

fn interior(text: &str, open: &str) -> (usize, usize) {
    let start = text.find(open).expect("the opening quote") + open.len();
    let quote = open.chars().last().expect("a quote");
    let end = start + text[start..].find(quote).expect("the closing quote");
    (start, end)
}

/// §14.2: the portable binary speaks the same session as `blastit lsp`.
#[test]
fn the_standalone_binary_speaks_the_same_session() {
    let mut session = Session::spawn(&[]);
    assert_capabilities(&session.initialize());
    let published = session.open("file:///typo", TYPO, 1);
    assert_eq!(published["version"], 1, "{published}");
    assert_typo(&published);
    session.finish();
}

/// §1.3 row 3 / §8.3a: a `BLASTIT_*` poisoning changes no byte of the session.
#[test]
fn a_poisoned_environment_changes_nothing() {
    let mut session = Session::spawn(&[
        ("BLASTIT_APP", "blastit.poisoned:app"),
        ("BLASTIT_CONFIG", "/nonexistent/poisoned/Blastfile"),
        ("BLASTIT_ROOT", "/nonexistent/poisoned"),
        ("BLASTIT_DATABASE_URL", "postgres://poisoned@127.0.0.1/x"),
        ("BLASTIT_LOG", "trace"),
    ]);
    assert_capabilities(&session.initialize());
    assert_typo(&session.open("file:///typo", TYPO, 1));
    session.finish();
}

/// §5.3: a clean `didOpen` publishes `[]`; a `didChange` to the typo publishes
/// the diagnostic with the new version, and a third change clears it.
#[test]
fn did_change_republishes_the_new_text() {
    let mut session = Session::spawn(&[]);
    session.initialize();
    let clean = session.open("file:///change", CLEAN, 1);
    assert_eq!(clean["diagnostics"], json!([]), "{clean}");
    let typo = session.change("file:///change", TYPO, 2);
    assert_eq!(typo["version"], 2, "{typo}");
    assert_typo(&typo);
    let clean = session.change("file:///change", CLEAN, 3);
    assert_eq!(clean["version"], 3, "{clean}");
    assert_eq!(clean["diagnostics"], json!([]), "{clean}");
    session.finish();
}

/// §5.1/§5.6: every error is published at its own span, a bad `query` is one
/// diagnostic at its first token, and `didClose` publishes `[]` with no `version`.
#[test]
fn every_error_is_published_and_did_close_clears() {
    let mut session = Session::spawn(&[]);
    session.initialize();
    let published = session.open("file:///two", TWO_ERRORS, 1);
    let list = published["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(list.len(), 2, "only the first: {published}");
    for (diagnostic, line) in list.iter().zip([4, 5]) {
        let (line, at) = line_point(TWO_ERRORS, line, "'");
        let message = diagnostic["message"].as_str().expect("a message");
        assert_eq!(diagnostic["severity"], 1, "{diagnostic}");
        let range = &diagnostic["range"];
        assert_eq!(range["start"], json!({"line": line, "character": at}));
        assert_eq!(range["end"], json!({"line": line, "character": at + 1}));
        assert!(message.contains("inlines a quoted literal"), "{message}");
        assert!(message.contains("bind the value with `$?`"), "{message}");
    }
    let closed = session.close("file:///two");
    assert_eq!(closed["diagnostics"], json!([]), "{closed}");
    assert!(closed.get("version").is_none(), "a close has no version");

    // The documented `query` squiggle: a statement that does not parse is one
    // severity-1 diagnostic at its first token, with the CLI's sentence.
    let broken = session.open("file:///typo-sql", PARSE_ERROR, 2);
    let list = broken["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(list.len(), 1, "one diagnostic for a bad query: {broken}");
    assert_eq!(list[0]["severity"], 1, "{broken}");
    assert_eq!(
        list[0]["message"],
        "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`"
    );
    let (line, at) = line_point(PARSE_ERROR, 4, "SELCT");
    let range = &list[0]["range"];
    assert_eq!(range["start"], json!({"line": line, "character": at}));
    assert_eq!(range["end"], json!({"line": line, "character": at + 5}));
    session.finish();
}

/// §4.4: `character` counts UTF-16 units, so `😀` puts the `=` at 20, not 19 or 22.
#[test]
fn diagnostics_count_utf16_units() {
    let mut session = Session::spawn(&[]);
    session.initialize();
    let published = session.open("file:///emoji", NON_BMP, 1);
    let list = published["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(list.len(), 1, "{published}");
    let range = &list[0]["range"];
    assert_eq!(range["start"], json!({"line": 0, "character": 20}));
    assert_eq!(range["end"], json!({"line": 0, "character": 21}));
    let message = list[0]["message"].as_str().expect("a message");
    assert!(message.contains("is not Blastfile syntax"), "{message}");
    session.finish();
}

/// §7.6: `$?` and `...` answer with the paragraph a user reads over the token's
/// own span; outside a `query` the reply is `null`.
#[test]
fn hover_explains_the_two_sql_tokens() {
    let placeholder = line_point(SQL_DOC, 4, "$?");
    let variadic = line_point(SQL_DOC, 4, "...");
    let mut session = Session::spawn(&[]);
    session.initialize();
    session.open("file:///tokens", SQL_DOC, 1);

    let hover = session.hover("file:///tokens", placeholder);
    assert_eq!(hover["contents"]["kind"], "markdown", "{hover}");
    assert_eq!(hover["contents"]["value"], SQL_PLACEHOLDER, "{hover}");
    let range = &hover["range"];
    assert_eq!(
        range["start"],
        json!({"line": placeholder.0, "character": placeholder.1})
    );
    assert_eq!(
        range["end"],
        json!({"line": placeholder.0, "character": placeholder.1 + 2})
    );

    let hover = session.hover("file:///tokens", variadic);
    assert_eq!(hover["contents"]["value"], SQL_VARIADIC, "{hover}");
    let range = &hover["range"];
    assert_eq!(
        range["start"],
        json!({"line": variadic.0, "character": variadic.1})
    );
    assert_eq!(
        range["end"],
        json!({"line": variadic.0, "character": variadic.1 + 3})
    );

    let outside = "view v \"/x\" {\n    body none # $? and ... are a comment\n}\n";
    session.open("file:///outside", outside, 2);
    for label in ["$?", "..."] {
        let position = line_point(outside, 1, label);
        assert_eq!(
            session.hover("file:///outside", position),
            Value::Null,
            "{label}"
        );
    }
    session.finish();
}

/// §7.1–§7.3: a directive answers with its paragraph over its own span, and an
/// `$IDENT` with the one environment-variable paragraph.
#[test]
fn hover_explains_the_directive_vocabulary() {
    let mut session = Session::spawn(&[]);
    session.initialize();
    session.open("file:///words", TYPO, 1);
    for (position, needle, end) in [((0, 0), "default store", 8), ((2, 0), "one sandbox", 4)] {
        let hover = session.hover("file:///words", position);
        let value = hover["contents"]["value"].as_str().expect("markdown");
        assert!(value.contains(needle), "{needle}: {value}");
        assert_eq!(hover["range"]["end"]["character"], end, "{hover}");
    }
    let hover = session.hover("file:///words", line_point(TYPO, 0, "DATABASE"));
    let value = hover["contents"]["value"].as_str().expect("markdown");
    assert!(value.contains("$DATABASE_URL"), "{value}");
    assert!(value.contains("stays in the environment"), "{value}");
    assert!(!value.contains("postgres://"), "never the value: {value}");
    assert_eq!(hover["range"]["start"]["character"], 9, "{hover}");
    session.finish();
}

/// §6: a canonical buffer is `[]`; a mis-indented one is one edit; a broken one
/// is `null`, never a JSON-RPC error.
#[test]
fn formatting_is_empty_edited_or_declined() {
    let mut session = Session::spawn(&[]);
    session.initialize();
    session.open("file:///canonical", CANONICAL, 1);
    assert_eq!(session.formatting("file:///canonical"), json!([]));

    session.open("file:///unformatted", MISINDENTED, 2);
    let edits = session.formatting("file:///unformatted");
    let edits = edits.as_array().expect("a list of edits");
    assert_eq!(edits.len(), 1, "one edit, one undo step: {edits:?}");
    let range = &edits[0]["range"];
    assert_eq!(range["start"], json!({"line": 0, "character": 0}));
    assert_eq!(range["end"], json!({"line": 5, "character": 0}));
    assert_eq!(edits[0]["newText"], CANONICAL);
    session.open("file:///broken", NON_BMP, 3);
    assert_eq!(session.formatting("file:///broken"), Value::Null);
    let alive = session.hover("file:///broken", (0, 0));
    assert!(alive["contents"]["value"].is_string(), "{alive}");
    session.finish();
}

/// §7A: in **both** spellings a psql paste gets one `quickfix` over the whole
/// `query` interior (any `only`), and an off-query caret gets nothing.
#[test]
fn the_quick_fix_parameterizes_both_spellings() {
    let cases = [
        ("file:///double", DOUBLE, "query \"", "SELECT", DOUBLE_FIX),
        ("file:///single", SINGLE, "query '", "UPDATE", SINGLE_FIX),
    ];
    let mut session = Session::spawn(&[]);
    session.initialize();
    for (uri, text, open, word, expected) in cases {
        let (from, to) = interior(text, open);
        let published = session.open(uri, text, 1);
        let diagnostic = published["diagnostics"][0].clone();
        let actions = session.code_action(uri, (4, 20), json!({"diagnostics": []}));
        let list = actions.as_array().expect("an array of actions");
        assert_eq!(list.len(), 1, "{uri}: {actions:?}");
        assert_eq!(list[0]["title"], "Parameterize SQL for Blastfile ($?)");
        assert_eq!(list[0]["kind"], "quickfix");
        let quickfix = json!({"diagnostics": [], "only": ["quickfix"]});
        assert_eq!(session.code_action(uri, (4, 20), quickfix), actions.clone());
        // `only` is not read: the capability is the boolean `true`, not a kind
        // list, so an unrelated kind still gets the one action (§7A).
        let refactor = json!({"diagnostics": [], "only": ["refactor"]});
        assert_eq!(session.code_action(uri, (4, 20), refactor), actions.clone());
        let edits = list[0]["edit"]["changes"][uri].as_array().expect("edits");
        assert_eq!(edits.len(), 1, "{uri}: {edits:?}");
        let start = line_point(text, 4, word);
        let end = (start.0, start.1 + (to - from) as u64);
        assert_eq!(
            edits[0]["range"]["start"],
            json!({"line": start.0, "character": start.1})
        );
        assert_eq!(
            edits[0]["range"]["end"],
            json!({"line": end.0, "character": end.1})
        );
        let new_text = edits[0]["newText"].as_str().expect("a newText");
        assert_eq!(new_text, expected, "{uri}");
        assert!(!new_text.contains("$1"), "{new_text}");
        let applied = format!("{}{}{}", &text[..from], new_text, &text[to..]);
        let applied_uri = format!("{uri}-applied");
        session.open(&applied_uri, &applied, 2);
        assert_eq!(
            session.code_action(&applied_uri, (4, 20), json!({"diagnostics": [diagnostic]})),
            json!([]),
            "{applied}"
        );
    }
    assert_eq!(
        session.code_action("file:///double", (0, 0), json!({"diagnostics": []})),
        json!([])
    );
    session.finish();
}

/// Runs the portable binary with `args` and no input at all.
fn run_argv(args: &[&str]) -> (ExitStatus, String, String) {
    let output = fresh_command()
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run blastit-lsp");
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("UTF-8 output");
    (output.status, text(output.stdout), text(output.stderr))
}

/// The probe (`--version`/`-V`), the misuse sentence (`2`), and the bare
/// session whose end of input is `1` — silent on both streams.
#[test]
fn the_argv_surface_is_the_probe_the_refusal_and_the_session() {
    let (status, stdout, stderr) = run_argv(&["--version"]);
    assert_eq!(status.code(), Some(0), "{stderr:?}");
    assert_eq!(
        stdout,
        format!("blastit-lsp {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(stderr.is_empty(), "{stderr:?}");
    let (status, short, stderr) = run_argv(&["-V"]);
    assert_eq!(status.code(), Some(0), "{stderr:?}");
    assert_eq!(short, stdout, "`-V` is the same line as `--version`");

    for argument in ["--help", "-h", "--bogus"] {
        let (status, stdout, stderr) = run_argv(&[argument]);
        assert_eq!(status.code(), Some(2), "{argument}: {stderr:?}");
        assert!(stdout.is_empty(), "{argument}: {stdout:?}");
        let sentence = stderr.trim_end();
        assert!(
            sentence.starts_with("blastit-lsp is a language server") && sentence.contains("stdin"),
            "{argument}: {sentence:?}"
        );
        assert_eq!(sentence.lines().count(), 1, "{argument}: {sentence:?}");
    }

    let (status, stdout, stderr) = run_argv(&[]);
    assert_eq!(status.code(), Some(1), "EOF without `shutdown` is `1`");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.is_empty(), "{stderr:?}");
}
