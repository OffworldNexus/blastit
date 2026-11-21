//! The `blastit-lsp` server — and `blastit lsp`, which delegates to it — is the
//! LSP 3.17 subset Zed needs, on stdin/stdout.
//!
//! This is a **text tool** the developer's editor spawns: it reads framed
//! JSON-RPC from stdin, answers on stdout, and never opens a file, reads an
//! environment variable, or holds a privilege. A `Blastfile` is policy, not
//! input (`blastit_manifest::blastfile`), so the only thing this module adds to
//! the manifest's threat model is a byte stream it must never let desynchronise.
//!
//! Every promise of `docs/design/blastfile-lsp-zed.md` lives here, once:
//!
//! * §3 — the frame reader and writer: `Content-Length: N` with `N` in UTF-8
//!   bytes, the header block capped before anything is allocated, the body read
//!   by length and never by newline, and `-32700` for a frame that is not JSON;
//! * §2 — the ten messages, the capability document, and the error-code table;
//!   ids are echoed verbatim, a notification is never answered;
//! * §4 — 1-based `char` positions in, 0-based UTF-16 units out, in one pass
//!   over the document per publish;
//! * §5 — the diagnostics are the CLI's own sentences at the CLI's own
//!   positions, pushed on every `didOpen`/`didChange` and cleared on `didClose`;
//! * §6 — one whole-document formatting edit from `blastfile::fmt`, or `[]`, or
//!   `null`;
//! * §7 — hover from a bounded static table, never a second grammar;
//! * §7.6 and §7A — the two SQL tokens `$?`/`...` inside a `query` interior,
//!   and one `quickfix` whose single `TextEdit` is the statement
//!   `blastit_manifest::parameterize` returns for the string under the range.
//!
//! The Quick Fix is not a second implementation: the rewrite is
//! `blastit_manifest::parameterize`, the manifest's re-export of the same
//! function `blastit check` and `--learn` use (`R66`), so the editor and the
//! CLI cannot drift.
//!
//! stdout carries framed messages and nothing else; stderr carries at most four
//! fixed sentences (§11.3) and never a byte of document text — a `Blastfile`
//! holds `query '…'` strings, and a log line is still a log line.
//!
//! The deny list below is enforced by `cargo clippy -- -D warnings`: a stray
//! `println!`, an `unwrap()` on a JSON shape, or a `v[i]` cannot compile, which
//! matters because an editor does not restart a server that dies on its own.
//! `#![forbid(unsafe_code)]` is the other half of the portability claim: a
//! foreign declaration needs `unsafe`, so no privilege call can be reached
//! without a crate, and this crate names none (§1.3, `ADR-0013`).

#![forbid(unsafe_code)]
#![deny(clippy::print_stdout)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::indexing_slicing
)]

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::panic::AssertUnwindSafe;

use blastit_manifest::blastfile;
use blastit_manifest::parameterize;
use blastit_manifest::{Manifest, ManifestError, ManifestWarning, Position, QuerySite, UidLayout};
use serde_json::{Map, Value, json};

// §7's hover **data** lives in its own file: the table is a transcription of
// `docs/reference/blastfile.md`, and keeping it out of this module is what lets
// the reader of the protocol see the protocol. The module §1's boundary table
// names is still this one, and the entry an editor spawns is still
// `crate::run`.
mod hover;

use hover::{env_var_paragraph, hover_text, is_env_name, sql_hover};

/// Most bytes one header block may hold, its `\r\n\r\n` terminator included
/// (§3.1). Zed writes one 19-byte header, so this is 400× headroom and `O(1)`
/// memory.
const MAX_FRAME_HEADER_BYTES: usize = 8_192;

/// Most bytes one frame body may hold (§3.1).
///
/// A legal 16 MiB `Blastfile` is newline-heavy and JSON escapes `\n` to two
/// bytes, so the bound is twice the parser's own cap plus one chunk of headroom
/// for the envelope and a `uri`. The value is written from the parser's cap, and
/// `lsp.rs`'s unit tests assert the literal, so a change to either is a test
/// failure rather than a silent resize.
const MAX_FRAME_BODY_BYTES: usize = 2 * blastfile::MAX_SOURCE_BYTES + 65_536;

/// Most `Blastfile`s one session tracks at once (§2.5): 32 is six times the
/// realistic maximum, and the 33rd gets a visible notice instead of memory.
const MAX_DOCUMENTS: usize = 32;

/// Most bytes one tracked document's text may hold: the parser's own cap, so
/// storing more would only be weight the parser refuses anyway (§2.5).
const MAX_DOCUMENT_BYTES: usize = blastfile::MAX_SOURCE_BYTES;

/// Most diagnostics one publish carries (§5.7): a file past 512 squiggles is
/// unreadable, so the list is bounded before it is serialized.
const MAX_DIAGNOSTICS: usize = 512;

/// The scratch buffer the header reader fills: no allocation is sized by
/// `Content-Length` before the cap check (§3.1).
const READ_CHUNK: usize = 8_192;

/// The one `source` every diagnostic carries (§5.2).
const SOURCE: &str = "blastit";

// §11.3's four stderr sentences. There is no fifth: an I/O error on either
// stream ends the loop with `ENDED`, because the stream is gone either way, and
// a panicking parse is `INTERNAL`.
const ENDED: &str = "the input ended inside a frame";
const HEADER_TOO_LONG: &str = "the header block is too long";
const BODY_TOO_LONG: &str = "the body is too long";
const INTERNAL: &str = "internal error";

/// The one sentence a misused `blastit-lsp` prints on stderr.
///
/// It is the binary's **whole** argv surface — there is no usage block and no
/// option to list, because the server takes none by design: what it answers is
/// on stdin, and what it says is on stdout.
const ARGUMENTS: &str = concat!(
    "blastit-lsp is a language server: it takes no arguments and speaks LSP ",
    "on stdin and stdout; point your editor at it."
);

// §2.4's codes. `-32603` and `-32800` are reserved and never emitted.
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const NOT_INITIALIZED: i32 = -32002;

