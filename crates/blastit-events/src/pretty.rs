//! The pretty line encoder (ARCHITECTURE.md §8.1: `BLASTIT_LOG_FORMAT=pretty`).

use crate::{
    Event, EventKind, ExitKind, Level, Lifecycle, MAX_EVENT_BYTES, Outcome, RequestInfo,
    floor_boundary, format_clock_time,
};

const RESET: &str = "\u{1b}[0m";
/// Column the event name starts at.
const NAME_COLUMN: usize = 12;

/// Encode one event as a single human-readable line, without a trailing
/// newline.
///
/// The shape is `HH:MM:SS.mmm LEVEL event  summary`, with the event name
/// padded to twelve columns, as in the §8.1 example. `color` wraps the level
/// word in ANSI colours (a handful of escape codes, no crate); the padding is
/// applied to the text, so coloured and plain lines line up.
///
/// Control characters from the event are escaped, so the result is always
/// exactly one line, and a line longer than [`MAX_EVENT_BYTES`] is cut and
/// ends with `…`.
pub fn encode_pretty(event: &Event, color: bool) -> String {
    let mut out = String::with_capacity(128);
    out.push_str(&format_clock_time(event.ts));
    out.push(' ');
    out.push_str(&level_word(event.level, color));
    out.push(' ');
    let name = event.kind.name();
    out.push_str(name);
    // Twelve columns, and always at least one space before the summary so a
    // long event name such as `manifest.warning` stays readable.
    let width = NAME_COLUMN.max(name.chars().count() + 1);
    for _ in name.chars().count()..width {
        out.push(' ');
    }
    out.push_str(&summary(event));
    cap(&mut out);
    out
}

/// The level word, padded to five columns and coloured when asked.
fn level_word(level: Level, color: bool) -> String {
    let mut word = format!("{:<5}", level.as_str().to_uppercase());
    if color {
        let code = match level {
            Level::Debug => "36",
            Level::Info => "32",
            Level::Warn => "33",
            Level::Error => "31",
        };
        word = format!("\u{1b}[{code}m{word}{RESET}");
    }
    word
}

fn summary(event: &Event) -> String {
    let route = || match event.route.as_deref() {
        Some(route) => escape_inline(route),
        None => "-".to_owned(),
    };
    match &event.kind {
        EventKind::Request(info) => request_summary(event, info),
        EventKind::SqlDenied(info) => format!(
            "{}  {}  {}  {}",
            route(),
            escape_inline(&info.proxy),
            escape_inline(info.kind.as_str()),
            escape_inline(&info.sql)
        ),
        EventKind::PgRefused(info) => format!(
            "{}  {}  {}",
            route(),
            escape_inline(&info.proxy),
            escape_inline(info.reason.as_str())
        ),
        EventKind::HttpRejected(info) => format!(
            "{}  {}  {}",
            escape_inline(&info.remote_addr),
            info.status,
            escape_inline(info.reason.as_str())
        ),
        EventKind::ProcessSpawned(info) => format!(
            "{}  pid={} uid={}  fork={:.1}ms",
            route(),
            info.pid,
            info.uid,
            info.fork_ms
        ),
        EventKind::ProcessExited(info) => {
            let ending = match info.exit {
                ExitKind::Status(status) => format!("status={status}"),
                ExitKind::Signal(signal) => format!("signal={signal}"),
            };
            format!("{}  pid={} uid={}  {ending}", route(), info.pid, info.uid)
        }
        EventKind::ProcessRetired(info) => format!(
            "{}  pid={} uid={}  reason={}",
            route(),
            info.pid,
            info.uid,
            escape_inline(info.reason.as_str())
        ),
        EventKind::AppOutput(output) => {
            let mut out = escape_inline(output.stream.as_str());
            if output.parts > 1 {
                out.push_str(&format!(" [{}/{}]", output.part, output.parts));
            }
            out.push_str("  ");
            out.push_str(&escape_inline(&output.line));
            out
        }
        EventKind::Boot(lifecycle)
        | EventKind::Ready(lifecycle)
        | EventKind::Shutdown(lifecycle) => lifecycle_summary(lifecycle),
        EventKind::ManifestWarning(info) => escape_inline(&info.message),
    }
}

fn request_summary(event: &Event, info: &RequestInfo) -> String {
    let target = match event.route.as_deref() {
        Some(route) => escape_inline(route),
        None => "(unmatched)".to_owned(),
    };
    let mut out = format!(
        "{} {} → {}  {}",
        escape_inline(&info.method),
        escape_inline(&info.path),
        target,
        info.status
    );
    if info.outcome != Outcome::Ok {
        out.push(' ');
        out.push_str(info.outcome.as_str());
    }
    out.push_str(&format!("  {:.1}ms", info.duration_ms));
    out
}

