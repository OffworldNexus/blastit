//! The log collector: pipes in, one event stream out (§8.1).
//!
//! init is the **only** process that writes to stdout, so lines never
//! interleave. Everything else arrives over pipes:
//!
//! * http, scheduler and dbproxy send structured events to a dedicated events
//!   pipe — one JSON object per line, written with a single `write` so a line
//!   is never split by a peer (≤ 2048 bytes is well under `PIPE_BUF`).
//! * every child's stdout and stderr go to their own pipe, and init wraps each
//!   complete line in an `app.output` event.
//!
//! [`Emitter`] stamps `component` — and `route`, where the pipe implies one —
//! from *which pipe* the data came from, so a compromised view cannot forge
//! another component's events: the most it can do is print lines labelled as
//! its own app output.

use std::os::fd::{BorrowedFd, RawFd};

use blastit_events::{
    AppOutput, Component, Event, EventKind, Level, ManifestWarning, Stream, encode_json,
    encode_pretty, split_app_output,
};
use serde_json::{Map, Value};

use crate::config::LogFormat;

/// Largest partial line init will hold before flushing it as it is (§8.1).
///
/// DigitalOcean truncates forwarded lines at 2048 bytes, and a line with no
/// newline at all must not grow without bound: past this, the bytes are logged
/// as a line of their own.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Which pipe a line came from, which is what the event is stamped with (§8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pipe {
    /// An events pipe: already a structured event, stamped with `component`.
    Events {
        /// The process the pipe belongs to.
        component: Component,
    },
    /// A stdout pipe: a line of app output.
    Stdout {
        /// The process the pipe belongs to.
        component: Component,
        /// The route, when the pipe belongs to a view process.
        route: Option<String>,
    },
    /// A stderr pipe: a line of app output, logged at `warn`.
    Stderr {
        /// The process the pipe belongs to.
        component: Component,
        /// The route, when the pipe belongs to a view process.
        route: Option<String>,
    },
}

impl Pipe {
    /// The component this pipe's data belongs to.
    pub fn component(&self) -> Component {
        match self {
            Self::Events { component }
            | Self::Stdout { component, .. }
            | Self::Stderr { component, .. } => *component,
        }
    }

    /// The route this pipe's data belongs to, if the pipe implies one.
    pub fn route(&self) -> Option<&str> {
        match self {
            Self::Stdout { route, .. } | Self::Stderr { route, .. } => route.as_deref(),
            Self::Events { .. } => None,
        }
    }

    /// Whether this pipe carries structured events rather than app output.
    pub fn is_events(&self) -> bool {
        matches!(self, Self::Events { .. })
    }
}

/// The single writer of blastit's event stream (§8.1).
///
/// It owns a descriptor (stdout in production), the format and the minimum
/// level, and it never blocks on anything but that descriptor.
#[derive(Debug)]
pub struct Emitter {
    fd: RawFd,
    format: LogFormat,
    level: Level,
    color: bool,
}

impl Emitter {
    /// An emitter writing to `fd`, with the given format and minimum level.
    ///
    /// `color` is only consulted in [`LogFormat::Pretty`].
    pub fn new(fd: RawFd, format: LogFormat, level: Level, color: bool) -> Self {
        Self {
            fd,
            format,
            level,
            color,
        }
    }

    /// An emitter writing to stdout, coloured only when stdout is a terminal.
    pub fn stdout(format: LogFormat, level: Level) -> Self {
        // SAFETY: `isatty` takes a descriptor number and cannot fail.
        let color = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;
        Self::new(libc::STDOUT_FILENO, format, level, color)
    }

    /// Prints one of init's own events, if it passes the level filter.
    pub fn emit(&self, event: &Event) {
        if event.level() < self.level {
            return;
        }
        let line = match self.format {
            LogFormat::Json => encode_json(event),
            LogFormat::Pretty => encode_pretty(event, self.color),
        };
        self.write_line(&line);
    }

    /// Wraps one complete line of a child's output in `app.output` events and
    /// prints them (§8.1).
    pub fn emit_output(&self, pipe: &Pipe, line: &str) {
        let (component, stream) = match pipe {
            Pipe::Stdout { component, .. } => (*component, Stream::Stdout),
            Pipe::Stderr { component, .. } => (*component, Stream::Stderr),
            // Not app output; the caller should have used `forward`.
            Pipe::Events { component } => (*component, Stream::Stdout),
        };
        for event in split_app_output(
            component,
            pipe.route().map(str::to_string),
            None,
            stream,
            line,
        ) {
            if event.level() >= self.level {
                self.emit(&event);
            }
        }
    }