/// The one-line ASCII sentence each error code carries (§2.4).
///
/// A fixed table, so an error reply can never hold a `serde_json` parse detail,
/// a URI, or a byte of document text.
const ERROR_MESSAGES: &[(i32, &str)] = &[
    (PARSE_ERROR, "the body is not a JSON message"),
    (INVALID_REQUEST, "this is not a JSON-RPC 2.0 request"),
    (METHOD_NOT_FOUND, "this method is not supported"),
    (INVALID_PARAMS, "the params of this request are unusable"),
    (NOT_INITIALIZED, "initialize must be the first request"),
];

/// The document states a `didChange` can be refused for, each published as a
/// severity-2 notice at `(0,0)` (§2.5).
const NO_DOCUMENT: &str = "blastit lsp has no open document for this file; reopen it";
const INCREMENTAL: &str = "incremental sync is not supported; reopen the file";
const TOO_MANY_DOCUMENTS: &str = "blastit lsp tracks at most 32 open Blastfiles; close one";

/// The one action §7A offers: the page's own words (`docs/local.md`
/// step 2), and the only kind `codeActionProvider: true` can produce.
const QUICK_FIX_TITLE: &str = "Parameterize SQL for Blastfile ($?)";
const QUICK_FIX_KIND: &str = "quickfix";

/// One document a session holds (§2.5), keyed by its `uri` — which is opaque:
/// compared and echoed, never parsed into a path, never opened.
///
/// Only the text is kept: a publish carries the version the client sent with the
/// text, so storing it a second time would be a second truth about the buffer.
struct Document {
    /// The whole document, as the client last sent it.
    text: String,
    /// The `query` interiors `Manifest::from_blastfile_str` found in [`text`],
    /// filled by the first hover or `codeAction` and dropped with the text it
    /// was parsed from (§13.3: one cache entry, invalidated by `didChange`).
    ///
    /// `None` is "this document does not parse", which is a real answer — a
    /// `LiteralQuery` never reaches a `Checked`, so §7.6's sites come from the
    /// parse-and-lower entry point and not from `check`.
    ///
    /// [`text`]: Document::text
    sites: OnceCell<Option<Vec<QuerySite>>>,
}

impl Document {
    /// A tracked document with its sites not yet computed.
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            sites: OnceCell::new(),
        }
    }
}

/// One session's state (§2.5).
struct Server {
    /// The open documents, keyed by their `uri`.
    documents: BTreeMap<String, Document>,
    /// Whether `initialize` completed.
    initialized: bool,
    /// Whether `shutdown` was received.
    shutdown: bool,
}

/// What the message loop does after one message.
enum Continuation {
    /// Read the next frame.
    Serve,
    /// Leave the loop: `exit` arrived.
    Exit,
}

/// What the frame reader found (§3.4).
enum Frame {
    /// A frame whose body is JSON.
    Body(Value),
    /// A body that is not UTF-8 or not JSON: answer `-32700` and keep serving,
    /// because the length was known and the next frame's offset is too.
    Malformed,
    /// A frame the reader cannot resynchronize from: answer `-32700`, say why
    /// on stderr, and end the session with `1`.
    Unframeable(&'static str),
    /// The stream ended.
    Eof,
}

/// Runs one LSP session on this process's stdin and stdout, returning the exit
/// status: `0` only after the client said `shutdown`, `1` otherwise (§3.4).
///
/// The panic hook is installed **first**: the default one prints the payload,
/// and a `ManifestError`'s `Debug` can quote document text. Every message is
/// then handled inside the loop, so one bad frame is never a dead session.
pub fn run() -> u8 {
    std::panic::set_hook(Box::new(|_| eprintln!("blastit lsp: {INTERNAL}")));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = Input::new(stdin.lock());
    let mut out = stdout.lock();
    Server::new().serve(&mut input, &mut out)
}

/// The `blastit-lsp` binary's entry point, given the argv its `main` read: argv
/// first, and — only with **no** argument — exactly the session
/// `blastit lsp` runs.
///
/// The argv is a parameter and not an environment read here on purpose: this
/// module is the text tool both entries share, and its critical suite asserts it
/// names no environment at all (`crates/blastit-lsp/tests/e2e_lsp.rs`, R3). The
/// binary's own `main` is the one place a process's arguments exist.
pub fn run_binary(args: &[OsString]) -> u8 {
    let mut out = io::stdout();
    let mut err = io::stderr();
    if let Some(status) = answer_argv(args, &mut out, &mut err) {
        return status;
    }
    run()
}

/// The one line `--version` prints: this package's own version, which is the
/// same `CARGO_PKG_VERSION` the capability document's `serverInfo` carries, so
/// a probe of the binary and a probe of the protocol cannot disagree (§2.3).
fn version_line() -> String {
    format!("blastit-lsp {}", env!("CARGO_PKG_VERSION"))
}

/// What an invocation with arguments says and returns, or `None` for "no
/// argument" — which is the session, and prints **nothing**: an LSP client's
/// stream must stay clean, so a bare run has no banner and no version line.
///
/// `--version`/`-V`, alone, is the probe a package manager or an editor
/// extension makes before it spawns anything: one line on stdout, `0`. Every
/// other argument is a misuse: one sentence on stderr, `2`.
pub fn answer_argv(args: &[OsString], out: &mut impl Write, err: &mut impl Write) -> Option<u8> {
    let first = args.first()?;
    if args.len() == 1 && matches!(first.to_str(), Some("--version" | "-V")) {
        // A stream that fails is still a probe that answered: the status is the
        // probe's, and there is nothing left to complain on.
        let _ = writeln!(out, "{}", version_line());
        let _ = out.flush();
        return Some(0);
    }
    let _ = writeln!(err, "{ARGUMENTS}");
    let _ = err.flush();
    Some(2)
}

/// One framed session: the frame reader and the message loop (§3).
struct Input<R> {
    /// The stream the frames arrive on.
    inner: R,
    /// The fixed scratch buffer a read fills.
    scratch: [u8; READ_CHUNK],
    /// How many bytes of `scratch` a read filled.
    filled: usize,
    /// How many of those bytes have been consumed.
    at: usize,
}

impl<R: Read> Input<R> {
    /// A reader over `inner` with an empty scratch buffer.
    fn new(inner: R) -> Self {
        Self {
            inner,
            scratch: [0; READ_CHUNK],
            filled: 0,
            at: 0,
        }
    }

