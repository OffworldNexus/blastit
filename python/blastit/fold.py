"""`blastit-manifest fold`: turn the statements a log denied into grants (D3c).

The `Blastfile` is written from a test suite (the pytest plugin), and the test
suite only makes the requests it makes. The rest shows up in production as
`sql.denied` events (§8.1): a 404's redirect lookup, a query whose shape depends
on the data, a code path no test covers. This command reads those logs and
prints the grants the file is missing:

```console
$ blastit-manifest fold app.log --manifest Blastfile
$ blastit-manifest fold app.log --manifest Blastfile --write
$ blastit-manifest fold app.log --manifest Blastfile --prefer-tables
$ python -m blastit.fold app.log --manifest Blastfile
```

It is the other half of the pytest plugin: the plugin checks what the tests do,
this checks what the deployment did. Both use blastit's own SQL normalisation
(`blastit._blastit.normalize`) and the same table-grant analysis
(`blastit._blastit.covering`), so a statement the log denied and a statement a
test ran are compared to the file in exactly one way.

What it folds
-------------

* every `sql.denied` event, by `(view, store)`, with the *normalised* text
  dbproxy logged — which is what a grant has to hold, so it can be pasted in as
  it is;
* never into an `unrestricted` view: such a grant has nothing left to check, so
  a denial there means the file changed under the view, and adding a `query`
  would be a boot error (§6);
* never twice: a statement the view's own grant or one of its traits already
  grants is skipped, and so is a statement its table grant already covers (in
  `--prefer-tables` mode);
* never a statement this build cannot read: `--prefer-tables` keeps those as
  exact `query` lines and says so, with the reason `analyze` gave — a system
  catalogue, an unknown function, a statement kind no table grant describes;
* never a statement that still holds a value: an inline quoted literal, or the
  `…` dbproxy redacted one to, is refused whatever `--allow-dangerous` says,
  because a grant is committed and must hold no value the application supplied
  (the two fixed protocol setup statements are the only literals allowed);
* never a statement of a kind a grant is not an allowlist for: DDL, `SET` and
  `RESET` (a `SET ROLE` above all), `COPY`, `DO`, `CALL`, `LOCK`, `GRANT`,
  `REVOKE`, `TRUNCATE`, `VACUUM`, `LISTEN`/`NOTIFY`, the transaction commands.
  PostgreSQL's own parser names the kind (never a pattern over the text), the
  statement is refused unless `--allow-dangerous` is given, and either way it is
  listed on stderr with the reason.

Nothing a log says is trusted as file text. A `route` names a view the
`Blastfile` already declares, a `proxy` a store it already declares *and* that
the view is granted; fold never creates a view, and it refuses — rather than
writes — a store name the Blastfile grammar cannot spell. The reason a denial
carries is printed on stderr and never written into the file.

Lines that are not events are skipped, and so is an App Platform prefix in front
of one (`2026-10-25T14:02:11.384Z app[web.1]: {…}`): see
[`blastit.logs`](logs.py).

Reading and writing
-------------------

There is exactly one `Blastfile` reader and one editor, and both are Rust's
(§15.2): the file is read with `blastit._blastit.blastfile_model` and written
with `blastit._blastit.blastfile_apply_edits`. This module plans against the
lowered model and hands the editor a `BlastfileEdits` document; it never splits,
patches or concatenates `Blastfile` text.

Without `--write` the file is not touched: the directives to paste inside each
view's `postgres [store] { }` block are printed, and the edit is built and proved
first, so a snippet that could not be written is never printed. With `--write`
the editor's proved result replaces the file. The editor re-parses, re-lowers and
compares the whole document with the model the proposal intends (P4), so a
comment, a blank line, the order of the views and every other byte the edit did
not touch survive; then blastit's own `audit` is run on the edited text with the
log's traffic as observations, which proves it parses, validates, and grants
every statement the plan added. An edit that fails any of that is refused, and
the file is left exactly as it was.

The file is replaced atomically: the edit goes to a sibling `.<name>.<pid>.tmp`,
is `fsync`ed, renamed over the file, and the directory is `fsync`ed after, so an
interrupted run leaves the old file or the new one and no temp file.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from blastit import logs

__all__ = ["main"]


def main(argv: Sequence[str] | None = None) -> int:
    """`blastit-manifest fold LOG... --manifest Blastfile [--write] [--prefer-tables]
    [--allow-dangerous]`.

    The console script's spelling: `blastit-manifest` is a command with
    subcommands, and `fold` is the one it has. `python -m blastit.fold` *is* this
    tool, so it takes the same arguments without the word `fold` (see
    [`_direct`]).

    Returns the process exit status: 0 when it printed (or wrote) what the
    `Blastfile` was missing — an empty diff is a normal, good outcome — and 1 on
    anything it could not do (an unreadable log, a file it cannot parse, edit or
    prove safe).
    """
    return _run(_parser(), argv)


def _direct(argv: Sequence[str] | None = None) -> int:
    """`python -m blastit.fold LOG... --manifest Blastfile …`.

    The same fold, without a subcommand to name: a module invoked with `-m` takes
    its own arguments, and `python -m blastit.fold` is what a deployment has when
    the package is importable but the console script is not.
    """
    parser = argparse.ArgumentParser(
        prog="python -m blastit.fold",
        description="Read blastit's JSON logs and the Blastfile, and say what grants are missing.",
    )
    _fold_arguments(parser)
    parser.set_defaults(handler=_fold)
    return _run(parser, argv)


def _run(parser: argparse.ArgumentParser, argv: Sequence[str] | None) -> int:
    """Parses one spelling's arguments, and runs the handler they select."""
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except _FoldError as error:
        print(f"blastit-manifest fold: {error}", file=sys.stderr)
        return 1


