//! Splitting one long app output line into several `app.output` events.

use time::OffsetDateTime;

use crate::json::encoded_len;
use crate::{AppOutput, Component, Event, EventKind, MAX_EVENT_BYTES, Stream, floor_boundary};

/// Largest size an over-long shared `route` or `request_id` is cut to before
/// splitting, in bytes.
const SHARED_FIELD_BYTES: usize = 256;

/// Turn one line of app output into `app.output` events at the current time.
///
/// See [`split_app_output_at`].
pub fn split_app_output(
    component: Component,
    route: Option<String>,
    request_id: Option<String>,
    stream: Stream,
    line: &str,
) -> Vec<Event> {
    split_app_output_at(
        OffsetDateTime::now_utc(),
        component,
        route,
        request_id,
        stream,
        line,
    )
}

/// Turn one line of app output into `app.output` events, all stamped `ts`.
///
/// init reads whole lines from a child's stdout or stderr. Journald and
/// DigitalOcean cut a line at 2048 bytes, which would make a JSON line
/// unparseable, so a line that does not fit is split (§8.1). Every returned
/// event encodes to at most [`MAX_EVENT_BYTES`] bytes with its `line` intact,
/// carries the shared `part` (1-based) and `parts` fields, and concatenating
/// their `line`s gives back `line` exactly. A line that fits is returned as a
/// single event with `part` 1 and `parts` 1.
///
/// `route` and `request_id` are the fields every piece would otherwise carry a
/// full copy of. A copy is paid for once per piece, so an over-long one is cut
/// to 256 bytes here, once, and declared in `truncated` on every piece instead
/// of spending the budget of each piece on it. That is the only truncation
/// splitting does: it never cuts `line`.
pub fn split_app_output_at(
    ts: OffsetDateTime,
    component: Component,
    route: Option<String>,
    request_id: Option<String>,
    stream: Stream,
    line: &str,
) -> Vec<Event> {
    let mut truncated: Vec<&'static str> = Vec::new();
    let context = Context {
        ts,
        component,
        route: route.map(|route| cut_shared("route", route, &mut truncated)),
        request_id: request_id.map(|id| cut_shared("request_id", id, &mut truncated)),
        stream,
        truncated,
    };

    // `parts` is written into every event, so a bigger number leaves less room
    // and can raise the number of pieces. Iterate until it is stable; the
    // count only ever grows, so this converges.
    let mut parts = 1u32;
    loop {
        let events = split_into(&context, line, parts);
        let count = u32::try_from(events.len()).unwrap_or(u32::MAX);
        if count == parts {
            return events;
        }
        parts = count;
    }
}

/// Cut a shared string to [`SHARED_FIELD_BYTES`] on a character boundary,
/// recording its name when it was longer.
fn cut_shared(name: &'static str, mut value: String, truncated: &mut Vec<&'static str>) -> String {
    if value.len() > SHARED_FIELD_BYTES {
        let keep = floor_boundary(&value, SHARED_FIELD_BYTES);
        value.truncate(keep);
        if !truncated.contains(&name) {
            truncated.push(name);
        }
    }
    value
}

/// What every piece of one line shares.
struct Context {
    ts: OffsetDateTime,
    component: Component,
    route: Option<String>,
    request_id: Option<String>,
    stream: Stream,
    truncated: Vec<&'static str>,
}

impl Context {
    fn event(&self, line: &str, part: u32, parts: u32) -> Event {
        let mut event = Event::at(
            self.ts,
            self.component,
            EventKind::AppOutput(AppOutput::piece(self.stream, line.to_owned(), part, parts)),
        );
        if let Some(route) = &self.route {
            event = event.with_route(route.clone());
        }
        if let Some(request_id) = &self.request_id {
            event = event.with_request_id(request_id.clone());
        }
        event.with_truncated(self.truncated.iter().copied())
    }
}

fn split_into(context: &Context, line: &str, parts: u32) -> Vec<Event> {
    let mut events = Vec::new();
    let mut rest = line;
    let mut part = 1u32;
    loop {
        let take = largest_prefix(context, rest, part, parts);
        let (chunk, tail) = rest.split_at(take);
        events.push(context.event(chunk, part, parts));
        if tail.is_empty() {
            return events;
        }
        rest = tail;
        part += 1;
    }
}

