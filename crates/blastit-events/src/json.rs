//! The JSON line encoder (ARCHITECTURE.md §8.1: `BLASTIT_LOG_FORMAT=json`), and
//! the decoder that reads a line back into an [`Event`].

use std::collections::BTreeMap;
use std::io::Write as _;

use serde_json::{Map, Value};
use time::{OffsetDateTime, UtcOffset};

use crate::{
    AppOutput, Component, DevReload, DevReloadUnavailable, Event, EventKind, ExitKind,
    HttpRejected, Level, Lifecycle, MAX_EVENT_BYTES, ManifestWarning, Outcome, PgRefuseReason,
    PgRefused, ProcessCap, ProcessExited, ProcessRetired, ProcessSpawned, RejectReason, ReloadBoot,
    ReloadReason, RequestInfo, RetireReason, Runtime, SqlDenied, SqlKind, Stream, floor_boundary,
};

/// Encode one event as a single JSON object, without a trailing newline.
///
/// The key order is `ts`, `level`, `event`, `component`, `route` and
/// `request_id`, followed by the fields of the event's kind. Optional fields
/// are omitted when unset, except `route` on a `request`, which is `null` when
/// no route matched.
///
/// The result is never longer than [`MAX_EVENT_BYTES`]. When the event does
/// not fit, the longest strings are cut, largest first, on character
/// boundaries and named in a trailing `"truncated"` array. In practice the
/// fields that grow are the free text ones — `sql`, `path`, `line`, `reason`,
/// `message` and `immortalize_reason` — but every string is covered, so the
/// limit holds whatever the caller passes.
///
/// [`Event::with_truncated`] names fields the *caller* already shortened;
/// those names are reported too, whether or not the event needed cutting here.
pub fn encode_json(event: &Event) -> String {
    let mut out = Vec::with_capacity(256);
    encode_json_into(&mut out, event);
    // SAFETY: the encoder writes only ASCII and the UTF-8 bytes of the strings
    // it is handed, so the buffer is always valid UTF-8.
    unsafe { String::from_utf8_unchecked(out) }
}

/// Encode one event as a single JSON object, appended to `out`, without a
/// trailing newline.
///
/// [`encode_json`] is this with a fresh buffer. This form exists for the event
/// sink, which emits one event per request: it clears and reuses a single
/// buffer, so a request costs no allocation for its line. Only the encoded
/// object is appended — whatever `out` already holds is left in front of it —
/// and the bytes are exactly what [`encode_json`] returns for the same event.
pub fn encode_json_into(out: &mut Vec<u8>, event: &Event) {
    let declared = event.truncated_fields();
    let start = out.len();
    encode_into_with(out, event, declared);
    if out.len() - start <= MAX_EVENT_BYTES {
        return;
    }

    // Work on a copy: the caller keeps the complete event.
    let mut truncated: Vec<&'static str> = declared.to_vec();
    let mut event = event.clone();

    // The `dev.reload` arrays grow with the app, and an entry is either whole or
    // gone: drop them from the tail first, so no path or route name is ever
    // half-written (§4.9, §4.10).
    loop {
        out.truncate(start);
        encode_into_with(out, &event, &truncated);
        if out.len() - start <= MAX_EVENT_BYTES {
            break;
        }
        if !drop_reload_entry(&mut event, &mut truncated) {
            break;
        }
    }

    // Cut the long string fields, largest first, until the line fits. Every
    // cut removes at least as many encoded bytes as raw bytes, so this
    // terminates; `false` means every string is already empty.
    loop {
        out.truncate(start);
        encode_into_with(out, &event, &truncated);
        if out.len() - start <= MAX_EVENT_BYTES {
            return;
        }
        let excess = out.len() - start - MAX_EVENT_BYTES;
        if !cut_longest_string(&mut event, excess, &mut truncated) {
            break;
        }
    }

    // The lifecycle `outputs` map is the only structured field that can grow
    // without bound, so if nothing is left to cut, whole proxies are dropped
    // and `outputs` joins `truncated`. The `capabilities` list is bounded in
    // practice (a dozen names the kernel defines), and is dropped the same way
    // so that no caller can break the line limit with it either.
    loop {
        out.truncate(start);
        encode_into_with(out, &event, &truncated);
        if out.len() - start <= MAX_EVENT_BYTES {
            break;
        }
        if !drop_output_proxy(&mut event, &mut truncated)
            && !drop_capability(&mut event, &mut truncated)
        {
            break;
        }
    }

    out.truncate(start);
    encode_into_with(out, &event, &truncated);
}

/// Encode without the encoder's own truncation, but with the fields the caller
/// already declared as shortened. Only used to measure how much room is left.
pub(crate) fn encoded_len(event: &Event) -> usize {
    let mut out = Vec::with_capacity(256);
    encode_into_with(&mut out, event, event.truncated_fields());
    out.len()
}

