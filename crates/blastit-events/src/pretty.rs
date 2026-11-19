//! The pretty line encoder (ARCHITECTURE.md §8.1: `BLASTIT_LOG_FORMAT=pretty`).

use crate::{
    DevReloadUnavailable, Event, EventKind, ExitKind, Level, Lifecycle, MAX_EVENT_BYTES, Outcome,
    RequestInfo, Runtime, floor_boundary, format_clock_time,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::{
        AppOutput, DevReload, ExitKind, Lifecycle, ManifestWarning, Outcome, ProcessCap,
        ProcessExited, ProcessRetired, ProcessSpawned, ReloadReason, RequestInfo, RetireReason,
        Stream,
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
            fixture::event(EventKind::ProcessCap(ProcessCap::new(141_557_760, 4))),
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
            fixture::event(EventKind::Runtime(Runtime::php(
                "8.5.4",
                "/usr/lib/libphp8.5.so",
                Some("/etc/php/8.5/embed/php.ini".to_owned()),
            ))),
            fixture::event(EventKind::Runtime(Runtime::php(
                "8.4.27",
                "/usr/lib/libphp8.4.so",
                None,
            ))),
            fixture::event(EventKind::Runtime(Runtime::python(
                "3.14.0",
                "/home/you/shop/.venv",
            ))),
            fixture::event(EventKind::Runtime(Runtime::Static)),
            fixture::event(EventKind::DevReload(DevReload::new(
                vec!["products".to_owned(), "cart".to_owned(), "about".to_owned()],
                vec![
                    "/srv/shop/app/views/cart.py".to_owned(),
                    "/srv/shop/app/views/products.py".to_owned(),
                ],
                ReloadReason::Loaded,
            ))),
            fixture::event(EventKind::DevReloadUnavailable(DevReloadUnavailable::new(
                "EPERM",
                "overlayfs",
            ))),
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
    fn a_cap_event_reports_the_measurement_and_the_cap() {
        let event = fixture::event(EventKind::ProcessCap(ProcessCap::new(141_557_760, 4)));
        let line = plain(&event);
        assert!(line.contains("process.cap"), "{line}");
        assert!(line.contains("estimate_bytes=141557760"), "{line}");
        assert!(line.contains("cap=4"), "{line}");
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
        assert!(encode_pretty(&event.with_level(Level::Debug), true).contains("\u{1b}[2m"));
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
    fn lifecycle_shows_the_privilege_mode_and_its_capabilities() {
        // D6: the pretty format says the same thing as the JSON one, next to
        // `seccomp`, because a human reading a boot line is asking the same
        // question.
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_privilege("capabilities")
                .with_capability("cap_chown")
                .with_capability("cap_setuid"),
        ));
        let line = encode_pretty(&event, false);
        assert!(line.contains("privilege=capabilities"), "{line}");
        assert!(
            line.contains("capabilities=[cap_chown cap_setuid]"),
            "{line}"
        );
        let root = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_privilege("root"),
        ));
        let line = encode_pretty(&root, false);
        assert!(line.contains("privilege=root"), "{line}");
        assert!(!line.contains("capabilities="), "{line}");
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
    fn immortalisation_shows_whether_it_ran_and_why_not() {
        let skipped = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_immortalize(false)
                .with_immortalize_reason("CPython 3.14 is unsafe")
                .with_immortalized(0),
        ));
        let line = plain(&skipped);
        assert!(line.contains("immortalize=off"), "{line}");
        assert!(line.contains("immortalized=0"), "{line}");
        assert!(line.contains("CPython 3.14 is unsafe"), "{line}");
        assert!(!line.contains("immortalize_ms"), "{line}");

        let ran = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("preload")
                .with_immortalize(true)
                .with_immortalized(1_610_300)
                .with_immortalize_ms(1785.4),
        ));
        let line = plain(&ran);
        assert!(line.contains("immortalize=on"), "{line}");
        assert!(line.contains("immortalized=1610300"), "{line}");
        assert!(line.contains("immortalize_ms=1785.4ms"), "{line}");
    }

    #[test]
    fn lifecycle_shows_the_preset_and_its_settings() {
        // E1: the pretty line says the same as the JSON one, with the same
        // `preset_*` names so a reader can move between the two.
        let event = fixture::event(EventKind::Boot(
            Lifecycle::default()
                .with_stage("config")
                .with_preset("compose-traefik")
                .with_preset_setting("BLASTIT_CLIENT_IP_HEADER", "x-forwarded-for")
                .with_preset_override("BLASTIT_LOG_FORMAT"),
        ));
        let line = encode_pretty(&event, false);
        assert!(line.contains("preset=compose-traefik"), "{line}");
        assert!(
            line.contains("preset_settings: BLASTIT_CLIENT_IP_HEADER=x-forwarded-for"),
            "{line}"
        );
        assert!(
            line.contains("preset_overridden: BLASTIT_LOG_FORMAT"),
            "{line}"
        );

        let plain = fixture::event(EventKind::Boot(Lifecycle::default().with_stage("stage0")));
        let line = encode_pretty(&plain, false);
        assert!(!line.contains("preset"), "{line}");
    }

    /// §4.9's frozen text form: the (empty) route column, the counts and the
    /// reason word.
    #[test]
    fn dev_reload_matches_the_documented_text_form() {
        let event = fixture::event(EventKind::DevReload(DevReload::new(
            vec!["products".to_owned(), "cart".to_owned(), "about".to_owned()],
            vec![
                "/srv/shop/app/views/cart.py".to_owned(),
                "/srv/shop/app/views/products.py".to_owned(),
            ],
            ReloadReason::Loaded,
        )));
        let line = plain(&event);
        assert_eq!(
            line,
            "14:02:11.384 INFO  dev.reload  -  routes=3  reason=loaded  files=2"
        );
    }

    /// §8.1's `runtime` text form: the three golden lines `docs/local.md`
    /// shows, byte for byte, including `php.ini none` and the static sentence.
    #[test]
    fn runtime_matches_the_documented_text_form() {
        let php = fixture::event(EventKind::Runtime(Runtime::php(
            "8.5.4",
            "/usr/lib/libphp8.5.so",
            Some("/etc/php/8.5/embed/php.ini".to_owned()),
        )));
        assert_eq!(
            plain(&php),
            "14:02:11.384 INFO  runtime     php 8.5.4  libphp /usr/lib/libphp8.5.so  \
             php.ini /etc/php/8.5/embed/php.ini"
        );

        // `null` is PHP's built-in defaults, and it says so in one word.
        let none = fixture::event(EventKind::Runtime(Runtime::php(
            "8.4.27",
            "/usr/lib/libphp8.4.so",
            None,
        )));
        assert_eq!(
            plain(&none),
            "14:02:11.384 INFO  runtime     php 8.4.27  libphp /usr/lib/libphp8.4.so  \
             php.ini none"
        );

        let python = fixture::event(EventKind::Runtime(Runtime::python(
            "3.14.0",
            "/home/you/shop/.venv",
        )));
        assert_eq!(
            plain(&python),
            "14:02:11.384 INFO  runtime     python 3.14.0  environment /home/you/shop/.venv"
        );

        let static_only = fixture::event(EventKind::Runtime(Runtime::Static));
        assert_eq!(
            plain(&static_only),
            "14:02:11.384 INFO  runtime     static  serving static assets only"
        );
    }

    /// A path with a control character cannot turn the runtime line into two.
    #[test]
    fn a_runtime_path_is_escaped_to_one_line() {
        let event = fixture::event(EventKind::Runtime(Runtime::php(
            "8.5.4",
            "/usr/lib/libphp8.5.so",
            Some("/etc/php/8.5/em\nbed/php.ini".to_owned()),
        )));
        let line = plain(&event);
        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("em\\nbed"), "{line}");
    }

    /// §4.7's one sentence, with the cause clause for each ladder row and a
    /// fallback that still says `reload disabled`.
    #[test]
    fn dev_reload_unavailable_matches_the_documented_sentence() {
        let sentence = |errno: &str, filesystem: &str| {
            plain(&fixture::event(EventKind::DevReloadUnavailable(
                DevReloadUnavailable::new(errno, filesystem),
            )))
        };
        assert_eq!(
            sentence("EPERM", "overlayfs"),
            "14:02:11.384 WARN  dev.reload.unavailable reload disabled: fanotify is not \
             permitted here: a container security profile (EPERM on overlayfs); serving \
             without live reload"
        );
        for (errno, filesystem) in [
            ("ENOSYS", "xfs"),
            ("EINVAL", "xfs"),
            ("EOPNOTSUPP", "9p"),
            ("ENOSPC", "xfs"),
            ("EMFILE", "xfs"),
            ("EIO", "xfs"),
            ("E99", "unknown"),
        ] {
            let line = sentence(errno, filesystem);
            assert!(
                line.starts_with(
                    "14:02:11.384 WARN  dev.reload.unavailable \
                     reload disabled: "
                ),
                "{line}"
            );
            assert!(
                line.contains(&format!(
                    "({errno} on {filesystem}); serving without live reload"
                )),
                "{line}"
            );
            assert!(!line.contains('\n'), "{line}");
        }
    }

    #[test]
    fn escape_inline_leaves_printable_text_alone() {
        assert_eq!(escape_inline("plain é 漢"), "plain é 漢");
        assert_eq!(escape_inline(""), "");
        assert_eq!(escape_inline("\u{7f}"), "\\u{7f}");
    }
}