def _parser() -> argparse.ArgumentParser:
    """The `blastit-manifest` command line: one subcommand today, `fold`."""
    parser = argparse.ArgumentParser(
        prog="blastit-manifest",
        description="Read blastit's JSON logs and the Blastfile, and say what grants are missing.",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)
    fold = subcommands.add_parser(
        "fold",
        help="print (or write) the grants the statements in LOG(s) were denied for",
        description=__doc__.splitlines()[0],
    )
    _fold_arguments(fold)
    fold.set_defaults(handler=_fold)
    return parser


def _fold_arguments(fold: argparse.ArgumentParser) -> None:
    """Fold's own options, shared by both spellings of the command."""
    fold.add_argument(
        "logs",
        nargs="+",
        metavar="LOG",
        help="a blastit log to read; `-` reads standard input",
    )
    fold.add_argument(
        "--manifest",
        required=True,
        metavar="PATH",
        help="the Blastfile to extend",
    )
    fold.add_argument(
        "--write",
        action="store_true",
        help="rewrite the Blastfile in place instead of printing the grants",
    )
    fold.add_argument(
        "--prefer-tables",
        action="store_true",
        help="add the smallest select/insert/update/delete scope covering the "
        "denied statements (D7), keeping as exact queries whatever a table grant "
        "cannot describe",
    )
    fold.add_argument(
        "--allow-dangerous",
        action="store_true",
        help="grant statements a grant is not an allowlist for, instead of refusing "
        "them: DDL, SET, COPY, DO, CALL, LOCK, GRANT, TRUNCATE, VACUUM, LISTEN and the "
        "transaction commands, and statements calling a function outside blastit's "
        "safe set (an exact `query` entry runs whatever it calls)",
    )


class _FoldError(Exception):
    """Something that stops the fold: a log or a `Blastfile` that cannot be read."""


@dataclass
class _Denial:
    """One statement a view was denied, as the log reported it."""

    route: str
    proxy: str
    sql: str
    reason: str
    count: int = 1

    @property
    def parseable(self) -> str:
        """The same statement as SQL PostgreSQL's parser accepts.

        The log holds the *normalised* text (`$?`, collapsed `IN` lists and
        `VALUES` runs), which no parser can read; the analysis needs the shape,
        not the numbering, so it gets this form — and the normalised text is
        what the report and the `Blastfile` keep.
        """
        return logs.parseable_sql(self.sql)


@dataclass
class _Addition:
    """What one `(view, store)` grant is missing."""

    route: str
    proxy: str
    #: The exact statements to add, as the log wrote them, with their reasons.
    queries: list[tuple[str, str]] = field(default_factory=list)
    #: The view's whole table scope, existing entries merged with new ones.
    tables: dict[str, dict[str, Any]] = field(default_factory=dict)
    #: `functions` entries beyond the safe set, the view's own included.
    functions: list[str] = field(default_factory=list)
    #: Every statement this addition makes runnable, as the log wrote it. The
    #: verification replays them through `blastit._blastit.audit` on the edited
    #: text and checks each one is granted there (D3c).
    statements: list[str] = field(default_factory=list)
    #: What already grants the view that store, when it is not the view's own
    #: `postgres` block: `the trait 'session'` (D3c). The edit names it in its
    #: refusal, because a trait's grant is not something fold writes into.
    via: str | None = None
    #: What was left out and why, for the summary.
    notes: list[str] = field(default_factory=list)

    def is_empty(self) -> bool:
        """Whether there is nothing to write."""
        return not self.queries and not self.tables and not self.functions


#
# Reading the logs.
#


def read_events(logs_or_stdin: Iterable[str]) -> list[dict[str, Any]]:
    """Every event the logs hold, in the order they were read (D3c).

    `-` reads standard input, so a live pipeline works:
    `ssh app blastit | blastit-manifest fold - --manifest Blastfile`. Lines that
    are not events are skipped — a log carries the app's own output, and an App
    Platform prefix in front of every line (`blastit.logs` knows both).
    """
    events: list[dict[str, Any]] = []
    for source in logs_or_stdin:
        if source == "-":
            text = sys.stdin.read()
        else:
            path = Path(source)
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError as error:
                raise _FoldError(f"{path}: {error}") from error
        events.extend(logs.parse_events(text.splitlines()))
    return events


def denials_of(events: Iterable[dict[str, Any]]) -> list[_Denial]:
    """The `sql.denied` events, deduplicated by `(view, store, statement)`."""
    denials: list[_Denial] = []
    seen: set[tuple[str, str, str]] = set()
    for denial in logs.denials(events):
        key = (denial["route"], denial["proxy"], denial["sql"])
        if key in seen:
            continue
        seen.add(key)
        denials.append(
            _Denial(
                route=denial["route"],
                proxy=denial["proxy"],
                sql=denial["sql"],
                reason=denial["reason"],
                count=denial["count"],
            )
        )
    return denials


def read_denials(logs_or_stdin: Iterable[str]) -> list[_Denial]:
    """The denials every log holds: [`read_events`] then [`denials_of`]."""
    return denials_of(read_events(logs_or_stdin))


#
# Reading the Blastfile: the model the extension hands over.
#


def model(source: str) -> dict[str, Any]:
    """The lowered `Blastfile` model, from `blastit._blastit.blastfile_model`.

    `{"stores": [...], "traits": {...}, "views": [...]}` — the same document the
    binding documents. There is one `Blastfile` reader (§15.2), and this is how
    Python reaches it; nothing here parses the text.
    """
    from blastit import _blastit

    try:
        return json.loads(_blastit.blastfile_model(source))
    except ValueError as error:
        raise _FoldError(f"not a Blastfile: {error}") from error