/// The full encoder. `truncated` is appended as the last member when non-empty.
///
/// Writes straight into `out` with no per-field `String`: `serde_json` formats
/// and escapes each string and float into the buffer, `write!` formats the
/// integers, and the values a caller can pick only from a fixed set are written
/// as literals. That is what keeps a `request` event from allocating once per
/// field. The byte sequence is the one [`encode_json`] documents.
fn encode_into_with(out: &mut Vec<u8>, event: &Event, truncated: &[&'static str]) {
    out.push(b'{');
    push_timestamp_member(out, event.ts);
    push_str_member(out, "level", event.level.as_str());
    push_str_member(out, "event", event.kind.name());
    push_str_member(out, "component", event.component.as_str());
    match &event.route {
        Some(route) => push_str_member(out, "route", route),
        // §8.1: `route` is null on a request that matched nothing.
        None if matches!(event.kind, EventKind::Request(_)) => {
            push_raw_member(out, "route", b"null");
        }
        None => {}
    }
    if let Some(request_id) = &event.request_id {
        push_str_member(out, "request_id", request_id);
    }

    match &event.kind {
        EventKind::Request(info) => {
            push_str_member(out, "method", &info.method);
            push_str_member(out, "path", &info.path);
            if let Some(rule) = info.rule {
                push_u64_member(out, "rule", u64::from(rule));
            }
            push_u64_member(out, "status", u64::from(info.status));
            push_str_member(out, "outcome", info.outcome.as_str());
            push_f64_member(out, "queued_ms", info.queued_ms);
            push_f64_member(out, "duration_ms", info.duration_ms);
            push_u64_member(out, "bytes_in", info.bytes_in);
            push_u64_member(out, "bytes_out", info.bytes_out);
            push_u64_member(out, "cost_ns", info.cost_ns);
            match info.cpu_ns {
                Some(cpu_ns) => push_u64_member(out, "cpu_ns", cpu_ns),
                None => push_raw_member(out, "cpu_ns", b"null"),
            }
            push_str_member(out, "remote_addr", &info.remote_addr);
        }
        EventKind::SqlDenied(info) => {
            push_u64_member(out, "pid", u64::from(info.pid));
            push_str_member(out, "proxy", &info.proxy);
            push_str_member(out, "kind", info.kind.as_str());
            push_str_member(out, "reason", &info.reason);
            push_str_member(out, "sqlstate", &info.sqlstate);
            push_bool_member(out, "in_transaction", info.in_transaction);
            push_str_member(out, "sql", &info.sql);
        }
        EventKind::PgRefused(info) => {
            push_str_member(out, "proxy", &info.proxy);
            push_str_member(out, "reason", info.reason.as_str());
        }
        EventKind::HttpRejected(info) => {
            push_str_member(out, "remote_addr", &info.remote_addr);
            push_u64_member(out, "status", u64::from(info.status));
            push_str_member(out, "reason", info.reason.as_str());
        }
        EventKind::ProcessSpawned(info) => {
            push_u64_member(out, "pid", u64::from(info.pid));
            push_u64_member(out, "uid", u64::from(info.uid));
            push_f64_member(out, "fork_ms", info.fork_ms);
        }
        EventKind::ProcessExited(info) => {
            push_u64_member(out, "pid", u64::from(info.pid));
            push_u64_member(out, "uid", u64::from(info.uid));
            // Exactly one of `status` and `signal` (§8.1).
            let (field, code) = match info.exit {
                ExitKind::Status(status) => ("status", status),
                ExitKind::Signal(signal) => ("signal", signal),
            };
            push_i64_member(out, field, i64::from(code));
        }

        EventKind::ProcessRetired(info) => {
            push_u64_member(out, "pid", u64::from(info.pid));
            push_u64_member(out, "uid", u64::from(info.uid));
            push_str_member(out, "reason", info.reason.as_str());
        }
        EventKind::ProcessCap(info) => {
            push_u64_member(out, "estimate_bytes", info.estimate_bytes);
            push_u64_member(out, "cap", u64::from(info.cap));
        }
        EventKind::AppOutput(output) => {
            push_str_member(out, "stream", output.stream.as_str());
            push_str_member(out, "line", &output.line);
            push_u64_member(out, "part", u64::from(output.part));
            push_u64_member(out, "parts", u64::from(output.parts));
        }
        EventKind::Boot(lifecycle) => {
            push_lifecycle(out, lifecycle, truncated, true);
        }
        EventKind::Ready(lifecycle) | EventKind::Shutdown(lifecycle) => {
            push_lifecycle(out, lifecycle, truncated, false);
        }
        EventKind::ManifestWarning(info) => {
            push_str_member(out, "message", &info.message);
        }
        EventKind::Runtime(info) => {
            // §8.1's `runtime` rows: the entrypoint, then only the fields that
            // row lists. `php_ini` is the one nullable field, written as `null`
            // when PHP opened no file — absent would read as "this build does
            // not know the field".
            match info {
                Runtime::Php(php) => {
                    push_str_member(out, "entrypoint", "php");
                    push_str_member(out, "version", &php.version);
                    push_str_member(out, "libphp", &php.libphp);
                    match &php.php_ini {
                        Some(path) => push_str_member(out, "php_ini", path),
                        None => push_raw_member(out, "php_ini", b"null"),
                    }
                }
                Runtime::Python(py) => {
                    push_str_member(out, "entrypoint", "python");
                    push_str_member(out, "version", &py.version);
                    push_str_member(out, "environment", &py.environment);
                }
                Runtime::Static => push_str_member(out, "entrypoint", "static"),
            }
        }
        EventKind::DevReload(info) => {
            // §4.9's order: `routes`, `files`, `reason`. Both arrays are always
            // written, empty when the lists are, so a reader never has to tell
            // "absent" from "nothing".
            begin_member(out, "routes");
            push_str_array(out, info.routes.iter().map(String::as_str));
            begin_member(out, "files");
            push_str_array(out, info.files.iter().map(String::as_str));
            push_str_member(out, "reason", info.reason.as_str());
        }
        EventKind::DevReloadUnavailable(info) => {
            push_str_member(out, "errno", &info.errno);
            push_str_member(out, "filesystem", &info.filesystem);
        }
        EventKind::Reload(info) => {
            push_u64_member(out, "directories", u64::from(info.directories()));
        }
    }

    if !truncated.is_empty() {
        begin_member(out, "truncated");
        push_str_array(out, truncated.iter().copied());
    }

    out.push(b'}');
}

/// The lifecycle fields, in the order §8.1 lists them.
///
/// `boot` writes `preset` even when there is none: a `boot` line with
/// `"preset":null` is a boot with no preset, which is a different statement from
/// a line that does not mention presets at all (E1).
fn push_lifecycle(
    out: &mut Vec<u8>,
    lifecycle: &Lifecycle,
    truncated: &[&'static str],
    boot: bool,
) {
    if let Some(stage) = &lifecycle.stage {
        push_str_member(out, "stage", stage);
    }
    if let Some(routes) = lifecycle.routes {
        push_u64_member(out, "routes", u64::from(routes));
    }
    if let Some(workers) = lifecycle.workers {
        push_u64_member(out, "workers", u64::from(workers));
    }
    if let Some(duration_ms) = lifecycle.duration_ms {
        push_f64_member(out, "duration_ms", duration_ms);
    }
    if let Some(seccomp) = lifecycle.seccomp {
        push_bool_member(out, "seccomp", seccomp);
    }
    if let Some(privilege) = &lifecycle.privilege {
        push_str_member(out, "privilege", privilege);
    }
    // R23: only the `preload` `boot` carries the build; every other lifecycle
    // event omits it entirely.
    if let Some(build) = &lifecycle.build {
        push_str_member(out, "build", build);
    }
    // D6/M1: whether the children's bounding set is emptied. Reported in both
    // modes, because it is a fact about the boot rather than about init's own
    // set.
    if let Some(dropped) = lifecycle.bounding_set {
        push_str_member(
            out,
            "bounding_set",
            if dropped { "dropped" } else { "kept" },
        );
    }
    // D6: the capabilities an unprivileged init holds. Absent in root mode,
    // where the set is the container's own (§10).
    if !lifecycle.capabilities.is_empty() || truncated.contains(&"capabilities") {
        begin_member(out, "capabilities");
        push_str_array(out, lifecycle.capabilities.iter().map(String::as_str));
    }
    if let Some(immortalize) = lifecycle.immortalize {
        push_str_member(out, "immortalize", if immortalize { "on" } else { "off" });
    }
    if let Some(reason) = &lifecycle.immortalize_reason {
        push_str_member(out, "immortalize_reason", reason);
    }
    if let Some(immortalized) = lifecycle.immortalized {
        push_u64_member(out, "immortalized", immortalized);
    }
    if let Some(immortalize_ms) = lifecycle.immortalize_ms {
        push_f64_member(out, "immortalize_ms", immortalize_ms);
    }
    if let Some(mem_total) = lifecycle.mem_total {
        push_u64_member(out, "mem_total", mem_total);
    }
    if let Some(mem_used) = lifecycle.mem_used {
        push_u64_member(out, "mem_used", mem_used);
    }
    if let Some(max_view_processes) = lifecycle.max_view_processes {
        push_u64_member(out, "max_view_processes", u64::from(max_view_processes));
    }
    // `outputs` is written even when it was emptied, so that a `truncated`
    // entry always refers to a member that is there.
    if !lifecycle.outputs.is_empty() || truncated.contains(&"outputs") {
        begin_member(out, "outputs");
        push_outputs(out, &lifecycle.outputs);
    }
    // E1: which preset is in force, what it supplied, and what an explicit
    // variable overrode. The values are the preset's own — blastit's
    // configuration, not the operator's — and the overridden settings are names
    // only.
    match &lifecycle.preset {
        Some(preset) => push_str_member(out, "preset", preset),
        None if boot => push_raw_member(out, "preset", b"null"),
        None => {}
    }
    if !lifecycle.preset_settings.is_empty() || truncated.contains(&"preset_settings") {
        begin_member(out, "preset_settings");
        push_settings(out, &lifecycle.preset_settings);
    }
    if !lifecycle.preset_overridden.is_empty() {
        begin_member(out, "preset_overridden");
        push_str_array(out, lifecycle.preset_overridden.iter().map(String::as_str));
    }
}

/// The presets' settings as a JSON object: variable name to the preset's value.
fn push_settings(out: &mut Vec<u8>, settings: &BTreeMap<String, String>) {
    out.push(b'{');
    for (index, (name, value)) in settings.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        put_str(out, name);
        out.push(b':');
        put_str(out, value);
    }
    out.push(b'}');
}

/// The lifecycle `outputs` map as a JSON object: proxy to variable-name list.
fn push_outputs(out: &mut Vec<u8>, outputs: &BTreeMap<String, Vec<String>>) {
    out.push(b'{');
    for (index, (proxy, names)) in outputs.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        put_str(out, proxy);
        out.push(b':');
        push_str_array(out, names.iter().map(String::as_str));
    }
    out.push(b'}');
}