fn lifecycle_summary(lifecycle: &Lifecycle) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(stage) = &lifecycle.stage {
        parts.push(escape_inline(stage));
    }
    if let Some(routes) = lifecycle.routes {
        parts.push(format!("routes={routes}"));
    }
    if let Some(workers) = lifecycle.workers {
        parts.push(format!("workers={workers}"));
    }
    if let Some(duration_ms) = lifecycle.duration_ms {
        parts.push(format!("{duration_ms:.1}ms"));
    }
    if let Some(seccomp) = lifecycle.seccomp {
        parts.push(format!("seccomp={}", if seccomp { "on" } else { "off" }));
    }
    if let Some(immortalized) = lifecycle.immortalized {
        parts.push(format!("immortalized={immortalized}"));
    }
    if let Some(immortalize_ms) = lifecycle.immortalize_ms {
        parts.push(format!("immortalize={immortalize_ms:.1}ms"));
    }
    if let Some(mem_total) = lifecycle.mem_total {
        parts.push(format!("mem_total={}", mebibytes(mem_total)));
    }
    if let Some(mem_used) = lifecycle.mem_used {
        parts.push(format!("mem_used={}", mebibytes(mem_used)));
    }
    if let Some(max_view_processes) = lifecycle.max_view_processes {
        parts.push(format!("max_view_processes={max_view_processes}"));
    }
    if !lifecycle.outputs.is_empty() {
        let mut rendered: Vec<String> = Vec::new();
        for (proxy, names) in &lifecycle.outputs {
            let names: Vec<String> = names.iter().map(|name| escape_inline(name)).collect();
            rendered.push(format!("{}=[{}]", escape_inline(proxy), names.join(", ")));
        }
        parts.push(format!("outputs: {}", rendered.join(", ")));
    }
    parts.join("  ")
}

/// Bytes as the nearest mebibyte, for the human format's memory fields.
fn mebibytes(bytes: u64) -> String {
    format!("{:.0}MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// Escape the control characters of text coming from an event, so that one
/// event stays one line and a view cannot redraw the operator's terminal.
fn escape_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                out.push_str(&format!("\\u{{{:x}}}", other as u32));
            }
            other => out.push(other),
        }
    }
    out
}