def _views(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The model's views by name, in declaration order."""
    views: dict[str, dict[str, Any]] = {}
    for view in document.get("views") or []:
        if isinstance(view, dict) and isinstance(view.get("name"), str):
            views[str(view["name"])] = view
    return views


def _traits(document: dict[str, Any]) -> dict[str, Any]:
    """The model's traits by name."""
    traits = document.get("traits")
    return traits if isinstance(traits, dict) else {}


def _stores(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The model's stores by the name a grant and a log use."""
    stores: dict[str, dict[str, Any]] = {}
    for store in document.get("stores") or []:
        if isinstance(store, dict) and isinstance(store.get("name"), str):
            stores[str(store["name"])] = store
    return stores


#
# Planning: what the Blastfile is missing.
#


def plan(
    document: dict[str, Any],
    denials: list[_Denial],
    *,
    prefer_tables: bool,
    allow_dangerous: bool = False,
    covering: Callable[[list[str], str], str] | None = None,
    classify: Callable[[str], str] | None = None,
    describe: Callable[[str], str] | None = None,
) -> list[_Addition]:
    """What each `(view, store)` grant has to gain, in view order.

    `document` is the model [`model`] returns. `covering`, `classify` and
    `describe` are injectable for tests and default to
    `blastit._blastit.covering`, `.classify` and `.analyze`; `allow_dangerous`
    lets through a statement a grant is not an allowlist for — another kind than
    `SELECT`/`INSERT`/`UPDATE`/`DELETE`, or one calling a function outside the
    safe set (D3c) — which is off by default because a `SET ROLE`, a `COPY`, a
    `CREATE TABLE` or a `pg_read_file` in a grant is a hole, not a fix.
    """
    views = _views(document)
    traits = _traits(document)
    stores = _stores(document)
    by_pair: dict[tuple[str, str], list[_Denial]] = {}
    for denial in denials:
        by_pair.setdefault((denial.route, denial.proxy), []).append(denial)
    results: list[_Addition] = []
    for route_name, proxy in _order(denials, views):
        view = views.get(route_name)
        addition = _Addition(route=route_name, proxy=proxy)
        if view is None:
            addition.notes.append(
                f"{route_name}: the Blastfile declares no view of that name — fold extends "
                "the views a file has, it never creates one"
            )
            results.append(addition)
            continue
        # Every value here comes out of a log line, which an application can
        # influence: a `proxy` that is not a store of this file, or a `route`
        # that is not a view of it, is a log from elsewhere — or a forgery —
        # and nothing of it may reach the file (D3c).
        if _store_name(proxy) is None:
            addition.notes.append(
                f"{route_name}: the log names the store {proxy!r}, which is not a name "
                "the Blastfile grammar can spell; fold cannot write a grant for it"
            )
            results.append(addition)
            continue
        if proxy not in stores:
            addition.notes.append(
                f"{route_name}: the log names the store {proxy!r}, which this Blastfile "
                "does not declare"
            )
            results.append(addition)
            continue
        grant = _own_grant(view, proxy)
        if grant is not None and grant.get("unrestricted") is True:
            addition.notes.append(
                f"{route_name} ({proxy}): the view is unrestricted — it may run anything, "
                "so there is nothing to grant (a `query` next to `unrestricted` is a "
                "boot error, §6)"
            )
            results.append(addition)
            continue
        via = _granted_via(proxy, grant, traits, view)
        if via == NO_GRANT:
            addition.notes.append(
                f"{route_name} ({proxy}): the view has no grant on that store — a view "
                "only gets a listener for a store it is granted (§2.2), so the log is "
                "older than the Blastfile; add the grant by hand, then fold again"
            )
            results.append(addition)
            continue
        addition = _plan_route(
            view=view,
            route_name=route_name,
            proxy=proxy,
            grant=grant,
            traits=traits,
            denials=by_pair[(route_name, proxy)],
            prefer_tables=prefer_tables,
            allow_dangerous=allow_dangerous,
            covering=covering,
            classify=classify,
            describe=describe,
        )
        addition.via = via
        if via is not None:
            addition.notes.append(
                f"{route_name} ({proxy}): {via} grants that store, and fold writes the "
                "view's own block, which unions with the trait (§6)"
            )
        results.append(addition)
    return results


def _plan_route(
    *,
    view: dict[str, Any],
    route_name: str,
    proxy: str,
    grant: dict[str, Any] | None,
    traits: dict[str, Any],
    denials: list[_Denial],
    prefer_tables: bool,
    allow_dangerous: bool,
    covering: Callable[[list[str], str], str] | None,
    classify: Callable[[str], str] | None,
    describe: Callable[[str], str] | None,
) -> _Addition:
    """What one view's grant is missing, from its denials."""
    addition = _Addition(route=route_name, proxy=proxy)
    existing = _granted_queries(proxy, grant, traits, view)
    judge = classify or _classify
    analyse = describe or _analyze
    new: list[_Denial] = []
    seen: set[str] = set()
    for denial in denials:
        # Every note quotes the statement back, so a person can see which query
        # was refused; the literal spans are redacted first (ADR-0003).
        shown = _redact_literal(denial.sql)
        normalised = _normalise(denial.sql)
        source = existing.get(normalised)
        if source is not None:
            addition.notes.append(
                f"{shown!r}: already granted by {source} of {route_name} — the log is "
                "from an older Blastfile, or the statement did not need the grant"
            )
            continue
        # Two spellings of one statement are one grant: the log's own text is
        # already normalised, but a log from another build, or a hand-edited
        # one, may not be.
        if normalised in seen:
            continue
        seen.add(normalised)
        # `…` is a value dbproxy redacted out of the statement: the value is
        # gone, so the statement can never match another request. It is checked
        # before the parser, exactly as `check_learnable` does, because a
        # redacted statement is not SQL any build can read (R25). This is not
        # something `--allow-dangerous` reaches.
        if "…" in normalised:
            addition.notes.append(
                f"{shown!r}: refused — it holds `…`, the redaction of a value the "
                "application inlined, so it can never match another request; bind the "
                "value as a parameter and fold the statement again"
            )
            continue
        # What kind of statement this is decides whether a grant may hold it at
        # all; PostgreSQL's own parser answers, never a guess from the text.
        kind = _kind(judge, denial)
        if kind["statements"] > 1:
            # A grant is one statement, and splitting a text is something only a
            # person can do: `--allow-dangerous` does not reach this (D3c).
            addition.notes.append(
                f"{shown!r}: refused — {kind['label']}, and a grant is one statement; "
                "split the text and fold it again"
            )
            continue
        if kind["label"] == "unreadable":
            why = f" ({kind['reason']})" if kind["reason"] else ""
            addition.notes.append(
                f"{shown!r}: refused — unreadable SQL{why}; a grant for text that is "
                "not valid SQL can never match a request"
            )
            continue
        if not kind["plain"] and not allow_dangerous:
            why = f" ({kind['reason']})" if kind["reason"] else ""
            addition.notes.append(
                f"{shown!r}: refused — {kind['label']} is not a plain SELECT, INSERT, "
                f"UPDATE or DELETE{why}; pass --allow-dangerous to grant it anyway"
            )
            continue
        # A committed file must never hold a value the application supplied
        # (ADR-0003): a quoted literal that survived a log from another build is
        # the same leak `…` is. The two protocol setup statements are the only
        # exception; `--allow-dangerous` does not reach this either.
        if normalised not in _KNOWN_STATIC_SETUP_SQL and _has_quoted_literal(
            normalised
        ):
            addition.notes.append(
                f"{shown!r}: refused — it inlines a quoted literal, which a committed "
                "Blastfile must never hold; bind the value as a parameter and fold the "
                "statement again"
            )
            continue
        # An exact `query` entry is trusted as it is: whatever function it calls
        # runs, which is why a statement calling one outside the safe set is
        # refused here unless the operator asks for it (D3c). The analysis is
        # asked, not the text: it is the same `pg_query` walk the policy runs,
        # and a statement it refuses (a system catalogue, a construct it cannot
        # walk) has nothing to say about its functions.
        try:
            analysis = json.loads(analyse(denial.parseable))
        except ValueError:
            analysis = {}
        functions = _text_list(analysis.get("functions"))
        if functions and not allow_dangerous:
            addition.notes.append(
                f"{shown!r}: refused — it calls {', '.join(functions)}, which is outside "
                "blastit's safe set, and an exact `query` entry grants it as it is; pass "
                "--allow-dangerous to grant it anyway"
            )
            continue
        if not kind["plain"]:
            addition.notes.append(
                f"{shown!r}: granted although it is {kind['label']}, which is not a "
                "plain SELECT, INSERT, UPDATE or DELETE (--allow-dangerous)"
            )
        if functions:
            addition.notes.append(
                f"{shown!r}: granted although it calls {', '.join(functions)}, which is "
                "outside blastit's safe set (--allow-dangerous)"
            )
        new.append(denial)
    if not new:
        return addition

    cover = covering or _covering
    locator = (
        f"view {route_name} postgres"
        if proxy == DEFAULT_STORE
        else f"view {route_name} postgres {proxy}"
    )
    existing_tables = (
        grant.get("tables") if grant and isinstance(grant.get("tables"), dict) else {}
    )
    existing_functions = _text_list(grant.get("functions") if grant else None)
    kept: list[_Denial] = []
    for denial in new:
        if existing_tables or existing_functions:
            single = json.loads(cover([denial.parseable], locator))
            if not single["unanalysable"] and _covered_by(
                existing_tables, single, existing_functions
            ):
                addition.notes.append(
                    f"{_redact_literal(denial.sql)!r}: already covered by the view's "
                    f"table grant on {proxy}"
                )
                continue
        kept.append(denial)
    if not kept:
        return addition
    addition.statements = [denial.sql for denial in kept]

    if not prefer_tables:
        addition.queries = [(denial.sql, denial.reason) for denial in kept]
        return addition

    merged = json.loads(cover([denial.parseable for denial in kept], locator))
    rejected = {entry["sql"] for entry in merged["unanalysable"]}
    reasons = {
        denial.parseable: entry["reason"]
        for entry in merged["unanalysable"]
        for denial in kept
        if denial.parseable == entry["sql"]
    }
    for denial in kept:
        if denial.parseable in rejected:
            addition.queries.append(
                (denial.sql, reasons.get(denial.parseable, denial.reason))
            )
    addition.tables = _merge_tables(existing_tables, merged["tables"])
    addition.functions = sorted(
        set(_text_list(grant.get("functions") if grant else None))
        | set(merged["functions"])
    )
    return addition


#: The name a `Blastfile` gives the store written without one (§7.4).
DEFAULT_STORE = "db"

#: The fixed protocol setup statements blastit sends (R25, ADR-0003): their
#: literals are constants, so they are the only statements fold grants as they are.
_KNOWN_STATIC_SETUP_SQL = frozenset(
    {
        "SET TIME ZONE 'UTC'",
        "SELECT set_config('TimeZone', $?, false)",
    }
)


def _has_quoted_literal(sql: str) -> bool:
    """Whether `sql` holds any of PostgreSQL 18's 8 quoted literal forms (R24, R25).

    [`blastit_sqlpolicy::has_quoted_literal`] through the compiled extension:
    `'…'`, `E'…'`, `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$` and `$tag$…$tag$`
    count (including unterminated spans), while quoted identifiers (`"…"`,
    `U&"…"`), `$` inside identifiers (`a$b$c`), placeholders (`$1`, `$?`) and
    comments do not. The lexer is Rust's; nothing here re-derives it.
    """
    from blastit import _blastit

    return _blastit.has_quoted_literal(sql)


def _redact_literal(sql: str) -> str:
    """`sql` with every quoted literal token (prefix included) replaced by `…`.

    [`blastit_sqlpolicy::redact_literals`] through the compiled extension (R24,
    ADR-0003): all 8 PostgreSQL literal forms (`'…'`, `E'…'` with `\\` escapes,
    `B'…'`, `X'…'`, `N'…'`, `U&'…'`, `$$…$$` and `$tag$…$tag$`) collapse to a
    single `…` with no surviving prefix or tag, while comments, quoted
    identifiers and exact [`_KNOWN_STATIC_SETUP_SQL`] entries are preserved
    verbatim.
    """
    from blastit import _blastit

    return _blastit.redact_literals(sql)


def _sanitize_parser_message(message: str) -> str:
    """Strips the offending token `libpg_query` quotes (`at or near "…"`)."""
    marker = message.find(" at or near ")
    head = message[:marker] if marker >= 0 else message
    out: list[str] = []
    quoted = False
    for char in head:
        if char == '"':
            if quoted:
                out.append("…")
            quoted = not quoted
            out.append('"')
        elif not quoted:
            out.append(char)
    if quoted:
        out.append("…")
    return "".join(out)


def _kind(judge: Callable[[str], str], denial: _Denial) -> dict[str, Any]:
    """What kind of statement a denial is, from PostgreSQL's own parser.

    The log holds the normalised text, and the parser needs the statement, so
    [`logs.parseable_sql`] is what the classification sees: the kind, how many
    statements the text holds, and whether a grant may hold it at all. A
    statement nothing can parse is refused and named — a grant for text that is
    not SQL is a grant nothing will ever match.

    Only the parser's refusal is caught: anything else (a broken installation,
    no extension at all) is a failure of the tool, not a verdict about a log.
    """
    try:
        verdict = json.loads(judge(denial.parseable))
    except ValueError as error:
        return {
            "plain": False,
            "label": "unreadable",
            "reason": _sanitize_parser_message(str(error)),
            "statements": 1,
        }
    verdict.setdefault("reason", "")
    verdict.setdefault("statements", 1)
    return verdict


def _order(
    denials: list[_Denial], views: dict[str, dict[str, Any]]
) -> list[tuple[str, str]]:
    """The `(view, store)` pairs to report on, in view order.

    View order, not log order: a diff a reader can follow, and the same order
    the pytest plugin's report uses. A view the file does not declare comes
    last, where it can be seen.
    """
    seen: list[tuple[str, str]] = []
    for denial in denials:
        pair = (denial.route, denial.proxy)
        if pair not in seen:
            seen.append(pair)
    declared = [pair for pair in seen if pair[0] in views]
    undeclared = [pair for pair in seen if pair[0] not in views]
    order = {name: index for index, name in enumerate(views)}
    declared.sort(key=lambda pair: (order[pair[0]], pair[1]))
    return declared + undeclared


def _own_grant(view: dict[str, Any], proxy: str) -> dict[str, Any] | None:
    """The view's own `postgres [store] { }` grant, or `None`."""
    grants = view.get("grants")
    if not isinstance(grants, dict):
        return None
    grant = grants.get(proxy)
    return grant if isinstance(grant, dict) else None


def _trait_grant(trait: Any, proxy: str) -> Any:
    """The `postgres [store] { }` grant of a trait, or `None`."""
    if not isinstance(trait, dict):
        return None
    return trait.get(proxy)


def _granted_queries(
    proxy: str,
    grant: dict[str, Any] | None,
    traits: dict[str, Any],
    view: dict[str, Any],
) -> dict[str, str]:
    """Every statement the view is already granted, normalised: what grants it.

    The effective grant of §6: what the view writes itself and every trait it
    `use`s. The source is kept so the summary can say *why* a denial in the log
    is not a grant to add.
    """
    sources: dict[str, str] = {}
    for name in view.get("use") or []:
        _collect_queries(
            _trait_grant(traits.get(str(name)), proxy), sources, f"the trait {name!r}"
        )
    _collect_queries(grant, sources, "the view's own grant")
    return sources


def _granted_via(
    proxy: str,
    grant: dict[str, Any] | None,
    traits: dict[str, Any],
    view: dict[str, Any],
) -> str | None:
    """What grants a view one store: its own block, a trait, or nothing (§6).

    A view only gets a listener for a store it is granted (§2.2), so a denial
    naming a store the view is not granted is a log of another file — and fold
    will not invent the grant for it. What the label is for is the *other* case:
    a view granted through a trait has no `postgres` block to write into, and a
    refusal that names the trait explains what to do about it.
    """
    if isinstance(grant, dict):
        return None
    for name in view.get("use") or []:
        if isinstance(_trait_grant(traits.get(str(name)), proxy), dict):
            return f"the trait {str(name)!r}"
    return NO_GRANT


def _store_name(name: str) -> str | None:
    """The name when the `Blastfile` grammar can spell it, or `None`.

    A view, trait or store name is `[a-z0-9][a-z0-9_-]*` (§6.7). The log's
    `proxy` is written into a grant, and a name that needs quoting is one fold
    says it cannot write, never one it writes wrong.
    """
    return name if re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name) else None


def _collect_queries(grant: Any, sources: dict[str, str], label: str) -> None:
    """Adds a grant's queries to `sources`, keeping the first label seen."""
    if not isinstance(grant, dict):
        return
    for query in _text_list(grant.get("queries")):
        sources.setdefault(_normalise(query), label)


def _normalise(sql: str) -> str:
    """blastit's own normalisation, from the compiled extension."""
    from blastit import _blastit

    return _blastit.normalize(sql)


def _covering(statements: list[str], locator: str) -> str:
    """`blastit._blastit.covering`: the grant that covers a set of statements."""
    from blastit import _blastit

    return _blastit.covering(statements, locator)


def _analyze(sql: str) -> str:
    """`blastit._blastit.analyze`: what a table grant would have to cover (D7).

    The same `pg_query` walk the policy runs: it reports the tables and columns a
    statement touches, and the functions it calls beyond blastit's safe set —
    which is what makes a statement like `SELECT set_config(…)` dangerous to
    grant as it stands.
    """
    from blastit import _blastit

    return _blastit.analyze(sql)


def _classify(sql: str) -> str:
    """`blastit._blastit.classify`: PostgreSQL's own parser naming the statement.

    The same parser the analysis uses, answering for *every* kind — the four a
    grant can hold, and the rest (DDL, `SET`, `COPY`, `DO`, `CALL`, `LOCK`,
    `GRANT`, `TRUNCATE`, `VACUUM`, `LISTEN`, the transaction commands) — which
    is what a tool that offers to write a grant has to know before it does.
    """
    from blastit import _blastit

    return _blastit.classify(sql)


def _text_list(value: Any) -> list[str]:
    """A JSON array of strings, or nothing when it is not one."""
    if not isinstance(value, list):
        return []
    return [str(item) for item in value]


#
# Table grants: merging and covering.
#


def _clean(grant: Any) -> dict[str, Any]:
    """One table grant without its empty entries.

    `covering` renders every operation it knows (`insert = null` and
    `delete = false` included, the shape of the report's JSON); a `Blastfile`
    writes only what it grants.
    """
    if not isinstance(grant, dict):
        return {}
    cleaned: dict[str, Any] = {}
    for operation, value in grant.items():
        if value is None or value is False:
            continue
        if operation == "delete":
            cleaned["delete"] = True
            continue
        if value == "*":
            cleaned[operation] = "*"
            continue
        if isinstance(value, list):
            cleaned[operation] = sorted({str(column) for column in value})
    return cleaned


def _merge_tables(existing: Any, new: Any) -> dict[str, dict[str, Any]]:
    """Unions two table grants the way §6 does: `"*"` absorbs a column list.

    `existing` is the view's table scope as the `Blastfile` holds it (which may
    use `"*"` or sub-tables), `new` the covering grant of the statements being
    added. The result is what the `postgres` block should hold, and it keeps
    every table the file already had.
    """
    merged: dict[str, dict[str, Any]] = {}
    if isinstance(existing, dict):
        for table, grant in existing.items():
            merged[str(table)] = _clean(grant)
    if isinstance(new, dict):
        for table, grant in new.items():
            target = merged.setdefault(str(table), {})
            for operation, value in _clean(grant).items():
                if operation == "delete":
                    target["delete"] = True
                elif value == "*" or target.get(operation) == "*":
                    target[operation] = "*"
                else:
                    target[operation] = sorted(
                        set(target.get(operation) or []) | set(value)
                    )
    return merged


def _covered_by(existing: Any, covering: Any, functions: list[str]) -> bool:
    """Whether a view's own table grant already covers a statement (D7).

    Deliberately conservative: it answers "yes" only when every column *and*
    every function the new grant names is named by the existing one, `"*"`
    included. A lock's `update` on one column is compared as a column list like
    any other, and anything this does not understand — a table the file does
    not mention, an entry that names no column — is a "no", because reporting a
    statement that is already covered is a nuisance while hiding one is a hole.

    An empty `update` is one of those: it is how a lock that read no column of
    the table comes back from the analysis (D3c), it grants nothing (a lock needs
    a column named), and calling it "covered" would silently drop the statement.
    An empty `select` is not the same thing — naming a table without reading a
    column of it *is* covered by any `select` entry (§6).

    A statement that touches no table at all is only covered when the functions
    it calls are granted: `SELECT st_distance(…)` needs `functions …`, and a
    table grant that names no function does not cover it (§6, D3c).
    """
    if not isinstance(covering, dict):
        return False
    for function in covering.get("functions") or []:
        if str(function) not in functions:
            return False
    tables = covering.get("tables")
    if not isinstance(tables, dict):
        return False
    for table, grant in tables.items():
        if not isinstance(grant, dict):
            continue
        target = existing.get(table) if isinstance(existing, dict) else None
        if not isinstance(target, dict):
            return False
        for operation, value in _clean(grant).items():
            have = target.get(operation)
            if operation == "delete":
                if have is not True:
                    return False
            elif operation == "update" and value == []:
                return False
            elif have == "*":
                continue
            elif isinstance(have, list) and isinstance(value, list):
                if not set(value) <= {str(column) for column in have}:
                    return False
            else:
                return False
    return True


#
# Rendering the directives, and the editor.
#


def _diagnostic(message: str) -> None:
    """One note for a person, on stderr, never as file text."""
    print(f"note: {message}", file=sys.stderr)


def _comment(text: str) -> str:
    """A log field as a comment line: one line, no controls, bounded.

    A route or store name comes out of a log line, which an application can
    influence. A newline there would end the comment and let the rest of the
    text be read as `Blastfile` syntax; a control character is noise; and a
    kilobyte of name is not a comment. So the text is flattened and cut, and the
    result can only ever be a comment.
    """
    flattened = "".join(
        " "
        if character.isspace() or ord(character) < 0x20 or ord(character) == 0x7F
        else character
        for character in text
    )
    return flattened.strip()[:COMMENT_LIMIT]


#: How long a rendered comment may be, in characters.
COMMENT_LIMIT = 160

#: What `_granted_via` answers when nothing grants a view a store.
NO_GRANT = "(nothing)"


def render(additions: list[_Addition]) -> str:
    """The directives to paste, one block per `(view, store)` grant.

    A grant with nothing to add prints nothing at all, and the notes — a denial
    that needs a hand, a statement that is already granted, one that was refused
    — are the caller's to print. What comes out of here is what goes inside the
    view's `postgres [store] { }` block, under a comment naming the view and the
    store, and the caller builds and proves the edit before printing it, so a
    directive that could not be written is never shown.
    """
    blocks: list[str] = []
    for addition in additions:
        if addition.is_empty():
            continue
        lines: list[str] = [
            (
                f"# view {_comment(addition.route)} (store {_comment(addition.proxy)}): "
                "what the log's denials are missing"
            ),
        ]
        if addition.functions:
            lines.append(f"functions {', '.join(addition.functions)}")
        for sql, _reason in addition.queries:
            lines.append(f"query {_blastfile_string(sql)}")
        for table, grant in sorted(addition.tables.items()):
            lines.extend(_table_directives(table, grant))
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks) + ("\n" if blocks else "")


