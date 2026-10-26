"""`blastit-manifest fold`: turn the statements a log denied into grants (D3c).

The manifest is written from a test suite (the pytest plugin), and the test
suite only makes the requests it makes. The rest shows up in production as
`sql.denied` events (§8.1): a 404's redirect lookup, a query whose shape depends
on the data, a code path no test covers. This command reads those logs and
prints the grants the manifest is missing:

```console
$ blastit-manifest fold app.log --manifest blastit.toml
$ blastit-manifest fold app.log --manifest blastit.toml --write
$ blastit-manifest fold app.log --manifest blastit.toml --prefer-tables
$ python -m blastit.fold app.log --manifest blastit.toml
```

It is the other half of the pytest plugin: the plugin checks what the tests do,
this checks what the deployment did. Both use blastit's own SQL normalisation
(`blastit._blastit.normalize`) and the same table-grant analysis
(`blastit._blastit.covering`), so a statement the log denied and a statement a
test ran are compared to the manifest in exactly one way.

What it folds
-------------

* every `sql.denied` event, by `(route, proxy)`, with the *normalised* text
  dbproxy logged — which is what a grant has to hold, so it can be pasted in as
  it is;
* never into an `unrestricted` route: such a grant has nothing left to check, so
  a denial there means the manifest changed under the route, and adding a
  `queries` list would be a boot error (§6);
* never twice: a statement the route's own grant or one of its groups already
  grants is skipped, and so is a statement its `tables` grant already covers
  (in `--prefer-tables` mode);
* never a statement this build cannot read: `--prefer-tables` keeps those as
  exact `queries` and says so, with the reason `analyze` gave — a system
  catalogue, an unknown function, a statement kind no table grant describes;
* never a statement of a kind a grant is not an allowlist for: DDL, `SET` and
  `RESET` (a `SET ROLE` above all), `COPY`, `DO`, `CALL`, `LOCK`, `GRANT`,
  `REVOKE`, `TRUNCATE`, `VACUUM`, `LISTEN`/`NOTIFY`, the transaction commands.
  PostgreSQL's own parser names the kind (never a pattern over the text), the
  statement is refused unless `--allow-dangerous` is given, and either way it is
  listed on stderr with the reason.

Nothing a log says is trusted as manifest text. A `route` names a route the
manifest already declares, a `proxy` a proxy it already declares *and* that the
route is granted; fold never creates a route, and it refuses — rather than
writes — a name it cannot spell as a plain TOML key. A reason becomes a
single-line comment, bounded in length, and every statement is written as an
escaped TOML string.

Lines that are not events are skipped, and so is an App Platform prefix in front
of one (`2026-10-25T14:02:11.384Z app[web.1]: {…}`): see
[`blastit.logs`](logs.py).

`--write`
---------

Without `--write` the file is not touched: the TOML to paste is printed, and it
is parsed before it is printed. With it, the additions are merged into the file
with a line-level edit — the new statements go into the route's existing
`queries = [ … ]`, a new `[route.<proxy>.tables]` section is written under the
route's grant, and a table entry that was there keeps its position and its
neighbours' comments. Nothing else in the file is rewritten.

The edit is then *proved* before anything is written: the whole result is parsed
again, its grants are compared key by key to the plan (no route, proxy, group or
version may differ, and no grant may hold anything the plan did not ask for),
and blastit's own `audit` is run on it with the log's traffic as observations, so
every statement the plan grants has to come out granted. An edit that fails any
of that is refused, and the file is left exactly as it was. A grant written as an
inline table (`db = { … }`, `tables = { … }`) is refused too: the line-level
edit cannot merge into it, and a duplicate key would not parse.
"""

from __future__ import annotations

import argparse
import copy
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable, Sequence

from blastit import logs

__all__ = ["main"]


def main(argv: Sequence[str] | None = None) -> int:
    """`blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables]
    [--allow-dangerous]`.

    The console script's spelling: `blastit-manifest` is a command with
    subcommands, and `fold` is the one it has. `python -m blastit.fold` *is* this
    tool, so it takes the same arguments without the word `fold` (see
    [`_direct`]).

    Returns the process exit status: 0 when it printed (or wrote) what the
    manifest was missing — an empty diff is a normal, good outcome — and 1 on
    anything it could not do (an unreadable log, a manifest it cannot parse,
    edit or prove safe).
    """
    return _run(_parser(), argv)


def _direct(argv: Sequence[str] | None = None) -> int:
    """`python -m blastit.fold LOG... --manifest blastit.toml …`.

    The same fold, without a subcommand to name: a module invoked with `-m` takes
    its own arguments, and `python -m blastit.fold` is what a deployment has when
    the package is importable but the console script is not.
    """
    parser = argparse.ArgumentParser(
        prog="python -m blastit.fold",
        description="Read blastit's JSON logs and the manifest, and say what grants are missing.",
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
        description="Read blastit's JSON logs and the manifest, and say what grants are missing.",
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
        help="the blastit.toml to extend",
    )
    fold.add_argument(
        "--write",
        action="store_true",
        help="rewrite the manifest in place instead of printing the TOML",
    )
    fold.add_argument(
        "--prefer-tables",
        action="store_true",
        help="add the smallest [route.<proxy>.tables] grant covering the denied "
        "statements (D7), keeping as exact queries whatever a table grant cannot "
        "describe",
    )
    fold.add_argument(
        "--allow-dangerous",
        action="store_true",
        help="grant statements a grant is not an allowlist for, instead of refusing "
        "them: DDL, SET, COPY, DO, CALL, LOCK, GRANT, TRUNCATE, VACUUM, LISTEN and the "
        "transaction commands, and statements calling a function outside blastit's "
        "safe set (an exact `queries` entry runs whatever it calls)",
    )


class _FoldError(Exception):
    """Something that stops the fold: a log or a manifest that cannot be read."""