/// A JSON array of strings, each escaped the way `serde_json` escapes one.
fn push_str_array<'a>(out: &mut Vec<u8>, items: impl Iterator<Item = &'a str>) {
    out.push(b'[');
    for (index, item) in items.enumerate() {
        if index > 0 {
            out.push(b',');
        }
        put_str(out, item);
    }
    out.push(b']');
}

/// Start a member: the separating comma, the key, and its colon.
fn begin_member(out: &mut Vec<u8>, key: &str) {
    if !out.ends_with(b"{") {
        out.push(b',');
    }
    put_str(out, key);
    out.push(b':');
}

/// Append the JSON string literal for `text`, with `serde_json`'s escaping.
///
/// `serde_json` owns the escaping rules, and writes straight into the buffer so
/// no per-field `String` is made; writing to a `Vec<u8>` cannot fail.
fn put_str(out: &mut Vec<u8>, text: &str) {
    let _ = serde_json::to_writer(&mut *out, text);
}

/// `"ts":"2026-10-25T14:02:11.384Z"`: the one spelling the stream uses.
///
/// Written digit by digit rather than through [`crate::format_timestamp`],
/// which would allocate a `String` per event for a line every event carries.
fn push_timestamp_member(out: &mut Vec<u8>, ts: OffsetDateTime) {
    let ts = ts.to_offset(UtcOffset::UTC);
    begin_member(out, "ts");
    let _ = write!(
        out,
        "\"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z\"",
        ts.year(),
        u8::from(ts.month()),
        ts.day(),
        ts.hour(),
        ts.minute(),
        ts.second(),
        ts.millisecond()
    );
}

/// `"key":"text"`, a string member with `serde_json`'s escaping.
fn push_str_member(out: &mut Vec<u8>, key: &str, text: &str) {
    begin_member(out, key);
    put_str(out, text);
}

/// `"key":raw`, for a value that is already JSON: `null`, or a literal.
fn push_raw_member(out: &mut Vec<u8>, key: &str, raw: &[u8]) {
    begin_member(out, key);
    out.extend_from_slice(raw);
}

/// `"key":123`, an unsigned count or identifier.
fn push_u64_member(out: &mut Vec<u8>, key: &str, value: u64) {
    begin_member(out, key);
    let _ = write!(out, "{value}");
}

/// `"key":-1`, a signed value: an exit code or a signal.
fn push_i64_member(out: &mut Vec<u8>, key: &str, value: i64) {
    begin_member(out, key);
    let _ = write!(out, "{value}");
}

/// A JSON number for `value`, through `serde_json` so the spelling is the one
/// it writes for finite values and the `null` it writes for the rest.
fn push_f64_member(out: &mut Vec<u8>, key: &str, value: f64) {
    begin_member(out, key);
    let _ = serde_json::to_writer(&mut *out, &value);
}

