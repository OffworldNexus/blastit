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
from collections.abc import Iterable
from typing import Any

__all__ = [
    "SQL_CODE",
    "SQL_COMMENT",
    "SQL_IDENT",
    "SQL_LITERAL",
    "denials",
    "parse_events",
    "parseable_sql",
    "routed_requests",
    "sql_regions",
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
            (route, method, path),
            {"route": route, "method": method, "path": path, "count": 0},
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
        entry = seen.setdefault(
            (method, path), {"method": method, "path": path, "count": 0}
        )
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
# PostgreSQL 18 `scan.l` region lexer & normalised SQL reconstruction.
#

SQL_CODE = "code"
SQL_LITERAL = "literal"
SQL_IDENT = "ident"
SQL_COMMENT = "comment"


def _is_ident_start(char: str) -> bool:
    """scan.l `ident_start` / `dolq_start`: `[A-Za-z\\200-\\377_]`."""
    return (char.isascii() and char.isalpha()) or ord(char) >= 0x80 or char == "_"


def _is_ident_cont(char: str) -> bool:
    """scan.l `ident_cont`: `[A-Za-z\\200-\\377_0-9$]`."""
    return _is_ident_start(char) or (char.isascii() and char.isdigit()) or char == "$"


def _is_dolq_cont(char: str) -> bool:
    """scan.l `dolq_cont`: `[A-Za-z\\200-\\377_0-9]` (`ident_cont` without `$`)."""
    return _is_ident_start(char) or (char.isascii() and char.isdigit())


def _decimal_end(sql: str, index: int) -> int | None:
    """End of `{decinteger}` = `{decdigit}(_?{decdigit})*` at `sql[index]`."""
    if index >= len(sql) or not (sql[index].isascii() and sql[index].isdigit()):
        return None
    cursor = index + 1
    while True:
        probe = cursor + 1 if cursor < len(sql) and sql[cursor] == "_" else cursor
        if probe < len(sql) and sql[probe].isascii() and sql[probe].isdigit():
            cursor = probe + 1
        else:
            break
    return cursor


def _radix_end(sql: str, start: int, digits: str) -> int:
    """End of a `0x`/`0o`/`0b` literal, or its `0[xob]_?` fail fallback."""
    cursor = start
    matched = False
    while True:
        probe = cursor + 1 if cursor < len(sql) and sql[cursor] == "_" else cursor
        if probe < len(sql) and sql[probe] in digits:
            cursor = probe + 1
            matched = True
        else:
            break
    if matched:
        return cursor
    if start < len(sql) and sql[start] == "_":
        return start + 1
    return start


def _number_end(sql: str, index: int) -> int | None:
    """End of a numeric literal starting at `sql[index]`, following `scan.l`."""
    if index < len(sql) and sql[index] == "0" and index + 1 < len(sql):
        prefix = sql[index + 1]
        if prefix in ("x", "X"):
            return _radix_end(sql, index + 2, "0123456789abcdefABCDEF")
        if prefix in ("o", "O"):
            return _radix_end(sql, index + 2, "01234567")
        if prefix in ("b", "B"):
            return _radix_end(sql, index + 2, "01")

    if index < len(sql) and sql[index] == ".":
        end = _decimal_end(sql, index + 1)
        if end is None:
            return None
    else:
        integer = _decimal_end(sql, index)
        if integer is None:
            return None
        if integer < len(sql) and sql[integer] == ".":
            if integer + 1 < len(sql) and sql[integer + 1] == ".":
                return integer
            fraction = _decimal_end(sql, integer + 1)
            end = integer + 1 if fraction is None else fraction
        else:
            end = integer

    if end < len(sql) and sql[end] in ("e", "E"):
        sign = end + 2 if end + 1 < len(sql) and sql[end + 1] in ("+", "-") else end + 1
        exponent = _decimal_end(sql, sign)
        if exponent is not None:
            end = exponent
    return end


def _code_token_end(sql: str, index: int) -> int:
    """End of the code token starting at `sql[index]` (`blastit_sqlpolicy::lexer`)."""
    if _is_ident_start(sql[index]):
        cursor = index + 1
        while cursor < len(sql) and _is_ident_cont(sql[cursor]):
            cursor += 1
        return cursor

    numeric = _number_end(sql, index)
    if numeric is not None:
        cursor = numeric
        if cursor < len(sql) and _is_ident_start(sql[cursor]):
            while cursor < len(sql) and _is_ident_cont(sql[cursor]):
                cursor += 1
        return cursor

    return index + 1


def _skip_doubled(sql: str, index: int, quote: str) -> int:
    """Skips a `''`-or-`""`-style SQL quoted region starting at its opening quote."""
    cursor = index + 1
    while cursor < len(sql):
        if sql[cursor] == quote:
            if cursor + 1 < len(sql) and sql[cursor + 1] == quote:
                cursor += 2
                continue
            return cursor + 1
        cursor += 1
    return cursor


def _skip_escapeless(sql: str, quote_index: int) -> int:
    """Skips `B'…'` or `X'…'` starting at the opening quote (no escape at all)."""
    closing = sql.find("'", quote_index + 1)
    return len(sql) if closing < 0 else closing + 1


def _skip_e_string(sql: str, index: int) -> int:
    """Skips `E'…'` starting at `E`: `\\` escapes next char and `''` escapes `'`."""
    cursor = index + 2
    while cursor < len(sql):
        char = sql[cursor]
        if char == "'":
            if cursor + 1 < len(sql) and sql[cursor + 1] == "'":
                cursor += 2
                continue
            return cursor + 1
        if char == "\\":
            if cursor + 1 >= len(sql):
                return len(sql)
            cursor += 2
            continue
        cursor += 1
    return cursor


def _skip_line_comment(sql: str, index: int) -> int:
    """Skips `--` up to and including `\\n` or `\\r` (`scan.l` `comment`)."""
    cursor = index + 2
    while cursor < len(sql) and sql[cursor] not in ("\n", "\r"):
        cursor += 1
    return cursor + 1 if cursor < len(sql) else cursor


def _skip_block_comment(sql: str, index: int) -> int:
    """Skips `/* … */` starting at `/`, honouring PostgreSQL comment nesting."""
    cursor = index + 2
    depth = 1
    while cursor < len(sql) and depth > 0:
        if sql[cursor] == "/" and cursor + 1 < len(sql) and sql[cursor + 1] == "*":
            depth += 1
            cursor += 2
        elif sql[cursor] == "*" and cursor + 1 < len(sql) and sql[cursor + 1] == "/":
            depth -= 1
            cursor += 2
        else:
            cursor += 1
    return cursor


def _dollar_quote_end(sql: str, index: int) -> int | None:
    """End of `$$…$$` or `$tag$…$tag$` starting at `sql[index]`, or `None`."""
    cursor = index + 1
    if cursor >= len(sql):
        return None
    if sql[cursor] == "$":
        delimiter = "$$"
    elif _is_ident_start(sql[cursor]):
        cursor += 1
        while cursor < len(sql) and _is_dolq_cont(sql[cursor]):
            cursor += 1
        if cursor >= len(sql) or sql[cursor] != "$":
            return None
        delimiter = sql[index : cursor + 1]
    else:
        return None
    closing = sql.find(delimiter, index + len(delimiter))
    return len(sql) if closing < 0 else closing + len(delimiter)


def _construct_at(sql: str, index: int) -> tuple[str, int] | None:
    """`(kind, end)` if a quoted region or comment opens at token start `index`."""
    char = sql[index]
    if char == "'":
        return SQL_LITERAL, _skip_doubled(sql, index, "'")
    if char == '"':
        return SQL_IDENT, _skip_doubled(sql, index, '"')
    if char in ("e", "E") and index + 1 < len(sql) and sql[index + 1] == "'":
        return SQL_LITERAL, _skip_e_string(sql, index)
    if char in ("b", "B", "x", "X") and index + 1 < len(sql) and sql[index + 1] == "'":
        return SQL_LITERAL, _skip_escapeless(sql, index + 1)
    if char in ("n", "N") and index + 1 < len(sql) and sql[index + 1] == "'":
        return SQL_LITERAL, _skip_doubled(sql, index + 1, "'")
    if (
        char in ("u", "U")
        and index + 2 < len(sql)
        and sql[index + 1] == "&"
        and sql[index + 2] == "'"
    ):
        return SQL_LITERAL, _skip_doubled(sql, index + 2, "'")
    if char == "$":
        end = _dollar_quote_end(sql, index)
        if end is not None:
            return SQL_LITERAL, end
    if char == "-" and index + 1 < len(sql) and sql[index + 1] == "-":
        return SQL_COMMENT, _skip_line_comment(sql, index)
    if char == "/" and index + 1 < len(sql) and sql[index + 1] == "*":
        return SQL_COMMENT, _skip_block_comment(sql, index)
    return None


def sql_regions(sql: str) -> list[tuple[str, int, int]]:
    """Splits `sql` into `(kind, start, end)` regions following PG 18 `scan.l`.

    Matches `blastit_sqlpolicy::lexer::lex`: all 8 quoted literal forms (`'…'`,
    `E'…'` with `\\` and `''` escapes, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$`,
    `$tag$…$tag$`), quoted identifiers (`"…"`), `--` and nested `/* … */`
    comments, and `$`-containing identifiers (`a$b$c`) are bounded identically.
    """
    regions: list[tuple[str, int, int]] = []
    code_start = 0
    index = 0
    while index < len(sql):
        found = _construct_at(sql, index)
        if found is not None:
            kind, end = found
            if index > code_start:
                regions.append((SQL_CODE, code_start, index))
            regions.append((kind, index, end))
            index = end
            code_start = end
        else:
            index = _code_token_end(sql, index)
    if code_start < len(sql):
        regions.append((SQL_CODE, code_start, len(sql)))
    return regions


def parseable_sql(sql: str) -> str:
    """Normalised text as a statement PostgreSQL's own parser accepts (D3c).

    The normaliser rewrites two shapes that no parser can read: every bind
    placeholder becomes `$?`, and a list after `IN` or a run of identical
    `VALUES` rows is cut short with `, ...` (§6, "Matching"). Both are exactly
    what a grant should hold, and neither is valid SQL — so
    `blastit._blastit.analyze` refuses the text of a `sql.denied` event.

    This rewrites them back into shapes PostgreSQL accepts, without touching
    quoted text (`'…'`, `E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`), quoted
    identifiers, dollar-quoted strings or comments, where `$?` or `, ...` may be
    data:

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
        found = _construct_at(sql, index)
        if found is not None:
            _, end = found
            out.append(sql[index:end])
            index = end
            continue
        char = sql[index]
        if char == "$" and sql.startswith("$?", index):
            out.append("$1")
            index += 2
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
        token_end = _code_token_end(sql, index)
        out.append(sql[index:token_end])
        index = token_end
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
        found = _construct_at(sql, scan)
        if found is not None:
            _, scan = found
            continue
        char = sql[scan]
        if char == "(":
            depth += 1
            scan += 1
            continue
        if char == ")":
            depth -= 1
            if depth == 0:
                return scan + 1
            scan += 1
            continue
        scan = _code_token_end(sql, scan)
    return len(sql)
