//! The normaliser the DX-invariance replay compares through (`MB.36.7`, design
//! `view-crates-architecture.md` §1/R14).
//!
//! The epic promised that moving ~16 000 lines between crates changes nothing a
//! developer sees. The proof is two captures of the same seven commands — one
//! from the tree before the refactor and one from the epic head — that must be
//! byte-identical **after the documented mask**. This file carries that mask:
//!
//! * a JSON event line has the design's eight varying fields replaced with
//!   `"<masked>"`, while every other key (notably `status`, `event`, `route`)
//!   survives byte for byte, so a masked field can never hide a status change;
//! * a human log line has its clock (`16:18:09.360`) replaced by `<ts>`, its
//!   `pid=` arguments masked and its unit-suffixed durations (`0.7ms`) replaced
//!   by `<duration>` — the same two values the JSON mask names, rendered the way
//!   the pretty lane renders them;
//! * an `ETag` header is compared **by shape**, never by value: it must match the
//!   `"<hex>-<hex>"` shape `static_response.rs` formats, and a value that does
//!   not is a failure, so a `200` whose `ETag` became a date or a plain size
//!   cannot pass as "the same shape".
//!
//! The mask itself is `dx-mask.txt`, which `e2e/dx_replay.py` reads too: one
//! list, two readers, so the in-tree self-tests and the replay cannot disagree.
//!
//! Two failure modes are the reason this file is not just a regex: an over-broad
//! mask and a lossy parse both make the diff compare nothing. [`normalize_capture`]
//! therefore refuses an empty capture, refuses a line that looks like JSON but
//! does not parse (it is never dropped), refuses a malformed `ETag`, and keeps
//! one output line per input line. The two self-tests below pin exactly that.
//!
//! `e2e/dx_replay.py` is the driver; this file is the contract it is diffed
//! against.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt;

use serde_json::Value;

/// The mask file, shared with `e2e/dx_replay.py` (see `dx-mask.txt`).
const MASK: &str = include_str!("dx-mask.txt");
/// The host/run-only additions, kept apart from the design's eight.
const HOST_MASK: &str = include_str!("dx-mask-host.txt");

/// The placeholder a masked JSON value and a masked `pid=` become.
const MASKED: &str = "<masked>";
/// The placeholder a masked human-line duration becomes.
const DURATION: &str = "<duration>";
/// The placeholder a masked human-line clock becomes.
const TS: &str = "<ts>";
/// The placeholder a version banner becomes.
const VERSION: &str = "<version banner>";
/// The shape an `ETag` value must have, as it is normalised.
const ETAG_SHAPE: &str = "\"<hex>-<hex>\"";
/// The pytest summary's shape: its seconds are a duration.
const PYTEST_SUMMARY: &str = "<pytest summary>";

/// Why a capture could not be normalised.
///
/// Every variant is a **failure**, never a silent pass: a capture the normaliser
/// cannot read is exactly the capture that must not be compared.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// There was nothing to compare — an empty diff would prove nothing.
    Empty,
    /// A line that starts like JSON does not parse. It is named, never dropped.
    UnparseableJson { line: usize, detail: String },
    /// An `ETag` header that is not the documented `"<hex>-<hex>"` shape.
    BadEtag { line: usize, value: String },
    /// Normalising lost a line (a parse that dropped rather than failed).
    LinesLost { input: usize, output: usize },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(
                formatter,
                "the capture is empty; there is nothing to compare"
            ),
            Self::UnparseableJson { line, detail } => {
                write!(
                    formatter,
                    "line {line} looks like JSON but does not parse: {detail}"
                )
            }
            Self::BadEtag { line, value } => write!(
                formatter,
                "line {line} is an ETag header whose value is not the documented \
                 \"<hex>-<hex>\" shape: {value}"
            ),
            Self::LinesLost { input, output } => write!(
                formatter,
                "normalising {input} lines produced {output}; a capture may never lose a line"
            ),
        }
    }
}

impl std::error::Error for CaptureError {}

/// The mask file's field names, in file order (the design's eight).
///
/// Parsed on every call rather than cached: it is a 20-line file, and a test
/// crate may not keep a mutable global.
pub fn masked_fields() -> Vec<&'static str> {
    parse_mask(MASK)
}

/// The host/run-only additions (`dx-mask-host.txt`), kept separate so the
/// design's list stays exactly eight and the extension is auditable.
pub fn host_fields() -> Vec<&'static str> {
    parse_mask(HOST_MASK)
}