/// `"key":true` or `"key":false`.
fn push_bool_member(out: &mut Vec<u8>, key: &str, value: bool) {
    begin_member(out, key);
    out.extend_from_slice(if value { b"true" } else { b"false" });
}

/// Parse one JSON line back into an [`Event`], or `None` when it is not a line
/// this build's [`encode_json`] could have written.
///
/// The inverse of the encoder, for the two things a forwarded line is used for:
/// the pretty rendering (§8.1's human form is written once, by
/// [`crate::encode_pretty`]) and anything else that wants the *event* rather
/// than its JSON. A line that does not decode — not JSON, not an object, an
/// unknown `event` name, a missing member, a member of the wrong type — is
/// [`None`] rather than a guess: a caller that has such a line says so in its
/// own words, and never invents a field.
///
/// `truncated` is the encoder's bookkeeping, and is not rebuilt: a decoded
/// event has no fields *it* shortened, and the strings it carries are the ones
/// the line holds.
pub fn decode_json(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;

    let ts = parse_timestamp(text(object, "ts")?)?;
    let level = text(object, "level")?.parse::<Level>().ok()?;
    let component = parse_component(text(object, "component")?)?;
    // `route` is null on a request that matched nothing, and absent elsewhere.
    let route = optional_text(object, "route")?;
    let request_id = optional_text(object, "request_id")?;
    let kind = decode_kind(text(object, "event")?, object)?;

    let mut event = Event::at(ts, component, kind).with_level(level);
    if let Some(route) = route {
        event = event.with_route(route);
    }
    if let Some(request_id) = request_id {
        event = event.with_request_id(request_id);
    }
    Some(event)
}

/// Restamp and canonicalise one line forwarded from an events pipe.
///
/// init is the only writer to stdout, so it labels every event it forwards
/// with the `component` — and `route`, when the pipe implies one — of the pipe
/// it arrived on, never the payload's own (§8.1). It also decides the key
/// order: the line an operator reads has its members sorted by key, which is
/// what a `serde_json` `Map` (a `BTreeMap`) produced when this went through
/// one, and the output must not change. This writes that same line straight
/// into `out`, borrowing each member's text from `line` instead of building the
/// map: the members are copied in order, with `component` and `route` replaced
/// or inserted, and then sorted by key.
///
/// Returns the line's `level` member when it carried one, so the caller can
/// apply `BLASTIT_LOG_LEVEL` without reading the line twice, and [`None`] when
/// `line` is not a JSON object — the caller's cue to drop it with a warning.
///
/// Only lines this build's [`encode_json`] wrote are expected here: an events
/// pipe belongs to http, scheduler or dbproxy, all of which encode with this
/// crate. The member values are copied verbatim, which is exact for them
/// because the encoder already writes each number and string the way
/// `serde_json` reads it back.
pub fn restamp_json_into<'a>(
    out: &mut Vec<u8>,
    line: &'a str,
    component: &str,
    route: Option<&str>,
) -> Option<Option<&'a str>> {
    let parsed = parse_members(line)?;
    let level = parsed
        .iter()
        .find(|member| member.key == "\"level\"")
        .and_then(|member| plain_string_value(member.value));

    let component_value = json_literal(component);
    let route_value = route.map(json_literal);

    let mut members: Vec<(&str, &str)> = Vec::with_capacity(parsed.len() + 2);
    let mut saw_component = false;
    let mut saw_route = false;
    for member in &parsed {
        let value = match member.key {
            "\"component\"" => {
                saw_component = true;
                component_value.as_str()
            }
            "\"route\"" => match route_value.as_deref() {
                Some(route) => {
                    saw_route = true;
                    route
                }
                // An events pipe implies no route, so the payload keeps its own.
                None => member.value,
            },
            _ => member.value,
        };
        members.push((member.key, value));
    }
    if !saw_component {
        members.push(("\"component\"", component_value.as_str()));
    }
    if let Some(route) = route_value.as_deref()
        && !saw_route
    {
        members.push(("\"route\"", route));
    }
    members.sort_unstable_by(|left, right| left.0.cmp(right.0));

    out.reserve(line.len() + component.len() + 16);
    out.push(b'{');
    for (index, (key, value)) in members.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.extend_from_slice(key.as_bytes());
        out.push(b':');
        out.extend_from_slice(value.as_bytes());
    }
    out.push(b'}');
    Some(level)
}

/// A guess at how many members an encoded line has, so the member list does not
/// grow a handful of times on every forwarded event. A line with more members
/// simply grows like any other `Vec`.
const MEMBERS_HINT: usize = 16;

/// One top-level member of a JSON object, as its source text.
struct Member<'a> {
    /// The key, with the surrounding quotes, as it appears in the source.
    key: &'a str,
    /// The value's raw text, as it appears in the source.
    value: &'a str,
}

/// The top-level members of a JSON object, or `None` when `line` is not one.
///
/// A scanner rather than a parse: every member keeps the text it had, so
/// nothing is normalised. Nesting is tracked so a comma or brace inside a
/// string, array or object does not end the member early.
fn parse_members(line: &str) -> Option<Vec<Member<'_>>> {
    let bytes = line.as_bytes();
    let mut index = skip_whitespace(bytes, 0);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    let mut members = Vec::with_capacity(MEMBERS_HINT);
    loop {
        index = skip_whitespace(bytes, index);
        if bytes.get(index) == Some(&b'}') {
            return Some(members);
        }
        if !members.is_empty() {
            if bytes.get(index) != Some(&b',') {
                return None;
            }
            index = skip_whitespace(bytes, index + 1);
        }
        if bytes.get(index) != Some(&b'"') {
            return None;
        }
        let key_end = scan_string(bytes, index)?;
        let key = &line[index..key_end];
        index = skip_whitespace(bytes, key_end);
        if bytes.get(index) != Some(&b':') {
            return None;
        }
        index = skip_whitespace(bytes, index + 1);
        let value_end = scan_value(bytes, index)?;
        members.push(Member {
            key,
            value: &line[index..value_end],
        });
        index = value_end;
    }
}

/// The index just past the whitespace starting at `index`.
fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while matches!(
        bytes.get(index).copied(),
        Some(b' ' | b'\t' | b'\n' | b'\r')
    ) {
        index += 1;
    }
    index
}