    /// Refills the scratch buffer when it is empty, returning the usable count.
    fn fill(&mut self) -> io::Result<usize> {
        if self.at >= self.filled {
            self.filled = self.inner.read(&mut self.scratch)?;
            self.at = 0;
        }
        Ok(self.filled.saturating_sub(self.at))
    }

    /// Reads one byte, or `None` at the end of the stream.
    fn byte(&mut self) -> io::Result<Option<u8>> {
        if self.fill()? == 0 {
            return Ok(None);
        }
        let byte = self.scratch.get(self.at).copied();
        self.at = self.at.saturating_add(1);
        Ok(byte)
    }

    /// Fills `out` completely, returning `false` when the stream ended first.
    fn exact(&mut self, out: &mut [u8]) -> io::Result<bool> {
        let mut written = 0_usize;
        while written < out.len() {
            let available = self.fill()?;
            if available == 0 {
                return Ok(false);
            }
            let take = available.min(out.len().saturating_sub(written));
            let source = self.scratch.get(self.at..self.at.saturating_add(take));
            let target = out.get_mut(written..written.saturating_add(take));
            let (Some(source), Some(target)) = (source, target) else {
                return Ok(false);
            };
            target.copy_from_slice(source);
            self.at = self.at.saturating_add(take);
            written = written.saturating_add(take);
        }
        Ok(true)
    }

    /// One frame: its body read by its declared length, never by newline (§3.2).
    fn frame(&mut self) -> io::Result<Frame> {
        let mut header: Vec<u8> = Vec::with_capacity(READ_CHUNK);
        loop {
            let Some(byte) = self.byte()? else {
                return Ok(if header.is_empty() {
                    Frame::Eof
                } else {
                    Frame::Unframeable(ENDED)
                });
            };
            header.push(byte);
            if header.ends_with(b"\r\n\r\n") {
                break;
            }
            // The cap includes the terminator, so a block of exactly
            // `MAX_FRAME_HEADER_BYTES` ending in `\r\n\r\n` was accepted above.
            if header.len() >= MAX_FRAME_HEADER_BYTES {
                return Ok(Frame::Unframeable(HEADER_TOO_LONG));
            }
        }
        let Some(length) = declared_length(&header) else {
            return Ok(Frame::Unframeable(ENDED));
        };
        if length > MAX_FRAME_BODY_BYTES {
            // Refused before the body is read or allocated (§3.2 step 3).
            return Ok(Frame::Unframeable(BODY_TOO_LONG));
        }
        let mut body = vec![0_u8; length];
        if !self.exact(&mut body)? {
            return Ok(Frame::Unframeable(ENDED));
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => Ok(Frame::Body(value)),
            Err(_) => Ok(Frame::Malformed),
        }
    }
}

/// The `Content-Length` a header block declares (§3.2 step 2), or `None` when
/// the block has no such line, has two, or holds a value that is not ASCII
/// digits.
///
/// The name is compared ASCII case-insensitively and every other header line is
/// ignored, which is the read half of §2.1; the write half is one exact line.
fn declared_length(header: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(header).ok()?;
    let mut found: Option<usize> = None;
    for line in text.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if found.is_some() {
            // A second `Content-Length` is a frame the reader cannot trust.
            return None;
        }
        let digits = value.strip_prefix(' ')?;
        // More than ten digits cannot be a body this server accepts, and the
        // check also rules out a sign, a space and an empty value.
        if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        found = Some(digits.parse::<usize>().ok()?);
    }
    found
}

/// Writes one framed message: the exact header line, then the body, then a
/// flush — a buffered reply would read as a hung server (§3.3).
fn write_message(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut frame = Vec::with_capacity(body.len().saturating_add(32));
    frame.extend_from_slice(b"Content-Length: ");
    frame.extend_from_slice(body.len().to_string().as_bytes());
    frame.extend_from_slice(b"\r\n\r\n");
    frame.extend_from_slice(&body);
    out.write_all(&frame)?;
    out.flush()
}

/// One `result` reply, with the client's id echoed verbatim (§2.2).
fn write_result(out: &mut impl Write, id: Value, result: Value) -> io::Result<()> {
    write_message(out, &json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// One `error` reply with a fixed ASCII sentence (§2.4).
fn write_error(out: &mut impl Write, id: Value, code: i32) -> io::Result<()> {
    let message = ERROR_MESSAGES
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, sentence)| *sentence)
        .unwrap_or("this request failed");
    write_message(
        out,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message},
        }),
    )
}

impl Server {
    /// A session with nothing open and no handshake behind it.
    fn new() -> Self {
        Self {
            documents: BTreeMap::new(),
            initialized: false,
            shutdown: false,
        }
    }

    /// The exit status this session earns: `0` only after `shutdown`.
    fn status(&self) -> u8 {
        if self.shutdown { 0 } else { 1 }
    }

    /// Reads, answers and repeats until the stream ends or `exit` arrives.
    fn serve(&mut self, input: &mut Input<impl Read>, out: &mut impl Write) -> u8 {
        loop {
            let frame = match input.frame() {
                Ok(frame) => frame,
                Err(_) => {
                    // §11.3: an I/O error on either stream is the stream being
                    // gone, and there is no fifth sentence for it.
                    eprintln!("blastit lsp: {ENDED}");
                    return self.status();
                }
            };
            match frame {
                Frame::Eof => return self.status(),
                Frame::Malformed => {
                    if write_error(out, Value::Null, PARSE_ERROR).is_err() {
                        eprintln!("blastit lsp: {ENDED}");
                        return self.status();
                    }
                }
                Frame::Unframeable(reason) => {
                    // §2.4: the offset of the next frame is unknown, so the
                    // session ends after the one answer it can give.
                    let _ = write_error(out, Value::Null, PARSE_ERROR);
                    eprintln!("blastit lsp: {reason}");
                    return 1;
                }
                Frame::Body(message) => match self.handle(message, out) {
                    Ok(Continuation::Serve) => {}
                    Ok(Continuation::Exit) => return self.status(),
                    Err(_) => {
                        eprintln!("blastit lsp: {ENDED}");
                        return self.status();
                    }
                },
            }
        }
    }

