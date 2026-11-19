//! Protected contract: `blastit --internal-json`, design §3's twelve rows.
//!
//! The pure-Python bridge (`python/blastit/_blastit.py`) is the only client of
//! this dispatcher, and the two are what makes one `py3-none-*` wheel serve every
//! CPython `>=3.12`. A row that changes shape here breaks `fold`, the pytest
//! plugin and `blastit check --learn` in a way no Rust caller can notice, so each
//! row is driven over a real pipe: a frame in, one line out.
//!
//! The adversarial half is the reason the loop exists at all: an unknown op, a
//! frame that is not JSON, a frame past the bound, and a request whose failure
//! text names a file and a line but must never repeat a credential.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The secret planted in every refused document. A refusal names the place it
/// failed, never a value the application supplied.
const SECRET: &str = "hunter2-contract";

/// The program under test.
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blastit"))
}

/// One `blastit --internal-json` session over real pipes.
struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Bridge {
    /// Starts the dispatcher with nothing but its own flag on the command line.
    fn start() -> Self {
        let mut child = binary()
            .arg("--internal-json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the dispatcher starts");
        let stdin = child.stdin.take().expect("a piped stdin");
        let stdout = child.stdout.take().expect("a piped stdout");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    /// Sends one raw frame (no newline added) and reads the one-line answer.
    fn frame(&mut self, frame: &[u8]) -> Value {
        self.stdin.write_all(frame).expect("the frame is written");
        self.stdin.write_all(b"\n").expect("the frame ends");
        self.stdin.flush().expect("the frame is flushed");
        self.answer()
    }

    /// One request, as the Python client spells it.
    fn ask(&mut self, op: &str, args: Value) -> Value {
        let request = json!({"op": op, "args": args}).to_string();
        self.frame(request.as_bytes())
    }

    /// The next answer line, bounded so a stalled dispatcher fails the suite.
    fn answer(&mut self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut line = String::new();
        while line.is_empty() {
            assert!(
                Instant::now() < deadline,
                "the dispatcher sent no line within 30 s"
            );
            let read = self.stdout.read_line(&mut line).expect("a readable line");
            assert_ne!(read, 0, "the dispatcher closed stdout");
        }
        serde_json::from_str(&line).expect("an answer is one JSON object")
    }

    /// Ends the session and requires a clean exit.
    fn finish(mut self) {
        drop(self.stdin);
        let status = self.child.wait().expect("the dispatcher exits");
        assert!(status.success(), "the dispatcher exited {status}");
    }
}

/// The `result` of an `{"ok":true}` answer.
fn ok(answer: &Value) -> &Value {
    assert_eq!(answer["ok"], Value::Bool(true), "{answer}");
    answer
        .get("result")
        .unwrap_or_else(|| panic!("an `ok` answer carries a result: {answer}"))
}

/// The `error` of an `{"ok":false}` answer.
fn error(answer: &Value) -> &str {
    assert_eq!(answer["ok"], Value::Bool(false), "{answer}");
    answer
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("a refusal carries an error: {answer}"))
}

/// A store-less app: no proxy, no statement, and the shape every row accepts.
const MANIFEST: &str = "view home \"/\" {}\n";

/// The same view beside a declared store: an edit may grant `db` here.
const PROXIED: &str = "postgres $DATABASE_URL\n\nview home \"/\" {}\n";

