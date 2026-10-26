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
  catalogue, an unknown function, a statement kind no table grant describes.

Lines that are not events are skipped, and so is an App Platform prefix in front
of one (`2026-10-25T14:02:11.384Z app[web.1]: {…}`): see
[`blastit.logs`](logs.py).

`--write`
---------

Without `--write` the file is not touched: the TOML to paste is printed. With
it, the additions are merged into the file with a line-level edit — the new
statements go into the route's existing `queries = [ … ]`, a new
`[route.<proxy>.tables]` section is added under the route's grant, and a table
entry that was there keeps its position and its neighbours' comments. Nothing
else in the file is rewritten, so a diff shows only the grants.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable, Sequence

from blastit import logs

__all__ = ["main"]


def main(argv: Sequence[str] | None = None) -> int:
    """`blastit-manifest fold LOG... --manifest blastit.toml [--write] [--prefer-tables]`.

    Returns the process exit status: 0 when it printed (or wrote) what the
    manifest was missing — an empty diff is a normal, good outcome — and 1 on
    anything it could not do (an unreadable log, a manifest it cannot parse or
    edit safely).
    """
    parser = _parser()
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except _FoldError as error:
        print(f"blastit-manifest fold: {error}", file=sys.stderr)
        return 1


def _parser() -> argparse.ArgumentParser:
    """The command line: one subcommand today, `fold`."""
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
    fold.set_defaults(handler=_fold)
    return parser


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
    #: What was left out and why, for the summary.
    notes: list[str] = field(default_factory=list)

    def is_empty(self) -> bool:
        """Whether there is nothing to write."""
        return not self.queries and not self.tables and not self.functions


#
# Reading the logs.
#


def read_denials(logs_or_stdin: Iterable[str]) -> list[_Denial]:
    """The denials every log holds, deduplicated, in the order they were read.

    `-` reads standard input, so a live pipeline works:
    `ssh app blastit | blastit-manifest fold - --manifest blastit.toml`.
    """
    denials: list[_Denial] = []
    seen: set[tuple[str, str, str]] = set()
    for source in logs_or_stdin:
        if source == "-":
            text = sys.stdin.read()
        else:
            path = Path(source)
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError as error:
                raise _FoldError(f"{path}: {error}") from error
        for denial in logs.denials(logs.parse_events(text.splitlines())):
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


#
# Planning: what the manifest is missing.
#


def plan(
    document: dict[str, Any],
    denials: list[_Denial],
    *,
    prefer_tables: bool,
    covering: Callable[[list[str], str], str] | None = None,
) -> list[_Addition]:
    """What each `(route, proxy)` grant has to gain, in manifest order.

    `document` is the manifest, parsed by `tomllib`; `covering` is in injectable
    for tests and defaults to `blastit._blastit.covering`.
    """
    routes = _routes(document)
    groups = document.get("group") or {}
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
                f"{route_name}: the manifest declares no route of that name — add it, "
                "then fold again"
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
        addition = _plan_route(
            route=route,
            route_name=route_name,
            proxy=proxy,
            grant=grant,
            groups=groups,
            global_groups=global_groups,
            denials=by_pair[(route_name, proxy)],
            prefer_tables=prefer_tables,
            covering=covering,
        )
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
    covering: Callable[[list[str], str], str] | None,
) -> _Addition:
    """What one route's grant is missing, from its denials."""
    addition = _Addition(route=route_name, proxy=proxy)
    existing = _granted_queries(proxy, grant, groups, global_groups, route)
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
        new.append(denial)
    if not new:
        return addition

    analyse = covering or _covering
    prefix = f"route.{proxy}"
    existing_tables = grant.get("tables") if grant and isinstance(grant.get("tables"), dict) else {}
    kept: list[_Denial] = []
    for denial in new:
        if existing_tables:
            single = json.loads(analyse([denial.parseable], prefix))
            if not single["unanalysable"] and _covered_by(existing_tables, single["tables"]):
                addition.notes.append(
                    f"{denial.sql!r}: already covered by the route's [{prefix}.tables] grant"
                )
                continue
        kept.append(denial)
    if not kept:
        return addition

    if not prefer_tables:
        addition.queries = [(denial.sql, denial.reason) for denial in kept]
        return addition

    merged = json.loads(analyse([denial.parseable for denial in kept], prefix))
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


