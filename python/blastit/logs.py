"""Reading blastit's event stream (D3c, ARCHITECTURE.md §8.1).

stdout is blastit's only output, and it is an **event stream: one JSON object
per line** (§8.1). Two tools read it, and they read it here so that they agree:

* the pytest plugin, which turns what a *live* server served through blastit —
  `blastit_live_server`, a Playwright suite, a `live_server` a project points at
  blastit — into the observations it diffs against the manifest, so front-end
  traffic is checked against the grants;
* `blastit-manifest fold`, which turns the statements a log denied into the
  grants a manifest is missing, because a test suite only makes the requests it
  makes.

Everything here is stdlib, and nothing here touches a manifest: this module
knows what an event looks like, not what it means.

What it tolerates
-----------------

* **Lines that are not events.** `BLASTIT_LOG_FORMAT=pretty` prints aligned
  human lines, a traceback goes straight to stderr, and a library can print
  anything at all: a line without a JSON object in it is skipped, never fatal.
* **A prefix before the object.** DigitalOcean App Platform prefixes every
  forwarded line with the app and its instance, so the object starts at the
  first `{` of the line, not at its beginning:

  ``2026-10-25T14:02:11.384Z app[web.1]: {"event":"request",…}``

* **Unknown fields and unknown events.** A log from another build still parses;
  what this module does not recognise is simply not returned.

What it does *not* do
---------------------

`sql.denied` carries the statement **already normalised** by dbproxy (§8.1):
`$?` placeholders, collapsed `IN` lists and `VALUES` runs. That is exactly the
text a grant has to hold, which is why `blastit-manifest fold` can paste it in
as it is — and it is *not* valid SQL any more, so `blastit._blastit.analyze`
refuses it. [`parseable_sql`] is what brings the two together: it rewrites the
two shapes the normaliser introduces into the shapes PostgreSQL's own parser
accepts (`$1`, a plain list), which normalise back to the very same text. A
table grant can then be derived from a log's denial, and the statement a reader
sees is the one dbproxy logged.
"""

from __future__ import annotations

import json
import re
from typing import Any, Iterable

__all__ = [
    "denials",
    "parse_events",
    "parseable_sql",
    "routed_requests",
    "unrouted_requests",
]

#: The event name of the routing decision the scheduler logs once per request.
REQUEST_EVENT = "request"

#: The event name of a statement the policy refused.
DENIED_EVENT = "sql.denied"


def parse_events(lines: Iterable[str]) -> list[dict[str, Any]]:
    """Every JSON event object in a blastit log, in order.

    `lines` is anything iterable over lines of text — a file object, a list, the
    output of a finished process. A line that holds no JSON object is skipped
    (see the module documentation), and so is one that holds an object of
    another shape, or a list: blastit writes one object per line and nothing
    else is an event of its.
    """
    events: list[dict[str, Any]] = []
    for line in lines:
        event = parse_event(line)
        if event is not None:
            events.append(event)
    return events


def parse_event(line: str) -> dict[str, Any] | None:
    """The event one log line carries, or `None` when it carries none.

    The object is looked for at the first `{` of the line, because App Platform
    prefixes forwarded lines with its own timestamp and app name (§8.1), and
    whatever follows the object is ignored — a line is *mostly* an event, and a
    reader that insists on exact framing would drop a denial because a wrapper
    printed something after it.
    """
    start = line.find("{")
    if start < 0:
        return None
    try:
        parsed, _ = json.JSONDecoder().raw_decode(line[start:])
    except ValueError:
        return None
    if not isinstance(parsed, dict):
        return None
    return parsed


