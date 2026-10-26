//! The JSON line encoder (ARCHITECTURE.md §8.1: `BLASTIT_LOG_FORMAT=json`).

use std::collections::BTreeMap;

use crate::{
    Event, EventKind, ExitKind, Lifecycle, MAX_EVENT_BYTES, PgRefuseReason, RejectReason,
    RetireReason, SqlKind, floor_boundary, format_timestamp,
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
    let declared = event.truncated_fields();
    let encoded = encode_with(event, declared);
    if encoded.len() <= MAX_EVENT_BYTES {
        return encoded;
    }

    // Work on a copy: the caller keeps the complete event.
    let mut truncated: Vec<&'static str> = declared.to_vec();
    let mut event = event.clone();

    // Cut the long string fields, largest first, until the line fits. Every
    // cut removes at least as many encoded bytes as raw bytes, so this
    // terminates; `false` means every string is already empty.
    loop {
        let encoded = encode_with(&event, &truncated);
        if encoded.len() <= MAX_EVENT_BYTES {
            return encoded;
        }
        let excess = encoded.len() - MAX_EVENT_BYTES;
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
        if encode_with(&event, &truncated).len() <= MAX_EVENT_BYTES {
            break;
        }
        if !drop_output_proxy(&mut event, &mut truncated)
            && !drop_capability(&mut event, &mut truncated)
        {
            break;
        }
    }

    encode_with(&event, &truncated)
}

/// Encode without the encoder's own truncation, but with the fields the caller
/// already declared as shortened. Only used to measure how much room is left.
pub(crate) fn encoded_len(event: &Event) -> usize {
    encode_with(event, event.truncated_fields()).len()
}