    /// Forwards one line that arrived on an *events* pipe (§8.1).
    ///
    /// The line is parsed as a JSON object, its `component` is overwritten from
    /// the pipe it came from, and its `route` too when the pipe implies one. An
    /// event already logged below the minimum level is dropped whole. A line
    /// that is not a JSON object is dropped with a warning event that carries
    /// no part of it: the line could contain anything, including a credential.
    pub fn forward(&self, pipe: &Pipe, line: &str) {
        let component = pipe.component();
        let mut value = match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(object)) => object,
            _ => {
                self.emit(&Event::now(
                    Component::Init,
                    EventKind::ManifestWarning(ManifestWarning::new(format!(
                        "{component}: dropped a line on the events pipe that is not a JSON object"
                    ))),
                ));
                return;
            }
        };
        value.insert("component".to_string(), Value::from(component.as_str()));
        if let Some(route) = pipe.route() {
            value.insert("route".to_string(), Value::from(route));
        }
        if !self.wants(&value) {
            return;
        }
        match self.format {
            LogFormat::Json => {
                // `serde_json` only fails on a float that is not representable
                // in JSON, and none of the values here can be one.
                if let Ok(text) = serde_json::to_string(&value) {
                    self.write_line(&text);
                }
            }
            LogFormat::Pretty => self.write_line(&pretty_line(&value, self.color)),
        }
    }

    /// Whether a forwarded event's `level` passes the filter.
    ///
    /// An event with no readable `level` is printed: it is more useful to see a
    /// malformed line than to lose it, and the level field is only compared,
    /// never interpreted.
    fn wants(&self, object: &Map<String, Value>) -> bool {
        match object.get("level").and_then(Value::as_str) {
            Some(text) => match text.parse::<Level>() {
                Ok(level) => level >= self.level,
                Err(_) => true,
            },
            None => true,
        }
    }

    /// Writes one line, with its newline, in a single `write`.
    fn write_line(&self, line: &str) {
        write_line(self.fd, line);
    }
}

/// Writes one line, with its newline, to `fd` in a single `write`.
///
/// This is how every child sends an event to init: one `write` per line means
/// lines never interleave, and a line of at most [`blastit_events::MAX_EVENT_BYTES`]
/// is well under `PIPE_BUF`, so it arrives whole.
///
/// A failure is swallowed: the pipe is gone, or the process is being torn down,
/// and losing a log line must never stop the server. `EINTR` is retried.
pub fn write_line(fd: RawFd, line: &str) {
    // SAFETY: the caller passes the descriptor it wants written to; nothing
    // here takes ownership of it, and it stays open for the caller's lifetime.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let mut written = 0;
    while written < bytes.len() {
        match rustix::io::write(borrowed, &bytes[written..]) {
            Ok(0) => return,
            Ok(count) => written += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return,
        }
    }
}

/// The pretty line for a forwarded event (§8.1).
///
/// Every event a component emits has a human form, and that form is written
/// once — by [`blastit_events::encode_pretty`], which is what init's *own*
/// events are rendered with — so a forwarded line is read back into the event
/// it came from and rendered the same way. A line this build cannot read (an
/// `event` name it does not know, a member that is not what the encoder writes)
/// falls back to [`render_pretty`]'s field dump, which is what every event used
/// to get: nothing is lost, the fields are all there in the order they arrived.
fn pretty_line(value: &Map<String, Value>, color: bool) -> String {
    match serde_json::to_string(value)
        .ok()
        .and_then(|line| blastit_events::decode_json(&line))
    {
        Some(event) => blastit_events::encode_pretty(&event, color),
        None => render_pretty(value, color),
    }
}