    /// One message: a notification is never answered, a request gets exactly
    /// one result or one error (§2.2).
    fn handle(&mut self, message: Value, out: &mut impl Write) -> io::Result<Continuation> {
        let Value::Object(object) = message else {
            write_error(out, Value::Null, INVALID_REQUEST)?;
            return Ok(Continuation::Serve);
        };
        let id = object.get("id").cloned();
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            write_error(out, id.unwrap_or(Value::Null), INVALID_REQUEST)?;
            return Ok(Continuation::Serve);
        }
        if matches!(id, Some(Value::Null)) {
            // An explicit `"id": null` is an invalid request, answered rather
            // than treated as a notification a client would hang on (§2.2).
            write_error(out, Value::Null, INVALID_REQUEST)?;
            return Ok(Continuation::Serve);
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            write_error(out, id.unwrap_or(Value::Null), INVALID_REQUEST)?;
            return Ok(Continuation::Serve);
        };
        let method = method.to_owned();
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        match id {
            None => self.notification(&method, &params, out),
            Some(id) => self.request(&method, params, id, out),
        }
    }

    /// A notification: it is never answered, whatever it asks for (§2.2).
    fn notification(
        &mut self,
        method: &str,
        params: &Value,
        out: &mut impl Write,
    ) -> io::Result<Continuation> {
        match method {
            "exit" => return Ok(Continuation::Exit),
            "initialized"
            | "workspace/didChangeConfiguration"
            | "$/cancelRequest"
            | "$/setTrace" => {}
            // Out of contract before the handshake: a client that has not
            // initialized is not expecting a notification stream either.
            _ if !self.initialized || self.shutdown => {}
            "textDocument/didOpen" => self.did_open(params, out)?,
            "textDocument/didChange" => self.did_change(params, out)?,
            "textDocument/didClose" => self.did_close(params, out)?,
            _ => {}
        }
        Ok(Continuation::Serve)
    }

    /// A request: exactly one reply, from the table of §2.2 and §2.4.
    fn request(
        &mut self,
        method: &str,
        params: Value,
        id: Value,
        out: &mut impl Write,
    ) -> io::Result<Continuation> {
        if !self.initialized && method != "initialize" {
            write_error(out, id, NOT_INITIALIZED)?;
            return Ok(Continuation::Serve);
        }
        if self.shutdown {
            write_error(out, id, INVALID_REQUEST)?;
            return Ok(Continuation::Serve);
        }
        match method {
            "initialize" => {
                if self.initialized {
                    write_error(out, id, INVALID_REQUEST)?;
                } else {
                    self.initialized = true;
                    write_result(out, id, capabilities())?;
                }
            }
            "exit" => {
                write_result(out, id, Value::Null)?;
                return Ok(Continuation::Exit);
            }
            "shutdown" => {
                self.shutdown = true;
                write_result(out, id, Value::Null)?;
            }
            "window/workDoneProgress/create" => write_result(out, id, Value::Null)?,
            // Declined, not errored: definition is not advertised, and a probe
            // of an unadvertised capability is a `null` result (§7.4).
            "textDocument/definition" => write_result(out, id, Value::Null)?,
            "textDocument/formatting" => match self.formatting(&params) {
                Some(result) => write_result(out, id, result)?,
                None => write_error(out, id, INVALID_PARAMS)?,
            },
            "textDocument/hover" => match self.hover(&params) {
                Some(result) => write_result(out, id, result)?,
                None => write_error(out, id, INVALID_PARAMS)?,
            },
            // §7A: at most one `quickfix`, or `[]`. `context.diagnostics` is
            // read by nobody (§7A.1), so a stale client copy cannot decide what
            // the edit says.
            "textDocument/codeAction" => match self.code_action(&params) {
                Some(result) => write_result(out, id, result)?,
                None => write_error(out, id, INVALID_PARAMS)?,
            },
            // A notification's method sent as a request is accepted, so a
            // client that attached an id is not left hanging on a reply — and
            // its params are read, so §2.4's `-32602` row is answered for a
            // missing `textDocument.uri`/`text` rather than silently dropped.
            "textDocument/didOpen" => {
                if document_uri(&params).is_some() && document_text(&params).is_some() {
                    self.did_open(&params, out)?;
                    write_result(out, id, Value::Null)?;
                } else {
                    write_error(out, id, INVALID_PARAMS)?;
                }
            }
            "textDocument/didChange" => {
                if document_uri(&params).is_some() {
                    self.did_change(&params, out)?;
                    write_result(out, id, Value::Null)?;
                } else {
                    write_error(out, id, INVALID_PARAMS)?;
                }
            }
            "textDocument/didClose" => {
                if document_uri(&params).is_some() {
                    self.did_close(&params, out)?;
                    write_result(out, id, Value::Null)?;
                } else {
                    write_error(out, id, INVALID_PARAMS)?;
                }
            }
            "initialized"
            | "workspace/didChangeConfiguration"
            | "$/cancelRequest"
            | "$/setTrace" => write_result(out, id, Value::Null)?,
            _ => write_error(out, id, METHOD_NOT_FOUND)?,
        }
        Ok(Continuation::Serve)
    }

    /// `textDocument/didOpen`: insert the document and publish its diagnostics
    /// (§2.5, §5.3).
    fn did_open(&mut self, params: &Value, out: &mut impl Write) -> io::Result<()> {
        let (Some(uri), Some(text)) = (document_uri(params), document_text(params)) else {
            return Ok(());
        };
        let version = document_version(params);
        if text.len() > MAX_DOCUMENT_BYTES {
            // The parser refuses from the length before lexing, so the notice
            // is its own `TooLarge` sentence at `(0,0)` — and the buffer is not
            // stored, so a later request for it answers `null`.
            self.documents.remove(uri);
            let diagnostics = diagnostics(text);
            return publish(out, uri, None, &diagnostics);
        }
        if !self.documents.contains_key(uri) && self.documents.len() >= MAX_DOCUMENTS {
            let notice = notice(TOO_MANY_DOCUMENTS);
            return publish(out, uri, None, &[notice]);
        }
        self.documents.insert(uri.to_owned(), Document::new(text));
        let diagnostics = diagnostics(text);
        publish(out, uri, Some(version), &diagnostics)
    }

    /// `textDocument/didChange`: the one full replacement, or the visible
    /// refusal (§2.2 item 4, §2.5).
    fn did_change(&mut self, params: &Value, out: &mut impl Write) -> io::Result<()> {
        let Some(uri) = document_uri(params) else {
            return Ok(());
        };
        let version = document_version(params);
        if !self.documents.contains_key(uri) {
            let notice = notice(NO_DOCUMENT);
            return publish(out, uri, None, &[notice]);
        }
        let changes = params.get("contentChanges").and_then(Value::as_array);
        // Exactly one full change object: zero or more than one is a broken
        // client, not a state this server recovers into, so it is dropped
        // silently and the stored text stands (§2.5).
        let Some(changes) = changes else {
            return Ok(());
        };
        if changes.len() != 1 {
            return Ok(());
        }
        let Some(change) = changes.first() else {
            return Ok(());
        };
        if change.get("range").is_some() || change.get("rangeLength").is_some() {
            // Incremental sync: the old text's diagnostics must not survive the
            // refusal, so the entry goes and the next request answers `null`.
            self.documents.remove(uri);
            let notice = notice(INCREMENTAL);
            return publish(out, uri, None, &[notice]);
        }
        let Some(text) = change.get("text").and_then(Value::as_str) else {
            return Ok(());
        };
        if text.len() > MAX_DOCUMENT_BYTES {
            self.documents.remove(uri);
            let diagnostics = diagnostics(text);
            return publish(out, uri, None, &diagnostics);
        }
        self.documents.insert(uri.to_owned(), Document::new(text));
        let diagnostics = diagnostics(text);
        publish(out, uri, Some(version), &diagnostics)
    }

    /// `textDocument/didClose`: drop the document and clear its squiggles, with
    /// no `version` (§5.6).
    fn did_close(&mut self, params: &Value, out: &mut impl Write) -> io::Result<()> {
        let Some(uri) = document_uri(params) else {
            return Ok(());
        };
        if self.documents.remove(uri).is_none() {
            return Ok(());
        }
        publish(out, uri, None, &[])
    }

    /// `textDocument/formatting`: `[]`, one whole-document edit, or `null`
    /// (§6.1). `None` is unusable params, which is `-32602`.
    fn formatting(&self, params: &Value) -> Option<Value> {
        let uri = document_uri(params)?;
        let Some(document) = self.documents.get(uri) else {
            // A formatting request for a document this session never received is
            // not an error worth a squiggle.
            return Some(Value::Null);
        };
        // §3.3: every call into `blastit-manifest` for one document is wrapped,
        // so a panic in the formatter is a declined request rather than a dead
        // session — and the hook already stopped the payload reaching stderr.
        let formatted =
            std::panic::catch_unwind(AssertUnwindSafe(|| blastfile::fmt(&document.text)));
        match formatted {
            Ok(Ok(formatted)) if formatted == document.text => Some(json!([])),
            Ok(Ok(formatted)) => {
                let (line, character) = max_point_utf16(&document.text);
                Some(json!([{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": line, "character": character},
                    },
                    "newText": formatted,
                }]))
            }
            // Never a JSON-RPC error: that would make an editor report a server
            // failure on every save of a broken file (§6.1).
            Ok(Err(_)) | Err(_) => Some(Value::Null),
        }
    }

    /// `textDocument/hover`: the directive's own sentence under the cursor, or
    /// `null` (§7). `None` is unusable params, which is `-32602`.
    fn hover(&self, params: &Value) -> Option<Value> {
        let uri = params
            .get("textDocument")
            .and_then(|document| document.get("uri"))
            .and_then(Value::as_str)?;
        let position = params.get("position")?;
        let line = u32::try_from(position.get("line")?.as_u64()?).ok()?;
        let character = u32::try_from(position.get("character")?.as_u64()?).ok()?;
        let Some(document) = self.documents.get(uri) else {
            return Some(Value::Null);
        };
        let lines = Lines::new(&document.text);
        let Some(line_text) = lines.line(line) else {
            return Some(Value::Null);
        };
        // §7.6: inside a `query` interior the reply is one of the two SQL
        // tokens or `null` — no word table, no SQL lesson. A position outside
        // every interior takes `word_at` exactly as it always has, so
        // `$DATABASE_URL` still answers and a `$?` in a comment does not.
        let byte = lines.offset(line, character);
        if let Some(site) = self
            .query_sites(document)
            .iter()
            .find(|site| inside(site, byte))
        {
            let Some((start, end)) = sql_token(&document.text, byte) else {
                return Some(Value::Null);
            };
            // The token must be this string's own characters: a `$?` that ends
            // where the opening quote begins is outside the interior.
            if start < site.interior.start || end > site.interior.end {
                return Some(Value::Null);
            }
            let Some(token) = document.text.get(start as usize..end as usize) else {
                return Some(Value::Null);
            };
            let Some(value) = sql_hover(token) else {
                return Some(Value::Null);
            };
            return Some(token_hover(&lines, start, end, value));
        }
        let start = char_index(line_text, character);
        let Some(word) = word_at(line_text, start) else {
            return Some(Value::Null);
        };
        let text = if word.0.starts_with('$') {
            let name = word.0.trim_start_matches('$');
            if is_env_name(name) {
                Some(env_var_paragraph(name))
            } else {
                None
            }
        } else {
            hover_text(&word.0).map(str::to_owned)
        };
        let Some(text) = text else {
            return Some(Value::Null);
        };
        // The word's own span, converted back: `word.1`/`word.2` are `char`
        // indices, and `Position` counts `char`s, so the 1-based column is the
        // index plus one and `Lines::point` does the UTF-16 unit.
        let one_based = line.saturating_add(1);
        let start_point = lines.point(Position::at(
            one_based,
            u32::try_from(word.1).unwrap_or(u32::MAX).saturating_add(1),
        ));
        let end_point = lines.point(Position::at(
            one_based,
            u32::try_from(word.2).unwrap_or(u32::MAX).saturating_add(1),
        ));
        Some(json!({
            "contents": {"kind": "markdown", "value": text},
            "range": {
                "start": {"line": start_point.0, "character": start_point.1},
                "end": {"line": end_point.0, "character": end_point.1},
            },
        }))
    }

    /// The `query` interiors of one tracked document, parsed once per text
    /// (§13.3). A document that does not parse has none, so a hover falls back
    /// to the word table and a `codeAction` answers `[]`.
    fn query_sites<'a>(&self, document: &'a Document) -> &'a [QuerySite] {
        document
            .sites
            .get_or_init(|| sites_of(&document.text))
            .as_deref()
            .unwrap_or_default()
    }

    /// `textDocument/codeAction`: at most one `quickfix` whose single
    /// `TextEdit` replaces the whole `query` string's interior with the
    /// statement `parameterize` wrote (§7A). `None` is unusable params
    /// (`-32602`); `[]` is "nothing to offer", which is never an error.
    fn code_action(&self, params: &Value) -> Option<Value> {
        let uri = document_uri(params)?;
        let range = params.get("range")?;
        let (start_line, start_character) = point_of(range.get("start")?)?;
        let (end_line, end_character) = point_of(range.get("end")?)?;
        let Some(document) = self.documents.get(uri) else {
            return Some(json!([]));
        };
        let lines = Lines::new(&document.text);
        let start = lines.offset(start_line, start_character);
        let end = lines.offset(end_line, end_character);
        // An inverted range is a client quirk, not an error (§7A.2).
        if start > end {
            return Some(json!([]));
        }
        let Some(site) = self
            .query_sites(document)
            .iter()
            .find(|site| intersects(site, start, end))
        else {
            return Some(json!([]));
        };
        // The one implementation (`R66`): `check`/`--learn`'s own rewrite. It
        // already refuses the two protocol statements, a redacted `…`, the
        // file/network capabilities `--learn` refuses, and everything that
        // would not re-parse clean.
        let Some(rewritten) = parameterize(&site.raw) else {
            return Some(json!([]));
        };
        let Some(interior) = document
            .text
            .get(site.interior.start as usize..site.interior.end as usize)
        else {
            return Some(json!([]));
        };
        let new_text = encode_interior(&document.text, site, &rewritten);
        // §7A.3(a) and (b): a real change, not an identity edit that re-checks
        // clean. The encoded text is what the file would hold, so it is what
        // has to differ.
        if new_text == interior {
            return Some(json!([]));
        }
        let start_point = lines.point_at(site.interior.start);
        let end_point = lines.point_at(site.interior.end);
        Some(json!([{
            "title": QUICK_FIX_TITLE,
            "kind": QUICK_FIX_KIND,
            "edit": {"changes": {uri: [{
                "range": {
                    "start": {"line": start_point.0, "character": start_point.1},
                    "end": {"line": end_point.0, "character": end_point.1},
                },
                "newText": new_text,
            }]}},
        }]))
    }
}