/// Cut an over-long line on a character boundary and mark it with `…`.
fn cap(line: &mut String) {
    if line.len() <= MAX_EVENT_BYTES {
        return;
    }
    let keep = floor_boundary(line, MAX_EVENT_BYTES - '…'.len_utf8());
    line.truncate(keep);
    line.push('…');
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::{
        AppOutput, ExitKind, Lifecycle, ManifestWarning, Outcome, ProcessExited, ProcessRetired,
        ProcessSpawned, RequestInfo, RetireReason, Stream,
    };

    fn plain(event: &Event) -> String {
        encode_pretty(event, false)
    }

    #[test]
    fn request_example_matches_the_text_form() {
        let line = plain(&fixture::request_event());
        assert_eq!(
            line,
            "14:02:11.384 INFO  request     GET /products/42/ → product-detail  200  18.4ms"
        );
    }

    #[test]
    fn sql_denied_example_matches_the_text_form() {
        let line = plain(&fixture::sql_denied_event());
        assert_eq!(
            line,
            concat!(
                "14:02:12.007 WARN  sql.denied  product-detail  db  parse  ",
                r#"SELECT "shop_product"."price" FROM "shop_product" WHERE "shop_product"."id" = $?"#
            )
        );
    }

    #[test]
    fn every_kind_renders_one_line() {
        let events = [
            fixture::request_event(),
            fixture::sql_denied_event(),
            fixture::event(EventKind::HttpRejected(crate::HttpRejected::new(
                "203.0.113.7",
                400,
                crate::RejectReason::MalformedRequest,
            ))),
            fixture::event(EventKind::ProcessSpawned(ProcessSpawned::new(
                57, 20_001, 12.5,
            ))),
            fixture::event(EventKind::ProcessExited(ProcessExited::new(
                57,
                20_001,
                ExitKind::Signal(9),
            ))),
            fixture::event(EventKind::ProcessExited(ProcessExited::new(
                57,
                20_001,
                ExitKind::Status(0),
            ))),
            fixture::event(EventKind::ProcessRetired(ProcessRetired::new(
                57,
                20_001,
                RetireReason::Idle,
            ))),
            fixture::event(EventKind::AppOutput(AppOutput::new(Stream::Stdout, "hi"))),
            fixture::event(EventKind::AppOutput(
                AppOutput::new(Stream::Stderr, "hi")
                    .with_part(2, 3)
                    .unwrap(),
            )),
            fixture::event(EventKind::Boot(
                Lifecycle::default()
                    .with_stage("preload")
                    .with_routes(3)
                    .with_workers(16)
                    .with_duration_ms(12.5)
                    .with_output("db", "DATABASE_URL"),
            )),
            fixture::event(EventKind::Ready(Lifecycle::default())),
            fixture::event(EventKind::Shutdown(Lifecycle::default())),
            fixture::event(EventKind::ManifestWarning(ManifestWarning::new("oops"))),
        ];
        for event in &events {
            for color in [false, true] {
                let line = encode_pretty(event, color);
                assert!(!line.contains('\n'), "{} is multi-line", event.kind.name());
                assert!(!line.is_empty());
                assert!(line.len() <= MAX_EVENT_BYTES);
                assert!(line.contains(event.kind.name()));
            }
        }
    }

    #[test]
    fn unmatched_requests_say_so() {
        let event = fixture::event(EventKind::Request(
            RequestInfo::from_target("GET", "/nope")
                .with_status(404)
                .with_outcome(Outcome::Unmatched),
        ));
        let line = plain(&event);
        assert!(
            line.contains("GET /nope → (unmatched)  404 unmatched"),
            "{line}"
        );
    }

    #[test]
    fn control_characters_never_reach_the_terminal_raw() {
        let event = fixture::event(EventKind::AppOutput(AppOutput::new(
            Stream::Stderr,
            "oops\nsecond line\rrest\ttab\u{1}\u{1b}[31mred",
        )));
        for color in [false, true] {
            let line = encode_pretty(&event, color);
            assert!(!line.contains('\n'));
            assert!(!line.contains('\r'));
            assert!(!line.contains('\t'));
            assert!(line.contains("oops\\nsecond line\\rrest\\ttab\\u{1}"));
            if !color {
                assert!(!line.contains('\u{1b}'), "no escapes without colour");
            }
        }
    }

    #[test]
    fn colour_wraps_only_the_level_word() {
        let event = fixture::request_event();
        let plain_line = plain(&event);
        let coloured = encode_pretty(&event, true);
        assert!(coloured.contains("\u{1b}[32mINFO \u{1b}[0m"));
        assert!(!coloured.contains("\u{1b}[0m\u{1b}[0m"));
        let stripped = coloured.replace("\u{1b}[32m", "").replace("\u{1b}[0m", "");
        assert_eq!(stripped, plain_line);
        assert_eq!(coloured.len(), plain_line.len() + 9);

        assert!(encode_pretty(&event.clone().with_level(Level::Warn), true).contains("\u{1b}[33m"));
        assert!(
            encode_pretty(&event.clone().with_level(Level::Error), true).contains("\u{1b}[31m")
        );
        assert!(encode_pretty(&event.with_level(Level::Debug), true).contains("\u{1b}[36m"));
    }

    #[test]
    fn long_lines_are_capped_with_an_ellipsis() {
        let event = fixture::event(EventKind::AppOutput(AppOutput::new(
            Stream::Stdout,
            "x".repeat(9000),
        )));
        let line = plain(&event);
        assert_eq!(line.len(), MAX_EVENT_BYTES);
        assert!(line.ends_with('…'));

        let unicode = fixture::event(EventKind::AppOutput(AppOutput::new(
            Stream::Stdout,
            "漢".repeat(4000),
        )));
        let line = plain(&unicode);
        assert!(line.len() <= MAX_EVENT_BYTES);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn name_padding_lines_up_short_names() {
        let ready = plain(&fixture::event(EventKind::Ready(Lifecycle::default())));
        assert!(
            ready.starts_with("14:02:11.384 INFO  ready       "),
            "{ready}"
        );
        let warning = plain(&fixture::event(EventKind::ManifestWarning(
            ManifestWarning::new("x"),
        )));
        assert!(
            warning.starts_with("14:02:11.384 WARN  manifest.warning x"),
            "{warning}"
        );
    }

    #[test]
    fn lifecycle_shows_variable_names_never_values() {
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_seccomp(true)
                .with_output("db", "DATABASE_URL")
                .with_output("db", "PGHOST"),
        ));
        let line = plain(&event);
        assert!(line.contains("preload"), "{line}");
        assert!(line.contains("seccomp=on"), "{line}");
        assert!(
            line.contains("outputs: db=[DATABASE_URL, PGHOST]"),
            "{line}"
        );
    }

    #[test]
    fn escape_inline_leaves_printable_text_alone() {
        assert_eq!(escape_inline("plain é 漢"), "plain é 漢");
        assert_eq!(escape_inline(""), "");
        assert_eq!(escape_inline("\u{7f}"), "\\u{7f}");
    }
}