def _covered_by(existing: Any, covering: Any) -> bool:
    """Whether a manifest's table grant already covers a covering grant (D7).

    Deliberately conservative: it answers "yes" only when every column the new
    grant names is named by the existing one, `"*"` included. A lock's `update`
    on one column is compared as a column list like any other, and anything this
    does not understand — an empty entry, a table the manifest does not mention
    — is a "no", because reporting a statement that is already covered is a
    nuisance while hiding one is a hole.
    """
    if not isinstance(covering, dict):
        return False
    for table, grant in covering.items():
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
    operations = []
    for operation in ("select", "insert", "update"):
        value = grant.get(operation)
        if value is None:
            continue
        if value == "*":
            operations.append(f'{operation} = "*"')
        else:
            columns = ", ".join(_toml_string(str(column)) for column in value)
            operations.append(f"{operation} = [{columns}]")
    if grant.get("delete"):
        operations.append("delete = true")
    return f"{_toml_key(table)} = {{ {', '.join(operations)} }}"


def _toml_string(value: str) -> str:
    """A TOML basic string, JSON-escaped except for the quote style."""
    return json.dumps(value, ensure_ascii=False)


def _toml_key(value: str) -> str:
    """A TOML key: bare when it can be, quoted otherwise."""
    if re.fullmatch(r"[A-Za-z0-9_-]+", value):
        return value
    return _toml_string(value)


#
# Rendering, and writing the file.
#


def render(additions: list[_Addition]) -> str:
    """The TOML to paste, one block per `(route, proxy)` grant.

    A grant with nothing to add prints nothing at all, and the notes — a denial
    that needs a hand, a statement that is already granted (an undeclared route,
    an unrestricted grant) — are the caller's to print: what comes out of here
    is TOML, and only TOML.
    """
    blocks: list[str] = []
    for addition in additions:
        if addition.is_empty():
            continue
        lines: list[str] = [
            f"# {addition.route} (proxy {addition.proxy}): what the log's denials are missing",
            f"[route.{addition.proxy}]",
        ]
        if addition.queries:
            lines.append("queries = [")
            for sql, reason in addition.queries:
                comment = f"  # {reason}" if reason else ""
                lines.append(f"  {_toml_string(sql)},{comment}")
            lines.append("]")
        if addition.functions:
            rendered = ", ".join(_toml_string(name) for name in addition.functions)
            lines.append(f"functions = [{rendered}]")
        if addition.tables:
            lines.append("")
            lines.append(f"[route.{addition.proxy}.tables]")
            for table, grant in sorted(addition.tables.items()):
                lines.append(_render_table(table, grant))
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks) + ("\n" if blocks else "")


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

    denials = read_denials(args.logs)
    additions = plan(document, denials, prefer_tables=args.prefer_tables)
    if args.write:
        return _write(manifest_path, text, additions)
    rendered = render(additions)
    notes = [note for addition in additions for note in addition.notes]
    for note in notes:
        print(f"# {note}", file=sys.stderr)
    if not rendered:
        print(f"{manifest_path}: nothing to add", file=sys.stderr)
        return 0
    print(rendered, end="")
    return 0


def _write(manifest_path: Path, text: str, additions: list[_Addition]) -> int:
    """Merges the additions into the manifest, keeping everything else."""
    wanted = [addition for addition in additions if not addition.is_empty()]
    for addition in additions:
        for note in addition.notes:
            print(f"# {note}", file=sys.stderr)
    if not wanted:
        print(f"{manifest_path}: nothing to add", file=sys.stderr)
        return 0
    try:
        edited = apply_additions(text, wanted)
    except _FoldError as error:
        raise _FoldError(f"{manifest_path}: {error}") from error
    manifest_path.write_text(edited, encoding="utf-8")
    for addition in wanted:
        print(
            f"{manifest_path}: {addition.route} ({addition.proxy}): "
            f"{len(addition.queries)} statement(s), "
            f"{len(addition.tables)} table(s) added",
            file=sys.stderr,
        )
    return 0