#[test]
fn every_row_of_the_contract_answers_its_own_type() {
    let mut bridge = Bridge::start();

    for (op, sql) in [
        ("normalize", "SELECT  1 ;"),
        ("redact_literals", "SELECT * FROM t WHERE name = 'x'"),
    ] {
        let answer = bridge.ask(op, json!({"sql": sql}));
        let result = ok(&answer).as_str().expect("a string result");
        assert!(!result.is_empty(), "{op} answered an empty string");
    }
    assert_eq!(
        ok(&bridge.ask("has_quoted_literal", json!({"sql": "SELECT 'x'"}))),
        &Value::Bool(true),
        "a quoted literal is a `bool` result"
    );
    assert_eq!(
        ok(&bridge.ask("has_quoted_literal", json!({"sql": "SELECT 1"}))),
        &Value::Bool(false)
    );

    // `audit` takes the observations *document* the pytest plugin writes.
    let observations = json!({"version": 1, "requests": []}).to_string();
    let audit = ok(&bridge.ask(
        "audit",
        json!({"manifest_text": MANIFEST, "observations_json": observations}),
    ))
    .as_str()
    .expect("JSON text")
    .to_string();
    let report: Value = serde_json::from_str(&audit).expect("`audit` answers JSON text");
    assert_eq!(report["version"], 1, "{report}");
    assert_eq!(report["manifest"]["routes"], 1, "{report}");
    assert_eq!(report["totals"]["clean"], Value::Bool(true), "{report}");

    let analyzed: Value = serde_json::from_str(
        ok(&bridge.ask("analyze", json!({"sql": "SELECT 1"})))
            .as_str()
            .unwrap(),
    )
    .expect("`analyze` answers JSON text");
    assert_eq!(analyzed["kind"], "select", "{analyzed}");

    let covering: Value = serde_json::from_str(
        ok(&bridge.ask(
            "covering",
            json!({"statements": ["SELECT 1"], "locator": "view home postgres"}),
        ))
        .as_str()
        .unwrap(),
    )
    .expect("`covering` answers JSON text");
    assert!(covering["tables"].is_object(), "{covering}");
    assert_eq!(covering["blastfile"], "", "{covering}");

    let class: Value = serde_json::from_str(
        ok(&bridge.ask("classify", json!({"sql": "SELECT 1"})))
            .as_str()
            .unwrap(),
    )
    .expect("`classify` answers JSON text");
    assert_eq!(class["class"], "select", "{class}");

    // A store-less app needs no proxy control frames: empty streams are a run.
    let check: Value = serde_json::from_str(
        ok(&bridge.ask(
            "check_audit",
            json!({
                "manifest_text": MANIFEST,
                "http_obs_jsonl": "",
                "sql_obs_jsonl": "",
            }),
        ))
        .as_str()
        .unwrap(),
    )
    .expect("`check_audit` answers JSON text");
    assert_eq!(check["schema_version"], 1, "{check}");
    assert_eq!(check["routes"]["total"], 1, "{check}");

    let learn: Value = serde_json::from_str(
        ok(&bridge.ask(
            "plan_learn",
            json!({
                "manifest_text": MANIFEST,
                "http_obs_jsonl": "",
                "sql_obs_jsonl": "",
                "dry_run": true,
            }),
        ))
        .as_str()
        .unwrap(),
    )
    .expect("`plan_learn` answers JSON text");
    assert_eq!(learn["wrote"], Value::Bool(false), "{learn}");

    // `prefix_b64` is the only bytes-on-the-wire field, and the sample never
    // carries a body byte: a MIME essence and three verdicts.
    let sample: Value = serde_json::from_str(
        ok(&bridge.ask(
            "probe_ingress",
            json!({"content_type": "application/json", "prefix_b64": "e30=", "total_len": 2}),
        ))
        .as_str()
        .unwrap(),
    )
    .expect("`probe_ingress` answers JSON text");
    assert_eq!(sample["mime"], "application/json", "{sample}");
    assert_eq!(sample["valid_json"], Value::Bool(true), "{sample}");

    let edits = json!({"views": [], "grants": []}).to_string();
    assert_eq!(
        ok(&bridge.ask(
            "blastfile_apply_edits",
            json!({"source": MANIFEST, "edits_json": edits}),
        )),
        &Value::String(MANIFEST.to_string()),
        "an empty edits document is the source, byte for byte"
    );

    let model: Value = serde_json::from_str(
        ok(&bridge.ask("blastfile_model", json!({"source": MANIFEST})))
            .as_str()
            .unwrap(),
    )
    .expect("`blastfile_model` answers JSON text");
    assert_eq!(model["views"][0]["name"], "home", "{model}");

    bridge.finish();
}