/// The `textDocument.uri` a sync message names (§2.5).
fn document_uri(params: &Value) -> Option<&str> {
    params
        .get("textDocument")
        .and_then(|document| document.get("uri"))
        .and_then(Value::as_str)
}

/// The `textDocument.text` a `didOpen` carries (§2.2).
fn document_text(params: &Value) -> Option<&str> {
    params
        .get("textDocument")
        .and_then(|document| document.get("text"))
        .and_then(Value::as_str)
}

/// The `textDocument.version` a sync message carries, `0` when absent (§2.5).
fn document_version(params: &Value) -> i64 {
    params
        .get("textDocument")
        .and_then(|document| document.get("version"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// One `{line, character}` object as an LSP point (§4.6). A missing or
/// non-numeric member is unusable params, which is `-32602` (§2.4, C8).
fn point_of(value: &Value) -> Option<(u32, u32)> {
    let line = u32::try_from(value.get("line")?.as_u64()?).ok()?;
    let character = u32::try_from(value.get("character")?.as_u64()?).ok()?;
    Some((line, character))
}

/// §7.6's predicate: whether `byte` lies inside a `query` interior.
///
/// `start <= byte < end`, exactly as the design writes it: the closing quote is
/// **not** inside the string, so hovering it — or the character after it —
/// answers `null` and falls through to nothing. A position outside every
/// interior takes the word table.
fn inside(site: &QuerySite, byte: u32) -> bool {
    site.interior.start <= byte && byte < site.interior.end
}

/// §7A.2's range test: a real range overlaps the interior, and a zero-width
/// caret (Zed's) sits inside it.
fn intersects(site: &QuerySite, start: u32, end: u32) -> bool {
    if start == end {
        site.interior.start <= start && start < site.interior.end
    } else {
        start < site.interior.end && site.interior.start < end
    }
}

/// §7.6's token at `byte`: the source byte span of `$?` or `...`.
///
/// The test is on **source** characters, so the range it returns is one the
/// editor can select: the `$`, its `?`, or the character just after it for
/// `$?`; the dot run the cursor is in, cut to its first three, for `...`.
fn sql_token(text: &str, byte: u32) -> Option<(u32, u32)> {
    let at = byte as usize;
    let bytes = text.as_bytes();
    let dollar = |start: usize| bytes.get(start..start.saturating_add(2)) == Some(&b"$?"[..]);
    if dollar(at) {
        return Some((byte, byte.saturating_add(2)));
    }
    if at >= 1 && dollar(at - 1) {
        return Some((byte - 1, byte.saturating_add(1)));
    }
    if at >= 2 && dollar(at - 2) {
        return Some((byte - 2, byte));
    }
    if bytes.get(at) == Some(&b'.') {
        let mut start = at;
        while start > 0 && bytes.get(start - 1) == Some(&b'.') {
            start = start.saturating_sub(1);
        }
        let mut end = at;
        while bytes.get(end) == Some(&b'.') {
            end = end.saturating_add(1);
        }
        if end.saturating_sub(start) >= 3 {
            let start = u32::try_from(start).ok()?;
            return Some((start, start.saturating_add(3)));
        }
    }
    None
}

/// One `Hover` whose `range` is the token's own source span (§7.6), converted
/// with [`Lines::point_at`] and never through the decoded statement.
fn token_hover(lines: &Lines<'_>, start: u32, end: u32, value: &str) -> Value {
    let start_point = lines.point_at(start);
    let end_point = lines.point_at(end);
    json!({
        "contents": {"kind": "markdown", "value": value},
        "range": {
            "start": {"line": start_point.0, "character": start_point.1},
            "end": {"line": end_point.0, "character": end_point.1},
        },
    })
}

/// The `query` sites of a document, from `Manifest::from_blastfile_str` —
/// parse and lower, **no** validate (§7.6), so a `LiteralQuery` still has its
/// site. `None` is a document that does not parse: there are no sites.
///
/// The walk is `validate`'s own order (groups, then routes), and a trait a view
/// `use`s contributes its site twice; the interiors are disjoint by
/// construction, so the first match is the only match.
fn sites_of(text: &str) -> Option<Vec<QuerySite>> {
    // §3.3: the parse is wrapped like every other call into `blastit-manifest`,
    // so a panic is "no sites" rather than a dead session — and the hook already
    // stopped the payload reaching stderr.
    let parsed = std::panic::catch_unwind(AssertUnwindSafe(|| Manifest::from_blastfile_str(text)));
    let manifest = parsed.ok()?.ok()?;
    let mut sites: Vec<QuerySite> = Vec::new();
    let grants = manifest
        .groups()
        .values()
        .flat_map(|group| group.grants().values());
    let route_grants = manifest
        .routes()
        .iter()
        .flat_map(|route| route.grants().values());
    for grant in grants.chain(route_grants) {
        if let Some(postgres) = grant.as_postgresql() {
            sites.extend(postgres.query_sites().iter().cloned());
        }
    }
    Some(sites)
}

/// §7A.4: `parameterize` returns **decoded** text, and the edit must be text
/// the file can hold.
///
/// A `'…'` interior takes it verbatim, because a parameterised statement holds
/// no `'`. A `"…"` interior escapes with the token's own four escapes, in this
/// order (`\` first, or `\"` would become `\\"`): `\` → `\\`, `"` → `\"`, a
/// newline → `\n`, a tab → `\t`.
fn encode_interior(source: &str, site: &QuerySite, decoded: &str) -> String {
    let quote = site
        .interior
        .start
        .checked_sub(1)
        .and_then(|at| source.as_bytes().get(at as usize))
        .copied();
    if quote != Some(b'"') {
        return decoded.to_owned();
    }
    let mut out = String::with_capacity(decoded.len());
    for character in decoded.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// §2.3's capability document, byte for byte where it matters.
fn capabilities() -> Value {
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
}

/// The whole diagnostic set for one text (§5.3): the errors of a refusal, or the
/// warnings of a clean file, never both.
///
/// Every call into `blastit-manifest` for one document is wrapped in
/// `catch_unwind`: a panic there would otherwise print its payload — which can
/// quote document text — and end the session (§3.3, S5).
fn diagnostics(text: &str) -> Vec<Value> {
    let checked = std::panic::catch_unwind(AssertUnwindSafe(|| {
        blastfile::check(text, UidLayout::DEFAULT_BASE)
    }));
    let (mut list, total) = match checked {
        Ok(Ok(checked)) => {
            let lines = Lines::new(text);
            let warnings: Vec<Value> = checked
                .warnings
                .iter()
                .zip(checked.sites.iter())
                .map(|(warning, site)| warning_diagnostic(warning, *site, &lines))
                .collect();
            let total = warnings.len();
            (warnings, total)
        }
        Ok(Err(errors)) => {
            let lines = Lines::new(text);
            let list: Vec<Value> = errors
                .errors()
                .iter()
                .map(|error| error_diagnostic(error, &lines))
                .collect();
            let total = list.len();
            (list, total)
        }
        Err(_) => {
            let lines = Lines::new(text);
            let fallback = [diagnostic(
                &lines,
                Position::NONE,
                1,
                "blastit lsp: internal error; run `blastit check`".to_owned(),
            )];
            (fallback.to_vec(), 1)
        }
    };
    if total > MAX_DIAGNOSTICS {
        // The first 511 squiggles, then one line saying how many are left: the
        // list is bounded before it is serialized (§5.7).
        list.truncate(MAX_DIAGNOSTICS.saturating_sub(1));
        list.push(notice(&format!(
            "{} more problems; run `blastit check` for the full list",
            total.saturating_sub(MAX_DIAGNOSTICS.saturating_sub(1))
        )));
    }
    list
}

/// One error, at its own site, with the CLI's sentence minus its location
/// prefix (§5.2).
fn error_diagnostic(error: &ManifestError, lines: &Lines<'_>) -> Value {
    diagnostic(lines, error.position(), 1, error.message())
}

/// One warning, at the CST site `sites()` found for it (§5.4).
fn warning_diagnostic(warning: &ManifestWarning, site: Position, lines: &Lines<'_>) -> Value {
    diagnostic(lines, site, 2, warning.to_string())
}

/// One diagnostic object: a range, a severity, the `source`, and the message —
/// no `code`, no `tags`, no `relatedInformation` (§5.2).
fn diagnostic(lines: &Lines<'_>, at: Position, severity: u8, message: String) -> Value {
    json!({
        "range": lines.range(at),
        "severity": severity,
        "source": SOURCE,
        "message": message,
    })
}

/// One severity-2 notice at `(0,0)`: a session-state message, not a text's
/// diagnostic, so it never carries a `version` (§2.5).
fn notice(message: &str) -> Value {
    json!({
        "range": {
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 0},
        },
        "severity": 2,
        "source": SOURCE,
        "message": message,
    })
}

/// One `textDocument/publishDiagnostics` notification (§5.1).
///
/// The whole set for that `uri`: an editor replaces a URI's set on every push,
/// so a shrinking file needs no deletion message. `version` is absent exactly
/// when the publish describes the session rather than a text it received.
fn publish(
    out: &mut impl Write,
    uri: &str,
    version: Option<i64>,
    diagnostics: &[Value],
) -> io::Result<()> {
    let mut params = Map::new();
    params.insert("uri".to_owned(), Value::String(uri.to_owned()));
    if let Some(version) = version {
        params.insert("version".to_owned(), json!(version));
    }
    params.insert("diagnostics".to_owned(), Value::Array(diagnostics.to_vec()));
    write_message(
        out,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": Value::Object(params),
        }),
    )
}

/// The byte offset of the start of every line, so a position is a binary search
/// and a publish is one pass over the document (§4.2).
struct Lines<'a> {
    /// The document the offsets index.
    text: &'a str,
    /// The byte offset of the start of every line, `0` first.
    starts: Vec<u32>,
}