def apply_additions(text: str, additions: list[_Addition]) -> str:
    """The manifest with every addition merged in, as text.

    A line-level edit, because what a manifest author cares about is their
    comments and their order: the new statements are appended inside the route's
    existing `queries = [ … ]` array (or the array is added to the route's
    grant), a `[route.<proxy>.tables]` section is written under the grant with
    the route's existing table entries updated in place, and no other line is
    touched.

    Raises [`_FoldError`] when the file is not shaped the way the edit needs —
    a route with no `[[route]]` block, a `queries` key this tool cannot edit —
    because half-editing a manifest would be worse than saying so.
    """
    lines = text.splitlines(keepends=True)
    for addition in additions:
        if addition.is_empty():
            continue
        lines = _apply(lines, addition)
    return "".join(lines)


#: A table header line: `[a.b]`, `[a.b.c]`, `[[route]]`.
_HEADER = re.compile(r"^\s*\[(?P<double>\[)?[^\]]+\]\]?\s*(#.*)?$")


def _header_key(line: str) -> str | None:
    """The dotted key of a table header line, or `None` for anything else."""
    match = _HEADER.match(line)
    if match is None:
        return None
    text = line.strip()
    double = text.startswith("[[")
    close = "]]" if double else "]"
    end = text.find(close, 2 if double else 1)
    start = 2 if double else 1
    return text[start:end].strip()


def _apply(lines: list[str], addition: _Addition) -> list[str]:
    """Merges one `(route, proxy)` addition into the manifest's lines."""
    start = _route_block(lines, addition.route)
    if start is None:
        raise _FoldError(
            f"no `[[route]]` block declares the route {addition.route!r}; add the route "
            "first, then fold again"
        )
    grant = _grant_block(lines, start, addition.proxy)
    if grant is None:
        grant = _add_grant_block(lines, _route_region_end(lines, start), addition.proxy)
    if addition.queries:
        lines = _add_queries(lines, grant, addition.queries)
    if addition.functions:
        lines = _set_functions(lines, grant, addition.functions)
    if addition.tables:
        lines = _set_tables(lines, start, addition.proxy, addition.tables)
    return lines


def _route_region_end(lines: list[str], start: int) -> int:
    """The line the route's own region ends at: the next `[[route]]`, or EOF.

    A `[route.<proxy>]` section and a `[route.<proxy>.tables]` one both belong
    to the last `[[route]]` before them, so everything up to the next route is
    this route's — wherever a `[group.*]` of another kind sits in between.
    """
    index = start + 1
    while index < len(lines):
        if _header_key(lines[index]) == "route":
            return index
        index += 1
    return len(lines)



def _route_block(lines: list[str], name: str) -> int | None:
    """The line index of the `[[route]]` block whose `name` is `name`."""
    index = 0
    while index < len(lines):
        if _header_key(lines[index]) == "route":
            end = _block_end(lines, index + 1)
            value = _find_key(lines, index + 1, end, "name")
            if value is not None and value.strip().strip("\"'") == name:
                return index
            index = end
            continue
        index += 1
    return None


def _block_end(lines: list[str], start: int) -> int:
    """The first line index at or after `start` that opens a new table."""
    index = start
    while index < len(lines):
        if _header_key(lines[index]) is not None:
            return index
        index += 1
    return len(lines)