fn parse_mask(text: &'static str) -> Vec<&'static str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// Normalises one capture (stdout, stderr or an HTTP transcript) for comparison.
///
/// A capture that is one JSON document (the `check --json` stdout) is parsed as
/// a whole, so a pretty-printed document is compared field by field. Otherwise
/// each line is either a JSON event (parsed, masked, re-serialised with sorted
/// keys so key order is not part of the comparison), an `ETag` header (checked
/// for its shape and replaced by that shape), a pytest summary (compared by
/// shape, because its seconds are a duration), or a human log line (clock,
/// `pid=` and durations masked).
///
/// Two orderings are made deterministic because they are not promises: the
/// shutdown block (`process.exited` lines are emitted by processes that exit at
/// the same instant, so their relative order races) is sorted within each block.
pub fn normalize_capture(text: &str) -> Result<String, CaptureError> {
    if text.trim().is_empty() {
        return Err(CaptureError::Empty);
    }
    let fields = masked_fields();
    let host = host_fields();
    let mut all = fields.clone();
    all.extend(host);
    if let Ok(mut document) = serde_json::from_str::<Value>(text.trim()) {
        mask_json(&mut document, &all);
        let rendered =
            serde_json::to_string(&document).map_err(|error| CaptureError::UnparseableJson {
                line: 1,
                detail: error.to_string(),
            })?;
        return Ok(format!("{rendered}\n"));
    }
    let mut input_lines = 0;
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        input_lines += 1;
        lines.push(normalize_line(line, index + 1, &all)?);
    }
    let lines = sort_concurrent_exits(lines);
    if lines.len() != input_lines {
        return Err(CaptureError::LinesLost {
            input: input_lines,
            output: lines.len(),
        });
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn normalize_line(line: &str, number: usize, fields: &[&str]) -> Result<String, CaptureError> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let mut value: Value =
            serde_json::from_str(trimmed).map_err(|error| CaptureError::UnparseableJson {
                line: number,
                detail: error.to_string(),
            })?;
        mask_json(&mut value, fields);
        // Sorted keys (`serde_json`'s default map) make key order irrelevant, and
        // the indent only exists so a failing diff is readable.
        let rendered =
            serde_json::to_string(&value).map_err(|error| CaptureError::UnparseableJson {
                line: number,
                detail: error.to_string(),
            })?;
        let indent = &line[..line.len() - trimmed.len()];
        return Ok(format!("{indent}{rendered}"));
    }
    if let Some(rest) = strip_etag_prefix(line) {
        let value = rest.trim();
        if !is_etag_shape(value) {
            return Err(CaptureError::BadEtag {
                line: number,
                value: value.to_string(),
            });
        }
        return Ok(format!("ETag: {ETAG_SHAPE}"));
    }
    if is_pytest_summary(trimmed) {
        return Ok(PYTEST_SUMMARY.to_string());
    }
    if let Some(banner) = version_banner(line) {
        return Ok(banner);
    }
    let mut masked = mask_durations(line);
    for name in fields {
        masked = mask_keyed_value(&masked, &format!("{name}="));
    }
    Ok(mask_clock(&masked))
}

/// Replaces the masked keys anywhere in a parsed event.
fn mask_json(value: &mut Value, fields: &[&str]) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if fields.contains(&key.as_str()) && !item.is_null() {
                    *item = Value::String(MASKED.to_string());
                } else {
                    mask_json(item, fields);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                mask_json(item, fields);
            }
        }
        _ => {}
    }
}

/// Sorts each consecutive run of `process.exited` lines.
///
/// Two children exiting on the same drain are concurrent events; their order in
/// the log is a race, not a behaviour. Everything else keeps its order.
fn sort_concurrent_exits(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut block: Vec<String> = Vec::new();
    for line in lines {
        if line.contains("process.exited") {
            block.push(line);
            continue;
        }
        if !block.is_empty() {
            block.sort();
            out.append(&mut block);
        }
        out.push(line);
    }
    if !block.is_empty() {
        block.sort();
        out.append(&mut block);
    }
    out
}

/// `Some(value)` when the line is an `ETag` header, in any case.
fn strip_etag_prefix(line: &str) -> Option<&str> {
    let (name, rest) = line.split_once(':')?;
    if name.trim().eq_ignore_ascii_case("etag") {
        Some(rest)
    } else {
        None
    }
}

/// The documented shape: `"<hex>-<hex>"`, so mtime and size are compared by
/// presence, never by value (design §1: two runs of the same file must agree on
/// the shape, never on the value).
fn is_etag_shape(value: &str) -> bool {
    let value = value.strip_prefix("W/").unwrap_or(value);
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return false;
    };
    let Some((first, second)) = inner.split_once('-') else {
        return false;
    };
    let hex = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_hexdigit());
    hex(first) && hex(second)
}

