//! `blastit --internal-json`: the NDJSON dispatcher a Python client drives.
//!
//! The published wheel is `py3-none-*` — one artifact for every CPython
//! `>=3.12` — so the audit, the `Blastfile` reader/editor and the grant analysis
//! can no longer be a per-minor extension. `python/blastit/_blastit.py` starts
//! this program once and writes one request per line:
//!
//! ```text
//! {"op":"normalize","args":{"sql":"SELECT 1"}}
//! -> {"ok":true,"result":"SELECT 1"}
//! {"op":"nope","args":{}}
//! -> {"ok":false,"error":"unknown op `nope`"}
//! ```
//!
//! Every `op` is one function of `blastit-audit`, the single owner of the audit,
//! the SQL normaliser, the `Blastfile` reader and the editor (`MB.42.2.1`). This
//! module adds no analysis at all: it decodes a frame, calls the owner and
//! renders the answer, so `fold`, the pytest plugin and `blastit check` can never
//! disagree. Design §3's twelve rows are the whole contract; [`args`] holds the
//! decoders.
//!
//! It is hidden from `--help`, refused together with `--stage=1`/`--role=*`, and
//! refuses a terminal: it is plumbing between this binary and the package that
//! ships it, not a command anyone types.

use std::io::{BufRead, IsTerminal, Write};

use serde_json::{Value, json};

use args::{boolean, decode_base64, integer, string, strings};

mod args;

/// The largest request line this loop buffers. A manifest is the biggest
/// document a caller sends and it is far below this; anything longer is a client
/// bug, refused instead of allocated.
const MAX_FRAME: usize = 8 * 1024 * 1024;

/// The one sentence an over-long or unterminated frame is answered with.
const BAD_FRAME: &str = "the request is not one newline-terminated frame of at most 8388608 bytes";

/// One request frame.
#[derive(serde::Deserialize)]
struct Request {
    op: String,
    #[serde(default)]
    args: Value,
}

/// What one bounded read of the client produced.
enum Frame {
    /// A complete, newline-terminated line.
    Line,
    /// A line past [`MAX_FRAME`] (or an unterminated last one): drained, and
    /// refused once.
    TooLong,
    /// The client closed its end.
    End,
}

/// Runs the dispatcher on stdin/stdout until the client closes its end.
///
/// A malformed frame is one `{"ok":false}` line: the loop keeps answering,
/// because a client that sent one bad frame still has a session.
pub(super) fn run() -> u8 {
    // A person who runs the hidden flag gets a sentence, not a hang: this reads
    // NDJSON on stdin, and a terminal never writes any.
    if std::io::stdin().is_terminal() {
        eprintln!(
            "blastit --internal-json: this is the NDJSON channel the Python package \
             drives, not a command to run (see `blastit --help`)"
        );
        return 2;
    }
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let answer = match read_frame(&mut reader, &mut line) {
            Ok(Frame::End) => return 0,
            Ok(Frame::Line) => match std::str::from_utf8(&line) {
                Ok(text) => respond(text),
                Err(_) => failure("the request is not UTF-8"),
            },
            Ok(Frame::TooLong) => failure(BAD_FRAME),
            Err(error) => {
                let message = format!("the request could not be read: {error}");
                let _ = write_line(&mut stdout, &failure(&message));
                return 0;
            }
        };
        if write_line(&mut stdout, &answer).is_err() {
            return 0;
        }
    }
}

