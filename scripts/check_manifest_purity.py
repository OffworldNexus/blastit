#!/usr/bin/env python3
"""The `std::env` guard for `crates/blastit-manifest/src/` (MB.21.3.1, MB.21.5.5).

A `Blastfile` names environment variables; it never reads them. `$DATABASE_URL`
is a name that the lowering step copies into `Proxy.input`, and the value is
resolved by `blastit-env` at boot. That is what makes a manifest safe to commit
and what keeps the parser from becoming an oracle for secrets (design §17.2),
so a `std::env` read under `crates/blastit-manifest/src/` is a build failure
rather than a review comment (R20, R24).

The literal spelling counts wherever it appears: the design's own words are "a
`gate` scan fails the build if `std::env` appears under
`crates/blastit-manifest/src/`", so `std::env` in a comment or a string literal
is a hit too, and a file that needs to explain the rule says "the environment"
instead.

"Appears" means any *spelling* of the path, not just the characters in that
sentence (MB.21.5.5): `std :: env`, `::std::env`, `use std::env as e`,
`use std::{fs, env}`, `use ::std::{env as e}`, `use std::{io, env::{self, var}}`,
a raw identifier (`use std::r#env as e;` then `e::var(..)`) and a call through
the imported name (`env::var(..)`) are all hits. The scan reads the source once
into identifiers and punctuation and walks every `use` tree, so a nested list, a
`self`, an alias or a multi-line group cannot hide the module the way one
literal needle could. That wider scan runs on code, with comments and string
literals skipped as trivia: a comment *inside* `use std::{…}` cannot break the
tree, and prose mentioning `env` cannot trip it.

Imports and paths are classified by their **origin**, not their spelling: `std`
and every transitive alias of it (`use std::{self as s}`, `extern crate std as
s`, `use s as t`) name the crate, so `use s::env as e` and `e::var(..)` are
`std::env` however the path is written. R24 is about `std::env`, so an honest
`mycrate::env` module is not this gate's business; a local module that reads
the environment contains a `std::env` read of its own and fails there.

A `use` item the scanner cannot classify (over the token or depth budget) is
refused, not passed, because a gate that cannot read its input must not report
success (MB.3.64/MB.3.67).

Enumerating spellings does not converge — three review rounds each found a new
one (`use std::{env}`, `r#env`, a `std` alias root) — so the rule is a
structural one: **in any file that mentions `env`, every `use` item and every
`X::…` path must be fully classifiable.** A glob import (`use std::*;`) and a
`use` window over the token/depth budget are refused; so is an alias — or any
path naming `env` — whose origin is outside this file (`crate::`/`super::`/
`self::`, or a path deeper than `root::name`). Each refusal names its
`file:line` and the construct; a plain alias-less `use crate::a::b;` that names
no `env` is classifiable and passes. A file that never mentions `env` is
scanned for hits only, so the clean tree gains no refusals. An honest
`mycrate::env` is a real crate path and stays legal (R24 is about `std::env`,
and a local module that reads the environment contains its own `std::env`).

Because the rule is textual it over-approximates, and deliberately so:
`std::env::consts` (target constants, not a read) fails. The covered crate
needs neither it nor the module, and a lexical gate that tried to tell a read
from an import would be the evasion again. The word `env` on its own
(`let env = …`, prose, a `"env"` literal) is not a path and passes; so do
`std::fs`, `std::process` and the other std modules.

Usage (stdlib only; `make manifest-purity` runs it):

    scripts/check_manifest_purity.py
    scripts/check_manifest_purity.py --root /path/to/fixture   # self-test hook

Exit 0 when no source file under the covered tree spells `std::env` and no
unclassifiable `use` item names `env`; exit 1 with one `path:line: …` line per
hit or refusal otherwise, and exit 1 rather than pass when the covered tree is
missing or empty.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

#: The tree the rule covers, relative to the scanned root.
COVERED = Path("crates/blastit-manifest/src")

#: The needle, kept for the refusal text: it names the rule, not the spelling.
NEEDLE = "std::env"

#: Identifiers are the only tokens the rule cares about.
_IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

#: A raw string opener: `r"`, `r#"`, `br##"`, `cr"`, …
_RAW_STRING = re.compile(r'(?:b|c)?r(?P<hashes>#*)"')

#: Longest `use` item scanned, in tokens. A `use` that does not close within
#: this many tokens is prose, not a `use` item; the widest real one is well
#: under a tenth of this.
_MAX_USE_TOKENS = 512

#: Deepest `use` group walked. Rust imports nest a couple of levels at most;
#: the bound keeps a malformed file from recursing without end.
_MAX_USE_DEPTH = 32

#: Alias-chain passes for `std` (`use s as t` chains) before the scan gives up.
_MAX_ALIAS_PASSES = 8

#: Roots whose origin is outside the file: a `std` alias can hide behind them.
_UNTRUSTED_ROOTS = frozenset({"crate", "super", "self"})

#: The identifier `env`, wherever it appears (code, comment or string).
_ENV_WORD = re.compile(r"\benv\b")

#: `(kind, text, line)` — kind is `"ident"` or `"punct"`.
Token = tuple[str, str, int]


def covered(root: Path) -> list[Path]:
    """Every Rust source file the guard reads, under `root`, in a stable order."""
    tree = root / COVERED
    if not tree.is_dir():
        return []
    return sorted(path for path in tree.rglob("*.rs") if path.is_file())


def _block_comment(text: str, start: int, line: int) -> tuple[int, int]:
    """The index after the `/* … */` at `start` (comments nest), and its line."""
    depth = 0
    index = start
    while index < len(text):
        if text.startswith("/*", index):
            depth += 1
            index += 2
        elif text.startswith("*/", index):
            depth -= 1
            index += 2
            if depth == 0:
                return index, line
        else:
            if text[index] == "\n":
                line += 1
            index += 1
    return len(text), line


def _quoted(text: str, quote: int, line: int) -> tuple[int, int]:
    """The index after the string literal whose opening `"` is at `quote`."""
    index = quote + 1
    while index < len(text):
        char = text[index]
        if char == "\\":
            if index + 1 < len(text) and text[index + 1] == "\n":
                line += 1
            index += 2
            continue
        if char == '"':
            return index + 1, line
        if char == "\n":
            line += 1
        index += 1
    return len(text), line


def _raw_string(text: str, quote: int, hashes: str, line: int) -> tuple[int, int]:
    """The index after the raw string opened at `quote` with `hashes` `#`s."""
    closing = '"' + hashes
    end = text.find(closing, quote + 1)
    end = len(text) if end < 0 else end + len(closing)
    return end, line + text.count("\n", quote, end)


def _tokens(text: str) -> list[Token]:
    """Identifiers and punctuation in *code*, each with the line it is on.

    Comments (nested, as Rust allows), string and char literals are trivia: the
    wider spellings are read from code, so a comment inside a `use` tree cannot
    break the parse. The literal `std::env` needle is applied to the raw lines
    by `scan`, which is what keeps the rule's own sentence comments-included.
    """
    tokens: list[Token] = []
    line = 1
    index = 0
    while index < len(text):
        char = text[index]
        if char == "\n":
            line += 1
            index += 1
            continue
        if char in " \t\r\f\v":
            index += 1
            continue
        if text.startswith("//", index):
            newline = text.find("\n", index)
            if newline < 0:
                break
            index = newline
            continue
        if text.startswith("/*", index):
            index, line = _block_comment(text, index, line)
            continue
        raw = _RAW_STRING.match(text, index)
        if raw is not None:
            index, line = _raw_string(text, raw.end() - 1, raw.group("hashes"), line)
            continue
        if char == "r" and text.startswith("#", index + 1):
            # A raw identifier names the same item: `r#env` is `env`. Emit the
            # inner name, never a bare `r` followed by a `#` (review, MB.21.5.5).
            raw_identifier = _IDENT.match(text, index + 2)
            if raw_identifier is not None:
                tokens.append(("ident", raw_identifier.group(), line))
                index = raw_identifier.end()
                continue
        if char == '"' or (char in "bc" and text.startswith('"', index + 1)):
            quote = index if char == '"' else index + 1
            index, line = _quoted(text, quote, line)
            continue
        if char == "'":
            following = index + 1
            if following < len(text) and text[following] == "\\":
                following += 2
                while following < len(text) and text[following] != "'":
                    if text[following] == "\n":
                        line += 1
                    following += 1
                index = min(following + 1, len(text))
            elif following + 1 < len(text) and text[following + 1] == "'":
                index = following + 2
            else:
                tokens.append(("punct", "'", line))
                index += 1
            continue
        identifier = _IDENT.match(text, index)
        if identifier is not None:
            tokens.append(("ident", identifier.group(), line))
            index = identifier.end()
        elif char == ":" and text.startswith("::", index):
            tokens.append(("punct", "::", line))
            index += 2
        else:
            tokens.append(("punct", char, line))
            index += 1
    return tokens


def _alias(
    tokens: list[Token], index: int, end: int
) -> tuple[int, Token | None] | None:
    """Reads an optional `as name`; returns `(next index, alias token or None)`."""
    if index < end and tokens[index][1] == "as":
        if index + 1 >= end or tokens[index + 1][0] != "ident":
            return None
        return index + 2, tokens[index + 1]
    return index, None


def _tree(
    tokens: list[Token],
    start: int,
    end: int,
    prefix: tuple[Token, ...],
    paths: list[tuple[list[Token], Token | None]],
    depth: int,
) -> int | None:
    """Parses one `use` tree at `tokens[start:end]`.

    Appends every path it imports to `paths` as `(segments, alias)`, where the
    alias token is the name an `as` gave that path (or None). Returns the index
    just past the tree, or None when the tokens are not a `use` tree. A failed
    attempt may leave partial `paths` behind, so the caller discards the list
    when this returns None or does not land on `end`.
    """
    if depth > _MAX_USE_DEPTH:
        return None
    index = start
    while index < end and tokens[index][1] == "::":
        index += 1
    if index >= end:
        return None
    kind, value, _line = tokens[index]
    if value == "{":
        index += 1
        if index >= end or tokens[index][1] == "}":
            return None
        while True:
            index = _tree(tokens, index, end, prefix, paths, depth + 1)
            if index is None or index >= end:
                return None
            if tokens[index][1] == ",":
                index += 1
                if index < end and tokens[index][1] == "}":
                    return index + 1
                continue
            if tokens[index][1] == "}":
                return index + 1
            return None
    if value == "*":
        paths.append(([*prefix, tokens[index]], None))
        return index + 1
    if kind != "ident":
        return None
    if value == "self":
        alias = _alias(tokens, index + 1, end)
        if alias is None:
            return None
        paths.append((list(prefix), alias[1]))
        return alias[0]
    segment = (*prefix, tokens[index])
    index += 1
    if index < end and tokens[index][1] == "as":
        alias = _alias(tokens, index, end)
        if alias is None:
            return None
        paths.append((list(segment), alias[1]))
        return alias[0]
    if index < end and tokens[index][1] == "::":
        return _tree(tokens, index + 1, end, segment, paths, depth + 1)
    paths.append((list(segment), None))
    return index


def _use_paths(
    tokens: list[Token], start: int, end: int
) -> list[tuple[list[Token], Token | None]] | None:
    """Every `(path segments, alias)` a `use` item imports, or None if unreadable.

    A window longer than `_MAX_USE_TOKENS` is not parsed at all: the caller
    treats the item as suspect rather than clean, because a gate that cannot
    read its input must not report success (MB.3.64/MB.3.67).
    """
    if end - start > _MAX_USE_TOKENS:
        return None
    paths: list[tuple[list[Token], Token | None]] = []
    if _tree(tokens, start, end, (), paths, 0) != end:
        return None
    return paths


def _use_end(tokens: list[Token], start: int) -> int | None:
    """The index of the `;` closing the `use` at `start`, or None.

    The search is not capped: a `use` item longer than `_MAX_USE_TOKENS` is
    still bounded by its own `;`, and the classification cap lives in
    `_use_paths` so the whole window is available to the suspect check.
    """
    for index in range(start, len(tokens)):
        if tokens[index][1] == ";":
            return index
    return None


def _use_spans(tokens: list[Token]) -> list[tuple[int, int, int]]:
    """Every `use` item as `(line, start, end)`; `end` is its `;` or the end."""
    spans: list[tuple[int, int, int]] = []
    for index, (kind, value, line) in enumerate(tokens):
        if kind != "ident" or value != "use":
            continue
        end = _use_end(tokens, index + 1)
        spans.append((line, index + 1, len(tokens) if end is None else end))
    return spans


def _std_names(tokens: list[Token], spans: list[tuple[int, int, int]]) -> set[str]:
    """`std` plus every name a `use` or `extern crate` gives it, transitively.

    `use std::{self as s}`, `extern crate std as s` and `use s as t` (when `s`
    is already a std name) all join the set. The loop is bounded by
    `_MAX_ALIAS_PASSES`: each pass adds at least one name, and Rust alias
    chains are a handful long, so a fixed point arrives long before the cap.
    """
    names = {"std"}
    for _pass in range(_MAX_ALIAS_PASSES):
        grown: set[str] = set()
        for index in range(len(tokens) - 4):
            if (
                tokens[index][1] == "extern"
                and tokens[index + 1][1] == "crate"
                and tokens[index + 2][1] == "std"
                and tokens[index + 3][1] == "as"
                and tokens[index + 4][0] == "ident"
            ):
                grown.add(tokens[index + 4][1])
        for _line, start, end in spans:
            paths = _use_paths(tokens, start, end)
            if paths is None:
                continue
            for path, alias in paths:
                if alias is None or len(path) != 1:
                    continue
                if path[0][1] in names:
                    grown.add(alias[1])
        grown -= names
        if not grown:
            break
        names |= grown
    return names


def _unknown_aliases(
    tokens: list[Token], spans: list[tuple[int, int, int]], std: set[str]
) -> set[str]:
    """Names whose origin the file does not resolve (fail-closed roots).

    An alias of `std` is resolved; an alias of a crate's own `root::name` is a
    known non-std path (`mycrate::env` stays legal). Everything else can hide a
    `std` alias: `crate::`/`super::`/`self::` roots, a path deeper than
    `root::name`, and an alias of an already-unknown name. Bounded like
    `_std_names`.
    """
    unknown: set[str] = set()
    for _pass in range(_MAX_ALIAS_PASSES):
        grown: set[str] = set()
        for _line, start, end in spans:
            paths = _use_paths(tokens, start, end)
            if paths is None:
                continue
            for path, alias in paths:
                if alias is None or not path:
                    continue
                names_std = path[0][1] in std
                if names_std:
                    continue
                if (
                    path[0][1] in _UNTRUSTED_ROOTS
                    or path[0][1] in unknown
                    or len(path) > 2
                ):
                    grown.add(alias[1])
        grown -= unknown
        if not grown:
            break
        unknown |= grown
    return unknown


def _inspect(
    tokens: list[Token], strict: bool
) -> tuple[set[int], list[tuple[int, str]]]:
    """`(hit lines, refusals)` for one file's code tokens.

    An import or a call is classified by its *origin*, not its spelling:
    `std` and every transitive alias of it — `use std::{self as s}`,
    `extern crate std as s`, `use s as t` — name the crate, so `s::env` and
    `e::var` after `use s::env as e` are `std::env` (review, MB.21.5.5).

    In a file that mentions `env` (`strict`), the gate also fails closed on
    every construct it cannot fully resolve: a glob import, an alias whose
    origin is outside the file (`crate::`/`super::`/`self::` or a path deeper
    than `root::name`), a path that reaches `env` behind an unresolved
    segment, and a `use` item over the token/depth budget. Three review rounds
    each found a fresh spelling, so enumeration does not converge; the safe
    rule is: if the file talks about `env` at all, nothing in it may be
    unclassifiable (round 3). A file that never mentions `env` is scanned for
    hits only, so the clean tree gains no refusals.
    """
    hits: set[int] = set()
    refusals: list[tuple[int, str]] = []
    spans = _use_spans(tokens)
    std = _std_names(tokens, spans)
    unknown = _unknown_aliases(tokens, spans, std)
    # A path or call through the aliased root: `s::env::var(..)` where s is std.
    for index in range(len(tokens) - 2):
        if (
            tokens[index][0] == "ident"
            and tokens[index][1] in std
            and tokens[index + 1][1] == "::"
            and tokens[index + 2][0] == "ident"
            and tokens[index + 2][1] == "env"
        ):
            hits.add(tokens[index + 2][2])
    for index, (kind, value, line) in enumerate(tokens):
        # A path or call through the imported name: `env::var(..)`.
        if (
            kind == "ident"
            and value == "env"
            and index + 1 < len(tokens)
            and tokens[index + 1][1] == "::"
        ):
            hits.add(line)
        # A path through a root the file cannot resolve.
        if (
            strict
            and kind == "ident"
            and value in unknown
            and index + 1 < len(tokens)
            and tokens[index + 1][1] == "::"
        ):
            message = (
                f"cannot classify `{value}::…`: `{value}` is an alias whose "
                "origin is outside this file"
            )
            refusals.append((line, message))
    for line, start, end in spans:
        paths = _use_paths(tokens, start, end)
        if paths is None:
            names_env = any(
                token[0] == "ident" and token[1] == "env" for token in tokens[start:end]
            )
            if strict or names_env:
                message = (
                    "suspect `use` item: cannot classify within the token/depth "
                    "budget in a file that mentions `env`"
                    if strict
                    else "suspect `use` item: cannot classify within the "
                    "token/depth budget, and its window names `env`"
                )
                refusals.append((line, message))
            continue
        for path, alias in paths:
            if not path:
                continue
            if strict and any(segment[1] == "*" for segment in path):
                message = (
                    "cannot classify a glob import (`use …::*`): it can inject "
                    "`std::env` into the file"
                )
                refusals.append((line, message))
                continue
            if path[0][1] in std:
                for token in path[1:]:
                    if token[0] == "ident" and token[1] == "env":
                        hits.add(token[2])
                continue
            if not strict:
                continue
            names_env = any(
                segment[0] == "ident" and segment[1] == "env" for segment in path
            )
            root = path[0][1]
            if (root in _UNTRUSTED_ROOTS or root in unknown) and (
                names_env or alias is not None
            ):
                message = (
                    f"cannot classify `{root}::…`: its origin is outside this file"
                )
                refusals.append((line, message))
                continue
            if any(
                segment[0] == "ident" and segment[1] == "env" for segment in path[2:]
            ):
                message = (
                    f"cannot classify `{root}::…::env`: a segment before it "
                    "could alias `std`"
                )
                refusals.append((line, message))
    return hits, refusals


def _hits(tokens: list[Token]) -> set[int]:
    """Every line that spells a `std::env` read, in any import spelling."""
    return _inspect(tokens, False)[0]


def _refusals(tokens: list[Token], strict: bool) -> list[tuple[int, str]]:
    """Every construct the scanner cannot fully classify in this file."""
    return _inspect(tokens, strict)[1]


def _sources(root: Path) -> list[tuple[str, list[str], list[Token], bool]]:
    """Each covered file as `(relative path, lines, tokens, mentions_env)`."""
    sources: list[tuple[str, list[str], list[Token], bool]] = []
    for path in covered(root):
        relative = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        sources.append(
            (
                relative,
                text.split("\n"),
                _tokens(text),
                _ENV_WORD.search(text) is not None,
            )
        )
    return sources


def scan(root: Path) -> list[tuple[str, int, str]]:
    """Every `(file, line, text)` hit under `root`, in file and line order.

    Two passes per file: the literal needle over the raw lines (so the rule's
    own sentence holds for comments and strings), and the wider spellings from
    the code tokens.
    """
    findings: list[tuple[str, int, str]] = []
    for relative, lines, tokens, _strict in _sources(root):
        numbers = _hits(tokens)
        numbers.update(number for number, line in enumerate(lines, 1) if NEEDLE in line)
        for number in sorted(numbers):
            findings.append((relative, number, lines[number - 1].strip()))
    return findings


def scan_refusals(root: Path) -> list[tuple[str, int, str]]:
    """Every `(file, line, message)` construct the scanner cannot classify."""
    findings: list[tuple[str, int, str]] = []
    for relative, _lines, tokens, strict in _sources(root):
        for number, message in sorted(_refusals(tokens, strict)):
            findings.append((relative, number, message))
    return findings


def main(argv: list[str] | None = None) -> int:
    """Runs the guard; returns the process exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        default=str(REPO),
        help="the tree to scan (default: this repository)",
    )
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    files = covered(root)
    if not files:
        print(
            f"manifest-purity: no Rust source under {COVERED.as_posix()}/ in "
            f"{root}; refusing to pass",
            file=sys.stderr,
        )
        return 1
    findings = scan(root)
    for path, line, text in findings:
        print(f"{path}:{line}: {NEEDLE} is not allowed here: {text}", file=sys.stderr)
    refusals = scan_refusals(root)
    for path, line, message in refusals:
        print(f"{path}:{line}: {message}", file=sys.stderr)
    if findings or refusals:
        print(
            f"manifest-purity: {len(findings)} {NEEDLE} use(s) and "
            f"{len(refusals)} unclassifiable construct(s) under "
            f"{COVERED.as_posix()}/; a Blastfile names variables and never reads "
            "them (R20/R24)",
            file=sys.stderr,
        )
        return 1
    print(f"manifest-purity: ok — {len(files)} source file(s), no {NEEDLE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