/// The full encoder. `truncated` is appended as the last member when non-empty.
fn encode_with(event: &Event, truncated: &[&'static str]) -> String {
    let mut out = String::with_capacity(256);
    out.push('{');
    push_member(&mut out, "ts", &json_string(&format_timestamp(event.ts)));
    push_member(&mut out, "level", &json_string(event.level.as_str()));
    push_member(&mut out, "event", &json_string(event.kind.name()));
    push_member(
        &mut out,
        "component",
        &json_string(event.component.as_str()),
    );
    match &event.route {
        Some(route) => push_member(&mut out, "route", &json_string(route)),
        // §8.1: `route` is null on a request that matched nothing.
        None if matches!(event.kind, EventKind::Request(_)) => {
            push_member(&mut out, "route", "null")
        }
        None => {}
    }
    if let Some(request_id) = &event.request_id {
        push_member(&mut out, "request_id", &json_string(request_id));
    }

    match &event.kind {
        EventKind::Request(info) => {
            push_member(&mut out, "method", &json_string(&info.method));
            push_member(&mut out, "path", &json_string(&info.path));
            if let Some(rule) = info.rule {
                push_member(&mut out, "rule", &rule.to_string());
            }
            push_member(&mut out, "status", &info.status.to_string());
            push_member(&mut out, "outcome", &json_string(info.outcome.as_str()));
            push_member(&mut out, "queued_ms", &json_f64(info.queued_ms));
            push_member(&mut out, "duration_ms", &json_f64(info.duration_ms));
            push_member(&mut out, "bytes_in", &info.bytes_in.to_string());
            push_member(&mut out, "bytes_out", &info.bytes_out.to_string());
            push_member(&mut out, "cost_ns", &info.cost_ns.to_string());
            match info.cpu_ns {
                Some(cpu_ns) => push_member(&mut out, "cpu_ns", &cpu_ns.to_string()),
                None => push_member(&mut out, "cpu_ns", "null"),
            }
            push_member(&mut out, "remote_addr", &json_string(&info.remote_addr));
        }
        EventKind::SqlDenied(info) => {
            push_member(&mut out, "pid", &info.pid.to_string());
            push_member(&mut out, "proxy", &json_string(&info.proxy));
            push_member(&mut out, "kind", &json_string(info.kind.as_str()));
            push_member(&mut out, "reason", &json_string(&info.reason));
            push_member(&mut out, "sqlstate", &json_string(&info.sqlstate));
            push_member(&mut out, "in_transaction", &json_bool(info.in_transaction));
            push_member(&mut out, "sql", &json_string(&info.sql));
        }
        EventKind::PgRefused(info) => {
            push_member(&mut out, "proxy", &json_string(&info.proxy));
            push_member(&mut out, "reason", &json_string(info.reason.as_str()));
        }
        EventKind::HttpRejected(info) => {
            push_member(&mut out, "remote_addr", &json_string(&info.remote_addr));
            push_member(&mut out, "status", &info.status.to_string());
            push_member(&mut out, "reason", &json_string(info.reason.as_str()));
        }
        EventKind::ProcessSpawned(info) => {
            push_member(&mut out, "pid", &info.pid.to_string());
            push_member(&mut out, "uid", &info.uid.to_string());
            push_member(&mut out, "fork_ms", &json_f64(info.fork_ms));
        }
        EventKind::ProcessExited(info) => {
            push_member(&mut out, "pid", &info.pid.to_string());
            push_member(&mut out, "uid", &info.uid.to_string());
            // Exactly one of `status` and `signal` (§8.1).
            let (field, code) = match info.exit {
                ExitKind::Status(status) => ("status", status),
                ExitKind::Signal(signal) => ("signal", signal),
            };
            push_member(&mut out, field, &code.to_string());
        }

        EventKind::ProcessRetired(info) => {
            push_member(&mut out, "pid", &info.pid.to_string());
            push_member(&mut out, "uid", &info.uid.to_string());
            push_member(&mut out, "reason", &json_string(info.reason.as_str()));
        }
        EventKind::ProcessCap(info) => {
            push_member(&mut out, "estimate_bytes", &info.estimate_bytes.to_string());
            push_member(&mut out, "cap", &info.cap.to_string());
        }
        EventKind::AppOutput(output) => {
            push_member(&mut out, "stream", &json_string(output.stream.as_str()));
            push_member(&mut out, "line", &json_string(&output.line));
            push_member(&mut out, "part", &output.part.to_string());
            push_member(&mut out, "parts", &output.parts.to_string());
        }
        EventKind::Boot(lifecycle) => {
            push_lifecycle(&mut out, lifecycle, truncated, true);
        }
        EventKind::Ready(lifecycle) | EventKind::Shutdown(lifecycle) => {
            push_lifecycle(&mut out, lifecycle, truncated, false);
        }
        EventKind::ManifestWarning(info) => {
            push_member(&mut out, "message", &json_string(&info.message));
        }
    }

    if !truncated.is_empty() {
        let names = truncated
            .iter()
            .map(|name| json_string(name))
            .collect::<Vec<String>>()
            .join(",");
        push_member(&mut out, "truncated", &format!("[{names}]"));
    }

    out.push('}');
    out
}

/// The lifecycle fields, in the order §8.1 lists them.
///
/// `boot` writes `preset` even when there is none: a `boot` line with
/// `"preset":null` is a boot with no preset, which is a different statement from
/// a line that does not mention presets at all (E1).
fn push_lifecycle(out: &mut String, lifecycle: &Lifecycle, truncated: &[&'static str], boot: bool) {
    if let Some(stage) = &lifecycle.stage {
        push_member(out, "stage", &json_string(stage));
    }
    if let Some(routes) = lifecycle.routes {
        push_member(out, "routes", &routes.to_string());
    }
    if let Some(workers) = lifecycle.workers {
        push_member(out, "workers", &workers.to_string());
    }
    if let Some(duration_ms) = lifecycle.duration_ms {
        push_member(out, "duration_ms", &json_f64(duration_ms));
    }
    if let Some(seccomp) = lifecycle.seccomp {
        push_member(out, "seccomp", &json_bool(seccomp));
    }
    if let Some(privilege) = &lifecycle.privilege {
        push_member(out, "privilege", &json_string(privilege));
    }
    // D6/M1: whether the children's bounding set is emptied. Reported in both
    // modes, because it is a fact about the boot rather than about init's own
    // set.
    if let Some(dropped) = lifecycle.bounding_set {
        push_member(
            out,
            "bounding_set",
            &json_string(if dropped { "dropped" } else { "kept" }),
        );
    }
    // D6: the capabilities an unprivileged init holds. Absent in root mode,
    // where the set is the container's own (§10).
    if !lifecycle.capabilities.is_empty() || truncated.contains(&"capabilities") {
        push_member(
            out,
            "capabilities",
            &encode_capabilities(&lifecycle.capabilities),
        );
    }
    if let Some(immortalize) = lifecycle.immortalize {
        push_member(
            out,
            "immortalize",
            &json_string(if immortalize { "on" } else { "off" }),
        );
    }
    if let Some(reason) = &lifecycle.immortalize_reason {
        push_member(out, "immortalize_reason", &json_string(reason));
    }
    if let Some(immortalized) = lifecycle.immortalized {
        push_member(out, "immortalized", &immortalized.to_string());
    }
    if let Some(immortalize_ms) = lifecycle.immortalize_ms {
        push_member(out, "immortalize_ms", &json_f64(immortalize_ms));
    }
    if let Some(mem_total) = lifecycle.mem_total {
        push_member(out, "mem_total", &mem_total.to_string());
    }
    if let Some(mem_used) = lifecycle.mem_used {
        push_member(out, "mem_used", &mem_used.to_string());
    }
    if let Some(max_view_processes) = lifecycle.max_view_processes {
        push_member(out, "max_view_processes", &max_view_processes.to_string());
    }
    // `outputs` is written even when it was emptied, so that a `truncated`
    // entry always refers to a member that is there.
    if !lifecycle.outputs.is_empty() || truncated.contains(&"outputs") {
        push_member(out, "outputs", &encode_outputs(&lifecycle.outputs));
    }
    // E1: which preset is in force, what it supplied, and what an explicit
    // variable overrode. The values are the preset's own — blastit's
    // configuration, not the operator's — and the overridden settings are names
    // only.
    match &lifecycle.preset {
        Some(preset) => push_member(out, "preset", &json_string(preset)),
        None if boot => push_member(out, "preset", "null"),
        None => {}
    }
    if !lifecycle.preset_settings.is_empty() || truncated.contains(&"preset_settings") {
        push_member(
            out,
            "preset_settings",
            &encode_settings(&lifecycle.preset_settings),
        );
    }
    if !lifecycle.preset_overridden.is_empty() {
        let names = lifecycle
            .preset_overridden
            .iter()
            .map(|name| json_string(name))
            .collect::<Vec<String>>()
            .join(",");
        push_member(out, "preset_overridden", &format!("[{names}]"));
    }
}

/// The presets' settings as a JSON object: variable name to the preset's value.
fn encode_settings(settings: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (index, (name, value)) in settings.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(name));
        out.push(':');
        out.push_str(&json_string(value));
    }
    out.push('}');
    out
}