/// `HH:MM:SS.mmm LEVEL event key=value …`: the field dump a forwarded line
/// that cannot be read back into an event gets.
///
/// It is the fallback of [`pretty_line`], for a line this build does not know:
/// an `event` a newer component emitted, or members that are not the ones the
/// encoder writes. Showing every field, in the order they arrived, is more
/// useful than dropping the line — and the timestamp is the event's own, so the
/// line keeps the time the component stamped it with.
pub fn render_pretty(value: &Map<String, Value>, color: bool) -> String {
    let clock = value
        .get("ts")
        .and_then(Value::as_str)
        .and_then(clock_of)
        .unwrap_or_else(|| "00:00:00.000".to_string());
    let level = value
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("info")
        .to_ascii_uppercase();
    let event = blastit_events::escape_inline(
        value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("event"),
    );
    let mut line = String::with_capacity(128);
    if color {
        line.push_str(colour_of(&level));
    }
    line.push_str(&clock);
    line.push(' ');
    // The level and the event name come from the line too, so they are escaped
    // for printing the same way every value is: only the colour decision reads
    // the raw level.
    line.push_str(&format!("{:<6}", blastit_events::escape_inline(&level)));
    if color {
        line.push_str("\u{1b}[0m");
        line.push_str("\u{1b}[1m");
    }
    line.push_str(&format!("{event:<12}"));
    if color {
        line.push_str("\u{1b}[0m");
    }
    for (key, item) in value {
        if matches!(key.as_str(), "ts" | "level" | "event") {
            continue;
        }
        line.push(' ');
        line.push_str(&blastit_events::escape_inline(key));
        line.push('=');
        line.push_str(&render_value(item));
    }
    line
}

/// `14:02:11.384` out of `2026-10-25T14:02:11.384Z`.
fn clock_of(ts: &str) -> Option<String> {
    let (_, rest) = ts.split_once('T')?;
    let clock = rest.get(..12)?;
    if clock.as_bytes().get(2) == Some(&b':') {
        Some(clock.to_string())
    } else {
        None
    }
}

/// The ANSI colour a level is printed in.
fn colour_of(level: &str) -> &'static str {
    match level {
        "DEBUG" => "\u{1b}[2m",
        "WARN" => "\u{1b}[33m",
        "ERROR" => "\u{1b}[31m",
        _ => "\u{1b}[32m",
    }
}

/// One JSON value as it appears in a `key=value` pair.
///
/// Every value here comes from a line a component wrote, which means it can
/// carry application data — a path, a header, a statement, a line of app output.
/// A control character in one of them is escaped, exactly as the human form
/// escapes it: a raw `\n` would make one event look like two lines, and a raw
/// `\u{1b}[2J` would let a view redraw the operator's terminal (or hide what it
/// wrote from whatever reads the log).
fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => blastit_events::escape_inline(text),
        Value::Array(items) => items.iter().map(render_value).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// A bounded line assembler for one pipe (§8.1's line size rules).
///
/// Bytes are appended until a newline ends a line; a line that grows past
/// [`MAX_LINE_BYTES`] without one is flushed as it is, so a child cannot make
/// init allocate without bound.
#[derive(Debug)]
pub struct LineBuffer {
    bytes: Vec<u8>,
}

impl LineBuffer {
    /// An empty buffer.
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(1024),
        }
    }

    /// Appends `chunk`, pushing every complete line into `lines`.
    ///
    /// `lines` is not cleared here: the caller decides when to empty it.
    pub fn push(&mut self, chunk: &[u8], lines: &mut Vec<Vec<u8>>) {
        self.bytes.extend_from_slice(chunk);
        loop {
            if let Some(end) = self.bytes.iter().position(|byte| *byte == b'\n') {
                let mut line: Vec<u8> = self.bytes.drain(..=end).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                lines.push(line);
                continue;
            }
            if self.bytes.len() >= MAX_LINE_BYTES {
                lines.push(std::mem::take(&mut self.bytes));
                continue;
            }
            return;
        }
    }

    /// Pushes whatever is left, as a final line, when the pipe reached EOF.
    pub fn flush(&mut self, lines: &mut Vec<Vec<u8>>) {
        if self.bytes.is_empty() {
            return;
        }
        let mut line = std::mem::take(&mut self.bytes);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        lines.push(line);
    }
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// One line of app output as a string, lossily decoded: §8.1 logs text, and a
/// child's output is not guaranteed to be UTF-8.
pub fn lossy(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}