def _blastfile_string(value: str) -> str:
    """A `Blastfile` string: raw when it holds no `'`, escaped otherwise.

    A raw string has no escapes, so `'…'` cannot hold the SQL of a statement
    with a `'` in it; that one is written `"…"`, and §5.1's four escapes
    (`\\"`, `\\\\`, `\\n`, `\\t`) keep the reader able to return it exactly.
    """
    if "'" not in value:
        return f"'{value}'"
    out = ['"']
    for character in value:
        if character == '"':
            out.append('\\"')
        elif character == "\\":
            out.append("\\\\")
        elif character == "\n":
            out.append("\\n")
        elif character == "\t":
            out.append("\\t")
        else:
            out.append(character)
    out.append('"')
    return "".join(out)


def _table_directives(table: str, grant: dict[str, Any]) -> list[str]:
    """One table grant as §13.1's `select`/`insert`/`update`/`delete` lines."""
    directives: list[str] = []
    for operation in ("select", "insert", "update"):
        value = grant.get(operation)
        if value is None:
            continue
        if value == "*":
            directives.append(f"{operation} {table}")
        else:
            columns = ", ".join(str(column) for column in value)
            directives.append(f"{operation} {table}({columns})")
    if grant.get("delete"):
        directives.append(f"delete {table}")
    return directives