/// The index just past the JSON string starting at `index`, which must be `"`.
fn scan_string(bytes: &[u8], mut index: usize) -> Option<usize> {
    index += 1;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

/// The index just past the JSON value starting at `index`.
fn scan_value(bytes: &[u8], index: usize) -> Option<usize> {
    match bytes.get(index)? {
        b'"' => scan_string(bytes, index),
        b'{' | b'[' => scan_container(bytes, index),
        // A number, `true`, `false` or `null`: everything up to the member's
        // end. The encoder writes no whitespace, so the end is the next comma
        // or brace at this level.
        _ => {
            let mut end = index;
            while !matches!(bytes.get(end).copied(), None | Some(b',' | b'}')) {
                end += 1;
            }
            // A member with no value at all (`{"a":}`) is not JSON.
            if end == index {
                return None;
            }
            Some(end)
        }
    }
}

/// The index just past the object or array starting at `index`, which must be
/// `{` or `[`.
fn scan_container(bytes: &[u8], index: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = index;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'"' => index = scan_string(bytes, index)?,
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                index += 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// The text inside a JSON string literal this build wrote.
///
/// The only values read this way are the enumerated names `level` and its
/// peers, which are ASCII and never escaped, so an escape means "not a value
/// this build knows" rather than something to decode.
fn plain_string_value(raw: &str) -> Option<&str> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('\\') {
        None
    } else {
        Some(inner)
    }
}

/// A JSON string literal for `text`, with `serde_json`'s escaping.
fn json_literal(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The event kind named by the line's `event` member.
fn decode_kind(name: &str, object: &Map<String, Value>) -> Option<EventKind> {
    let kind = match name {
        "request" => {
            let mut info = RequestInfo::from_target(text(object, "method")?, text(object, "path")?);
            if let Some(rule) = optional_u32(object, "rule")? {
                info = info.with_rule(rule);
            }
            info = info
                .with_status(number_u16(object, "status")?)
                .with_outcome(parse_outcome(text(object, "outcome")?)?)
                .with_timings(
                    number_f64(object, "queued_ms")?,
                    number_f64(object, "duration_ms")?,
                )
                .with_bytes(
                    number_u64(object, "bytes_in")?,
                    number_u64(object, "bytes_out")?,
                )
                .with_cost(
                    number_u64(object, "cost_ns")?,
                    optional_u64(object, "cpu_ns")?,
                )
                .with_remote_addr(text(object, "remote_addr")?);
            EventKind::Request(info)
        }
        "sql.denied" => {
            let denial = SqlDenied::new(
                number_u32(object, "pid")?,
                text(object, "proxy")?,
                parse_sql_kind(text(object, "kind")?),
                text(object, "sql")?,
                text(object, "sqlstate")?,
                flag(object, "in_transaction")?,
            );
            match optional_text(object, "reason")? {
                Some(reason) => EventKind::SqlDenied(denial.with_reason(reason)),
                None => EventKind::SqlDenied(denial),
            }
        }
        "pg.refused" => EventKind::PgRefused(PgRefused::new(
            text(object, "proxy")?,
            parse_pg_refuse_reason(text(object, "reason")?),
        )),
        "http.rejected" => EventKind::HttpRejected(HttpRejected::new(
            text(object, "remote_addr")?,
            number_u16(object, "status")?,
            parse_reject_reason(text(object, "reason")?),
        )),
        "process.spawned" => EventKind::ProcessSpawned(ProcessSpawned::new(
            number_u32(object, "pid")?,
            number_u32(object, "uid")?,
            number_f64(object, "fork_ms")?,
        )),
        "process.exited" => {
            // Exactly one of `status` and `signal` (§8.1).
            let exit = match (
                optional_i32(object, "status")?,
                optional_i32(object, "signal")?,
            ) {
                (Some(status), None) => ExitKind::Status(status),
                (None, Some(signal)) => ExitKind::Signal(signal),
                _ => return None,
            };
            EventKind::ProcessExited(ProcessExited::new(
                number_u32(object, "pid")?,
                number_u32(object, "uid")?,
                exit,
            ))
        }
        "process.retired" => EventKind::ProcessRetired(ProcessRetired::new(
            number_u32(object, "pid")?,
            number_u32(object, "uid")?,
            parse_retire_reason(text(object, "reason")?),
        )),
        "process.cap" => EventKind::ProcessCap(ProcessCap::new(
            number_u64(object, "estimate_bytes")?,
            number_u32(object, "cap")?,
        )),
        "app.output" => {
            let output = AppOutput::new(
                parse_stream(text(object, "stream")?)?,
                text(object, "line")?,
            )
            .with_part(number_u32(object, "part")?, number_u32(object, "parts")?)
            .ok()?;
            EventKind::AppOutput(output)
        }
        "boot" => EventKind::Boot(decode_lifecycle(object)?),
        "ready" => EventKind::Ready(decode_lifecycle(object)?),
        "shutdown" => EventKind::Shutdown(decode_lifecycle(object)?),
        "manifest.warning" => {
            EventKind::ManifestWarning(ManifestWarning::new(text(object, "message")?))
        }
        "runtime" => EventKind::Runtime(decode_runtime(object)?),
        "dev.reload" => EventKind::DevReload(DevReload::new(
            string_array(object, "routes")?,
            string_array(object, "files")?,
            parse_reload_reason(text(object, "reason")?)?,
        )),
        "dev.reload.unavailable" => EventKind::DevReloadUnavailable(DevReloadUnavailable::new(
            text(object, "errno")?,
            text(object, "filesystem")?,
        )),
        "reload" => EventKind::Reload(ReloadBoot::new(number_u32(object, "directories")?)),
        _ => return None,
    };
    Some(kind)
}

/// A `runtime` line: the entrypoint it names decides which fields it has.
///
/// An entrypoint outside the frozen three words, or a required field missing,
/// makes the line undecodable rather than inventing a runtime. `php_ini` is
/// nullable, and absent reads as `None` for a line written before the field
/// existed — never as a path.
fn decode_runtime(object: &Map<String, Value>) -> Option<Runtime> {
    Some(match text(object, "entrypoint")? {
        "php" => Runtime::php(
            text(object, "version")?,
            text(object, "libphp")?,
            optional_text(object, "php_ini")?.map(str::to_owned),
        ),
        "python" => Runtime::python(text(object, "version")?, text(object, "environment")?),
        "static" => Runtime::Static,
        _ => return None,
    })
}

/// The lifecycle fields of a `boot`, `ready` or `shutdown` line.
fn decode_lifecycle(object: &Map<String, Value>) -> Option<Lifecycle> {
    let mut lifecycle = Lifecycle::default();
    if let Some(stage) = optional_text(object, "stage")? {
        lifecycle = lifecycle.with_stage(stage);
    }
    if let Some(routes) = optional_u32(object, "routes")? {
        lifecycle = lifecycle.with_routes(routes);
    }
    if let Some(workers) = optional_u32(object, "workers")? {
        lifecycle = lifecycle.with_workers(workers);
    }
    if let Some(duration_ms) = optional_f64(object, "duration_ms")? {
        lifecycle = lifecycle.with_duration_ms(duration_ms);
    }
    if let Some(seccomp) = optional_flag(object, "seccomp")? {
        lifecycle = lifecycle.with_seccomp(seccomp);
    }
    if let Some(privilege) = optional_text(object, "privilege")? {
        lifecycle = lifecycle.with_privilege(privilege);
    }
    if let Some(build) = optional_text(object, "build")? {
        lifecycle = lifecycle.with_build(build);
    }
    if let Some(bounding_set) = optional_text(object, "bounding_set")? {
        lifecycle = lifecycle.with_bounding_set(bounding_set == "dropped");
    }
    for capability in optional_strings(object, "capabilities")? {
        lifecycle = lifecycle.with_capability(capability);
    }
    if let Some(immortalize) = optional_text(object, "immortalize")? {
        lifecycle = lifecycle.with_immortalize(immortalize == "on");
    }
    if let Some(reason) = optional_text(object, "immortalize_reason")? {
        lifecycle = lifecycle.with_immortalize_reason(reason);
    }
    if let Some(immortalized) = optional_u64(object, "immortalized")? {
        lifecycle = lifecycle.with_immortalized(immortalized);
    }
    if let Some(immortalize_ms) = optional_f64(object, "immortalize_ms")? {
        lifecycle = lifecycle.with_immortalize_ms(immortalize_ms);
    }
    if let Some(mem_total) = optional_u64(object, "mem_total")? {
        lifecycle = lifecycle.with_mem_total(mem_total);
    }
    if let Some(mem_used) = optional_u64(object, "mem_used")? {
        lifecycle = lifecycle.with_mem_used(mem_used);
    }
    if let Some(cap) = optional_u32(object, "max_view_processes")? {
        lifecycle = lifecycle.with_max_view_processes(cap);
    }
    for (proxy, names) in optional_lists(object, "outputs")? {
        for name in names {
            lifecycle = lifecycle.with_output(proxy.as_str(), name);
        }
    }
    if let Some(preset) = optional_text(object, "preset")? {
        lifecycle = lifecycle.with_preset(preset);
    }
    for (name, value) in optional_strings_map(object, "preset_settings")? {
        lifecycle = lifecycle.with_preset_setting(name.as_str(), value.as_str());
    }
    for name in optional_strings(object, "preset_overridden")? {
        lifecycle = lifecycle.with_preset_override(name);
    }
    Some(lifecycle)
}

/// A member of the object, whatever it is.
fn member<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name)
}

