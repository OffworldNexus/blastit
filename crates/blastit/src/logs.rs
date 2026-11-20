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

use std::cell::RefCell;
use std::os::fd::{BorrowedFd, RawFd};

use blastit_events::{
    AppOutput, Component, Event, EventKind, Level, MAX_EVENT_BYTES, ManifestWarning, Stream,
    encode_json, encode_pretty, restamp_json_into, split_app_output,
};
use serde_json::{Map, Value};

use crate::config::LogFormat;

/// Largest partial line init will hold before flushing it as it is (§8.1).
///
/// DigitalOcean truncates forwarded lines at 2048 bytes, and a line with no
/// newline at all must not grow without bound: past this, the bytes are logged
/// as a line of their own.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

// One reusable line buffer per thread, for `Emitter::forward`.
//
// init's own thread is the only caller in production, and a forwarded line is
// never longer than `MAX_EVENT_BYTES`; a buffer per thread keeps the canonical
// line off the heap for every event a child forwards, and stays correct if a
// test drives an emitter from its own thread.
thread_local! {
    static FORWARDED_LINE: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(MAX_EVENT_BYTES + 1));
}

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
    /// Whether a fatal refusal is printed as text on stderr (a person is
    /// watching) instead of as `app.output` events on the stream.
    refusals: bool,
}

impl Emitter {
    /// An emitter writing to `fd`, with the given format and minimum level.
    ///
    /// `color` is only consulted in [`LogFormat::Pretty`], and it is also what
    /// decides a fatal refusal's shape: a coloured stream is a terminal, and a
    /// terminal gets prose on stderr rather than JSON on stdout. [`Emitter::stdout`]
    /// makes the same decision from the stderr side of [`Colour`].
    pub fn new(fd: RawFd, format: LogFormat, level: Level, color: bool) -> Self {
        Self {
            fd,
            format,
            level,
            color,
            refusals: color && format != LogFormat::Json,
        }
    }

    /// An emitter writing to stdout, coloured only when the process decided
    /// stdout should be.
    pub fn stdout(format: LogFormat, level: Level, colour: Colour) -> Self {
        Self {
            fd: libc::STDOUT_FILENO,
            format,
            level,
            color: colour.stdout,
            // A refusal goes where a *person* reads: stderr is the terminal's
            // stream, so `colour.stderr` — the TTY-and-not-dumb decision taken
            // once by the composition root — is the one that counts here, not
            // `colour.stdout`. A `json` boot keeps the events for its pipeline.
            refusals: colour.stderr && format != LogFormat::Json,
        }
    }

    /// Whether this emitter's stream is the human format (§8.1).
    pub fn is_pretty(&self) -> bool {
        self.format == LogFormat::Pretty
    }

    /// Whether a fatal refusal from this emitter is text on stderr.
    pub fn refuses_in_text(&self) -> bool {
        self.refusals
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
    /// The line is read back into its members, its `component` is overwritten
    /// from the pipe it came from, and its `route` too when the pipe implies
    /// one. An event already logged below the minimum level is dropped whole. A
    /// line that is not a JSON object is dropped with a warning event that
    /// carries no part of it: the line could contain anything, including a
    /// credential.
    ///
    /// In the JSON format — the hot path, one line per request — the members
    /// are restamped and reordered straight into a reused buffer with
    /// [`restamp_json_into`], so no `Value` tree and no per-event `String` is
    /// built. `pretty` is for a person at a terminal, not for every request, so
    /// it keeps the map-based path that also backs its field-dump fallback.
    pub fn forward(&self, pipe: &Pipe, line: &str) {
        let component = pipe.component();
        match self.format {
            LogFormat::Json => self.forward_json(pipe, line, component),
            LogFormat::Pretty => {
                let Some(mut value) = object_of(line) else {
                    self.dropped_line(component);
                    return;
                };
                value.insert("component".to_string(), Value::from(component.as_str()));
                if let Some(route) = pipe.route() {
                    value.insert("route".to_string(), Value::from(route));
                }
                if !self.wants(&value) {
                    return;
                }
                self.write_line(&pretty_line(&value, self.color));
            }
        }
    }

    /// Forwards one line in the JSON format, rebuilding it in a reused buffer.
    fn forward_json(&self, pipe: &Pipe, line: &str, component: Component) {
        FORWARDED_LINE.with(|buffer| {
            let mut buffer = buffer.borrow_mut();
            buffer.clear();
            match restamp_json_into(&mut buffer, line, component.as_str(), pipe.route()) {
                None => {
                    // Release the buffer before the warning, which writes a
                    // line of its own.
                    drop(buffer);
                    self.dropped_line(component);
                }
                Some(level) => {
                    if !passes(level, self.level) {
                        return;
                    }
                    buffer.push(b'\n');
                    let _ = write_bytes(self.fd, &buffer);
                }
            }
        });
    }

    /// Drops one unreadable events-pipe line, with a warning that names only
    /// the component it came from and none of the line's own text.
    fn dropped_line(&self, component: Component) {
        self.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(ManifestWarning::new(format!(
                "{component}: dropped a line on the events pipe that is not a JSON object"
            ))),
        ));
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

/// Whether each of blastit's two streams should carry ANSI colour (`A-4`).
///
/// One value, decided once by the composition root ([`crate::term::colour`]) and
/// handed down: the event emitter takes [`Colour::stdout`], a refusal takes
/// [`Colour::stderr`]. Nothing asks a descriptor twice, and nothing below the
/// root reads `NO_COLOR` or `TERM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    /// The event stream, `--help`, `--version`.
    pub stdout: bool,
    /// Command-line refusals and every other word written to stderr.
    pub stderr: bool,
}