def apply_additions(source: str, additions: list[_Addition]) -> str:
    """The `Blastfile` with every addition applied, through the one editor.

    The proposals become one `BlastfileEdits` document and `blastit._blastit.
    blastfile_apply_edits` returns the proved result: it re-parses, re-lowers
    and compares the whole document with the model the proposals intend (P4), so
    comments, ordering and every byte outside the spliced lines are the
    author's. A grant merges into the view's `postgres [store] { }` block — a
    second `select <table>` line would be a duplicate directive, not a union —
    and a repeated proposal adds nothing.

    Raises [`_FoldError`] when the editor refuses the document: a view the file
    does not declare, a store nothing declares, an unrestricted view, a table or
    function name the grammar cannot spell, or a failed postcondition. Nothing is
    written by this function.
    """
    from blastit import _blastit

    edits = {
        "views": [],
        "grants": [
            {
                "view": addition.route,
                "store": None if addition.proxy == DEFAULT_STORE else addition.proxy,
                "queries": [sql for sql, _reason in addition.queries],
                "tables": addition.tables,
                "functions": sorted(addition.functions),
            }
            for addition in additions
            if not addition.is_empty()
        ],
    }
    try:
        return _blastit.blastfile_apply_edits(source, json.dumps(edits))
    except ValueError as error:
        raise _FoldError(str(error)) from error


