//! The pretty line encoder (ARCHITECTURE.md §8.1: `BLASTIT_LOG_FORMAT=pretty`).

use crate::{
    DevReloadUnavailable, Event, EventKind, ExitKind, Level, Lifecycle, MAX_EVENT_BYTES, Outcome,
    RequestInfo, Runtime, floor_boundary, format_clock_time,
};

const RESET: &str = "\u{1b}[0m";
/// The width every event name is padded to.
///
/// Every event name is padded to this many columns, so the summary starts at
/// the same place on every line — that is what makes a stream of events a
/// table. It is the longest name this crate can emit
/// (`dev.reload.unavailable`, 22 bytes) plus the one space that keeps the
/// summary readable: a narrower column pushed *that* line's summary right and
/// broke the alignment on every other line.
const NAME_COLUMN: usize = 23;

/// Encode one event as a single human-readable line, without a trailing
/// newline.
///
/// The shape is `HH:MM:SS.mmm LEVEL event  summary`, with the event name
/// padded to `NAME_COLUMN` columns, as in the §8.1 example. `color` wraps the
/// level word — and a `request`'s HTTP status — in ANSI colours (a handful of
/// escape codes, no crate); the padding is applied to the text, so coloured and
/// plain lines line up.
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
    // NAME_COLUMN columns, and always at least one space before the summary
    // so a name wider than the column stays readable.
    let width = NAME_COLUMN.max(name.chars().count() + 1);
    for _ in name.chars().count()..width {
        out.push(' ');
    }
    out.push_str(&summary(event, color));
    cap(&mut out);
    out
}

/// The level word, padded to five columns and coloured when asked.
///
/// DEBUG is `dim` (`ESC[2m`), the same code the logger's field-dump fallback
/// uses: one palette for a build's output, not two (`A-4`).
fn level_word(level: Level, color: bool) -> String {
    let mut word = format!("{:<5}", level.as_str().to_uppercase());
    if color {
        let code = match level {
            Level::Debug => "2",
            Level::Info => "32",
            Level::Warn => "33",
            Level::Error => "31",
        };
        word = format!("\u{1b}[{code}m{word}{RESET}");
    }
    word
}

fn summary(event: &Event, color: bool) -> String {
    let route = || match event.route.as_deref() {
        Some(route) => escape_inline(route),
        None => "-".to_owned(),
    };
    match &event.kind {
        EventKind::Request(info) => request_summary(event, info, color),
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
        EventKind::ProcessCap(info) => {
            format!("estimate_bytes={}  cap={}", info.estimate_bytes, info.cap)
        }
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
        EventKind::Runtime(info) => runtime_summary(info),
        // §4.9's frozen shape: the route column (no route, so `-`), then the
        // counts and the reason word.
        EventKind::DevReload(info) => format!(
            "{}  routes={}  reason={}  files={}",
            route(),
            info.routes.len(),
            info.reason.as_str(),
            info.files.len()
        ),
        EventKind::DevReloadUnavailable(info) => reload_disabled_summary(info),
        // §4.7's boot line: `1 directory` / `N directories`, in the summary
        // column like every other event.
        EventKind::Reload(info) => format!(
            "{} {}",
            info.directories(),
            if info.directories() == 1 {
                "directory"
            } else {
                "directories"
            }
        ),
    }
}

/// §4.7's one sentence: `reload disabled: <cause> (<ERRNO> on <fs>); serving
/// without live reload`.
///
/// The cause clause is chosen by the symbolic errno, so every row of the
/// failure ladder has its own words; anything outside the ladder still says
/// `reload disabled` and carries the errno it was given.
fn reload_disabled_summary(info: &DevReloadUnavailable) -> String {
    let cause = match info.errno() {
        "ENOSYS" => "fanotify is not available here: this kernel has no fanotify",
        "EPERM" => "fanotify is not permitted here: a container security profile",
        "EINVAL" => {
            "fanotify refused this boot's flags: this kernel does not accept the file-handle group"
        }
        "EOPNOTSUPP" => "this filesystem has no file handles: bind-mount the project from the host",
        "ENOSPC" => "no marks left: the reloads already marked keep working",
        "EMFILE" => "no fanotify group is left: close another one and restart",
        _ => "fanotify refused to start",
    };
    format!(
        "reload disabled: {cause} ({} on {}); serving without live reload",
        escape_inline(info.errno()),
        escape_inline(info.filesystem())
    )
}