fn encode_capabilities(capabilities: &[String]) -> String {
    let names = capabilities
        .iter()
        .map(|name| json_string(name))
        .collect::<Vec<String>>()
        .join(",");
    format!("[{names}]")
}

fn encode_outputs(outputs: &BTreeMap<String, Vec<String>>) -> String {
    let mut out = String::from("{");
    for (index, (proxy, names)) in outputs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(proxy));
        out.push_str(":[");
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&json_string(name));
        }
        out.push(']');
    }
    out.push('}');
    out
}

/// Append `"key":raw` to a half-built object, with the separating comma.
fn push_member(out: &mut String, key: &str, raw: &str) {
    if !out.ends_with('{') {
        out.push(',');
    }
    out.push_str(&json_string(key));
    out.push(':');
    out.push_str(raw);
}

/// A JSON string literal. `serde_json` owns the escaping rules.
fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// A JSON number. Non-finite values become `null`, which is what
/// `serde_json` does for them, so the result is always parseable.
fn json_f64(value: f64) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned())
}

fn json_bool(value: bool) -> String {
    if value { "true" } else { "false" }.to_owned()
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
        EventKind::ProcessSpawned(_) | EventKind::ProcessExited(_) | EventKind::ProcessCap(_) => {}
    }
    slots
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use serde_json::Value;
    use time::{Duration, OffsetDateTime};

    use super::*;
    use crate::fixture;
    use crate::{
        AppOutput, Component, ExitKind, HttpRejected, ManifestWarning, Outcome, PgRefuseReason,
        PgRefused, ProcessCap, ProcessExited, ProcessRetired, ProcessSpawned, RejectReason,
        RequestInfo, RetireReason, SqlDenied, Stream,
    };

    /// The keys of the top-level object, in order.
    ///
    /// `serde_json::Value` keeps no order without the `preserve_order`
    /// feature, which would add a dependency, so this walks the line.
    fn top_level_keys(json: &str) -> Vec<String> {
        let bytes = json.as_bytes();
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'{' | b'[' => {
                    depth += 1;
                    index += 1;
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    index += 1;
                }
                b'"' => {
                    let start = index + 1;
                    let mut end = start;
                    let mut escaped = false;
                    while end < bytes.len() {
                        match bytes[end] {
                            _ if escaped => escaped = false,
                            b'\\' => escaped = true,
                            b'"' => break,
                            _ => {}
                        }
                        end += 1;
                    }
                    if depth == 1 && bytes.get(end + 1) == Some(&b':') {
                        keys.push(json[start..end].to_owned());
                    }
                    index = end + 1;
                }
                _ => index += 1,
            }
        }
        keys
    }

    fn parse(json: &str) -> Value {
        serde_json::from_str(json).expect("valid JSON")
    }

    fn check_fits(_event: &Event, json: &str) -> Value {
        assert!(
            json.len() <= MAX_EVENT_BYTES,
            "line is {} bytes",
            json.len()
        );
        assert!(!json.contains('\n'), "line must not contain a newline");
        parse(json)
    }

    fn request_event() -> Event {
        fixture::request_event()
    }

    #[test]
    fn request_example_has_the_documented_values_and_key_order() {
        let json = encode_json(&request_event());
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "route",
                "request_id",
                "method",
                "path",
                "rule",
                "status",
                "outcome",
                "queued_ms",
                "duration_ms",
                "bytes_in",
                "bytes_out",
                "cost_ns",
                "cpu_ns",
                "remote_addr",
            ]
        );

        let value = check_fits(&request_event(), &json);
        assert_eq!(value["level"], "info");
        assert_eq!(value["event"], "request");
        assert_eq!(value["component"], "scheduler");
        assert_eq!(value["request_id"], "01JB9");
        assert_eq!(value["method"], "GET");
        assert_eq!(value["path"], "/products/42/");
        assert_eq!(value["route"], "product-detail");
        assert_eq!(value["rule"], 0);
        assert_eq!(value["status"], 200);
        assert_eq!(value["outcome"], "ok");
        assert_eq!(value["queued_ms"], 0.1);
        assert_eq!(value["duration_ms"], 18.4);
        assert_eq!(value["bytes_out"], 5120);
        // D5: what the request cost the view process, and the thread CPU time
        // it reported over the same span.
        assert_eq!(value["cost_ns"], 18_400_000);
        assert_eq!(value["cpu_ns"], 9_100_000);
        assert_eq!(value["remote_addr"], "203.0.113.7");
        // §8.1's example line omits `bytes_in`; the table lists it, so it is
        // always written.
        assert_eq!(value["bytes_in"], 0);
        assert_eq!(value["ts"], "2026-10-25T14:02:11.384Z");
    }

    #[test]
    fn sql_denied_example_has_the_documented_values_and_key_order() {
        let event = fixture::sql_denied_event();
        let json = encode_json(&event);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "route",
                "pid",
                "proxy",
                "kind",
                "reason",
                "sqlstate",
                "in_transaction",
                "sql",
            ]
        );

        let value = check_fits(&event, &json);
        assert_eq!(value["ts"], "2026-10-25T14:02:12.007Z");
        assert_eq!(value["level"], "warn");
        assert_eq!(value["event"], "sql.denied");
        assert_eq!(value["component"], "dbproxy");
        assert_eq!(value["route"], "product-detail");
        assert_eq!(value["pid"], 57);
        assert_eq!(value["proxy"], "db");
        assert_eq!(value["kind"], "parse");
        assert_eq!(value["sqlstate"], "42501");
        assert_eq!(value["reason"], "select on shop_product.price not granted");
        assert_eq!(value["in_transaction"], false);
        assert_eq!(
            value["sql"],
            r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?"#
        );
        assert!(value.get("request_id").is_none());
        assert!(value.get("truncated").is_none());
    }

    #[test]
    fn every_kind_encodes_to_valid_json() {
        let events = vec![
            request_event(),
            fixture::sql_denied_event(),
            fixture::event(EventKind::PgRefused(PgRefused::new(
                "db",
                PgRefuseReason::TooManyConnections,
            )))
            .with_route("product-detail"),
            fixture::event(EventKind::HttpRejected(HttpRejected::new(
                "203.0.113.7",
                431,
                RejectReason::HeaderTooLarge,
            ))),
            fixture::event(EventKind::ProcessSpawned(ProcessSpawned::new(
                57, 20_001, 12.5,
            )))
            .with_route("product-detail"),
            fixture::event(EventKind::ProcessExited(ProcessExited::new(
                57,
                20_001,
                ExitKind::Status(0),
            ))),
            fixture::event(EventKind::ProcessExited(ProcessExited::new(
                57,
                20_001,
                ExitKind::Signal(9),
            ))),
            fixture::event(EventKind::ProcessRetired(ProcessRetired::new(
                57,
                20_001,
                RetireReason::MemoryPressure,
            ))),
            fixture::event(EventKind::ProcessCap(ProcessCap::new(141_557_760, 4))),
            fixture::event(EventKind::AppOutput(AppOutput::new(
                Stream::Stderr,
                "boom\n",
            ))),
            fixture::event(EventKind::Boot(
                Lifecycle::default()
                    .with_stage("preload")
                    .with_duration_ms(12.5)
                    .with_output("db", "DATABASE_URL"),
            )),
            fixture::event(EventKind::Ready(
                Lifecycle::default().with_routes(3).with_workers(16),
            )),
            fixture::event(EventKind::Shutdown(
                Lifecycle::default()
                    .with_stage("drain")
                    .with_duration_ms(2.0),
            )),
            fixture::event(EventKind::ManifestWarning(ManifestWarning::new(
                "unattached group `web`",
            ))),
        ];

        for event in &events {
            let json = encode_json(event);
            let value = check_fits(event, &json);
            assert_eq!(value["event"], event.kind.name());
            assert_eq!(value["component"], event.component.as_str());
            assert_eq!(value["level"], event.level.as_str());
            assert!(value["ts"].is_string());
            let keys = top_level_keys(&json);
            assert_eq!(
                &keys[..4],
                ["ts", "level", "event", "component"],
                "common prefix for {}",
                event.kind.name()
            );
        }
    }

    #[test]
    fn variant_fields_are_written_in_the_documented_order() {
        let boot = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_routes(3)
                .with_workers(16)
                .with_duration_ms(1.0)
                .with_seccomp(false)
                .with_output("db", "DATABASE_URL"),
        ));
        let json = encode_json(&boot);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "routes",
                "workers",
                "duration_ms",
                "seccomp",
                "outputs",
                "preset",
            ]
        );
        let value = parse(&json);
        assert_eq!(value["seccomp"], serde_json::json!(false));
        assert_eq!(value["outputs"]["db"][0], "DATABASE_URL");

        // §8 Sizing and §9.2 #7: the preload phase of `boot` reports the
        // immortalisation and the memory the view cap was derived from.
        let preload = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_immortalize(true)
                .with_immortalized(1_610_300)
                .with_immortalize_ms(1785.4)
                .with_mem_total(1_073_741_824)
                .with_mem_used(203_423_744)
                .with_max_view_processes(22),
        ));
        let json = encode_json(&preload);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "immortalize",
                "immortalized",
                "immortalize_ms",
                "mem_total",
                "mem_used",
                "max_view_processes",
                "preset",
            ]
        );
        let value = parse(&json);
        assert_eq!(value["immortalize"], "on");
        assert_eq!(value["immortalized"], 1_610_300);
        assert_eq!(value["immortalize_ms"], 1785.4);
        assert_eq!(value["mem_total"], 1_073_741_824);
        assert_eq!(value["mem_used"], 203_423_744);
        assert_eq!(value["max_view_processes"], 22);
        assert!(value["immortalized"].is_u64(), "a count, not a float");
        assert!(value["mem_total"].is_u64(), "byte counts are integers");
        assert!(value["mem_used"].is_u64());
        assert!(value["max_view_processes"].is_u64());

        let output = fixture::event(EventKind::AppOutput(AppOutput::new(Stream::Stdout, "hi")));
        assert_eq!(
            top_level_keys(&encode_json(&output)),
            [
                "ts",
                "level",
                "event",
                "component",
                "stream",
                "line",
                "part",
                "parts"
            ]
        );

        let process = fixture::event(EventKind::ProcessExited(ProcessExited::new(
            57,
            20_001,
            ExitKind::Signal(9),
        )));
        assert_eq!(
            top_level_keys(&encode_json(&process)),
            ["ts", "level", "event", "component", "pid", "uid", "signal"]
        );
        assert!(parse(&encode_json(&process)).get("status").is_none());

        // §8 Sizing: the measured cost per view and the cap it bought.
        let cap = fixture::event(EventKind::ProcessCap(ProcessCap::new(141_557_760, 4)));
        let json = encode_json(&cap);
        assert_eq!(
            top_level_keys(&json),
            ["ts", "level", "event", "component", "estimate_bytes", "cap"]
        );
        let value = parse(&json);
        assert_eq!(value["estimate_bytes"], 141_557_760);
        assert_eq!(value["cap"], 4);
        assert!(value["estimate_bytes"].is_u64(), "byte counts are integers");
        assert!(value["cap"].is_u64());
    }

    #[test]
    fn optional_fields_are_omitted_but_route_is_null_for_unmatched_requests() {
        let unmatched = fixture::event(EventKind::Request(
            RequestInfo::from_target("GET", "/nope?x=1")
                .with_status(404)
                .with_outcome(Outcome::Unmatched),
        ));
        let json = encode_json(&unmatched);
        assert_eq!(
            top_level_keys(&json)[..5],
            ["ts", "level", "event", "component", "route"]
        );
        let value = parse(&json);
        assert!(value["route"].is_null());
        assert_eq!(value["path"], "/nope");
        assert!(value.get("request_id").is_none());
        assert!(value.get("rule").is_none());

        // Other kinds simply leave `route` out.
        let spawned = fixture::event(EventKind::ProcessSpawned(ProcessSpawned::new(1, 2, 3.0)));
        assert_eq!(
            top_level_keys(&encode_json(&spawned)),
            ["ts", "level", "event", "component", "pid", "uid", "fork_ms"]
        );

        let with_ids = request_event()
            .with_request_id("01JB9")
            .with_route("product-detail");
        let value = parse(&encode_json(&with_ids));
        assert_eq!(value["request_id"], "01JB9");
    }

    #[test]
    fn strings_are_escaped_and_the_line_stays_one_line() {
        let event = fixture::event(EventKind::ManifestWarning(ManifestWarning::new(
            "quote \" backslash \\ newline \n tab \t control \u{1} unicode é 漢",
        )));
        let json = encode_json(&event);
        assert!(!json.contains('\n'));
        assert!(json.contains("\\n"));
        assert!(json.contains("\\u0001"));
        let value = parse(&json);
        assert!(value["message"].as_str().unwrap().contains("é 漢"));
    }

    #[test]
    fn a_request_with_no_cost_report_still_has_the_fields() {
        // D5: `cost_ns` is always a number (it is what the scheduler observed
        // even when nothing reported), and `cpu_ns` is null when no view
        // measured it, so a reader never has to guess which happened.
        let event = fixture::event(EventKind::Request(
            RequestInfo::from_target("GET", "/a").with_cost(0, None),
        ));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["cost_ns"], 0);
        assert!(value["cpu_ns"].is_null());

        // ... and a report of a cost of zero is a report: `cpu_ns` is present.
        let event = fixture::event(EventKind::Request(
            RequestInfo::from_target("GET", "/a").with_cost(0, Some(0)),
        ));
        let value = check_fits(&event, &encode_json(&event));
        assert_eq!(value["cpu_ns"], 0);
    }

    #[test]
    fn non_finite_timings_stay_valid_json() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let event = fixture::event(EventKind::Request(
                RequestInfo::from_target("GET", "/a").with_timings(bad, bad),
            ));
            let json = encode_json(&event);
            let value = check_fits(&event, &json);
            assert!(value["queued_ms"].is_null());
            assert!(value["duration_ms"].is_null());
        }
        let event = fixture::event(EventKind::ProcessSpawned(ProcessSpawned::new(
            1,
            2,
            f64::MAX,
        )));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert!(value["fork_ms"].as_f64().unwrap().is_finite());
    }

    #[test]
    fn a_long_sql_is_cut_and_named_in_truncated() {
        let sql = "SELECT ".to_owned() + &"a".repeat(3000);
        let event = fixture::event(EventKind::SqlDenied(SqlDenied::new(
            57,
            "db",
            SqlKind::Query,
            sql.clone(),
            "42501",
            false,
        )))
        .with_route("product-detail");
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["sql"]));
        let kept = value["sql"].as_str().unwrap();
        assert!(sql.starts_with(kept), "truncation must keep the prefix");
        assert!(kept.len() < sql.len());
        // The rest of the event is untouched.
        assert_eq!(value["sqlstate"], "42501");
        assert_eq!(value["route"], "product-detail");
    }

    #[test]
    fn the_longest_field_is_cut_first() {
        let long_path = "/".to_owned() + &"p".repeat(4000);
        let event = fixture::event(EventKind::Request(
            RequestInfo::from_target("GET", &long_path)
                .with_status(200)
                .with_remote_addr("203.0.113.7"),
        ))
        .with_route("product-detail");
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["path"]));
        assert_eq!(value["remote_addr"], "203.0.113.7");
        assert_eq!(value["route"], "product-detail");
        assert!(long_path.starts_with(value["path"].as_str().unwrap()));
    }

    #[test]
    fn several_fields_can_be_cut() {
        let big = "x".repeat(3000);
        let event = fixture::event(EventKind::ManifestWarning(ManifestWarning::new(
            big.clone(),
        )))
        .with_route(big.clone())
        .with_request_id(big.clone());
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        let truncated = value["truncated"].as_array().unwrap();
        assert_eq!(truncated.len(), 3, "{truncated:?}");
        for name in ["message", "route", "request_id"] {
            assert!(truncated.iter().any(|v| v == name), "missing {name}");
        }
    }

    #[test]
    fn a_long_app_output_line_is_cut_too() {
        let line = "l".repeat(9000);
        let event = fixture::event(EventKind::AppOutput(AppOutput::new(
            Stream::Stdout,
            line.clone(),
        )));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["line"]));
        assert!(line.starts_with(value["line"].as_str().unwrap()));
    }

    #[test]
    fn a_long_stage_is_cut() {
        let event = fixture::event(EventKind::Ready(
            Lifecycle::default().with_stage("s".repeat(4000)),
        ));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["stage"]));
    }

    #[test]
    fn the_privilege_mode_and_its_capabilities_are_reported() {
        // D6: the `boot` event says `root` or `capabilities`, and in the
        // rootless case the set init holds.
        let root = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_seccomp(true)
                .with_privilege("root"),
        ));
        let json = encode_json(&root);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "seccomp",
                "privilege",
                "preset",
            ]
        );
        let value = parse(&json);
        assert_eq!(value["privilege"], "root");
        assert!(value.get("capabilities").is_none(), "{value}");

        let rootless = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_privilege("capabilities")
                .with_capability("cap_chown")
                .with_capability("cap_setuid"),
        ));
        let json = encode_json(&rootless);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "privilege",
                "capabilities",
                "preset",
            ]
        );
        let value = parse(&json);
        assert_eq!(value["privilege"], "capabilities");
        assert_eq!(
            value["capabilities"],
            serde_json::json!(["cap_chown", "cap_setuid"])
        );
    }

    #[test]
    fn an_over_long_capability_list_is_cut_and_named() {
        // A caller can put anything in the list; the line limit still holds,
        // and the names that went are in `truncated`.
        let mut lifecycle = Lifecycle::default().with_privilege("capabilities");
        for index in 0..400 {
            lifecycle = lifecycle.with_capability(format!("cap_{index}"));
        }
        let event = fixture::event(EventKind::Boot(lifecycle));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["capabilities"]));
        let kept = value["capabilities"].as_array().unwrap();
        assert!(!kept.is_empty());
        assert!(kept.len() < 400);
        assert_eq!(kept[0], "cap_0", "a prefix of the set survives");
    }

    #[test]
    fn a_skipped_immortalisation_is_reported_with_its_reason() {
        // §9.2 #7 on CPython 3.14: the pass is skipped, nothing is marked, and
        // the event carries the same sentence as the `warn` line.
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_immortalize(false)
                .with_immortalize_reason(
                    "CPython 3.14: heap immortalisation is unsafe on this version",
                )
                .with_immortalized(0),
        ));
        let json = encode_json(&event);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "immortalize",
                "immortalize_reason",
                "immortalized",
                "preset",
            ]
        );
        let value = check_fits(&event, &json);
        assert_eq!(value["immortalize"], "off");
        assert_eq!(value["immortalized"], 0);
        assert!(value["immortalized"].is_u64(), "a count, not a float");
        assert!(
            value["immortalize_reason"]
                .as_str()
                .unwrap()
                .contains("3.14")
        );
        // A skipped pass has no duration: `immortalize_ms` is the pass's own.
        assert!(value.get("immortalize_ms").is_none());
    }

    #[test]
    fn a_boot_event_names_the_preset_and_its_settings() {
        // E1: the preset, the settings it supplied and their values, and the
        // settings an explicit variable overrode — names only, because those
        // values came from the environment.
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("config")
                .with_preset("compose-traefik")
                .with_preset_setting("BLASTIT_CLIENT_IP_HEADER", "x-forwarded-for")
                .with_preset_setting("BLASTIT_FAIR_QUEUING", "on")
                .with_preset_override("BLASTIT_LOG_FORMAT"),
        ));
        let json = encode_json(&event);
        assert_eq!(
            top_level_keys(&json),
            [
                "ts",
                "level",
                "event",
                "component",
                "stage",
                "preset",
                "preset_settings",
                "preset_overridden",
            ]
        );
        let value = check_fits(&event, &json);
        assert_eq!(value["preset"], "compose-traefik");
        assert_eq!(
            value["preset_settings"],
            serde_json::json!({
                "BLASTIT_CLIENT_IP_HEADER": "x-forwarded-for",
                "BLASTIT_FAIR_QUEUING": "on",
            })
        );
        assert_eq!(
            value["preset_overridden"],
            serde_json::json!(["BLASTIT_LOG_FORMAT"])
        );
        // Never the overriding value, and never a value of the environment's.
        assert!(!json.contains("\"log_format\""));
    }

    #[test]
    fn a_boot_event_without_a_preset_says_null() {
        // E1: a `boot` line that says `"preset":null` is a boot with no preset,
        // which is a different statement from a line that never mentions one —
        // and `ready` and `shutdown`, which share the payload, stay quiet.
        let boot = fixture::event(EventKind::Boot(Lifecycle::default().with_stage("stage0")));
        let json = encode_json(&boot);
        assert_eq!(
            top_level_keys(&json),
            ["ts", "level", "event", "component", "stage", "preset"]
        );
        assert!(check_fits(&boot, &json)["preset"].is_null());

        for kind in [
            EventKind::Ready(Lifecycle::default().with_stage("serving")),
            EventKind::Shutdown(Lifecycle::default().with_stage("drain")),
        ] {
            let event = fixture::event(kind);
            let json = encode_json(&event);
            assert_eq!(
                top_level_keys(&json),
                ["ts", "level", "event", "component", "stage"]
            );
            assert!(!json.contains("preset"));
        }

        // A `ready` that does carry one reports it, for a reader that only kept
        // the last lifecycle line.
        let ready = fixture::event(EventKind::Ready(
            Lifecycle::default()
                .with_stage("serving")
                .with_preset("compose-traefik"),
        ));
        let value = check_fits(&ready, &encode_json(&ready));
        assert_eq!(value["preset"], "compose-traefik");
        assert!(value.get("preset_settings").is_none());
        assert!(value.get("preset_overridden").is_none());
    }

    #[test]
    fn a_long_preset_setting_is_cut_and_named() {
        // The preset tables are blastit's own, but the line limit has to hold
        // whatever they hold.
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_preset("compose-traefik")
                .with_preset_setting("BLASTIT_TRUSTED_PROXIES", "x".repeat(4000))
                .with_preset_override("y".repeat(4000)),
        ));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        let truncated = value["truncated"].as_array().unwrap();
        assert!(
            truncated.contains(&serde_json::json!("preset_settings")),
            "{value}"
        );
        assert!(
            truncated.contains(&serde_json::json!("preset_overridden")),
            "{value}"
        );
        assert!(value["preset"].is_string(), "the name stays: {value}");
    }

    #[test]
    fn a_long_immortalisation_reason_is_cut() {
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default().with_immortalize_reason("r".repeat(4000)),
        ));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(
            value["truncated"],
            serde_json::json!(["immortalize_reason"])
        );
        assert!(value["immortalize_reason"].as_str().unwrap().len() < 4000);
    }

    #[test]
    fn huge_escaped_and_unicode_input_stays_valid() {
        let inputs = [
            "\"".repeat(4000),
            "\\".repeat(4000),
            "\n".repeat(4000),
            "\u{1}".repeat(4000),
            "É".repeat(4000),
            "漢字".repeat(2000),
            "🦀".repeat(2000),
            format!("\u{0}{}", "a".repeat(4000)),
        ];
        for input in inputs {
            for event in [
                fixture::event(EventKind::ManifestWarning(ManifestWarning::new(
                    input.clone(),
                ))),
                fixture::event(EventKind::AppOutput(AppOutput::new(
                    Stream::Stderr,
                    input.clone(),
                ))),
                fixture::event(EventKind::Request(
                    RequestInfo::from_target("GET", &input).with_status(500),
                ))
                .with_request_id(input.clone()),
            ] {
                let json = encode_json(&event);
                let value = check_fits(&event, &json);
                if let Some(names) = value["truncated"].as_array() {
                    assert!(!names.is_empty());
                }
            }
        }
    }

    #[test]
    fn unrecognised_reasons_are_cut_as_reason() {
        let event = fixture::event(EventKind::PgRefused(PgRefused::new(
            "db",
            PgRefuseReason::Other("r".repeat(5000)),
        )));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["reason"]));
        assert!(value["reason"].as_str().unwrap().len() < 5000);
        assert_eq!(value["proxy"], "db");
    }

    #[test]
    fn many_outputs_lose_whole_proxies_until_the_line_fits() {
        let mut lifecycle = Lifecycle::default();
        for proxy in 0..500 {
            lifecycle = lifecycle
                .with_output(format!("proxy-{proxy:03}"), "VARIABLE_NAME_HERE")
                .with_output(format!("proxy-{proxy:03}"), "ANOTHER_VARIABLE");
        }
        let event = fixture::event(EventKind::Boot(lifecycle));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["outputs"]));
        let outputs = value["outputs"].as_object().unwrap();
        assert!(!outputs.is_empty());
        assert!(outputs.len() < 500);
        // Whole proxies only: every proxy that is still there is complete.
        for (proxy, names) in outputs {
            assert_eq!(
                names,
                &serde_json::json!(["VARIABLE_NAME_HERE", "ANOTHER_VARIABLE"]),
                "proxy {proxy} lost part of its variable list"
            );
        }
        // The surviving proxies are the first keys in BTreeMap order.
        assert!(outputs.contains_key("proxy-000"));
    }

    #[test]
    fn a_huge_outputs_map_of_empty_names_still_fits() {
        let mut lifecycle = Lifecycle::default();
        for proxy in 0..500 {
            lifecycle = lifecycle.with_output(format!("p{proxy:03}"), "");
        }
        let event = fixture::event(EventKind::Boot(lifecycle));
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(value["truncated"], serde_json::json!(["outputs"]));
        assert!(value["outputs"].as_object().unwrap().len() < 500);
    }

    #[test]
    fn declared_truncated_fields_are_reported_even_when_the_event_fits() {
        let event = fixture::request_event().with_truncated(["route", "request_id"]);
        let json = encode_json(&event);
        assert!(json.len() < MAX_EVENT_BYTES);
        let value = check_fits(&event, &json);
        assert_eq!(
            value["truncated"]
                .as_array()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [
                &serde_json::json!("route"),
                &serde_json::json!("request_id")
            ]
        );
        // A declared field is merged with the encoder's own cuts, once each.
        let long = "x".repeat(4000);
        let event = fixture::event(EventKind::ManifestWarning(ManifestWarning::new(long)))
            .with_route("product-detail")
            .with_truncated(["route"]);
        let json = encode_json(&event);
        let value = check_fits(&event, &json);
        assert_eq!(
            value["truncated"],
            serde_json::json!(["route", "message"]),
            "declared names come first, then the encoder's"
        );
        // No `truncated` member at all when nothing was cut.
        assert!(
            parse(&encode_json(&fixture::request_event()))
                .get("truncated")
                .is_none()
        );
    }

    #[test]
    fn truncation_is_a_property_over_generated_inputs() {
        // Deterministic generator: no rand, just a growing alphabet.
        let alphabet = ["a", "\"", "\\", "\n", "é", "漢", "\u{7}", "x"];
        let path = "p".to_owned();
        for round in 0..64usize {
            let mut text = String::new();
            for step in 0..(round * 37 % 3000) + 1 {
                text.push_str(alphabet[step % alphabet.len()]);
            }
            let events = [
                fixture::event(EventKind::SqlDenied(SqlDenied::new(
                    57,
                    "db",
                    SqlKind::Other("listen".to_owned()),
                    text.clone(),
                    "42501",
                    true,
                )))
                .with_route(text.clone()),
                fixture::event(EventKind::HttpRejected(HttpRejected::new(
                    text.clone(),
                    400,
                    RejectReason::Other(text.clone()),
                )))
                .with_request_id(text.clone()),
                fixture::event(EventKind::ProcessRetired(ProcessRetired::new(
                    57,
                    20_001,
                    RetireReason::Other(text.clone()),
                ))),
                fixture::event(EventKind::AppOutput(AppOutput::new(
                    Stream::Stdout,
                    text.clone(),
                )))
                .with_route(text.clone()),
                fixture::event(EventKind::ManifestWarning(ManifestWarning::new(
                    text.clone(),
                ))),
                fixture::event(EventKind::Ready(
                    Lifecycle::default()
                        .with_stage(&text)
                        .with_output(&text, &path)
                        .with_output(&text, &text),
                )),
            ];
            for event in &events {
                let json = encode_json(event);
                assert!(
                    json.len() <= MAX_EVENT_BYTES,
                    "round {round}: {} bytes for {}",
                    json.len(),
                    event.kind.name()
                );
                let value = parse(&json);
                assert_eq!(value["event"], event.kind.name());
                for name in value["truncated"].as_array().unwrap_or(&Vec::new()) {
                    let name = name.as_str().unwrap();
                    assert!(
                        value.get(name).is_some(),
                        "round {round}: {name} listed but absent"
                    );
                    if let Some(kept) = value[name].as_str() {
                        assert!(
                            text.starts_with(kept),
                            "round {round}: {name} kept a non-prefix"
                        );
                    }
                }
                // Four copies of `text`, each at most six times its length
                // once escaped, still fit comfortably.
                if text.len() <= 50 {
                    assert!(
                        value.get("truncated").is_none(),
                        "round {round}: cut too early"
                    );
                }
            }
        }
    }

    #[test]
    fn a_fitting_event_is_not_truncated() {
        let event = request_event();
        assert!(parse(&encode_json(&event)).get("truncated").is_none());
        assert_eq!(encode_json(&request_event()).len(), encoded_len(&event));
    }

    #[test]
    fn timestamps_of_any_offset_encode_as_utc() {
        let ts = OffsetDateTime::from_unix_timestamp(1_792_936_931).unwrap()
            + Duration::milliseconds(384);
        let event = fixture::event(EventKind::Ready(Lifecycle::default())).with_ts(ts);
        let value = parse(&encode_json(&event));
        assert_eq!(value["ts"], "2026-10-25T14:02:11.384Z");
        let offset = fixture::event(EventKind::Ready(Lifecycle::default()))
            .with_ts(ts.to_offset(time::UtcOffset::from_hms(-5, 0, 0).unwrap()));
        assert_eq!(
            parse(&encode_json(&offset))["ts"],
            "2026-10-25T14:02:11.384Z"
        );
    }

    #[test]
    fn top_level_keys_helper_ignores_nested_keys() {
        let json = r#"{"a":"x","b":{"c":"d"},"e":[{"f":"g"}],"h":1}"#;
        assert_eq!(top_level_keys(json), ["a", "b", "e", "h"]);
        let json = r#"{"a":"}","b":"\"x\",\"y\":"}"#;
        assert_eq!(top_level_keys(json), ["a", "b"]);
    }

    #[test]
    fn push_member_separates_members() {
        let mut out = String::from("{");
        push_member(&mut out, "a", "1");
        push_member(&mut out, "b", "\"x\"");
        out.push('}');
        assert_eq!(out, r#"{"a":1,"b":"x"}"#);
    }

    #[test]
    fn no_event_kind_encodes_with_an_empty_event_name() {
        let events = [request_event(), fixture::sql_denied_event()];
        for event in &events {
            assert!(!event.kind.name().is_empty());
        }
        assert_eq!(Component::Init.as_str(), "init");
    }
}
