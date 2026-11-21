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