def _find_key(lines: list[str], start: int, end: int, key: str) -> str | None:
    """The value of `key = …` inside a block, as it was written."""
    pattern = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(?P<value>.*?)\s*(#.*)?$")
    for index in range(start, end):
        match = pattern.match(lines[index])
        if match is not None:
            return match.group("value")
    return None


def _grant_block(lines: list[str], start: int, proxy: str) -> int | None:
    """The line index of the route's `[route.<proxy>]` header, or `None`."""
    wanted = f"route.{proxy}"
    for index in range(start + 1, _route_region_end(lines, start)):
        if _header_key(lines[index]) == wanted:
            return index
    return None


def _add_grant_block(lines: list[str], insert: int, proxy: str) -> int:
    """Writes a new `[route.<proxy>]` block, and returns its header's line index."""
    block: list[str] = []
    if insert > 0 and lines[insert - 1].strip() != "":
        block.append("\n")
    block.append(f"[route.{proxy}]\n")
    lines[insert:insert] = block
    return insert + len(block) - 1


def _add_queries(
    lines: list[str], grant: int, queries: list[tuple[str, str]]
) -> list[str]:
    """Appends statements to the grant's `queries`, or writes the array."""
    end = _block_end(lines, grant + 1)
    items = [
        f"  {_toml_string(sql)},{f'  # {reason}' if reason else ''}\n" for sql, reason in queries
    ]
    for index in range(grant + 1, end):
        stripped = lines[index].lstrip()
        if not stripped.startswith("queries"):
            continue
        opening = stripped.find("[")
        if opening < 0:
            raise _FoldError(
                f"the `queries` key of `{lines[grant].strip()}` is not a literal array; "
                "edit it by hand"
            )
        prefix = lines[index][: len(lines[index]) - len(stripped)]
        if "]" not in stripped[opening + 1 :]:
            # A multi-line array: the new items go in front of its closing bracket.
            closing = _closing_bracket(lines, index, end)
            lines[closing:closing] = items
            return lines
        # A one-line array: rewritten as a multi-line one, its text kept as it is
        # and whatever followed the `]` (a comment, usually) kept with it.
        closing = stripped.find("]", opening)
        body = stripped[opening + 1 : closing].strip()
        suffix = stripped[closing + 1 :].rstrip("\n").rstrip()
        block = [f"{prefix}{stripped[: opening + 1]}\n"]
        if body:
            block.append(f"  {body},\n")
        block.extend(items)
        block.append(f"{prefix}]{'  ' + suffix if suffix else ''}\n")
        lines[index : index + 1] = block
        return lines
    lines[grant + 1 : grant + 1] = ["queries = [\n", *items, "]\n"]
    return lines


def _closing_bracket(lines: list[str], start: int, end: int) -> int:
    """The line index of the `]` that closes the array opened on `start`."""
    for index in range(start, end):
        if lines[index].strip().startswith("]"):
            return index
    raise _FoldError(
        f"the `queries` array opened on line {start + 1} is never closed; edit it by hand"
    )


def _set_functions(lines: list[str], grant: int, functions: list[str]) -> list[str]:
    """Writes the grant's `functions = [ … ]` line, or replaces it."""
    end = _block_end(lines, grant + 1)
    rendered = ", ".join(_toml_string(name) for name in functions)
    line = f"functions = [{rendered}]\n"
    for index in range(grant + 1, end):
        if lines[index].lstrip().startswith("functions"):
            lines[index] = line
            return lines
    lines[grant + 1 : grant + 1] = [line]
    return lines


def _set_tables(
    lines: list[str], route_start: int, proxy: str, tables: dict[str, dict[str, Any]]
) -> list[str]:
    """Writes a route's `[route.<proxy>.tables]` section, merged in place."""
    wanted = f"route.{proxy}.tables"
    route_end = _route_region_end(lines, route_start)
    header: int | None = None
    for index in range(route_start + 1, route_end):
        if _header_key(lines[index]) == wanted:
            header = index
            break
    if header is None:
        block: list[str] = []
        if route_end > 0 and lines[route_end - 1].strip() != "":
            block.append("\n")
        block.append(f"[{wanted}]\n")
        lines[route_end:route_end] = block
        header = route_end + len(block) - 1
    end = _block_end(lines, header + 1)
    remaining = dict(tables)
    for index in range(header + 1, end):
        key = _entry_key(lines[index])
        if key is None or key not in remaining:
            continue
        lines[index] = _render_table(key, remaining.pop(key)) + "\n"
    if remaining:
        additions = [f"{_render_table(table, grant)}\n" for table, grant in sorted(remaining.items())]
        lines[end:end] = additions
    return lines


#: `name = …` or `"name" = …` inside a `[…tables]` section.
_ENTRY = re.compile(r'^\s*(?:"(?P<quoted>[^"]*)"|(?P<bare>[A-Za-z0-9_-]+))\s*=')


def _entry_key(line: str) -> str | None:
    """The table name a `table = { … }` line grants, or `None`."""
    match = _ENTRY.match(line)
    if match is None:
        return None
    return match.group("quoted") or match.group("bare")


if __name__ == "__main__":
    raise SystemExit(main())