#[test]
fn an_adversarial_or_oversized_frame_is_refused_and_the_session_survives() {
    let mut bridge = Bridge::start();

    for (name, frame) in [
        (
            "an unknown op",
            json!({"op": "drop_tables", "args": {}}).to_string(),
        ),
        (
            "a wrong argument type",
            json!({"op": "normalize", "args": {"sql": 1}}).to_string(),
        ),
        (
            "a missing argument",
            json!({"op": "analyze", "args": {}}).to_string(),
        ),
        (
            "a URL-safe base64 alphabet",
            json!({"op": "probe_ingress", "args": {"prefix_b64": "-_==", "total_len": 0}})
                .to_string(),
        ),
        ("no op at all", json!({"args": {}}).to_string()),
    ] {
        let answer = bridge.frame(frame.as_bytes());
        assert!(
            !error(&answer).is_empty(),
            "{name} is refused with a sentence"
        );
    }
    // A frame that is not JSON at all.
    error(&bridge.frame(b"not json"));

    // Past the bound: 9 MiB with no newline in it. The write itself is the
    // adversarial part — a loop that did not drain would deadlock here.
    let oversized = vec![b'x'; 9 * 1024 * 1024];
    let answer = bridge.frame(&oversized);
    assert!(error(&answer).contains("8388608"), "{answer}");

    // The session is still usable: the next frame is answered normally.
    assert_eq!(
        ok(&bridge.ask("normalize", json!({"sql": "SELECT 1"}))),
        &Value::String("SELECT 1".to_string())
    );
    bridge.finish();
}

#[test]
fn no_refusal_repeats_a_value_from_the_document() {
    let mut bridge = Bridge::start();
    // A corrupt observation line that also carries a credential: the refusal
    // names the file and the line, never the value.
    let corrupt = format!("not json {SECRET}");
    let answer = bridge.ask(
        "check_audit",
        json!({
            "manifest_text": MANIFEST,
            "http_obs_jsonl": "",
            "sql_obs_jsonl": corrupt,
        }),
    );
    let message = error(&answer);
    assert!(message.contains("sql_obs.jsonl"), "{message}");
    assert!(message.contains("line 1"), "{message}");
    assert!(
        !message.contains(SECRET),
        "a value reached the refusal: {message}"
    );

    // A refused edit names the view and the store, never the statement text.
    let statement = format!("SELECT 1 WHERE 'x' = '{SECRET}'");
    let edits = json!({
        "views": [],
        "grants": [{
            "view": "home", "store": null, "queries": [statement],
            "tables": {}, "functions": [],
        }],
    })
    .to_string();
    let answer = bridge.ask(
        "blastfile_apply_edits",
        json!({"source": PROXIED, "edits_json": edits}),
    );
    let message = error(&answer);
    assert!(message.contains("inline literal"), "{message}");
    assert!(
        !message.contains(SECRET),
        "a value reached the refusal: {message}"
    );
    bridge.finish();
}

#[test]
fn the_dispatcher_is_hidden_and_never_pairs_with_a_reexec_line() {
    // Not a command a person is taught: `--help` does not name it.
    let help = binary().arg("--help").output().expect("`--help` runs");
    assert!(help.status.success(), "--help exits 0");
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(!text.contains("--internal-json"), "{text}");

    // The two graces it may never share a command line with, either way round.
    for argv in [
        ["--internal-json", "--stage=1"],
        ["--stage=1", "--internal-json"],
        ["--internal-json", "--role=dbproxy"],
    ] {
        let refused = binary().args(argv).output().expect("the command runs");
        assert!(!refused.status.success(), "{argv:?} is refused");
        assert!(refused.stdout.is_empty(), "{argv:?} answers no frame");
    }
}