#
# Writing the file.
#


def _fold(args: argparse.Namespace) -> int:
    """Reads the logs, plans the additions, and prints or writes them."""
    manifest_path = Path(args.manifest)
    try:
        text = manifest_path.read_text(encoding="utf-8")
    except OSError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error
    try:
        document = model(text)
    except _FoldError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error

    events = read_events(args.logs)
    denials = denials_of(events)
    additions = plan(
        document,
        denials,
        prefer_tables=args.prefer_tables,
        allow_dangerous=args.allow_dangerous,
    )
    for addition in additions:
        for note in addition.notes:
            _diagnostic(note)
    wanted = [addition for addition in additions if not addition.is_empty()]
    if not wanted:
        print(f"{manifest_path}: nothing to add", file=sys.stderr)
        return 0
    try:
        edited = apply_additions(text, wanted)
    except _FoldError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error
    _prove(manifest_path, edited, document, wanted, events, denials)
    if args.write:
        _write_atomic(manifest_path, edited)
        for addition in wanted:
            print(
                f"{manifest_path}: {addition.route} ({addition.proxy}): "
                f"{len(addition.queries)} statement(s), "
                f"{len(addition.tables)} table(s) and "
                f"{len(addition.functions)} function(s) added",
                file=sys.stderr,
            )
        return 0
    print(render(wanted), end="")
    return 0