impl Colour {
    /// No colour anywhere: a capture that is not a terminal, and the value a
    /// JSON boot under no settings uses.
    pub const PLAIN: Self = Self {
        stdout: false,
        stderr: false,
    };

    /// The decision for a pair of descriptors, with the two variables an
    /// operator sets applied.
    ///
    /// This is pure on purpose: `NO_COLOR` (set to a non-empty value) and
    /// `TERM=dumb` win over the descriptor, and the *reading* of those two
    /// variables is the boot's job, once, in [`crate::term`]. stdout and stderr
    /// are separate answers because a redirect changes one and not the other.
    pub fn new(
        stdout_terminal: bool,
        stderr_terminal: bool,
        no_color: Option<&str>,
        term: Option<&str>,
    ) -> Self {
        let plain = no_color.is_some_and(|value| !value.is_empty()) || term == Some("dumb");
        Self {
            stdout: stdout_terminal && !plain,
            stderr: stderr_terminal && !plain,
        }
    }
}

/// The object a forwarded line is, or `None` when it is not a JSON object.
fn object_of(line: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// Whether the `level` a forwarded line carried passes the minimum level.
///
/// The same rule as [`Emitter::wants`], without a map: a missing or unreadable
/// level passes, so a malformed line is seen rather than lost.
fn passes(level: Option<&str>, minimum: Level) -> bool {
    match level {
        Some(text) => text.parse::<Level>().map_or(true, |level| level >= minimum),
        None => true,
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
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let _ = write_bytes(fd, &bytes);
}

/// Writes `bytes` to `fd` as-is, retrying `EINTR`.
///
/// [`write_line`] is this with the newline appended to a fresh buffer; the
/// event sink owns one buffer per thread and appends its own newline, so a
/// request costs no copy on the way to the pipe. The failure handling is
/// [`write_line`]'s: a dead pipe must never stop the server.
///
/// Returns whether every byte went to the pipe, so a caller with nothing left
/// to write to can stop trying.
pub fn write_bytes(fd: RawFd, bytes: &[u8]) -> bool {
    // SAFETY: the caller passes the descriptor it wants written to; nothing
    // here takes ownership of it, and it stays open for the caller's lifetime.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut written = 0;
    while written < bytes.len() {
        match rustix::io::write(borrowed, &bytes[written..]) {
            Ok(0) => return false,
            Ok(count) => written += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return false,
        }
    }
    true
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
/// On a stream a person is watching — a TTY, [`LogFormat::Pretty`] — the same
/// sentence is written to stderr as prose instead, and stdout stays empty: a
/// machine reading the event stream is not there, and JSON on the terminal was
/// the one refusal nobody could read (`§1.1`, A10.3). The decision was taken
/// once, by the composition root, when the [`Emitter`] was built.
///
/// This uses `_exit`, not `std::process::exit`: init may be running with CPython
/// initialised and the GIL held, and running libc's `atexit` handlers there can
/// deadlock.
pub fn fail(emitter: &Emitter, message: &str) -> ! {
    if emitter.refuses_in_text() {
        refuse_text(message);
    }
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
    crate::coverage::flush();
    // SAFETY: `_exit` never returns and runs no handler.
    unsafe { libc::_exit(1) }
}

/// A fatal refusal whose sentence already carries its `blastit {cmd}: ` head.
///
/// The error type is the one owner of that head (the discovery's `Display`),
/// so this printer emits the sentence **verbatim**: prose on stderr without a
/// second `blastit: `, and the same one JSON event per line. A boot that mixed
/// both heads printed `blastit: blastit serve: …` on one lane and
/// `blastit serve: …` on the other.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub fn fail_sentence(emitter: &Emitter, message: &str) -> ! {
    if emitter.refuses_in_text() {
        for line in message.lines() {
            eprintln!("{line}");
        }
        crate::coverage::flush();
        // SAFETY: `_exit` never returns and runs no handler.
        unsafe { libc::_exit(1) }
    }
    fail(emitter, message)
}

/// A fatal refusal as prose on stderr, for a boot a person is watching
/// (`§1.1`): the first line carries blastit's name, and every continuation line
/// is printed as it was written (the advice's own indentation).
pub fn refuse_text(message: &str) -> ! {
    for (index, line) in message.lines().enumerate() {
        if index == 0 {
            eprintln!("blastit: {line}");
        } else {
            eprintln!("{line}");
        }
    }
    crate::coverage::flush();
    // SAFETY: the boot is aborting; `_exit` runs no handler.
    unsafe { libc::_exit(1) }
}

/// Whether a boot that has not read its settings yet should refuse in prose.
///
/// This is the composition root's one read of `BLASTIT_LOG_FORMAT`, taken
/// before `Config::from_env` can fail: a pipeline that asked for `json` still
/// gets the event stream, and a person at a terminal gets a sentence on stderr.
/// `colour` is the terminal decision the same root made, never a probe here.
pub fn text_refusals(colour: Colour) -> bool {
    colour.stderr && std::env::var_os("BLASTIT_LOG_FORMAT").is_none_or(|format| format != "json")
}

/// One startup phase line, on stderr, before a wait that can take a while
/// (`§1.2`).
///
/// A person watching a boot must always see a line before the server blocks, or
/// a slow start looks hung. The line goes to stderr — the progress stream, not
/// the event stream — and only in the human format, so a `json` boot writes
/// exactly the events it did before.
pub fn phase(emitter: &Emitter, label: &str, detail: &str) {
    if emitter.is_pretty() {
        write_line(libc::STDERR_FILENO, &phase_line(label, detail));
    }
}

/// The text of one [`phase`] line: `  <label>  <detail>`, the two columns
/// wide enough for the labels this build uses (`dbproxy`, `app`).
pub fn phase_line(label: &str, detail: &str) -> String {
    format!("  {label:<10} {detail}")
}