@dataclass
class _Denial:
    """One statement a route was denied, as the log reported it."""

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
        what the report and the manifest keep.
        """
        return logs.parseable_sql(self.sql)


@dataclass
class _Addition:
    """What one `(route, proxy)` grant is missing."""

    route: str
    proxy: str
    #: The exact statements to add, as the log wrote them, with their reasons.
    queries: list[tuple[str, str]] = field(default_factory=list)
    #: The route's whole `tables` grant, existing entries merged with new ones.
    tables: dict[str, dict[str, Any]] = field(default_factory=dict)
    #: `functions` entries beyond the safe set, the route's own included.
    functions: list[str] = field(default_factory=list)
    #: Every statement this addition makes runnable, as the log wrote it. The
    #: verification replays them through `blastit._blastit.audit` on the edited
    #: manifest and checks each one is granted there (D3c).
    statements: list[str] = field(default_factory=list)
    #: What already grants the route that proxy, when it is not the route's own
    #: `[route.<proxy>]` section: `the group 'session'`, or a global group's name
    #: (D3c). The edit names it in its refusal, because a group's grant is not
    #: something fold writes into.
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
    `ssh app blastit | blastit-manifest fold - --manifest blastit.toml`. Lines
    that are not events are skipped — a log carries the app's own output, and an
    App Platform prefix in front of every line (`blastit.logs` knows both).
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
    """The `sql.denied` events, deduplicated by `(route, proxy, statement)`."""
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
# Planning: what the manifest is missing.
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
    """What each `(route, proxy)` grant has to gain, in manifest order.

    `document` is the manifest, parsed by `tomllib`. `covering`, `classify` and
    `describe` are injectable for tests and default to
    `blastit._blastit.covering`, `.classify` and `.analyze`; `allow_dangerous`
    lets through a statement a grant is not an allowlist for — another kind than
    `SELECT`/`INSERT`/`UPDATE`/`DELETE`, or one calling a function outside the
    safe set (D3c) — which is off by default because a `SET ROLE`, a `COPY`, a
    `CREATE TABLE` or a `pg_read_file` in a grant is a hole, not a fix.
    """
    routes = _routes(document)
    groups = document.get("group") or {}
    proxies = set((document.get("proxy") or {}).keys())
    global_groups = [str(name) for name in document.get("groups") or []]
    by_pair: dict[tuple[str, str], list[_Denial]] = {}
    for denial in denials:
        by_pair.setdefault((denial.route, denial.proxy), []).append(denial)
    results: list[_Addition] = []
    for route_name, proxy in _order(denials, routes):
        route = routes.get(route_name)
        addition = _Addition(route=route_name, proxy=proxy)
        if route is None:
            addition.notes.append(
                f"{route_name}: the manifest declares no route of that name — fold extends "
                "the routes a manifest has, it never creates one"
            )
            results.append(addition)
            continue
        # Every value here comes out of a log line, which an application can
        # influence: a `proxy` that is not a key of this manifest, or a `route`
        # that is not a route of it, is a log from elsewhere — or a forgery —
        # and nothing of it may reach the file (D3c).
        if _bare_key(proxy) is None:
            addition.notes.append(
                f"{route_name}: the log names the proxy {proxy!r}, which is not a plain "
                "TOML key; fold cannot write a grant section for it"
            )
            results.append(addition)
            continue
        if proxy not in proxies:
            addition.notes.append(
                f"{route_name}: the log names the proxy {proxy!r}, which this manifest "
                "does not declare"
            )
            results.append(addition)
            continue
        grant = route.get(proxy) if isinstance(route.get(proxy), dict) else None
        if grant is not None and grant.get("unrestricted") is True:
            addition.notes.append(
                f"{route_name} ({proxy}): the route is unrestricted — it may run anything, "
                "so there is nothing to grant (a `queries` list next to `unrestricted` is a "
                "boot error, §6)"
            )
            results.append(addition)
            continue
        via = _granted_via(proxy, grant, groups, global_groups, route)
        if via == NO_GRANT:
            addition.notes.append(
                f"{route_name} ({proxy}): the route has no grant on that proxy — a route "
                "only gets a listener for a proxy it is granted (§2.2), so the log is "
                "older than the manifest; add the grant by hand, then fold again"
            )
            results.append(addition)
            continue
        addition = _plan_route(
            route=route,
            route_name=route_name,
            proxy=proxy,
            grant=grant,
            groups=groups,
            global_groups=global_groups,
            denials=by_pair[(route_name, proxy)],
            prefer_tables=prefer_tables,
            allow_dangerous=allow_dangerous,
            covering=covering,
            classify=classify,
            describe=describe,
        )
        addition.via = via
        results.append(addition)
    return results


def _plan_route(
    *,
    route: dict[str, Any],
    route_name: str,
    proxy: str,
    grant: dict[str, Any] | None,
    groups: dict[str, Any],
    global_groups: list[str],
    denials: list[_Denial],
    prefer_tables: bool,
    allow_dangerous: bool,
    covering: Callable[[list[str], str], str] | None,
    classify: Callable[[str], str] | None,
    describe: Callable[[str], str] | None,
) -> _Addition:
    """What one route's grant is missing, from its denials."""
    addition = _Addition(route=route_name, proxy=proxy)
    existing = _granted_queries(proxy, grant, groups, global_groups, route)
    judge = classify or _classify
    analyse = describe or _analyze
    new: list[_Denial] = []
    seen: set[str] = set()
    for denial in denials:
        normalised = _normalise(denial.sql)
        source = existing.get(normalised)
        if source is not None:
            addition.notes.append(
                f"{denial.sql!r}: already granted by {source} of {route_name} — the log is "
                "from an older manifest, or the statement did not need the grant"
            )
            continue
        # Two spellings of one statement are one grant: the log's own text is
        # already normalised, but a log from another build, or a hand-edited
        # one, may not be.
        if normalised in seen:
            continue
        seen.add(normalised)
        # What kind of statement this is decides whether a grant may hold it at
        # all; PostgreSQL's own parser answers, never a guess from the text.
        kind = _kind(judge, denial)
        if kind["statements"] > 1:
            # A grant is one statement, and splitting a text is something only a
            # person can do: `--allow-dangerous` does not reach this (D3c).
            addition.notes.append(
                f"{denial.sql!r}: refused — {kind['label']}, and a grant is one statement; "
                "split the text and fold it again"
            )
            continue
        if not kind["plain"] and not allow_dangerous:
            why = f" ({kind['reason']})" if kind["reason"] else ""
            addition.notes.append(
                f"{denial.sql!r}: refused — {kind['label']} is not a plain SELECT, INSERT, "
                f"UPDATE or DELETE{why}; pass --allow-dangerous to grant it anyway"
            )
            continue
        # An exact `queries` entry is trusted as it is: whatever function it
        # calls runs, which is why a statement calling one outside the safe set
        # is refused here unless the operator asks for it (D3c).
        functions = _unsafe_functions(analyse, denial)
        if functions and not allow_dangerous:
            addition.notes.append(
                f"{denial.sql!r}: refused — it calls {', '.join(functions)}, which is outside "
                "blastit's safe set, and an exact `queries` entry grants it as it is; pass "
                "--allow-dangerous to grant it anyway"
            )
            continue
        if not kind["plain"]:
            addition.notes.append(
                f"{denial.sql!r}: granted although it is {kind['label']}, which is not a "
                "plain SELECT, INSERT, UPDATE or DELETE (--allow-dangerous)"
            )
        if functions:
            addition.notes.append(
                f"{denial.sql!r}: granted although it calls {', '.join(functions)}, which is "
                "outside blastit's safe set (--allow-dangerous)"
            )
        new.append(denial)
    if not new:
        return addition

    cover = covering or _covering
    prefix = f"route.{proxy}"
    existing_tables = grant.get("tables") if grant and isinstance(grant.get("tables"), dict) else {}
    existing_functions = _text_list(grant.get("functions") if grant else None)
    kept: list[_Denial] = []
    for denial in new:
        if existing_tables or existing_functions:
            single = json.loads(cover([denial.parseable], prefix))
            if not single["unanalysable"] and _covered_by(
                existing_tables, single, existing_functions
            ):
                addition.notes.append(
                    f"{denial.sql!r}: already covered by the route's [{prefix}.tables] grant"
                )
                continue
        kept.append(denial)
    if not kept:
        return addition
    addition.statements = [denial.sql for denial in kept]

    if not prefer_tables:
        addition.queries = [(denial.sql, denial.reason) for denial in kept]
        return addition

    merged = json.loads(cover([denial.parseable for denial in kept], prefix))
    rejected = {entry["sql"] for entry in merged["unanalysable"]}
    reasons = {
        denial.parseable: entry["reason"]
        for entry in merged["unanalysable"]
        for denial in kept
        if denial.parseable == entry["sql"]
    }
    for denial in kept:
        if denial.parseable in rejected:
            addition.queries.append((denial.sql, reasons.get(denial.parseable, denial.reason)))
    addition.tables = _merge_tables(existing_tables, merged["tables"])
    addition.functions = sorted(
        set(_text_list(grant.get("functions") if grant else None)) | set(merged["functions"])
    )
    return addition


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
            "reason": str(error),
            "statements": 1,
        }
    verdict.setdefault("reason", "")
    verdict.setdefault("statements", 1)
    return verdict