def _write_atomic(path: Path, text: str) -> None:
    """Replaces `path` with `text` through a sibling `.<name>.<pid>.tmp` (R29).

    The temp file is created `O_CREAT | O_EXCL | O_NOFOLLOW`, so an existing
    file — or a symlink planted at the predictable temp name by anyone who can
    write the directory — is never written through; it is removed first and a
    fresh regular file is created in its place. The bytes are `fsync`ed before
    `os.replace` renames the temp file over the `Blastfile`, and the directory is
    `fsync`ed after, so a crash leaves either the old file or the new one —
    never half of either. An error unlinks the temp file: a `.tmp` left next to
    a `Blastfile` is a `guard.py` finding, and the next `git status` would show
    it. A file that could not be replaced is reported, not swallowed, and the
    old file is still the file.
    """
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        try:
            existing_mode: int | None = stat.S_IMODE(path.stat().st_mode)
        except FileNotFoundError:
            existing_mode = None
        try:
            # A temp file from a crashed run of this very pid (pid reuse) is
            # stale and ours to remove; there is no second writer with this
            # pid. A symlink planted here is removed too, never followed.
            temporary.unlink()
        except FileNotFoundError:
            pass
        descriptor = os.open(
            temporary,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o600 if existing_mode is not None else 0o666,
        )
        try:
            handle = os.fdopen(descriptor, "w", encoding="utf-8")
        except OSError:
            os.close(descriptor)
            raise
        with handle:
            handle.write(text)
            handle.flush()
            if existing_mode is not None:
                if hasattr(os, "fchmod"):
                    os.fchmod(handle.fileno(), existing_mode)
                else:
                    os.chmod(temporary, existing_mode)
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        _fsync_directory(path.parent)
    except OSError as error:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
        raise _FoldError(
            f"{path}: could not write the edit ({error}); the Blastfile is unchanged"
        ) from error