impl<'a> Lines<'a> {
    /// Indexes `text` once.
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0_u32];
        for (at, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                // `at + 1` is a boundary: the byte after a `\n` starts a line.
                starts.push(at as u32 + 1);
            }
        }
        Self { text, starts }
    }

    /// One line's text, without its terminator: `None` past the last line.
    ///
    /// The document is split on `'\n'` and the terminator is not part of the
    /// line, so a CRLF buffer keeps one `'\r'` that counts as one UTF-16 unit —
    /// which is what the lexer counted as one column (§4.2).
    fn line(&self, line_index: u32) -> Option<&'a str> {
        let start = self.starts.get(line_index as usize).copied()?;
        let end = self
            .starts
            .get(line_index as usize + 1)
            .copied()
            .unwrap_or(self.text.len() as u32)
            .max(start);
        let line = self.text.get(start as usize..end as usize)?;
        Some(line.strip_suffix('\n').unwrap_or(line))
    }

    /// One 1-based `Position` in the editor's coordinates, by binary search
    /// (§4.2).
    fn point(&self, at: Position) -> (u32, u32) {
        if at.is_none() {
            return (0, 0); // `Position::NONE` → (0,0)
        }
        let line_index = at.line().saturating_sub(1);
        let Some(line) = self.line(line_index) else {
            return (0, 0); // a line past the end clamps to (0,0)
        };
        let chars = at.col().saturating_sub(1) as usize;
        let character: usize = line.chars().take(chars).map(char::len_utf16).sum();
        (line_index, character as u32)
    }

    /// The byte offset of one LSP position (§4.6): §4.2's units read
    /// backwards, clamped exactly like [`Lines::point`] — a line past the end is
    /// the document's length, and a character past the line's end is its last
    /// byte.
    fn offset(&self, line: u32, character: u32) -> u32 {
        let Some(line_text) = self.line(line) else {
            return u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        };
        let start = self.starts.get(line as usize).copied().unwrap_or(0);
        let chars = char_index(line_text, character);
        let bytes: usize = line_text.chars().take(chars).map(char::len_utf8).sum();
        start.saturating_add(u32::try_from(bytes).unwrap_or(u32::MAX))
    }

    /// The LSP point of a byte offset (§4.6): the line whose start is the last
    /// one `<= at`, and the UTF-16 length of that line's prefix before `at`.
    ///
    /// The exact inverse of [`Lines::offset`] on a character boundary
    /// (`point_at(offset(line, character)) == (line, character)` for every
    /// legal position), and `O(log lines + character)`.
    fn point_at(&self, at: u32) -> (u32, u32) {
        let at = at.min(u32::try_from(self.text.len()).unwrap_or(u32::MAX));
        let line = self
            .starts
            .partition_point(|start| *start <= at)
            .saturating_sub(1);
        let start = self.starts.get(line).copied().unwrap_or(0) as usize;
        let prefix = self.text.get(start..at as usize).unwrap_or_default();
        let character: usize = prefix.chars().map(char::len_utf16).sum();
        (line as u32, character as u32)
    }

    /// The range a diagnostic squiggles (§4.3): the maximal run of
    /// `[A-Za-z0-9_-]` at the position, one unit for punctuation, and a
    /// zero-width range for [`Position::NONE`].
    fn range(&self, at: Position) -> Value {
        let (line, character) = self.point(at);
        let width = self.width(at);
        json!({
            "start": {"line": line, "character": character},
            "end": {"line": line, "character": character.saturating_add(width)},
        })
    }

    /// The squiggle's width in UTF-16 units on the same line (§4.3).
    fn width(&self, at: Position) -> u32 {
        if at.is_none() {
            return 0;
        }
        let Some(line) = self.line(at.line().saturating_sub(1)) else {
            return 0;
        };
        let mut run = 0_u32;
        let mut started = false;
        for character in line.chars().skip(at.col().saturating_sub(1) as usize) {
            if !is_word_char(character) {
                break;
            }
            run = run.saturating_add(character.len_utf16() as u32);
            started = true;
        }
        if started { run } else { 1 }
    }
}