/// §8.1's `runtime` text form: the entrypoint and its version, then the
/// labelled fields two spaces apart, exactly as `docs/local.md` shows them.
///
/// A `null` `php_ini` prints `php.ini none` — PHP's built-in defaults — and a
/// static entrypoint is followed by its one sentence. Every value is escaped,
/// so the path PHP opened cannot turn the line into two.
fn runtime_summary(info: &Runtime) -> String {
    match info {
        Runtime::Php(php) => format!(
            "php {}  libphp {}  php.ini {}",
            escape_inline(php.version()),
            escape_inline(php.libphp()),
            match php.php_ini() {
                Some(path) => escape_inline(path),
                None => "none".to_owned(),
            }
        ),
        Runtime::Python(py) => format!(
            "python {}  environment {}",
            escape_inline(py.version()),
            escape_inline(py.environment())
        ),
        Runtime::Static => "static  serving static assets only".to_owned(),
    }
}

fn request_summary(event: &Event, info: &RequestInfo, color: bool) -> String {
    let target = match event.route.as_deref() {
        Some(route) => escape_inline(route),
        None => "(unmatched)".to_owned(),
    };
    let mut out = format!(
        "{} {} → {}  {}",
        escape_inline(&info.method),
        escape_inline(&info.path),
        target,
        status_word(info.status, color)
    );
    if info.outcome != Outcome::Ok {
        out.push(' ');
        out.push_str(info.outcome.as_str());
    }
    out.push_str(&format!("  {:.1}ms", info.duration_ms));
    out
}

/// A `request`'s HTTP status, coloured by class when the stream is coloured.
///
/// 2xx green, 3xx cyan, 4xx yellow, 5xx red; anything outside those classes —
/// and every line on an uncoloured stream — is the bare number, so a log an
/// operator pipes into a file is exactly the bytes it always was.
fn status_word(status: u16, color: bool) -> String {
    if !color {
        return status.to_string();
    }
    let code = match status / 100 {
        2 => "32",
        3 => "36",
        4 => "33",
        5 => "31",
        _ => return status.to_string(),
    };
    format!("\u{1b}[{code}m{status}{RESET}")
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
    // D6: which mode init runs in, and the capabilities it holds.
    if let Some(privilege) = &lifecycle.privilege {
        parts.push(format!("privilege={privilege}"));
    }
    // R23: the `preload` `boot` line names the build it is about to serve.
    if let Some(build) = &lifecycle.build {
        parts.push(format!("build={}", escape_inline(build)));
    }
    if let Some(dropped) = lifecycle.bounding_set {
        parts.push(format!(
            "bounding_set={}",
            if dropped { "dropped" } else { "kept" }
        ));
    }
    if !lifecycle.capabilities.is_empty() {
        parts.push(format!(
            "capabilities=[{}]",
            lifecycle.capabilities.join(" ")
        ));
    }
    if let Some(immortalize) = lifecycle.immortalize {
        parts.push(format!(
            "immortalize={}",
            if immortalize { "on" } else { "off" }
        ));
    }
    if let Some(reason) = &lifecycle.immortalize_reason {
        parts.push(format!("because {}", escape_inline(reason)));
    }
    if let Some(immortalized) = lifecycle.immortalized {
        parts.push(format!("immortalized={immortalized}"));
    }
    if let Some(immortalize_ms) = lifecycle.immortalize_ms {
        parts.push(format!("immortalize_ms={immortalize_ms:.1}ms"));
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
    // E1: the preset in force, what it supplied, and what an explicit variable
    // overrode. The names are the same ones the JSON format uses, so a reader
    // can move between the two.
    if let Some(preset) = &lifecycle.preset {
        parts.push(format!("preset={}", escape_inline(preset)));
    }
    if !lifecycle.preset_settings.is_empty() {
        let settings: Vec<String> = lifecycle
            .preset_settings
            .iter()
            .map(|(name, value)| format!("{}={}", escape_inline(name), escape_inline(value)))
            .collect();
        parts.push(format!("preset_settings: {}", settings.join(", ")));
    }
    if !lifecycle.preset_overridden.is_empty() {
        let names: Vec<String> = lifecycle
            .preset_overridden
            .iter()
            .map(|name| escape_inline(name))
            .collect();
        parts.push(format!("preset_overridden: {}", names.join(", ")));
    }
    parts.join("  ")
}

/// Bytes as the nearest mebibyte, for the human format's memory fields.
fn mebibytes(bytes: u64) -> String {
    format!("{:.0}MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// One printable line's worth of arbitrary text: control characters escaped.
///
/// Every string a pretty line carries comes from a component that may be
/// carrying application data — a path, a statement, a line of app output — and a
/// raw `\n` or `\u{1b}[2J` in one of them would turn one event into two lines, or
/// clear the operator's terminal. So the escapes are spelled out (`\n`, `\r`,
/// `\t`, or `\u{…}` for anything else in Unicode's control ranges), and the
/// result is always exactly one line. It is public because the *fallback* the
/// logger keeps for a line it cannot read back into an event (`blastit`'s
/// `render_pretty`) has to escape exactly the same way.
pub fn escape_inline(text: &str) -> String {
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