def _unsafe_functions(describe: Callable[[str], str], denial: _Denial) -> list[str]:
    """The functions a statement calls that are outside blastit's safe set.

    The analysis is asked, not the text: it is the same `pg_query` walk the
    policy runs, and it reports a function it does not know as a `functions`
    entry the grant would have to name — `set_config`, `pg_read_file`,
    `lo_import`, `pg_terminate_backend`, and a cast to a type that needs one.

    `[]` when the analysis refuses the statement (a system catalogue, a
    construct it cannot walk): there is nothing to say about its functions, and
    refusing it for that reason is the analysis's business, not this one's.
    """
    try:
        analysis = json.loads(describe(denial.parseable))
    except ValueError:
        return []
    functions = analysis.get("functions")
    if not isinstance(functions, list):
        return []
    return [str(function) for function in functions]


def _order(
    denials: list[_Denial], routes: dict[str, dict[str, Any]]
) -> list[tuple[str, str]]:
    """The `(route, proxy)` pairs to report on, in manifest order.

    Manifest order, not log order: a diff a reader can follow, and the same
    order the pytest plugin's report uses. A route the manifest does not declare
    comes last, where it can be seen.
    """
    seen: list[tuple[str, str]] = []
    for denial in denials:
        pair = (denial.route, denial.proxy)
        if pair not in seen:
            seen.append(pair)
    declared = [pair for pair in seen if pair[0] in routes]
    undeclared = [pair for pair in seen if pair[0] not in routes]
    order = {name: index for index, name in enumerate(routes)}
    declared.sort(key=lambda pair: (order[pair[0]], pair[1]))
    return declared + undeclared