/// `[A-Za-z0-9_-]`: the characters a word, a keyword, a method and an
/// environment-variable name are made of (§4.3, §7.1).
fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

/// A UTF-16 `character` as a `char` index into `line` (§4.4), so a hover past
/// the middle of a surrogate pair resolves to the code point that starts at it.
fn char_index(line: &str, character: u32) -> usize {
    let mut units = 0_u32;
    let mut index = 0_usize;
    for symbol in line.chars() {
        if units >= character {
            break;
        }
        units = units.saturating_add(symbol.len_utf16() as u32);
        index = index.saturating_add(1);
    }
    index
}

/// The word under one `char` index: its text, its start and its end, with one
/// leading `$` included when the run is preceded by one (§7.1).
fn word_at(line: &str, index: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let at = index.min(chars.len());
    let mut start = at;
    while start > 0 {
        match chars.get(start.saturating_sub(1)) {
            Some(character) if is_word_char(*character) => start = start.saturating_sub(1),
            _ => break,
        }
    }
    let mut end = at;
    while let Some(character) = chars.get(end) {
        if !is_word_char(*character) {
            break;
        }
        end = end.saturating_add(1);
    }
    if start == end {
        return None;
    }
    if start > 0 && matches!(chars.get(start.saturating_sub(1)), Some('$')) {
        start = start.saturating_sub(1);
    }
    let word: String = chars.get(start..end)?.iter().collect();
    Some((word, start, end))
}

/// The `buffer.max_point_utf16()` an edit's `end` is (§6.2): the number of
/// `'\n'`, and the UTF-16 length of the text after the last one.
fn max_point_utf16(text: &str) -> (u32, u32) {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for symbol in text.chars() {
        if symbol == '\n' {
            line = line.saturating_add(1);
            character = 0;
        } else {
            character = character.saturating_add(symbol.len_utf16() as u32);
        }
    }
    (line, character)
}