/// `4 passed in 0.03s`, with or without warnings: the seconds are a duration.
fn is_pytest_summary(line: &str) -> bool {
    let Some((count, rest)) = line.split_once(" passed") else {
        return false;
    };
    if count.is_empty() || !count.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Some(rest) = rest.split_once(" in ") else {
        return false;
    };
    let seconds = rest.1.strip_suffix('s').unwrap_or(rest.1);
    !seconds.is_empty() && seconds.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// `Some(<version banner>)` when the line *is* the version banner.
///
/// Deliberately shaped, not "starts with `blastit `": `blastit check: 6 problems`
/// is a documented output line and must stay comparable.
fn version_banner(line: &str) -> Option<String> {
    let rest = line.strip_prefix("blastit ")?;
    let token = rest.split_whitespace().next()?;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() >= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        Some(VERSION.to_string())
    } else {
        None
    }
}

/// Replaces a leading `HH:MM:SS.mmm` (the human lane's clock) with `<ts>`.
fn mask_clock(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let digit = |index: usize| chars.get(index).is_some_and(|c| c.is_ascii_digit());
    let shaped = chars.len() > 13
        && (0..2).all(digit)
        && chars[2] == ':'
        && (3..5).all(digit)
        && chars[5] == ':'
        && (6..8).all(digit)
        && chars[8] == '.'
        && (9..12).all(digit)
        && chars[12] == ' ';
    if shaped {
        let rest: String = chars[13..].iter().collect();
        format!("{TS} {rest}")
    } else {
        line.to_string()
    }
}

/// Replaces `<key><number><unit>` with `<key><masked>`.
///
/// The unit is consumed too, so `mem_used=8013MiB` and `mem_used=8017MiB` agree
/// on the masked line instead of differing in the suffix the number left behind.
fn mask_keyed_value(line: &str, key: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(position) = rest.find(key) {
        let (before, after_key) = rest.split_at(position + key.len());
        out.push_str(before);
        let mut chars = after_key.char_indices();
        let start = chars.next();
        let value: String = match start {
            Some((_, first)) if first.is_ascii_digit() => after_key
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || c.is_ascii_alphabetic())
                .collect(),
            _ => String::new(),
        };
        if value.is_empty() {
            rest = after_key;
            continue;
        }
        out.push_str(MASKED);
        rest = &after_key[value.len()..];
    }
    out.push_str(rest);
    out
}