/// A member that must be a string.
fn text<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    member(object, name)?.as_str()
}

/// A member that is a string, or absent, or `null`.
///
/// # Errors
///
/// `None` — "this is not a line the encoder wrote" — for anything else: a
/// `route` that is a number is not a route this crate ever wrote.
fn optional_text<'a>(object: &'a Map<String, Value>, name: &str) -> Option<Option<&'a str>> {
    match member(object, name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_str()?)),
    }
}

fn number_u64(object: &Map<String, Value>, name: &str) -> Option<u64> {
    member(object, name)?.as_u64()
}

fn number_u32(object: &Map<String, Value>, name: &str) -> Option<u32> {
    u32::try_from(number_u64(object, name)?).ok()
}

fn number_u16(object: &Map<String, Value>, name: &str) -> Option<u16> {
    u16::try_from(number_u64(object, name)?).ok()
}

fn number_f64(object: &Map<String, Value>, name: &str) -> Option<f64> {
    member(object, name)?.as_f64()
}

/// A member that is a number, or absent, or `null`.
fn optional_u64(object: &Map<String, Value>, name: &str) -> Option<Option<u64>> {
    match member(object, name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_u64()?)),
    }
}

fn optional_u32(object: &Map<String, Value>, name: &str) -> Option<Option<u32>> {
    optional_u64(object, name).map(|value| value.and_then(|value| u32::try_from(value).ok()))
}

fn optional_i32(object: &Map<String, Value>, name: &str) -> Option<Option<i32>> {
    match member(object, name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(i32::try_from(value.as_i64()?).ok()?)),
    }
}

fn optional_f64(object: &Map<String, Value>, name: &str) -> Option<Option<f64>> {
    match member(object, name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_f64()?)),
    }
}

fn flag(object: &Map<String, Value>, name: &str) -> Option<bool> {
    member(object, name)?.as_bool()
}

/// A member that is a boolean, or absent.
fn optional_flag(object: &Map<String, Value>, name: &str) -> Option<Option<bool>> {
    match member(object, name) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_bool()?)),
    }
}

/// A member that is an array of strings, or absent.
fn optional_strings(object: &Map<String, Value>, name: &str) -> Option<Vec<String>> {
    let Some(value) = member(object, name) else {
        return Some(Vec::new());
    };
    let array = value.as_array()?;
    let mut strings = Vec::with_capacity(array.len());
    for item in array {
        strings.push(item.as_str()?.to_string());
    }
    Some(strings)
}

/// A member that is an array of strings, required: the encoder always writes
/// both `dev.reload` arrays, so a missing one is not this build's line.
fn string_array(object: &Map<String, Value>, name: &str) -> Option<Vec<String>> {
    let array = member(object, name)?.as_array()?;
    let mut strings = Vec::with_capacity(array.len());
    for item in array {
        strings.push(item.as_str()?.to_string());
    }
    Some(strings)
}