/// Reads one line of at most [`MAX_FRAME`] bytes, draining an over-long one so
/// the frames that follow are read from a boundary.
///
/// Hand-rolled rather than `Read::take`: a `Take` bounds one read, and a frame
/// past the bound has to be *drained* to the next newline instead of split into
/// two frames that were never sent.
fn read_frame<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> std::io::Result<Frame> {
    let mut overlong = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            let end = line.is_empty() && !overlong;
            return Ok(if end { Frame::End } else { Frame::TooLong });
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                if !overlong && line.len() + index > MAX_FRAME {
                    overlong = true;
                }
                if !overlong {
                    line.extend_from_slice(&available[..index]);
                }
                reader.consume(index + 1);
                return Ok(if overlong {
                    Frame::TooLong
                } else {
                    Frame::Line
                });
            }
            None => {
                if line.len() + available.len() > MAX_FRAME {
                    overlong = true;
                }
                if !overlong {
                    line.extend_from_slice(available);
                }
                let read = available.len();
                reader.consume(read);
            }
        }
    }
}

/// One response line for one request frame.
fn respond(text: &str) -> String {
    match serde_json::from_str::<Request>(text) {
        Ok(request) => match call(&request.op, &request.args) {
            Ok(result) => render(&json!({"ok": true, "result": result})),
            Err(error) => failure(&error),
        },
        Err(error) => failure(&format!("the request is not JSON: {error}")),
    }
}

/// `{"ok":false,"error":…}`, one line.
fn failure(message: &str) -> String {
    render(&json!({"ok": false, "error": message}))
}

/// One rendered line, newline included.
fn render(value: &Value) -> String {
    let text = serde_json::to_string(value)
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"unrenderable\"}".to_string());
    format!("{text}\n")
}

/// Writes one answer and flushes it: the client is blocked on the line.
fn write_line(out: &mut impl Write, line: &str) -> std::io::Result<()> {
    out.write_all(line.as_bytes())?;
    out.flush()
}

/// A JSON-text operation's answer, its own error rendered to one line.
fn rendered<E: ToString>(result: Result<String, E>) -> Result<Value, String> {
    result.map(Value::from).map_err(|error| error.to_string())
}

/// Dispatches one `op` to the one owner that implements it.
///
/// The result is exactly what design §3's row says: a rendered document or a
/// `str` for the JSON-text ops, `str` for the two text ops and the edit, a
/// `bool` for `has_quoted_literal`.
fn call(op: &str, args: &Value) -> Result<Value, String> {
    let sql = || string(args, "sql");
    Ok(match op {
        "normalize" => Value::from(blastit_audit::normalize(&sql()?)),
        "has_quoted_literal" => Value::from(blastit_audit::has_quoted_literal(&sql()?)),
        "redact_literals" => Value::from(blastit_audit::redact_literals(&sql()?)),
        "audit" => rendered(blastit_audit::audit(
            &string(args, "manifest_text")?,
            &string(args, "observations_json")?,
        ))?,
        "analyze" => rendered(blastit_audit::analyze(&sql()?))?,
        "covering" => rendered(blastit_audit::covering(
            &strings(args, "statements")?,
            &string(args, "locator")?,
        ))?,
        "classify" => rendered(blastit_audit::classify(&sql()?))?,
        "check_audit" => rendered(blastit_audit::check_audit(
            &string(args, "manifest_text")?,
            &string(args, "http_obs_jsonl")?,
            &string(args, "sql_obs_jsonl")?,
        ))?,
        "plan_learn" => rendered(blastit_audit::plan_learn_json(
            &string(args, "manifest_text")?,
            &string(args, "http_obs_jsonl")?,
            &string(args, "sql_obs_jsonl")?,
            boolean(args, "dry_run")?,
        ))?,
        "probe_ingress" => rendered(blastit_audit::probe_ingress_json(
            match args.get("content_type") {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => Some(text.as_str()),
                Some(_) => return Err("`content_type` must be a string or null".to_string()),
            },
            &decode_base64(&string(args, "prefix_b64")?)?,
            integer(args, "total_len")?,
        ))?,
        "blastfile_apply_edits" => Value::from(blastit_audit::blastfile_apply_edits(
            &string(args, "source")?,
            &string(args, "edits_json")?,
        )?),
        "blastfile_model" => rendered(blastit_audit::blastfile_model(&string(args, "source")?))?,
        other => return Err(format!("unknown op `{other}`")),
    })
}