def routed_requests(events: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """The requests blastit routed, one entry per distinct `(route, method, path)`.

    The scheduler logs one `request` event per request with the route it picked
    (null when nothing matched), so this is what a live server really served —
    which is the part a pytest suite cannot see from inside its own process.
    Entries keep the order they were first seen in, and `count` says how often
    that `(route, method, path)` was served.
    """
    seen: dict[tuple[str, str, str], dict[str, Any]] = {}
    for event in events:
        if event.get("event") != REQUEST_EVENT:
            continue
        route = event.get("route")
        if not isinstance(route, str) or not route:
            continue
        method = _text(event.get("method")) or "GET"
        path = _text(event.get("path"))
        entry = seen.setdefault(
            (route, method, path), {"route": route, "method": method, "path": path, "count": 0}
        )
        entry["count"] += 1
    return list(seen.values())


def unrouted_requests(events: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """The requests blastit could not route, one entry per distinct `(method, path)`.

    `route` is null on those `request` events: blastit answered 404 itself,
    before the app saw anything, which is the `unrouted` section of the report.
    """
    seen: dict[tuple[str, str], dict[str, Any]] = {}
    for event in events:
        if event.get("event") != REQUEST_EVENT:
            continue
        if event.get("route") not in (None, ""):
            continue
        method = _text(event.get("method")) or "GET"
        path = _text(event.get("path"))
        entry = seen.setdefault((method, path), {"method": method, "path": path, "count": 0})
        entry["count"] += 1
    return list(seen.values())


def denials(events: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """The statements blastit refused, one entry per distinct `(route, proxy, sql)`.

    Every `sql.denied` event is a statement a route ran and its grant does not
    allow (§8.1). What a manifest needs is the *statement*, so the same one
    denied twice is one entry with `count == 2`; `reason` is dbproxy's own
    explanation and is kept as a comment for whoever edits the manifest.
    """
    seen: dict[tuple[str, str, str], dict[str, Any]] = {}
    for event in events:
        if event.get("event") != DENIED_EVENT:
            continue
        route = _text(event.get("route"))
        sql = _text(event.get("sql"))
        if not route or not sql:
            continue
        proxy = _text(event.get("proxy"))
        entry = seen.setdefault(
            (route, proxy, sql),
            {
                "route": route,
                "proxy": proxy,
                "sql": sql,
                "reason": _text(event.get("reason")),
                "count": 0,
            },
        )
        entry["count"] += 1
    return list(seen.values())


def _text(value: Any) -> str:
    """A field of an event as a string; `None` and anything else become `""`."""
    return value if isinstance(value, str) else ""


#
# The normalised text of a `sql.denied` event, as SQL again.
#


#: A dollar-quoted string's opening or closing tag: `$$`, or `$tag$`.
_DOLLAR_TAG = re.compile(r"\$[A-Za-z_][A-Za-z0-9_]*\$|\$\$")


def parseable_sql(sql: str) -> str:
    """Normalised text as a statement PostgreSQL's own parser accepts (D3c).

    The normaliser rewrites two shapes that no parser can read: every bind
    placeholder becomes `$?`, and a list after `IN` or a run of identical
    `VALUES` rows is cut short with `, ...` (§6, "Matching"). Both are exactly
    what a grant should hold, and neither is valid SQL — so
    `blastit._blastit.analyze` refuses the text of a `sql.denied` event.

    This rewrites them back into shapes PostgreSQL accepts, without touching
    quoted text, quoted identifiers or dollar-quoted strings, where `$?` or
    `, ...` may be data:

    * a placeholder becomes `$1` (the number carries no meaning for the policy);
    * a list cut short inside parentheses (`IN ($?, ...)`) loses the marker,
      because one placeholder normalises to the same text as the whole list;
    * a **row** cut short (`VALUES ($1, $2), ...`) is written twice instead,
      because a `VALUES` run only collapses when its rows are identical — one
      row is a statement of its own, and granting it would not match what the
      app sent.

    Normalising the result gives this function's input back, so a grant written
    from the log is the grant dbproxy compares, and the analysis has something
    to parse.
    """
    out: list[str] = []
    index = 0
    while index < len(sql):
        char = sql[index]
        if char in ("'", '"'):
            end = _quoted_end(sql, index, char)
            out.append(sql[index:end])
            index = end
            continue
        if char == "$":
            tag = _DOLLAR_TAG.match(sql, index)
            if tag is not None:
                end = sql.find(tag.group(0), tag.end())
                end = len(sql) if end < 0 else end + len(tag.group(0))
                out.append(sql[index:end])
                index = end
                continue
            if sql.startswith("$?", index):
                out.append("$1")
                index += 2
                continue
            out.append(char)
            index += 1
            continue
        if char == "(":
            end = _group_end(sql, index)
            if sql[end - 1 : end] != ")":
                # Not a group after all (an unbalanced parenthesis in text the
                # app sent): carry it as it is rather than guess at its shape.
                out.append(char)
                index += 1
                continue
            group = "(" + parseable_sql(sql[index + 1 : end - 1]) + ")"
            collapsed = _collapsed_run(sql, end)
            if collapsed is not None:
                # A run of identical rows: writing the row twice normalises to
                # the same cut-short text, and parses.
                out.append(group)
                out.append(", ")
                out.append(group)
                index = collapsed
                continue
            out.append(group)
            index = end
            continue
        if char == ",":
            collapsed = _collapsed_run(sql, index)
            if collapsed is not None:
                # A cut-short placeholder list (`IN ($?, ...)`): one placeholder
                # is the same statement to the normaliser.
                index = collapsed
                continue
        out.append(char)
        index += 1
    return "".join(out)


def _collapsed_run(sql: str, index: int) -> int | None:
    """Where a `, ...` marker starting at `index` ends, or `None`.

    The marker is what the normaliser writes for a collapsed run (§6,
    "Matching"): `, ...` with nothing in between, at the position it collapsed.
    """
    scan = index
    while scan < len(sql) and sql[scan] == " ":
        scan += 1
    if not sql.startswith(",", scan):
        return None
    scan += 1
    while scan < len(sql) and sql[scan] == " ":
        scan += 1
    if not sql.startswith("...", scan):
        return None
    return scan + 3


def _group_end(sql: str, index: int) -> int:
    """The position just after the parenthesised group that opens at `index`."""
    depth = 0
    scan = index
    while scan < len(sql):
        char = sql[scan]
        if char in ("'", '"'):
            scan = _quoted_end(sql, scan, char)
            continue
        if char == "$":
            tag = _DOLLAR_TAG.match(sql, scan)
            if tag is not None:
                end = sql.find(tag.group(0), tag.end())
                scan = len(sql) if end < 0 else end + len(tag.group(0))
                continue
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return scan + 1
        scan += 1
    return len(sql)


def _quoted_end(sql: str, index: int, quote: str) -> int:
    """The position just after the quoted region that opens at `index`."""
    scan = index + 1
    while scan < len(sql):
        if sql[scan] == quote:
            if scan + 1 < len(sql) and sql[scan + 1] == quote:
                scan += 2
                continue
            return scan + 1
        scan += 1
    return len(sql)