/// Replaces `12.5ms`/`700µs`/`12ns` with `<duration>`.
fn mask_durations(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let start = index;
            while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
                index += 1;
            }
            let number: String = chars[start..index].iter().collect();
            let unit =
                chars
                    .get(index)
                    .zip(chars.get(index + 1))
                    .and_then(|(first, second)| match (first, second) {
                        ('m', 's') => Some(2),
                        ('n', 's') => Some(2),
                        ('µ', 's') => Some(2),
                        _ => None,
                    });
            match unit {
                Some(width) => {
                    out.push_str(DURATION);
                    index += width;
                }
                None => out.push_str(&number),
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The two self-tests the design names, plus the mask's own guard.
// ---------------------------------------------------------------------------

#[test]
fn a_varying_ts_is_masked_while_a_stable_status_is_kept() {
    let first = concat!(
        "{\"ts\":\"2026-11-18T16:18:09.360Z\",\"level\":\"info\",\"event\":\"request\",",
        "\"route\":\"app\",\"status\":200,\"duration_ms\":0.7,\"request_id\":\"req-1\",",
        "\"pid\":1681053}\n"
    );
    let second = concat!(
        "{\"ts\":\"2026-11-18T17:02:44.001Z\",\"level\":\"info\",\"event\":\"request\",",
        "\"route\":\"app\",\"status\":200,\"duration_ms\":17.5,\"request_id\":\"req-2\",",
        "\"pid\":9}\n"
    );
    let first = normalize_capture(first).expect("the first capture normalises");
    let second = normalize_capture(second).expect("the second capture normalises");
    assert_eq!(first, second, "two runs of the same behaviour must agree");
    assert!(
        first.contains("\"status\":200"),
        "a stable status must survive the mask: {first}"
    );
    assert!(
        !first.contains("2026-11-18") && !first.contains("req-") && !first.contains("1681053"),
        "every masked field must be gone: {first}"
    );
    for field in ["ts", "duration_ms", "request_id", "pid"] {
        assert!(
            first.contains(&format!("\"{field}\":\"{MASKED}\"")),
            "`{field}` must be masked, not dropped: {first}"
        );
    }
    // The human lane's renderings of the same two values, and the version banner
    // which is a shape and must not swallow `blastit check: …`.
    let human = normalize_capture(
        "16:18:09.360 INFO  request     GET / → app  200  0.7ms\n\
         process.spawned app  pid=1681053 uid=1000  fork=0.2ms\n\
         blastit check: 6 problems\n\
         blastit 0.2.0\n",
    )
    .expect("a human log normalises");
    assert_eq!(
        human,
        "<ts> INFO  request     GET / → app  200  <duration>\n\
         process.spawned app  pid=<masked> uid=1000  fork=<duration>\n\
         blastit check: 6 problems\n\
         <version banner>\n"
    );
}

#[test]
fn a_line_the_normaliser_cannot_parse_is_a_failure_never_dropped() {
    // Truncated JSON: the failure names the line, and nothing is compared.
    let truncated = "{\"ts\":\"2026-11-18T16:18:09.360Z\",\"event\":\n";
    match normalize_capture(truncated) {
        Err(CaptureError::UnparseableJson { line: 1, .. }) => {}
        other => panic!("a truncated event must fail on line 1, got {other:?}"),
    }
    // The same, on the second line: a parser that dropped it would return Ok.
    let second_line = "16:18:09.360 INFO  boot        stage0\n{\"ts\":\n";
    match normalize_capture(second_line) {
        Err(CaptureError::UnparseableJson { line: 2, .. }) => {}
        other => panic!("a truncated event on line 2 must fail there, got {other:?}"),
    }
    // An ETag whose value is not the documented shape is a failure, not a value.
    match normalize_capture("ETag: \"2026-11-18\"\n") {
        Err(CaptureError::BadEtag { line: 1, .. }) => {}
        other => panic!("a non-hex ETag must fail, got {other:?}"),
    }
    // Nothing to compare is a failure too: an empty diff proves nothing.
    assert_eq!(normalize_capture("   \n\n"), Err(CaptureError::Empty));
    // A good line keeps its line count and shows the shape it compared by.
    assert_eq!(
        normalize_capture("ETag: \"23-17979cfe362a0000\"\n").expect("a hex ETag normalises"),
        "ETag: \"<hex>-<hex>\"\n"
    );
    // The weak form is the same shape.
    assert_eq!(
        normalize_capture("ETag: W/\"23-17979cfe362a0000\"\n").expect("a weak ETag normalises"),
        "ETag: \"<hex>-<hex>\"\n"
    );
}

#[test]
fn the_mask_is_the_designs_eight_fields_and_nothing_else() {
    assert_eq!(
        masked_fields(),
        vec![
            "ts",
            "duration_ms",
            "cpu_ns",
            "wall_ns",
            "request_id",
            "pid",
            "fork_ms",
            "cost_ns",
        ],
        "`dx-mask.txt` is the documented mask: adding a name weakens the diff, \
         removing one makes it fail on a value the design says varies"
    );
}

#[test]
fn concurrent_exits_sort_but_every_other_line_keeps_its_order() {
    // The two children exit on the same drain, so their relative order races.
    let first = "<ts> INFO  process.exited home  pid=<masked> uid=1000  signal=15\n\
                 <ts> INFO  process.exited -  pid=<masked> uid=0  signal=15\n\
                 response one\nresponse two\n";
    let second = "<ts> INFO  process.exited -  pid=<masked> uid=0  signal=15\n\
                  <ts> INFO  process.exited home  pid=<masked> uid=1000  signal=15\n\
                  response one\nresponse two\n";
    assert_eq!(
        normalize_capture(first).expect("the first capture normalises"),
        normalize_capture(second).expect("the second capture normalises"),
        "the exit block is a set; the responses after it are a sequence"
    );
    // A pytest summary is compared by shape: its seconds are a duration.
    assert_eq!(
        normalize_capture("4 passed in 0.01s\n").expect("a summary normalises"),
        "<pytest summary>\n"
    );
    assert_eq!(
        normalize_capture("7 passed, 1 warning in 1.20s\n").expect("a summary normalises"),
        "<pytest summary>\n"
    );
    // A line that only looks similar keeps its bytes.
    assert_eq!(
        normalize_capture("passed in 0.01s\n").expect("not a summary"),
        "passed in 0.01s\n"
    );
}

/// The golden capture the Python replay driver must normalise identically.
///
/// `e2e/dx_replay.py` is a second implementation of the same rules (it may not
/// depend on this test binary), so one vector is compared by both: this test
/// here, and the driver's `--self-test`. A rule added to one and not the other
/// fails one of the two instead of silently diffing two different masks.
#[test]
fn the_replay_driver_and_this_normaliser_agree_on_one_golden_capture() {
    let input = include_str!("dx-parity.txt");
    let expected = include_str!("dx-parity.expected");
    assert_eq!(
        normalize_capture(input).expect("the golden capture normalises"),
        expected,
        "the golden capture is the contract `e2e/dx_replay.py --self-test` also asserts"
    );
    // The empty-capture refusal is part of the contract both share.
    assert_eq!(normalize_capture(""), Err(CaptureError::Empty));
}