def _fsync_directory(directory: Path) -> None:
    """`fsync`s a directory, so the rename itself is durable (R29).

    Best effort: a filesystem that cannot open or sync a directory (some network
    mounts) is not a reason to fail an edit that already renamed the file.
    """
    try:
        descriptor = os.open(directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    except OSError:
        return
    try:
        os.fsync(descriptor)
    except OSError:
        pass
    finally:
        os.close(descriptor)


def _prove(
    manifest_path: Path,
    new_text: str,
    document: dict[str, Any],
    additions: list[_Addition],
    events: list[dict[str, Any]],
    denials: list[_Denial],
) -> None:
    """Proves the edit is the `Blastfile` plus exactly the planned grants (D3c).

    The editor has already re-parsed the document, re-lowered it and compared
    the whole model with the one its proposals intend (§14.5's P4), so nothing
    else can have changed. What is left is blastit's own `audit` on the edited
    **text** with the log's traffic as observations: it parses the document with
    the one reader, validates it, and proves that every statement the plan
    grants comes out *granted*. A document the audit refuses is a refusal, and
    the file is left exactly as it was.
    """
    missing = _missing_after_edit(new_text, document, additions, events, denials)
    for addition in additions:
        for sql in addition.statements:
            if (addition.route, addition.proxy, sql) in missing:
                raise _FoldError(
                    f"{manifest_path}: after the edit, blastit's own audit still refuses "
                    f"{sql!r} on the view {addition.route!r} (store {addition.proxy!r}); "
                    "nothing was written"
                )


#: Where the audit runs on the edited file: the statements the plan grants,
#: keyed by `(view, store, sql)`, and only for the pairs the log shows a request
#: for — a `sql.denied` event carries a route, not a request id (§8.1).
def _missing_after_edit(
    new_text: str,
    document: dict[str, Any],
    additions: list[_Addition],
    events: list[dict[str, Any]],
    denials: list[_Denial],
) -> set[tuple[str, str, str]]:
    """Which planned statements blastit's own audit still refuses (D3c).

    The observations are the ones the pytest plugin's live server builds out of
    the same log: the `request` events give each view the `(method, path)` it
    was served at, and a denial is attached to the first request of its view.
    A denial for a view or store the file does not declare is left out — that
    is exactly what the plan refused with a note, and the audit cannot attribute
    it at all.

    What comes back is keyed by `(view, store, statement)` and holds only what
    the audit actually *looks at*: a view is "checked" when its own report says
    it served a request, not merely when the log had one. A path the edited file
    routes elsewhere — a log from an older `Blastfile`, a rule that moved —
    would otherwise make the proof pass by looking at nothing.
    """
    from blastit import _blastit

    views = _views(document)
    stores = _stores(document)
    observable = [
        denial for denial in denials if denial.route in views and denial.proxy in stores
    ]
    requests: list[dict[str, Any]] = []
    by_route: dict[str, list[_Denial]] = {}
    for denial in observable:
        by_route.setdefault(denial.route, []).append(denial)
    seen: set[str] = set()
    for entry in logs.routed_requests(events):
        route = str(entry["route"])
        statements: list[dict[str, Any]] = []
        if route not in seen:
            seen.add(route)
            statements = [
                {"alias": None, "proxy": denial.proxy or None, "sql": denial.parseable}
                for denial in by_route.get(route, [])
            ]
        requests.append(
            {
                "method": entry["method"],
                "path_info": entry["path"],
                "nodeid": None,
                "transaction_control": False,
                "statements": statements,
                "egress": [],
                "source": "live",
            }
        )
    observations = json.dumps({"version": 1, "requests": requests})
    try:
        report = json.loads(_blastit.audit(new_text, observations))
    except Exception as error:
        raise _FoldError(
            f"blastit's own audit refused the edited file ({error}); nothing was written"
        ) from error
    # Where the audit actually routed a request, and what it still refuses there.
    checked: set[str] = set()
    blocks: set[tuple[str, str]] = set()
    refused: set[tuple[str, str, str]] = set()
    for route in report["routes"]:
        name = str(route["name"])
        if route["requests"]:
            checked.add(name)
        for block in route["proxies"]:
            blocks.add((name, str(block["name"])))
            for entry in block["missing"]:
                refused.add((name, str(block["name"]), str(entry["sql"])))
    for addition in additions:
        if addition.route in checked and (addition.route, addition.proxy) in blocks:
            continue
        _diagnostic(
            f"{addition.route} ({addition.proxy}): the edited file routes none of the "
            "log's requests here, so the audit could not check that the new grant covers "
            "it; fold wrote it, and the next run will say"
        )
    return refused


if __name__ == "__main__":
    raise SystemExit(_direct())