def _routes(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The manifest's routes by name, in declaration order."""
    routes: dict[str, dict[str, Any]] = {}
    for route in document.get("route") or []:
        if isinstance(route, dict) and isinstance(route.get("name"), str):
            routes[str(route["name"])] = route
    return routes


def _granted_queries(
    proxy: str,
    grant: dict[str, Any] | None,
    groups: dict[str, Any],
    global_groups: list[str],
    route: dict[str, Any],
) -> dict[str, str]:
    """Every statement the route is already granted, normalised: what grants it.

    The effective grant of §6: what the route writes itself, the groups it names
    and the global groups that apply to every route. The source is kept so the
    summary can say *why* a denial in the log is not a grant to add.
    """
    sources: dict[str, str] = {}
    for name in global_groups:
        _collect_queries(
            _group_grant(groups.get(name), proxy), sources, f"the global group {name!r}"
        )
    for name in route.get("groups") or []:
        _collect_queries(
            _group_grant(groups.get(str(name)), proxy), sources, f"the group {name!r}"
        )
    _collect_queries(grant, sources, "the route's own grant")
    return sources


def _group_grant(group: Any, proxy: str) -> Any:
    """The `[group.<name>.<proxy>]` grant of a group, or `None`."""
    return group.get(proxy) if isinstance(group, dict) else None


def _granted_via(
    proxy: str,
    grant: dict[str, Any] | None,
    groups: dict[str, Any],
    global_groups: list[str],
    route: dict[str, Any],
) -> str | None:
    """What grants a route one proxy: its own section, a group, or nothing (§6).

    A route only gets a listener for a proxy it is granted (§2.2), so a denial
    naming a proxy the route is not granted is a log of another manifest — and
    fold will not invent the grant for it. What the label is for is the *other*
    case: a route granted through a group has no `[route.<proxy>]` section to
    write into, and a refusal that names the group explains what to do about it.
    """
    if isinstance(grant, dict):
        return None
    for name in global_groups:
        if isinstance(_group_grant(groups.get(name), proxy), dict):
            return f"the global group {name!r}"
    for name in route.get("groups") or []:
        if isinstance(_group_grant(groups.get(str(name)), proxy), dict):
            return f"the group {str(name)!r}"
    return NO_GRANT


def _bare_key(name: str) -> str | None:
    """The name when it is a plain TOML key, or `None`.

    Fold writes `[route.<proxy>]` headers and `[route.<proxy>.tables]` entries,
    and a name that needs quoting would make every one of those a quoting
    problem of its own. A proxy the manifest declares under a quoted key is one
    fold says it cannot write into, never one it writes wrong.
    """
    return name if re.fullmatch(r"[A-Za-z0-9_-]+", name) else None


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


def _covering(statements: list[str], prefix: str) -> str:
    """`blastit._blastit.covering`: the grant that covers a set of statements."""
    from blastit import _blastit

    return _blastit.covering(statements, prefix)


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
    """A TOML array of strings, or nothing when it is not one."""
    if not isinstance(value, list):
        return []
    return [str(item) for item in value]


#
# Table grants: merging, covering and rendering.
#


def _clean(grant: Any) -> dict[str, Any]:
    """One table grant without its empty entries.

    `covering` renders every operation it knows (`insert = null` and
    `delete = false` included, the shape of the report's JSON); a manifest
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

    `existing` is the route's `tables` grant as the manifest holds it (which may
    use `"*"` or sub-tables), `new` the covering grant of the statements being
    added. The result is what `[route.<proxy>.tables]` should hold, and it keeps
    every table the manifest already had.
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
    """Whether a route's own table grant already covers a statement (D7).

    Deliberately conservative: it answers "yes" only when every column *and*
    every function the new grant names is named by the existing one, `"*"`
    included. A lock's `update` on one column is compared as a column list like
    any other, and anything this does not understand — a table the manifest does
    not mention, an entry that names no column — is a "no", because reporting a
    statement that is already covered is a nuisance while hiding one is a hole.

    An empty `update` is one of those: it is how a lock that read no column of
    the table comes back from the analysis (D3c), it grants nothing (a lock needs
    a column named), and calling it "covered" would silently drop the statement.
    An empty `select` is not the same thing — naming a table without reading a
    column of it *is* covered by any `select` entry (§6).

    A statement that touches no table at all is only covered when the functions
    it calls are granted: `SELECT st_distance(…)` needs `functions = …`, and a
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
                if not set(value) <= set(str(column) for column in have):
                    return False
            else:
                return False
    return True


def _render_table(table: str, grant: dict[str, Any]) -> str:
    """One `table = { … }` line of a `[route.<proxy>.tables]` section."""
    rendered = ", ".join(
        f"{key} = {value}" for key, value in _table_operations(grant)
    )
    return f"{_toml_key(table)} = {{ {rendered} }}"


def _toml_string(value: str) -> str:
    """A TOML basic string.

    Every character TOML forbids raw is escaped: a statement out of a log can
    hold anything an application put in a quoted identifier, and a manifest that
    does not parse is worse than one that does. That includes U+007F (`DEL`) and
    the whole C1 range, which `json.dumps` alone leaves as they are ("*every*
    control character", TOML's own rule), and it keeps printable text — accents,
    dashes, emoji — as it is, because a manifest is read by people.
    """
    out = ['"']
    for character in value:
        if character == '"':
            out.append('\\"')
        elif character == "\\":
            out.append("\\\\")
        elif character == "\n":
            out.append("\\n")
        elif character == "\r":
            out.append("\\r")
        elif character == "\t":
            out.append("\\t")
        elif character.isprintable():
            out.append(character)
        else:
            out.append(f"\\u{ord(character):04X}")
    out.append('"')
    return "".join(out)


def _toml_key(value: str) -> str:
    """A TOML key: bare when it can be, quoted otherwise."""
    if re.fullmatch(r"[A-Za-z0-9_-]+", value):
        return value
    return _toml_string(value)


#
# The manifest as TOML: scanning, rendering, and the line-level edit.
#


@dataclass
class _Line:
    """One line of a manifest, and what a scan of the whole file made of it."""

    #: Index in the file, 0-based.
    index: int
    #: The line as it was written, newline included.
    text: str
    #: The dotted key of a table header (`[route.db]`, `[[route]]`), if the line
    #: opens one. `None` for every other line, *including* a line that merely
    #: looks like a header inside a string or an array.
    header: tuple[str, ...] | None = None
    #: Whether the header opens an array of tables (`[[route]]`).
    array_of_tables: bool = False
    #: The key this line assigns (`queries = …`), split into its components and
    #: unquoted. `None` when the line assigns nothing.
    key: tuple[str, ...] | None = None
    #: The text after `key =`, stripped of trailing whitespace.
    value: str = ""
    #: How deep in an array the line starts, and how deep it ends.
    depth: int = 0
    end_depth: int = 0


class _Scan:
    """A manifest, walked once: which lines are structural, and which are data.

    A line regex cannot tell a table header from a line that merely looks like
    one. Inside a multi-line string (three quotes, or three apostrophes) a
    `[[route]]` line is text; inside a multi-line array, a line that starts with
    `[` is an element; and a `queries = [` line opens an array the *rest of the
    file* may be inside. The two things that make a line structural are
    therefore being outside a string and being outside an array, and only a walk
    of the file knows them, so this is that walk: one pass, character by
    character, tracking every string, every array and every comment, and
    recording for each line the header, key and array depth the walk saw at its
    start.

    It is deliberately not a TOML parser: it does not have to agree with one on
    what a value means, only on where a line stops being structure and starts
    being data. The edit is verified — re-parsed and audited — before anything
    is written, so a line this scanner gets wrong is refused, never written.
    """

    def __init__(self, text: str) -> None:
        """Walks the text once."""
        self.lines: list[_Line] = []
        depth = 0
        multiline: str | None = None
        for index, raw in enumerate(text.splitlines(keepends=True)):
            line = _Line(index=index, text=raw, depth=depth)
            self.lines.append(line)
            if depth == 0 and multiline is None:
                after = self._structure(line)
            else:
                after = 0
            depth, multiline = self._walk(raw, after, depth, multiline)
            line.end_depth = depth

    def _structure(self, line: _Line) -> int:
        """Fills in a line's header or key, and says where its body starts."""
        stripped = line.text.lstrip()
        offset = len(line.text) - len(stripped)
        if not stripped or stripped.startswith("#"):
            return 0
        if stripped.startswith("["):
            header = _header_key(stripped)
            if header is not None:
                line.header = header
                line.array_of_tables = stripped.startswith("[[")
                return offset + _header_end(stripped)
            return offset
        key, value, start = _key_of(stripped)
        if key is not None:
            line.key = key
            line.value = value
            return offset + start
        return offset

    def _walk(
        self, raw: str, start: int, depth: int, multiline: str | None
    ) -> tuple[int, str | None]:
        """Updates the array depth and the open multi-line string."""
        index = start
        while index < len(raw):
            char = raw[index]
            if multiline is not None:
                if raw.startswith(multiline, index):
                    index += len(multiline)
                    multiline = None
                    continue
                if multiline == '"""' and char == "\\":
                    index += 2
                    continue
                index += 1
                continue
            if char == "#":
                break  # a comment runs to the end of the line
            if raw.startswith('"""', index) or raw.startswith("'''", index):
                multiline = raw[index : index + 3]
                index += 3
                continue
            if char in ("'", '"'):
                index = _quoted_end(raw, index, char)
                continue
            if char == "[":
                depth += 1
            elif char == "]":
                depth = max(0, depth - 1)
            index += 1
        return depth, multiline

    def headers(self, key: tuple[str, ...] | None = None) -> list[_Line]:
        """Every line that opens a table, in file order."""
        found = [line for line in self.lines if line.header is not None]
        if key is not None:
            found = [line for line in found if line.header == key]
        return found

    def block_end(self, index: int) -> int:
        """The first line after `index` that opens a table, or the file's end."""
        for line in self.lines[index + 1 :]:
            if line.header is not None:
                return line.index
        return len(self.lines)

    def keys(self, start: int, end: int) -> list[_Line]:
        """The lines between `start` and `end` that assign a key."""
        return [
            line
            for line in self.lines[start:end]
            if line.key is not None and line.depth == 0
        ]

    def array_end(self, line: _Line) -> int | None:
        """The line a key's array closes on, when it closes on a line of its own.

        `None` when the value is not an array, or when the array opens and closes
        on `line` itself — the caller finds the bracket on the line then.
        """
        if line.end_depth <= line.depth:
            return None
        for other in self.lines[line.index + 1 :]:
            if other.end_depth == line.depth:
                return other.index
        return None

    def text(self) -> str:
        """Every line, joined: the file the scan walked."""
        return "".join(line.text for line in self.lines)


def _header_end(text: str) -> int:
    """The offset just after the `]`/`]]` that closes a table header."""
    closing = "]]" if text.startswith("[[") else "]"
    found = _find_unquoted(text, closing, 1)
    if found < 0:
        return len(text)
    return found + len(closing)


def _header_key(text: str) -> tuple[str, ...] | None:
    """The dotted key components of a header line, or `None`."""
    double = text.startswith("[[")
    closing = "]]" if double else "]"
    start = 2 if double else 1
    found = _find_unquoted(text, closing, start)
    if found < 0:
        return None
    return _split_key(text[start:found])


def _key_of(text: str) -> tuple[tuple[str, ...] | None, str, int]:
    """The key a line assigns, its value, and where the value starts.

    `(None, "", 0)` when the line assigns nothing: a value without a key, or a
    line this build does not understand. The `=` is looked for outside quoted
    keys and strings, so a `=` inside a key spelled `"a=b"` is not one.
    """
    found = _find_unquoted(text, "=", 0)
    if found < 0:
        return None, "", 0
    key = _split_key(text[:found])
    if key is None:
        return None, "", 0
    value = text[found + 1 :].strip()
    return key, value, found + 1


def _split_key(text: str) -> tuple[str, ...] | None:
    """A dotted TOML key's components, unquoted, or `None` when it is not one."""
    parts: list[str] = []
    for part in _split_dotted(text):
        name = _unquote_key(part.strip())
        if name is None:
            return None
        parts.append(name)
    return tuple(parts) if parts else None


def _split_dotted(text: str) -> list[str]:
    """Splits a dotted key on its dots, outside quoted components."""
    parts: list[str] = []
    start = 0
    index = 0
    while index < len(text):
        char = text[index]
        if char in ("'", '"'):
            index = _quoted_end(text, index, char)
            continue
        if char == ".":
            parts.append(text[start:index])
            start = index + 1
        index += 1
    parts.append(text[start:])
    return parts


def _unquote_key(part: str) -> str | None:
    """A key component's name, or `None` when it is neither bare nor quoted."""
    if not part:
        return None
    if part[0] in ("'", '"'):
        quote = part[0]
        if len(part) < 2 or part[-1] != quote:
            return None
        inner = part[1:-1]
        if quote == '"':
            try:
                return json.loads(part)
            except ValueError:
                return None
        return inner
    if re.fullmatch(r"[A-Za-z0-9_-]+", part):
        return part
    return None


def _find_unquoted(text: str, wanted: str, start: int) -> int:
    """The offset of `wanted` outside every quoted region, or -1.

    This is what a `]` or an `=` inside a TOML string must not be mistaken for
    (D3c): a statement in a one-line `queries` array can hold either.
    """
    index = start
    while index < len(text):
        char = text[index]
        if char in ("'", '"'):
            index = _quoted_end(text, index, char)
            continue
        if char == "#":
            return -1  # a comment runs to the end of the line
        if text.startswith(wanted, index):
            return index
        index += 1
    return -1


def _quoted_end(text: str, index: int, quote: str) -> int:
    """The offset just after the quoted region that opens at `index`.

    Single-line only: a manifest's strings are one line each (a `queries` entry
    with a newline in it is written as `\\n`), and the multi-line forms are the
    scan's business, not this helper's.
    """
    scan = index + 1
    while scan < len(text):
        char = text[scan]
        if quote == '"' and char == "\\":
            scan += 2
            continue
        if char == quote:
            if scan + 1 < len(text) and text[scan + 1] == quote:
                scan += 2
                continue
            return scan + 1
        scan += 1
    return len(text)


#
# The edit itself.
#


def _diagnostic(message: str) -> None:
    """One note for a person, on stderr, never as TOML."""
    print(f"note: {message}", file=sys.stderr)


def _comment(text: str) -> str:
    """A log field as a TOML comment: one line, no controls, bounded.

    A `sql.denied` reason is dbproxy's, and dbproxy's reasons quote the
    statement — including, for a quoted identifier, whatever the application put
    in it. A newline there would end the comment and let the rest of the text be
    read as TOML; a control character is noise; and a kilobyte of reason is not
    a comment. So the text is flattened and cut, and the result can only ever be
    a comment.
    """
    flattened = "".join(
        " " if character.isspace() or ord(character) < 0x20 or ord(character) == 0x7F else character
        for character in text
    )
    return flattened.strip()[:COMMENT_LIMIT]


#: How long a rendered comment may be, in characters.
COMMENT_LIMIT = 160

#: What `_granted_via` answers when nothing grants a route a proxy.
NO_GRANT = "(nothing)"

#: The key a `queries` grant is written under, and the two others fold knows.
_QUERIES = ("queries",)
_FUNCTIONS = ("functions",)
_TABLES = ("tables",)


def _inline_table(line: _Line) -> bool:
    """Whether a key's value is an inline table (`db = { … }`)."""
    return line.value.startswith("{")


def _apply(lines: list[str], scan: _Scan, addition: _Addition) -> _Scan:
    """Merges one `(route, proxy)` addition into the manifest, and re-scans it.

    The scan is rebuilt after every change: line indices move, and a second
    addition has to see the file as it now is.
    """
    route = _route_element(scan, addition.route)
    grant = _grant_element(scan, route, addition.proxy, addition)
    if addition.queries:
        lines = _add_queries(scan, grant, addition.queries)
        scan = _Scan("".join(lines))
        route = _route_element(scan, addition.route)
        grant = _grant_element(scan, route, addition.proxy, addition)
    if addition.functions:
        lines = _set_functions(scan, grant, addition.functions)
        scan = _Scan("".join(lines))
        route = _route_element(scan, addition.route)
    if addition.tables:
        lines = _set_tables(scan, route, grant, addition.proxy, addition.tables)
    return _Scan("".join(lines))


def _route_element(scan: _Scan, name: str) -> _Line:
    """The `[[route]]` line of the route called `name`.

    Raises [`_FoldError`] when no such element exists: fold extends a route, it
    never invents one.
    """
    for header in scan.headers(("route",)):
        end = _element_end(scan, header)
        for line in scan.keys(header.index + 1, end):
            if line.key == ("name",) and _unquote_value(line.value) == name:
                return header
    raise _FoldError(
        f"the manifest declares no `[[route]]` named {name!r}; fold extends the routes "
        "a manifest already has, it never creates one"
    )


def _element_end(scan: _Scan, header: _Line) -> int:
    """Where an array-of-tables element ends: the next one, or the file's end."""
    for other in scan.lines[header.index + 1 :]:
        if other.header == ("route",) and other.array_of_tables:
            return other.index
    return len(scan.lines)


def _unquote_value(value: str) -> str:
    """The text of a one-line TOML string value, or the value as it is."""
    text = value.strip()
    if text[:1] == '"':
        found = _quoted_end(text, 0, '"')
        if found > 1:
            try:
                return json.loads(text[:found])
            except ValueError:
                return text[1 : found - 1]
    if text[:1] == "'":
        found = _quoted_end(text, 0, "'")
        if found > 1:
            return text[1 : found - 1]
    return text


@dataclass
class _Grant:
    """Where a route's own grant on one proxy is, and where its keys go.

    A grant blastit boots may be written as nothing but sections of its own:
    `[route.db.tables]` is the route's `db` grant (§6), and a route whose tables
    live there is a route with a grant. So the edit accepts it as one — and a key
    that belongs to the *grant* rather than to one of its sections (`queries`,
    `functions`) has to be written under a `[route.<proxy>]` header of its own,
    just before the first section: a key written inside `[route.db.tables]` would
    be a key of the `tables` table.
    """

    #: The `[route.<proxy>]` header, when the manifest has one.
    header: _Line | None
    #: The first `[route.<proxy>.<section>]` header, when the grant is written as
    #: sections alone.
    section: _Line | None
    #: The grant's dotted key, for a header the edit has to write.
    key: tuple[str, ...]

    @property
    def insert_at(self) -> int:
        """The index a key of the grant itself belongs at."""
        assert self.header is not None or self.section is not None
        if self.header is not None:
            return self.header.index + 1
        return self.section.index  # type: ignore[union-attr]

    def block_end(self, scan: _Scan) -> int:
        """The first line after the block a key of the grant would land in.

        For a grant with its own header that is the next header; for one written
        as sections alone there is no such block yet, so the key cannot already
        be there.
        """
        if self.header is None:
            return self.insert_at
        return scan.block_end(self.header.index)


def _grant_element(scan: _Scan, route: _Line, proxy: str, addition: _Addition) -> _Grant:
    """Where the route's `[route.<proxy>]` grant is.

    Raises [`_FoldError`] when the route has no grant on that proxy at all —
    naming the group that grants the proxy instead when that is what the route
    has, because folder writes into a route's own section and a group's grant is
    a different one — and when a grant the edit has to write into is an inline
    table, which the line-level edit cannot merge into.

    A grant written as `[route.<proxy>.tables]` (or any other section of it) *is*
    a grant, so it is accepted: that is a manifest blastit boots, and the tables
    it names are the route's.

    Only what the edit *writes* is refused: a `tables = { … }` entry is fine as
    long as nothing is added to the tables grant, and so is an inline grant on
    another proxy.
    """
    wanted = ("route", *proxy.split("."))
    end = _element_end(scan, route)
    for line in scan.keys(route.index + 1, end):
        if line.key is not None and line.key[0] == proxy and _inline_table(line):
            raise _FoldError(
                f"the route {_route_name(scan, route)!r} writes its {proxy!r} grant as an "
                f"inline table (`{proxy} = {{ … }}`, line {line.index + 1}); write it as "
                f"`[route.{proxy}]` first, so fold can add to it"
            )
    section: _Line | None = None
    for header in scan.headers():
        if not (route.index < header.index < end):
            continue
        if header.header == wanted:
            if addition.tables:
                for line in scan.keys(header.index + 1, scan.block_end(header.index)):
                    if line.key == _TABLES and _inline_table(line):
                        raise _FoldError(
                            f"`[route.{proxy}]` writes its `tables` grant as an inline table "
                            f"(line {line.index + 1}); write it as `[route.{proxy}.tables]` "
                            "first, so fold can add to it"
                        )
            return _Grant(header=header, section=None, key=wanted)
        if (
            section is None
            and header.header is not None
            and len(header.header) > len(wanted)
            and header.header[: len(wanted)] == wanted
        ):
            section = header
    if section is not None:
        return _Grant(header=None, section=section, key=wanted)
    where = (
        f"the route {_route_name(scan, route)!r} is granted that proxy by {addition.via}, "
        f"which has no `[route.{proxy}]` section to write into"
        if addition.via
        else f"the route {_route_name(scan, route)!r} has no `[route.{proxy}]` grant: no "
        f"`[route.{proxy}]` section, no `[route.{proxy}.tables]` one, and no group that "
        "grants it"
    )
    raise _FoldError(
        f"{where}; a route's own grant is unioned with its groups' (§6), so write a "
        f"`[route.{proxy}]` section by hand, then fold again"
    )


def _route_name(scan: _Scan, route: _Line) -> str:
    """The `name` a route element declares, for a message."""
    for line in scan.keys(route.index + 1, _element_end(scan, route)):
        if line.key == ("name",):
            return _unquote_value(line.value)
    return "?"


def _add_queries(scan: _Scan, grant: _Grant, queries: list[tuple[str, str]]) -> list[str]:
    """Appends statements to the grant's `queries`, or writes the array."""
    items = [_query_line(sql, reason) for sql, reason in queries]
    if grant.header is not None:
        for line in scan.keys(grant.header.index + 1, grant.block_end(scan)):
            if line.key == _QUERIES:
                return _append_to_array(scan, line, items)
    lines = [line.text for line in scan.lines]
    block = _grant_key_lines(grant, ["queries = [\n", *items, "]\n"])
    lines[grant.insert_at : grant.insert_at] = block
    return lines


def _grant_key_lines(grant: _Grant, key: list[str]) -> list[str]:
    """The lines that write one key under the grant.

    A grant with a header of its own takes the key as it is. One written as
    sections alone needs the `[route.<proxy>]` header written first, and a blank
    line after the key — the section that follows has to stay a section, and a
    key written inside `[route.db.tables]` would be a key of the *tables* table.
    """
    if grant.header is not None:
        return key
    header = f"[{'.'.join(_toml_key(part) for part in grant.key)}]\n"
    return [header, *key, "\n"]


def _append_to_array(scan: _Scan, line: _Line, items: list[str]) -> list[str]:
    """Appends entries to the array a key holds, keeping its text and comments.

    A multi-line array gains the entries in front of its own closing bracket, so
    whatever is in it — other entries, a comment next to one — stays where it is.
    A one-line array is rewritten as a multi-line one instead, with its text kept
    as it was: the closing bracket is found by a quote-aware scan, because a
    statement inside it may hold a `]` (in a quoted identifier, say).
    """
    lines = [line.text for line in scan.lines]
    name = ".".join(line.key or ())
    if not line.value.startswith("["):
        raise _FoldError(
            f"the `{name}` key of `{line.value!r}` (line {line.index + 1}) is not a literal "
            "array; edit it by hand"
        )
    closing = scan.array_end(line)
    if closing is not None:
        lines[closing:closing] = items
        return lines
    opening = line.text.find("[")
    bracket = _find_unquoted(line.text, "]", opening)
    if bracket < 0:
        raise _FoldError(
            f"the `{name}` array on line {line.index + 1} is not closed; edit it by hand"
        )
    head = line.text[: opening + 1].rstrip("\n")
    body = line.text[opening + 1 : bracket].strip()
    suffix = line.text[bracket + 1 :].rstrip("\n").rstrip()
    block = [f"{head}\n"]
    if body:
        block.append(f"  {body},\n")
    block.extend(items)
    block.append(f"]{'  ' + suffix if suffix else ''}\n")
    lines[line.index : line.index + 1] = block
    return lines


def _array_values(scan: _Scan, line: _Line) -> list[str]:
    """The strings a key's array already holds, or a refusal.

    The array is handed to `tomllib`, so what comes back is what a manifest
    parser would see — comments, spacing and line breaks included in the text,
    none of them in the answer.
    """
    import tomllib

    name = ".".join(line.key or ())
    stop = scan.array_end(line)
    chunk = "".join(
        other.text for other in scan.lines[line.index : (stop + 1 if stop is not None else line.index + 1)]
    )
    try:
        parsed = tomllib.loads(chunk).get(name)
    except tomllib.TOMLDecodeError as error:
        raise _FoldError(
            f"the `{name}` array on line {line.index + 1} is not valid TOML: {error}"
        ) from error
    if not isinstance(parsed, list):
        raise _FoldError(
            f"the `{name}` key on line {line.index + 1} is not a literal array; edit it by hand"
        )
    return [str(value) for value in parsed]


def _query_line(sql: str, reason: str) -> str:
    """One entry of a `queries` array, with its reason as a comment."""
    comment = f"  # {_comment(reason)}" if reason else ""
    return f"  {_toml_string(sql)},{comment}\n"


def _set_functions(scan: _Scan, grant: _Grant, functions: list[str]) -> list[str]:
    """Appends the functions a grant is missing, keeping what it already had.

    A `functions` entry the manifest already wrote stays where it is, comments
    included: what is added is what the analysis found and the route did not have.
    """
    if grant.header is not None:
        for line in scan.keys(grant.header.index + 1, grant.block_end(scan)):
            if line.key != _FUNCTIONS:
                continue
            have = _array_values(scan, line)
            missing = [name for name in functions if name not in have]
            if not missing:
                return [other.text for other in scan.lines]
            return _append_to_array(
                scan, line, [f"  {_toml_string(name)},\n" for name in missing]
            )
    lines = [line.text for line in scan.lines]
    rendered = ", ".join(_toml_string(name) for name in functions)
    block = _grant_key_lines(grant, [f"functions = [{rendered}]\n"])
    lines[grant.insert_at : grant.insert_at] = block
    return lines


def _table_operations(grant: dict[str, Any]) -> list[tuple[str, str]]:
    """The operations of one table grant, as `(key, rendered value)` pairs."""
    operations: list[tuple[str, str]] = []
    for operation in ("select", "insert", "update"):
        value = grant.get(operation)
        if value is None:
            continue
        if value == "*":
            operations.append((operation, '"*"'))
        else:
            columns = ", ".join(_toml_string(str(column)) for column in value)
            operations.append((operation, f"[{columns}]"))
    if grant.get("delete"):
        operations.append(("delete", "true"))
    return operations


def _set_tables(
    scan: _Scan, route: _Line, grant: _Grant, proxy: str, tables: dict[str, dict[str, Any]]
) -> list[str]:
    """Writes a route's `[route.<proxy>.tables]` section, merged in place.

    A table the manifest already grants is updated where it stands, whichever way
    it was written: an entry of the `tables` table (`products = { … }`) or a
    section of its own (`[route.db.tables.products]`), whose keys are rewritten in
    place and which gains the ones the grant has and it does not. Tables the
    manifest did not have are added as entries of the `tables` table, which is
    what the rest of a manifest in this repository looks like.

    The section is written where the grant's own block ends — or, for a grant
    that is nothing but sections (`[route.db.tables.products]` and no `[route.db]`
    at all), just before the first of them, which is where a super-table belongs.
    """
    lines = [line.text for line in scan.lines]
    wanted = ("route", *proxy.split("."), "tables")
    route_end = _element_end(scan, route)
    header = next(
        (line for line in scan.headers(wanted) if route.index < line.index < route_end),
        None,
    )
    if header is None:
        if grant.header is not None:
            insert = scan.block_end(grant.header.index)
        else:
            assert grant.section is not None
            insert = grant.section.index
        block: list[str] = []
        if insert > 0 and lines[insert - 1].strip() != "":
            block.append("\n")
        block.append(f"[{'.'.join(_toml_key(part) for part in wanted)}]\n")
        lines[insert:insert] = block
        scan = _Scan("".join(lines))
        header = next(line for line in scan.headers(wanted) if line.index >= insert)
    end = scan.block_end(header.index)
    remaining = dict(tables)
    for line in scan.keys(header.index + 1, end):
        table = line.key[0] if line.key is not None and len(line.key) == 1 else None
        if table is None or table not in remaining:
            continue
        lines[line.index] = _render_table(table, remaining.pop(table)) + "\n"
    for table in list(remaining):
        nested = next(
            (
                line
                for line in scan.headers((*wanted, table))
                if route.index < line.index < route_end
            ),
            None,
        )
        if nested is None:
            continue
        lines = _set_nested_table(lines, _Scan("".join(lines)), nested, remaining.pop(table), table)
    if remaining:
        additions = [
            f"{_render_table(table, grant)}\n" for table, grant in sorted(remaining.items())
        ]
        lines[end:end] = additions
        end += len(additions)
    if end < len(lines) and lines[end].strip() != "":
        # A blank line between sections: a manifest is read by people, and the
        # next header may be the next `[[route]]`.
        lines[end:end] = ["\n"]
    return lines


def _set_nested_table(
    lines: list[str], scan: _Scan, header: _Line, grant: dict[str, Any], table: str
) -> list[str]:
    """Rewrites the operations of a table written as `[…tables.<table>]`."""
    operations = dict(_table_operations(grant))
    end = scan.block_end(header.index)
    for line in scan.keys(header.index + 1, end):
        key = line.key[0] if line.key is not None and len(line.key) == 1 else None
        if key is None or key not in operations:
            continue
        lines[line.index] = f"{key} = {operations.pop(key)}\n"
    if operations:
        additions = [f"{key} = {value}\n" for key, value in operations.items()]
        lines[end:end] = additions
        end += len(additions)
    return lines


def apply_additions(text: str, additions: list[_Addition]) -> str:
    """The manifest with every addition merged in, as text.

    A line-level edit, because what a manifest author cares about is their
    comments and their order: the new statements are appended inside the route's
    existing `queries = [ … ]` array (or the array is added to the route's
    grant), a `[route.<proxy>.tables]` section is written under the grant with
    the route's existing table entries updated in place, and no other line is
    touched.

    Raises [`_FoldError`] when the file is not shaped the way the edit needs — a
    route with no `[[route]]` element, a grant written as an inline table, a
    `queries` key that is not a literal array — because half-editing a manifest
    would be worse than saying so, and because [`_verify_edit`] is what proves
    the result is the manifest plus exactly the plan.
    """
    scan = _Scan(text)
    for addition in additions:
        if addition.is_empty():
            continue
        scan = _apply([line.text for line in scan.lines], scan, addition)
    return scan.text()

#
# Rendering, and writing the file.
#


def render(additions: list[_Addition]) -> str:
    """The TOML to paste, one block per `(route, proxy)` grant.

    A grant with nothing to add prints nothing at all, and the notes — a denial
    that needs a hand, a statement that is already granted, one that was refused
    — are the caller's to print: what comes out of here is TOML, and only TOML,
    which is checked by parsing it before it is returned.
    """
    blocks: list[str] = []
    for addition in additions:
        if addition.is_empty():
            continue
        lines: list[str] = [
            f"# {_comment(addition.route)} (proxy {_comment(addition.proxy)}): "
            "what the log's denials are missing",
        ]
        # The grant's own section, only when something goes in it: a
        # `[route.<proxy>]` with nothing under it says nothing, and a route that
        # only gains a table grant needs none.
        # Every name in a header goes through `_toml_key`: a proxy that is not a
        # plain key is quoted here, and refused long before the edit (D3c).
        proxy = _toml_key(addition.proxy)
        if addition.queries or addition.functions:
            lines.append(f"[route.{proxy}]")
        if addition.queries:
            lines.append("queries = [")
            for sql, reason in addition.queries:
                lines.append(_query_line(sql, reason).rstrip("\n"))
            lines.append("]")
        if addition.functions:
            rendered = ", ".join(_toml_string(name) for name in addition.functions)
            lines.append(f"functions = [{rendered}]")
        if addition.tables:
            lines.append("")
            lines.append(f"[route.{proxy}.tables]")
            for table, grant in sorted(addition.tables.items()):
                lines.append(_render_table(table, grant))
        block = "\n".join(lines)
        if not _parses(block):
            raise _FoldError(
                f"the block written for {addition.route} ({addition.proxy}) is not valid "
                "TOML, which is a bug in fold: nothing was written"
            )
        blocks.append(block)
    return "\n\n".join(blocks) + ("\n" if blocks else "")


def _parses(text: str) -> bool:
    """Whether a rendered snippet is valid TOML on its own."""
    import tomllib

    try:
        tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return False
    return True


def _fold(args: argparse.Namespace) -> int:
    """Reads the logs, plans the additions, and prints or writes them."""
    import tomllib

    manifest_path = Path(args.manifest)
    try:
        text = manifest_path.read_text(encoding="utf-8")
        document = tomllib.loads(text)
    except OSError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise _FoldError(f"{manifest_path} is not valid TOML: {error}") from error

    events = read_events(args.logs)
    denials = denials_of(events)
    additions = plan(
        document,
        denials,
        prefer_tables=args.prefer_tables,
        allow_dangerous=args.allow_dangerous,
    )
    if args.write:
        return _write(manifest_path, text, document, events, denials, additions)
    rendered = render(additions)
    for addition in additions:
        for note in addition.notes:
            _diagnostic(note)
    if not rendered:
        print(f"{manifest_path}: nothing to add", file=sys.stderr)
        return 0
    print(rendered, end="")
    return 0


def _write(
    manifest_path: Path,
    text: str,
    document: dict[str, Any],
    events: list[dict[str, Any]],
    denials: list[_Denial],
    additions: list[_Addition],
) -> int:
    """Merges the additions into the manifest, after proving they are all it changes."""
    wanted = [addition for addition in additions if not addition.is_empty()]
    for addition in additions:
        for note in addition.notes:
            _diagnostic(note)
    if not wanted:
        print(f"{manifest_path}: nothing to add", file=sys.stderr)
        return 0
    try:
        edited = apply_additions(text, wanted)
    except _FoldError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error
    _verify_edit(manifest_path, text, edited, document, wanted, events, denials)
    manifest_path.write_text(edited, encoding="utf-8")
    for addition in wanted:
        print(
            f"{manifest_path}: {addition.route} ({addition.proxy}): "
            f"{len(addition.queries)} statement(s), "
            f"{len(addition.tables)} table(s) and "
            f"{len(addition.functions)} function(s) added",
            file=sys.stderr,
        )
    return 0


def _verify_edit(
    manifest_path: Path,
    old_text: str,
    new_text: str,
    document: dict[str, Any],
    additions: list[_Addition],
    events: list[dict[str, Any]],
    denials: list[_Denial],
) -> None:
    """Proves the edit is the manifest plus exactly the planned grants (D3c).

    Nothing here trusts the writer or the plan: the edited document is parsed
    again, its grants are compared key by key to what the plan says they should
    be, and blastit's own `audit` is run on the whole document with the log's
    traffic as observations, so every statement the plan grants must come out
    *granted* there. A result that changed anything else — a route invented, a
    proxy renamed, a group edited, an entry that grew a column nobody planned —
    is a refusal, and the file is left exactly as it was.
    """
    import tomllib

    try:
        old = tomllib.loads(old_text)
        new = tomllib.loads(new_text)
    except tomllib.TOMLDecodeError as error:
        raise _FoldError(f"{manifest_path}: the edit is not valid TOML: {error}") from error

    _unchanged(new.get("version"), old.get("version"), "the manifest version")
    _unchanged(new.get("groups"), old.get("groups"), "the global groups")
    _unchanged(new.get("proxy"), old.get("proxy"), "the proxy declarations")
    _unchanged(new.get("group"), old.get("group"), "the groups")

    planned = {(addition.route, addition.proxy): addition for addition in additions}
    old_routes = old.get("route") or []
    new_routes = new.get("route") or []
    if len(old_routes) != len(new_routes) or [
        route.get("name") for route in old_routes
    ] != [route.get("name") for route in new_routes]:
        raise _FoldError(
            f"{manifest_path}: the edit changed the routes a manifest declares; "
            "fold never creates, removes or renames one"
        )
    for before, after in zip(old_routes, new_routes):
        name = str(before.get("name"))
        for key in sorted(set(before) | set(after), key=str):
            addition = planned.get((name, key))
            if addition is None:
                _unchanged(after.get(key), before.get(key), f"the {key!r} of route {name!r}")
                continue
            expected = _expected_grant(before.get(key), addition)
            if after.get(key) != expected:
                raise _FoldError(
                    f"{manifest_path}: the edit of route {name!r} ({key}) is not the grant "
                    "the plan asked for; nothing was written"
                )
    for route_name, proxy in planned:
        written = next(
            (route for route in new_routes if str(route.get("name")) == route_name), None
        )
        if written is None or proxy not in written:
            raise _FoldError(
                f"{manifest_path}: the plan names the grant {route_name} ({proxy}), which "
                "the edit did not write; nothing was written"
            )

    missing = _missing_after_edit(new_text, document, additions, events, denials)
    for addition in additions:
        for sql in addition.statements:
            key = (addition.route, addition.proxy, sql)
            if key not in missing:
                continue
            raise _FoldError(
                f"{manifest_path}: after the edit, blastit's own audit still refuses "
                f"{sql!r} on the route {addition.route!r} (proxy {addition.proxy!r}); "
                "nothing was written"
            )


def _unchanged(after: Any, before: Any, what: str) -> None:
    """Refuses an edit that changed something it had no business changing."""
    if after != before:
        raise _FoldError(f"the edit changed {what}; nothing was written")


def _expected_grant(before: Any, addition: _Addition) -> dict[str, Any]:
    """The grant an addition plans: the old one, plus exactly the plan."""
    expected = copy.deepcopy(before) if isinstance(before, dict) else {}
    queries = [str(sql) for sql in _text_list(expected.get("queries"))]
    for sql, _reason in addition.queries:
        if sql not in queries:
            queries.append(sql)
    if queries:
        expected["queries"] = queries
    if addition.tables:
        tables = dict(expected.get("tables") or {})
        tables.update(addition.tables)
        expected["tables"] = tables
    if addition.functions:
        # The writer appends what the route did not have to the array it has, so
        # that is what the file has to hold: nothing already written moves.
        functions = [str(name) for name in _text_list(expected.get("functions"))]
        functions.extend(name for name in addition.functions if name not in functions)
        expected["functions"] = functions
    return expected


#: Where the audit runs on the edited manifest: the statements the plan grants,
#: keyed by `(route, proxy, sql)`, and only for the pairs the log shows a request
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
    the same log: the `request` events give each route the `(method, path)` it
    was served at, and a denial is attached to the first request of its route.
    A denial for a route or proxy the manifest does not declare is left out —
    that is exactly what the plan refused with a note, and the audit cannot
    attribute it at all.

    What comes back is keyed by `(route, proxy, statement)` and holds only what
    the audit actually *looks at*: a route is "checked" when its own report says
    it served a request, not merely when the log had one. A path the edited
    manifest routes elsewhere — a log from an older manifest, a rule that moved —
    would otherwise make the proof pass by looking at nothing.
    """
    from blastit import _blastit

    routes = _routes(document)
    proxies = set((document.get("proxy") or {}).keys())
    observable = [
        denial
        for denial in denials
        if denial.route in routes and denial.proxy in proxies
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
    except Exception as error:  # noqa: BLE001 - blastit refuses the manifest, or the audit
        raise _FoldError(
            f"blastit's own audit refused the edited manifest ({error}); nothing was written"
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
        if (
            addition.route in checked
            and (addition.route, addition.proxy) in blocks
        ):
            continue
        _diagnostic(
            f"{addition.route} ({addition.proxy}): the edited manifest routes none of the "
            "log's requests here, so the audit could not check that the new grant covers "
            "it; fold wrote it, and the next run will say"
        )
    return refused


if __name__ == "__main__":
    raise SystemExit(_direct())