/// A member that is an object of strings, or absent.
fn optional_strings_map(object: &Map<String, Value>, name: &str) -> Option<Vec<(String, String)>> {
    let Some(value) = member(object, name) else {
        return Some(Vec::new());
    };
    let map = value.as_object()?;
    let mut pairs = Vec::with_capacity(map.len());
    for (key, item) in map {
        pairs.push((key.clone(), item.as_str()?.to_string()));
    }
    Some(pairs)
}

/// A member that is an object of arrays of strings, or absent: the lifecycle
/// `outputs` map, proxy name to variable names.
fn optional_lists(object: &Map<String, Value>, name: &str) -> Option<Vec<(String, Vec<String>)>> {
    let Some(value) = member(object, name) else {
        return Some(Vec::new());
    };
    let map = value.as_object()?;
    let mut lists = Vec::with_capacity(map.len());
    for (key, item) in map {
        let array = item.as_array()?;
        let mut names = Vec::with_capacity(array.len());
        for name in array {
            names.push(name.as_str()?.to_string());
        }
        lists.push((key.clone(), names));
    }
    Some(lists)
}

/// The vocabularies the encoder writes by name, read back.
fn parse_component(text: &str) -> Option<Component> {
    Some(match text {
        "init" => Component::Init,
        "http" => Component::Http,
        "scheduler" => Component::Scheduler,
        "dbproxy" => Component::Dbproxy,
        "view" => Component::View,
        _ => return None,
    })
}

fn parse_outcome(text: &str) -> Option<Outcome> {
    Some(match text {
        "ok" => Outcome::Ok,
        "unmatched" => Outcome::Unmatched,
        "queue_full" => Outcome::QueueFull,
        "queue_timeout" => Outcome::QueueTimeout,
        "timeout" => Outcome::Timeout,
        "view_died" => Outcome::ViewDied,
        "bailed" => Outcome::Bailed,
        "client_gone" => Outcome::ClientGone,
        _ => return None,
    })
}

fn parse_stream(text: &str) -> Option<Stream> {
    Some(match text {
        "stdout" => Stream::Stdout,
        "stderr" => Stream::Stderr,
        _ => return None,
    })
}

/// A `sql.denied` kind: anything that is not one of the four names is the
/// protocol's own word for it, which is what `Other` holds.
fn parse_sql_kind(text: &str) -> SqlKind {
    match text {
        "query" => SqlKind::Query,
        "parse" => SqlKind::Parse,
        "function_call" => SqlKind::FunctionCall,
        "copy" => SqlKind::Copy,
        other => SqlKind::Other(other.to_string()),
    }
}

fn parse_pg_refuse_reason(text: &str) -> PgRefuseReason {
    match text {
        "credentials_mismatch" => PgRefuseReason::CredentialsMismatch,
        "too_many_connections" => PgRefuseReason::TooManyConnections,
        "upstream_unreachable" => PgRefuseReason::UpstreamUnreachable,
        other => PgRefuseReason::Other(other.to_string()),
    }
}

fn parse_reject_reason(text: &str) -> RejectReason {
    match text {
        "malformed_request" => RejectReason::MalformedRequest,
        "header_too_large" => RejectReason::HeaderTooLarge,
        "body_too_large" => RejectReason::BodyTooLarge,
        "upgrade_unsupported" => RejectReason::UpgradeUnsupported,
        other => RejectReason::Other(other.to_string()),
    }
}

fn parse_retire_reason(text: &str) -> RetireReason {
    match text {
        "idle" => RetireReason::Idle,
        "max_requests" => RetireReason::MaxRequests,
        "stuck_threads" => RetireReason::StuckThreads,
        "memory_pressure" => RetireReason::MemoryPressure,
        "lru_cap" => RetireReason::LruCap,
        "reload" => RetireReason::Reload,
        other => RetireReason::Other(other.to_string()),
    }
}

/// A `dev.reload` reason: a closed set, so a word outside it makes the line
/// undecodable rather than inventing a reason.
fn parse_reload_reason(text: &str) -> Option<ReloadReason> {
    Some(match text {
        "loaded" => ReloadReason::Loaded,
        "preload" => ReloadReason::Preload,
        "blastfile" => ReloadReason::Blastfile,
        "overflow" => ReloadReason::Overflow,
        _ => return None,
    })
}

/// `2026-10-25T14:02:11.384Z`, the one spelling `format_timestamp` writes.
///
/// Parsed field by field rather than with `time`'s RFC 3339 parser: that one
/// lives behind a feature this crate does not need for anything else, and a
/// decoder that cannot read the encoder's own output is a decoder that does not
/// work.
fn parse_timestamp(text: &str) -> Option<OffsetDateTime> {
    let (date, rest) = text.strip_suffix('Z')?.split_once('T')?;
    let (clock, millis) = rest.split_once('.')?;
    let mut date = date.split('-');
    let year: i32 = date.next()?.parse().ok()?;
    let month: u8 = date.next()?.parse().ok()?;
    let day: u8 = date.next()?.parse().ok()?;
    if date.next().is_some() {
        return None;
    }
    let mut clock = clock.split(':');
    let hour: u8 = clock.next()?.parse().ok()?;
    let minute: u8 = clock.next()?.parse().ok()?;
    let second: u8 = clock.next()?.parse().ok()?;
    if clock.next().is_some() {
        return None;
    }
    let millisecond: u16 = millis.parse().ok()?;
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let clock = time::Time::from_hms_milli(hour, minute, second, millisecond).ok()?;
    Some(time::PrimitiveDateTime::new(date, clock).assume_utc())
}