/// The longest prefix of `rest` whose event still fits, at least one character
/// long when `rest` is not empty.
///
/// Escaping never makes a string shorter, so the answer is at most
/// [`MAX_EVENT_BYTES`] bytes; the search probes at most that much text.
fn largest_prefix(context: &Context, rest: &str, part: u32, parts: u32) -> usize {
    if rest.is_empty() {
        return 0;
    }
    let mut hi = floor_boundary(rest, MAX_EVENT_BYTES.min(rest.len()));
    if encoded_len(&context.event(&rest[..hi], part, parts)) <= MAX_EVENT_BYTES {
        return hi;
    }
    // Invariant: `rest[..lo]` fits, `rest[..hi]` does not.
    let mut lo = 0usize;
    while hi - lo > 1 {
        let mid = floor_boundary(rest, lo + (hi - lo) / 2);
        if mid <= lo {
            break;
        }
        if encoded_len(&context.event(&rest[..mid], part, parts)) <= MAX_EVENT_BYTES {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // Always make progress: a piece of at least one character, even in the
    // impossible case where the fixed part of the event fills the budget.
    let first = rest.chars().next().map_or(0, char::len_utf8);
    lo.max(first)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::encode_json;
    use serde_json::Value;

    fn lines(events: &[Event]) -> String {
        events
            .iter()
            .map(|event| match event.kind() {
                EventKind::AppOutput(output) => output.line().to_owned(),
                other => panic!("unexpected event {}", other.name()),
            })
            .collect()
    }

    fn parts_of(events: &[Event]) -> u32 {
        match events.first().map(Event::kind) {
            Some(EventKind::AppOutput(output)) => output.parts(),
            other => panic!("unexpected event {other:?}"),
        }
    }

    /// Every piece fits, is numbered consistently, and the pieces rebuild the
    /// line. The `line` itself is never truncated.
    fn check(events: &[Event], original: &str) {
        assert!(!events.is_empty());
        let parts = u32::try_from(events.len()).unwrap();
        assert_eq!(parts_of(events), parts);
        for (index, event) in events.iter().enumerate() {
            let EventKind::AppOutput(output) = event.kind() else {
                panic!("unexpected event {}", event.kind().name());
            };
            assert_eq!(output.parts(), parts);
            assert_eq!(output.part(), u32::try_from(index).unwrap() + 1);
            assert_eq!(event.level(), event.kind().default_level());
            let json = encode_json(event);
            assert!(
                json.len() <= MAX_EVENT_BYTES,
                "part {} is {} bytes",
                output.part(),
                json.len()
            );
            let value: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                json.len(),
                encoded_len(event),
                "a split piece must fit without the encoder cutting it"
            );
            assert_eq!(value["line"], output.line());
            if let Some(names) = value.get("truncated") {
                let names = names.as_array().unwrap();
                assert!(!names.is_empty());
                for name in names {
                    assert!(
                        name == "route" || name == "request_id",
                        "splitting must never truncate the line itself, got {name}"
                    );
                }
            }
        }
        assert_eq!(lines(events), original);
    }

    fn split(line: &str) -> Vec<Event> {
        split_app_output(
            Component::Init,
            Some("product-detail".to_owned()),
            None,
            Stream::Stdout,
            line,
        )
    }

    fn split_at(line: &str) -> Vec<Event> {
        split_app_output_at(
            crate::fixture::request_ts(),
            Component::Init,
            Some("product-detail".to_owned()),
            None,
            Stream::Stdout,
            line,
        )
    }

    #[test]
    fn a_short_line_is_one_event() {
        let events = split("hello");
        assert_eq!(events.len(), 1);
        check(&events, "hello");
        let EventKind::AppOutput(output) = events[0].kind() else {
            panic!()
        };
        assert_eq!((output.part(), output.parts()), (1, 1));
        assert_eq!(events[0].route(), Some("product-detail"));
        assert_eq!(events[0].component(), Component::Init);
        assert!(events[0].truncated_fields().is_empty());
    }

    #[test]
    fn an_empty_line_is_one_empty_event() {
        let events = split("");
        assert_eq!(events.len(), 1);
        check(&events, "");
    }

    #[test]
    fn stderr_is_warn() {
        let events = split_app_output(Component::Init, None, None, Stream::Stderr, "boom");
        assert_eq!(events[0].level(), crate::Level::Warn);
        assert!(events[0].route().is_none());
        assert!(events[0].request_id().is_none());
    }

    #[test]
    fn a_long_ascii_line_is_split_exactly() {
        let line = "a".repeat(9000);
        let events = split(&line);
        assert!(events.len() > 1);
        check(&events, &line);
    }

    #[test]
    fn a_long_unicode_line_keeps_every_character() {
        let line = "漢字é🦀".repeat(2000);
        let events = split(&line);
        assert!(events.len() > 1);
        check(&events, &line);
    }

    #[test]
    fn a_line_that_grows_when_escaped_is_still_split() {
        let line = "\"\u{1}\n\\".repeat(3000);
        let events = split(&line);
        assert!(events.len() > 1);
        check(&events, &line);
    }

    #[test]
    fn parts_count_matches_across_sizes() {
        for size in [0usize, 1, 100, 1900, 2000, 2030, 4000, 100_000] {
            let line = "x".repeat(size);
            let events = split(&line);
            assert_eq!(events.len(), usize::try_from(parts_of(&events)).unwrap());
            check(&events, &line);
        }
    }

    #[test]
    fn digit_width_of_parts_does_not_overflow_the_limit() {
        // Enough pieces that `parts` gains a digit, checked by the invariant
        // in `check`, which re-encodes every event.
        for size in [2_000usize, 9_900, 10_100, 99_900] {
            let line = "y".repeat(size);
            let events = split(&line);
            assert!(events.len() >= 2);
            check(&events, &line);
        }
    }

    #[test]
    fn an_over_long_shared_route_is_cut_once_before_splitting() {
        let route = "r".repeat(2048);
        let request_id = "i".repeat(1024);
        let line = "a".repeat(10_000);
        let events = split_app_output_at(
            crate::fixture::request_ts(),
            Component::Init,
            Some(route.clone()),
            Some(request_id.clone()),
            Stream::Stdout,
            &line,
        );
        check(&events, &line);

        let parts = parts_of(&events);
        // Without the cut, a 2 KiB route leaves no room for the line in any
        // piece and this would be one piece per character.
        assert!((5..=10).contains(&parts), "unexpected part count {parts}");
        for event in &events {
            assert_eq!(event.route().unwrap().len(), 256);
            assert!(route.starts_with(event.route().unwrap()));
            assert_eq!(event.request_id().unwrap().len(), 256);
            assert!(request_id.starts_with(event.request_id().unwrap()));
            assert_eq!(event.truncated_fields(), ["route", "request_id"]);
            let value: Value = serde_json::from_str(&encode_json(event)).unwrap();
            assert_eq!(
                value["truncated"],
                serde_json::json!(["route", "request_id"])
            );
            assert_eq!(value["route"].as_str().unwrap().len(), 256);
        }

        // A route that already fits is left exactly as it is, with no
        // `truncated` bookkeeping.
        let short = split_app_output_at(
            crate::fixture::request_ts(),
            Component::Init,
            Some("r".repeat(256)),
            Some("i".repeat(255)),
            Stream::Stdout,
            &line,
        );
        assert!(short[0].truncated_fields().is_empty());
        assert_eq!(short[0].route().unwrap().len(), 256);
        assert!(short[0].route().unwrap().ends_with('r'));
    }

    #[test]
    fn cutting_a_shared_field_keeps_it_a_prefix_on_a_char_boundary() {
        let mut truncated = Vec::new();
        let value = cut_shared("route", "é".repeat(1000), &mut truncated);
        assert_eq!(value.len(), 256);
        assert!(value.chars().all(|c| c == 'é'));
        assert!(value.is_char_boundary(value.len()));
        assert_eq!(truncated, ["route"]);

        let mut truncated = Vec::new();
        let exactly = cut_shared("route", "é".repeat(128), &mut truncated);
        assert_eq!(exactly.len(), 256);
        assert!(truncated.is_empty());

        let mut truncated = Vec::new();
        let odd = cut_shared("route", "éa".repeat(500), &mut truncated);
        assert!(odd.is_char_boundary(odd.len()));
        assert_eq!(odd.len(), 255);
        assert_eq!(truncated, ["route"]);

        // "aé" puts a boundary exactly on the limit, so nothing is lost.
        let mut truncated = Vec::new();
        let aligned = cut_shared("route", "aé".repeat(500), &mut truncated);
        assert_eq!(aligned.len(), 256);
        assert!(aligned.ends_with('a'));
        assert_eq!(truncated, ["route"]);
    }

    #[test]
    fn every_piece_uses_the_given_timestamp() {
        let ts = crate::fixture::request_ts();
        let line = "z".repeat(5000);
        let events = split_at(&line);
        assert!(events.iter().all(|event| event.ts() == ts));
        assert!(
            events
                .iter()
                .all(|event| event.route() == Some("product-detail"))
        );
        check(&events, &line);
    }
}