/// Builds an `app.output` event for one line, for tests and for init's own
/// failures.
pub fn output_event(
    component: Component,
    route: Option<&str>,
    stream: Stream,
    line: &str,
) -> Event {
    let event = Event::now(
        component,
        EventKind::AppOutput(AppOutput::new(stream, line.to_string())),
    );
    match route {
        Some(route) => event.with_route(route.to_string()),
        None => event,
    }
}

/// Prints `message` as error-level `app.output` events, one per line, and exits
/// non-zero (§4: a half-started sandbox is worse than a crash loop).
///
/// This uses `_exit`, not `std::process::exit`: init may be running with CPython
/// initialised and the GIL held, and running libc's `atexit` handlers there can
/// deadlock.
pub fn fail(emitter: &Emitter, message: &str) -> ! {
    if message.is_empty() {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, "fatal").with_level(Level::Error),
        );
    }
    for line in message.lines() {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, line).with_level(Level::Error),
        );
    }
    // SAFETY: `_exit` never returns and runs no handler.
    unsafe { libc::_exit(1) }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Read;
    use std::os::fd::AsRawFd;

    use super::*;

    /// Collects what an emitter writes to a pipe.
    fn capture(format: LogFormat, level: Level, body: impl FnOnce(&Emitter)) -> String {
        let (read_end, write_end) = blastit_sys::pipe().unwrap();
        let emitter = Emitter::new(write_end.as_raw_fd(), format, level, false);
        body(&emitter);
        drop(write_end);
        let mut file = std::fs::File::from(read_end);
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    fn info_event(name: &str) -> Event {
        Event::now(
            Component::Scheduler,
            EventKind::ManifestWarning(ManifestWarning::new(name.to_string())),
        )
        .with_level(Level::Info)
    }

    #[test]
    fn a_forwarded_line_this_build_cannot_read_is_escaped_before_it_is_printed() {
        // The fallback renders whatever the line holds, and what a line holds
        // can be an application's own text: a newline would make one event look
        // like two, and `\u{1b}[2J` would clear the operator's terminal — or hide
        // what a view wrote from whatever reads the log.
        let text = capture(LogFormat::Pretty, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::View,
                },
                "{\"ts\":\"2026-10-25T14:02:11.384Z\",\"level\":\"info\",\
                 \"event\":\"something.new\",\"line\":\"first\\nsecond\\u001b[2Jthird\",\
                 \"path\":\"/a\\u0007b\"}",
            );
        });
        assert_eq!(text.lines().count(), 1, "one line, always: {text:?}");
        assert!(
            text.contains(r"first\nsecond\u{1b}[2Jthird"),
            "the escapes are spelled out: {text:?}"
        );
        assert!(
            text.contains(r"path=/a\u{7}b"),
            "and every value is escaped, not just the one: {text:?}"
        );
        let line = text.strip_suffix('\n').unwrap_or(&text);
        assert!(
            !line.chars().any(char::is_control),
            "no control character survives: {line:?}"
        );
    }

    #[test]
    fn a_forwarded_line_that_is_not_an_event_is_escaped_in_the_dump() {
        // The event name and the level come from the line too, and a line this
        // build does not know goes through the fallback as it is.
        let text = capture(LogFormat::Pretty, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::View,
                },
                "{\"ts\":\"2026-10-25T14:02:11.384Z\",\"level\":\"warn\\u001b[2J\",\
                 \"event\":\"a\\nb\"}",
            );
        });
        assert_eq!(text.lines().count(), 1, "{text:?}");
        assert!(text.contains(r"a\nb"), "{text:?}");
        assert!(text.contains(r"WARN\u{1b}[2J"), "{text:?}");
    }

    #[test]
    fn json_lines_end_with_one_newline() {
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.emit(&info_event("one"));
            emitter.emit(&info_event("two"));
        });
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert!(serde_json::from_str::<Value>(line).is_ok(), "{line}");
            assert!(!line.contains('\n'));
        }
    }

    #[test]
    fn the_level_filter_drops_quiet_events() {
        let text = capture(LogFormat::Json, Level::Warn, |emitter| {
            emitter.emit(&info_event("dropped"));
        });
        assert!(text.is_empty(), "{text}");
    }

    #[test]
    fn forwarded_lines_are_restamped_from_the_pipe() {
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::Http,
                },
                r#"{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"http.rejected","component":"view","route":"forge"}"#,
            );
        });
        let value: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(value["component"], "http");
        assert_eq!(
            value["route"], "forge",
            "an events pipe does not imply a route, so the payload keeps it"
        );
    }

    #[test]
    fn a_view_output_pipe_rewrites_the_route() {
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Stdout {
                    component: Component::View,
                    route: Some("one".to_string()),
                },
                r#"{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"x","component":"dbproxy","route":"other"}"#,
            );
        });
        let value: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(
            value["component"], "view",
            "the pipe decides the component, not the payload"
        );
        assert_eq!(
            value["route"], "one",
            "a view's own pipe also decides the route"
        );
    }

    #[test]
    fn an_unparseable_events_line_is_dropped_with_a_warning() {
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::Dbproxy,
                },
                "not json at all",
            );
        });
        let value: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(value["level"], "warn");
        assert_eq!(value["event"], "manifest.warning");
        assert!(!text.contains("not json at all"), "{text}");
    }

    #[test]
    fn a_quiet_forwarded_event_is_dropped_whole() {
        let text = capture(LogFormat::Json, Level::Warn, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::Http,
                },
                r#"{"ts":"t","level":"info","event":"x"}"#,
            );
        });
        assert!(text.is_empty(), "{text}");
    }

    #[test]
    fn pretty_forwarded_lines_carry_a_clock_and_the_fields() {
        let text = capture(LogFormat::Pretty, Level::Info, |emitter| {
            emitter.forward(
                &Pipe::Events {
                    component: Component::Scheduler,
                },
                r#"{"ts":"2026-10-25T14:02:11.384Z","level":"warn","event":"sql.denied","proxy":"db","route":"about"}"#,
            );
        });
        assert!(text.starts_with("14:02:11.384 WARN  sql.denied"), "{text}");
        assert!(text.contains("proxy=db"), "{text}");
        assert!(text.contains("route=about"), "{text}");
        assert!(!text.contains('\u{1b}'), "colour is off off a terminal");
    }

    #[test]
    fn output_lines_are_split_into_app_output_events() {
        let long = "é".repeat(2000);
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.emit_output(
                &Pipe::Stdout {
                    component: Component::View,
                    route: Some("about".to_string()),
                },
                &long,
            );
        });
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() > 1, "a long line is split");
        for line in &lines {
            assert!(
                line.len() <= blastit_events::MAX_EVENT_BYTES,
                "{}",
                line.len()
            );
            let value: Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["event"], "app.output");
            assert_eq!(value["component"], "view");
            assert_eq!(value["route"], "about");
        }
    }

    #[test]
    fn a_stderr_line_is_a_warning() {
        let text = capture(LogFormat::Json, Level::Info, |emitter| {
            emitter.emit_output(
                &Pipe::Stderr {
                    component: Component::Http,
                    route: None,
                },
                "boom",
            );
        });
        let value: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(value["level"], "warn");
        assert_eq!(value["stream"], "stderr");
        assert_eq!(value["line"], "boom");
    }

    #[test]
    fn line_buffer_yields_complete_lines_and_keeps_the_rest() {
        let mut buffer = LineBuffer::new();
        let mut lines = Vec::new();
        buffer.push(b"one\ntw", &mut lines);
        assert_eq!(lines, vec![b"one".to_vec()]);
        lines.clear();
        buffer.push(b"o\r\n", &mut lines);
        assert_eq!(lines, vec![b"two".to_vec()]);
        lines.clear();
        buffer.flush(&mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn line_buffer_flushes_an_overlong_line_without_a_newline() {
        let mut buffer = LineBuffer::new();
        let mut lines = Vec::new();
        let chunk = vec![b'x'; MAX_LINE_BYTES + 10];
        buffer.push(&chunk, &mut lines);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), MAX_LINE_BYTES + 10);
        lines.clear();
        let mut tail = Vec::new();
        buffer.push(b"\nrest", &mut tail);
        assert_eq!(tail, vec![Vec::<u8>::new()]);
        tail.clear();
        buffer.flush(&mut tail);
        assert_eq!(tail, vec![b"rest".to_vec()]);
    }

    #[test]
    fn an_empty_buffer_flushes_nothing() {
        let mut buffer = LineBuffer::new();
        let mut lines = Vec::new();
        buffer.flush(&mut lines);
        assert!(lines.is_empty());
    }
}
