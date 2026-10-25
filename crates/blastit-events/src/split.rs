//! Splitting one long app output line into several `app.output` events.

use time::OffsetDateTime;

use crate::json::encoded_len;
use crate::{AppOutput, Component, Event, EventKind, MAX_EVENT_BYTES, Stream, floor_boundary};

/// Turn one line of app output into `app.output` events at the current time.
///
/// See [`split_app_output_at`].
pub fn split_app_output(
    component: Component,
    route: Option<String>,
    stream: Stream,
    line: &str,
) -> Vec<Event> {
    split_app_output_at(OffsetDateTime::now_utc(), component, route, stream, line)
}

/// Turn one line of app output into `app.output` events, all stamped `ts`.
///
/// init reads whole lines from a child's stdout or stderr. Journald and
/// DigitalOcean cut a line at 2048 bytes, which would make a JSON line
/// unparseable, so a line that does not fit is split (§8.1). Every returned
/// event encodes to at most [`MAX_EVENT_BYTES`] bytes with its `line` intact,
/// carries the shared `part` (1-based) and `parts` fields, and concatenating
/// their `line`s gives back `line` exactly.
///
/// A line that fits is returned as a single event with `part` 1 and `parts` 1.
pub fn split_app_output_at(
    ts: OffsetDateTime,
    component: Component,
    route: Option<String>,
    stream: Stream,
    line: &str,
) -> Vec<Event> {
    let context = Context {
        ts,
        component,
        route,
        stream,
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

/// What every piece of one line shares.
struct Context {
    ts: OffsetDateTime,
    component: Component,
    route: Option<String>,
    stream: Stream,
}

impl Context {
    fn event(&self, line: &str, part: u32, parts: u32) -> Event {
        let mut event = Event::at(
            self.ts,
            self.component,
            EventKind::AppOutput(AppOutput {
                stream: self.stream,
                line: line.to_owned(),
                part,
                parts,
            }),
        );
        event.route.clone_from(&self.route);
        event
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
            .map(|event| match &event.kind {
                EventKind::AppOutput(output) => output.line.clone(),
                other => panic!("unexpected event {}", other.name()),
            })
            .collect()
    }

    fn check(events: &[Event], original: &str) {
        assert!(!events.is_empty());
        let parts = u32::try_from(events.len()).unwrap();
        for (index, event) in events.iter().enumerate() {
            let EventKind::AppOutput(output) = &event.kind else {
                panic!("unexpected event {}", event.kind.name());
            };
            assert_eq!(output.parts, parts);
            assert_eq!(output.part, u32::try_from(index).unwrap() + 1);
            assert_eq!(event.level, event.kind.default_level());
            let json = encode_json(event);
            assert!(
                json.len() <= MAX_EVENT_BYTES,
                "part {} is {} bytes",
                output.part,
                json.len()
            );
            let value: Value = serde_json::from_str(&json).unwrap();
            assert!(
                value.get("truncated").is_none(),
                "split parts are never truncated"
            );
            assert_eq!(value["line"], output.line);
        }
        assert_eq!(lines(events), original);
    }

    fn split(line: &str) -> Vec<Event> {
        split_app_output(
            Component::Init,
            Some("product-detail".to_owned()),
            Stream::Stdout,
            line,
        )
    }

    #[test]
    fn a_short_line_is_one_event() {
        let events = split("hello");
        assert_eq!(events.len(), 1);
        check(&events, "hello");
        let EventKind::AppOutput(output) = &events[0].kind else {
            panic!()
        };
        assert_eq!((output.part, output.parts), (1, 1));
        assert_eq!(events[0].route.as_deref(), Some("product-detail"));
        assert_eq!(events[0].component, Component::Init);
    }

    #[test]
    fn an_empty_line_is_one_empty_event() {
        let events = split("");
        assert_eq!(events.len(), 1);
        check(&events, "");
    }

    #[test]
    fn stderr_is_warn() {
        let events = split_app_output(Component::Init, None, Stream::Stderr, "boom");
        assert_eq!(events[0].level, crate::Level::Warn);
        assert!(events[0].route.is_none());
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

    fn parts_of(events: &[Event]) -> u32 {
        match &events[0].kind {
            EventKind::AppOutput(output) => output.parts,
            other => panic!("unexpected event {}", other.name()),
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
    fn every_piece_uses_the_given_timestamp() {
        let ts = crate::fixture::request_ts();
        let line = "z".repeat(5000);
        let events = split_app_output_at(
            ts,
            Component::Init,
            Some("r".to_owned()),
            Stream::Stderr,
            &line,
        );
        assert!(events.iter().all(|event| event.ts == ts));
        assert!(
            events
                .iter()
                .all(|event| event.route.as_deref() == Some("r"))
        );
        check(&events, &line);
    }
}