/// Every string in `event` that may be shortened, in encode order.
fn string_slots(event: &mut Event) -> Vec<(&'static str, &mut String)> {
    let mut slots: Vec<(&'static str, &mut String)> = Vec::new();
    if let Some(route) = event.route.as_mut() {
        slots.push(("route", route));
    }
    if let Some(request_id) = event.request_id.as_mut() {
        slots.push(("request_id", request_id));
    }
    match &mut event.kind {
        EventKind::Request(info) => {
            slots.push(("method", &mut info.method));
            slots.push(("path", &mut info.path));
            slots.push(("remote_addr", &mut info.remote_addr));
        }
        EventKind::SqlDenied(info) => {
            slots.push(("proxy", &mut info.proxy));
            slots.push(("reason", &mut info.reason));
            slots.push(("sqlstate", &mut info.sqlstate));
            slots.push(("sql", &mut info.sql));
            if let SqlKind::Other(kind) = &mut info.kind {
                slots.push(("kind", kind));
            }
        }
        EventKind::PgRefused(info) => {
            slots.push(("proxy", &mut info.proxy));
            if let PgRefuseReason::Other(reason) = &mut info.reason {
                slots.push(("reason", reason));
            }
        }
        EventKind::HttpRejected(info) => {
            slots.push(("remote_addr", &mut info.remote_addr));
            if let RejectReason::Other(reason) = &mut info.reason {
                slots.push(("reason", reason));
            }
        }
        EventKind::ProcessRetired(info) => {
            if let RetireReason::Other(reason) = &mut info.reason {
                slots.push(("reason", reason));
            }
        }
        EventKind::AppOutput(output) => {
            slots.push(("line", &mut output.line));
        }
        EventKind::Boot(lifecycle)
        | EventKind::Ready(lifecycle)
        | EventKind::Shutdown(lifecycle) => {
            if let Some(stage) = lifecycle.stage.as_mut() {
                slots.push(("stage", stage));
            }
            if let Some(build) = lifecycle.build.as_mut() {
                slots.push(("build", build));
            }
            if let Some(reason) = lifecycle.immortalize_reason.as_mut() {
                slots.push(("immortalize_reason", reason));
            }
            if let Some(preset) = lifecycle.preset.as_mut() {
                slots.push(("preset", preset));
            }
            // The preset's own values, and the names it was overridden for.
            // Presets are blastit's own tables, so these cannot grow with the
            // environment — but a value that is long enough to matter is cut
            // like any other string, so the line limit holds here too.
            for value in lifecycle.preset_settings.values_mut() {
                slots.push(("preset_settings", value));
            }
            for name in &mut lifecycle.preset_overridden {
                slots.push(("preset_overridden", name));
            }
        }
        EventKind::ManifestWarning(info) => {
            slots.push(("message", &mut info.message));
        }
        EventKind::Runtime(info) => match info {
            Runtime::Php(php) => {
                slots.push(("version", &mut php.version));
                slots.push(("libphp", &mut php.libphp));
                if let Some(path) = php.php_ini.as_mut() {
                    slots.push(("php_ini", path));
                }
            }
            Runtime::Python(py) => {
                slots.push(("version", &mut py.version));
                slots.push(("environment", &mut py.environment));
            }
            // `static` carries one fixed word and nothing else.
            Runtime::Static => {}
        },
        // `dev.reload`'s only variable-size fields are its arrays, which are cut
        // whole by `drop_reload_entry` before any string is touched; its
        // `reason` is a fixed word.
        EventKind::DevReload(_) => {}
        EventKind::DevReloadUnavailable(info) => {
            slots.push(("errno", &mut info.errno));
            slots.push(("filesystem", &mut info.filesystem));
        }
        // The boot line carries one integer and no string that can grow.
        EventKind::Reload(_) => {}
        EventKind::ProcessSpawned(_) | EventKind::ProcessExited(_) | EventKind::ProcessCap(_) => {}
    }
    slots
}

/// Drop one whole `dev.reload` entry from the tail, recording the array's name
/// in `truncated`. Returns `false` when both arrays are empty.
///
/// Files go first, then routes: the reason and the route set serving after the
/// reload are the more useful half of the event, and a README-length path list
/// is what actually forces the cut in practice. Whole entries only, so a reader
/// never mistakes a sliced path for a file the app loaded.
fn drop_reload_entry(event: &mut Event, truncated: &mut Vec<&'static str>) -> bool {
    let EventKind::DevReload(info) = &mut event.kind else {
        return false;
    };

    if info.files.pop().is_some() {
        mark(truncated, "files");
        return true;
    }
    if info.routes.pop().is_some() {
        mark(truncated, "routes");
        return true;
    }
    false
}

/// Cut at most `excess` raw bytes off the longest string, keeping its prefix.
///
/// Returns `false` when every string is already empty.
fn cut_longest_string(event: &mut Event, excess: usize, truncated: &mut Vec<&'static str>) -> bool {
    let mut slots = string_slots(event);
    let mut longest: Option<usize> = None;
    for (index, (_, text)) in slots.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let better = match longest {
            None => true,
            Some(best) => text.len() > slots[best].1.len(),
        };
        if better {
            longest = Some(index);
        }
    }
    let Some(index) = longest else {
        return false;
    };

    let (name, text) = slots.swap_remove(index);
    let keep = if excess >= text.len() {
        0
    } else {
        floor_boundary(text, text.len() - excess)
    };
    text.truncate(keep);
    if !truncated.contains(&name) {
        truncated.push(name);
    }
    true
}

/// Drop one whole proxy from the lifecycle `outputs` map, recording `outputs`
/// in `truncated`. Returns `false` when the map is empty.
///
/// Whole proxies only: a proxy's list of variable names is either complete or
/// gone, so a reader never sees a half-rendered set and mistakes it for the
/// app's real environment. The last key in `BTreeMap` order goes first, which
/// keeps the choice deterministic.
fn drop_output_proxy(event: &mut Event, truncated: &mut Vec<&'static str>) -> bool {
    let (EventKind::Boot(lifecycle) | EventKind::Ready(lifecycle) | EventKind::Shutdown(lifecycle)) =
        &mut event.kind
    else {
        return false;
    };

    if lifecycle.outputs.pop_last().is_some() {
        mark(truncated, "outputs");
        return true;
    }
    false
}

/// Record that a structured field was cut, once.
fn mark(truncated: &mut Vec<&'static str>, name: &'static str) {
    if !truncated.contains(&name) {
        truncated.push(name);
    }
}

/// Drop one capability name from a lifecycle event, recording `capabilities` in
/// `truncated`. Returns `false` when the list is empty.
///
/// One name at a time, from the end: the list is a set of what the process
/// holds, so a reader who sees it cut knows it is not the whole story — which is
/// what `truncated` is for. Last first, so the choice is deterministic.
fn drop_capability(event: &mut Event, truncated: &mut Vec<&'static str>) -> bool {
    let (EventKind::Boot(lifecycle) | EventKind::Ready(lifecycle) | EventKind::Shutdown(lifecycle)) =
        &mut event.kind
    else {
        return false;
    };

    if lifecycle.capabilities.pop().is_some() {
        mark(truncated, "capabilities");
        return true;
    }
    false
}
