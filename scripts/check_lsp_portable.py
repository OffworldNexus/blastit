#!/usr/bin/env python3
"""The portability guard for `crates/blastit-lsp` (`MB.35.6`, design §1.3).

`blastit-lsp` is the only artifact shipped for macOS and Windows, and it is
portable for one reason: its closure is `blastit-manifest` + `serde_json`, and its
own code is `std` plus those two. One `use` that reaches for `rustix` compiles on
Linux and breaks two of five targets in a release nobody re-read, so the promise
is a gate.

Six promises; every refusal names the one it broke:

* `manifest` — no more than `{blastit-manifest, serde_json}`, in **any**
  dependency table (a `[target.'cfg(windows)'.dependencies]` row is the edge a
  `[dependencies]` count misses), no `build.rs`, no `[features]` entry, no
  `optional` dependency, and every `[lib]`/`[[bin]]` target `path` resolves inside
  `crates/blastit-lsp/src/` so Cargo compiles no root file this guard did not read.
  A dependency the code still needs but the manifest stopped naming is the
  **compiler's** red (`E0433`): this guard enforces the ceiling, not the floor.
* `source` — **a token-stream scan and a `std` allowlist, not a per-line regex.**
  Every `.rs` file under `src/` (refusing symlinked files and directories out of
  `src/`) is lexed by
  `code_tokens()`: comments and char literals are skipped as trivia, string
  literals become single `"str"` tokens (so prose inside `"…"` never masquerades
  as code, while `#[cfg(target_os = "linux")]` keeps its predicate), and `r#ident`
  raw identifiers normalize to `ident`. Over that token stream:
  - every `extern crate X [as Y]` and `use` tree is flattened and crate/module
    aliases for `std`/`core`/`alloc` and the named foreign crates are resolved to
    a fixed point (`extern\ncrate std as s; s::process::Command::new(..)` resolves
    to `std::process::Command::new`);
  - every resolved `std`/`core`/`alloc` path must match one of the exact rows of
    `STD_ALLOWLIST` below (never a prefix: `std::io` does not allow
    `std::io::Result`), and a legitimate new item stays red until its row and
    reason are added;
  - every `#[…]`/`#![…]` attribute and macro invocation is checked on tokens, so
    split forms (`#[\ncfg\n(unix)]`, `cfg\n!(unix)`, `include\n!("…")`,
    `#[\npath = "…"]`, `extern\ncrate`, `extern\n"C"`, `libc\n::…`) are caught
    identically to single-line spellings; only outer `#[cfg(test)]` is legal;
  - `#![forbid(unsafe_code)]` must be a real inner attribute in the token stream
    of `lib.rs`, `main.rs` and every manifest target root (not merely text in a
    comment), and `src/main.rs` is pinned byte for byte to its three-line shape.
* `environment` — any resolved `std::env` path (including through an alias such as
  `extern crate std as s; s::env::var(..)`) is reported as its own broken promise,
  and `STD_ALLOWLIST` itself is checked and refused if a `std::env` row is ever
  added to it.
* `closure` — `cargo metadata --offline --locked` shows no **named** serving or
  privilege crate in the normal closure, unfiltered by target (a host-filtered
  graph proves the host's edges only), and a refusal prints the path that brought
  it in. An unreadable or stale graph is red, never a skip. **This is a denylist
  of the crates §1.3 names**: the `manifest` ceiling makes the *direct* case true
  by construction, while the *transitive* case is only as good as the list — a
  future `tokio` arriving through `blastit-manifest` would pass. Freezing the
  reviewed closure in a checked-in file is the follow-up that closes it.
* `identity` — one home: no `crates/blastit/src/lsp/`, `src/lib.rs` re-exports
  `blastit_lsp as lsp`, `Cargo.toml` names `blastit-lsp` once.
* `wiring` — `make check` runs `lsp-portable`, which runs this guard **and** its
  negative twins.

Scope of what is proved: within `crates/blastit-lsp`'s manifest and token stream,
target-path escapes, line splits, whitespace, comments, raw identifiers, `use`
groups/globs/aliases and `extern crate` aliases reduce to the canonical paths and
constructs above. A single-crate lexical scan does **not** inspect a `pub use std`
re-export or macro body inside an allowed dependency (`blastit-manifest` and
`serde_json` expose none today).

    scripts/check_lsp_portable.py
    scripts/check_lsp_portable.py --root /path/to/fixture   # self-test hook

Exit 0 with one line when all six hold; exit 1 with one `path:line: property:
detail` line per refusal, and 1 rather than pass on a tree it cannot read. No
`--allow`, no `--force` and no `--only`: an escape hatch is the next regression's
home.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from collections import deque
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

#: The crate this guard is about, relative to the scanned root.
CRATE = Path("crates/blastit-lsp")

#: The whole allowed `[dependencies]` set, from design §1.3.
ALLOWED = ("blastit-manifest", "serde_json")

#: The named forbidden set of a crate graph, grouped by the class its message names.
SERVING = (
    "blastit",
    "blastit-sys",
    "blastit-dbproxy",
    "blastit-scheduler",
    "blastit-http",
    "blastit-view",
    "blastit-py",
    "blastit-php-sapi",
)
PRIVILEGE = ("rustix", "nix", "libc", "pyo3")

#: Dependency table names Cargo accepts at the root or under `target.*`.
DEP_TABLES = ("dependencies", "build-dependencies", "dev-dependencies")

#: **The `source` promise, as an allowlist.** Every `std` path `src/` may name,
#: with the reason this crate needs it. A path that is not here is a refusal, so
#: no spelling of an alias, group, glob or split can smuggle one in; adding a
#: legitimate path means adding its row and its reason, which is the point.
STD_ALLOWLIST = {
    "std::cell::OnceCell": "the per-document cell the session caches before it fills it",
    "std::collections::BTreeMap": "the `documents` map, ordered so a replay is deterministic",
    "std::io": "`io::stdin`/`io::stdout`: the frames are this process's only transport",
    "std::io::Read": "reading one frame's body off stdin",
    "std::io::Write": "writing one frame to stdout",
    "std::panic::AssertUnwindSafe": "the parser/formatter call is wrapped (§3.3)",
    "std::panic::catch_unwind": "so a panic in the parser declines a request, never kills the session",
    "std::panic::set_hook": "the hook that keeps a panic payload off stderr",
    "std::process::exit": "`main.rs`'s pinned three-line entry point",
    "std::str::from_utf8": "a frame's header block is bytes; `Content-Length` is read as text",
}

#: Standard-library crate roots whose paths are gated by `STD_ALLOWLIST`.
STD_ROOTS = frozenset({"std", "core", "alloc"})

#: Foreign crates and token-level constructs outside `std` that `source` refuses.
FOREIGN_CRATES = frozenset({"libc", "rustix", "nix", "pyo3", "blastit_sys"})
TRACKED_ROOTS = STD_ROOTS | FOREIGN_CRATES

FOREIGN_RULES: dict[str, tuple[str, str]] = {
    "libc": (
        "libc::",
        "the `libc` crate is not in the closure, and no C ABI may be reached around it",
    ),
    "rustix": (
        "rustix",
        "`rustix` is a capability crate: it links `capset`/`setresuid` on Linux",
    ),
    "nix": (
        "nix::",
        "`nix` is a capability crate: it links `fork`/`execve`/`seccomp`",
    ),
    "pyo3": (
        "pyo3",
        "`pyo3` embeds an interpreter: the portable server has none",
    ),
    "blastit_sys": (
        "blastit_sys",
        "`blastit-sys` is the capability crate; it is not in this closure",
    ),
    "extern_abi": (
        'extern "C"',
        "an extern block or fn is the `unsafe` shortcut to a privilege call",
    ),
    "include": (
        "include!",
        (
            "`include!` compiles a file's text into the binary at build time, which is a "
            "path this scan cannot follow"
        ),
    ),
    "path_attr": (
        "#[path",
        (
            "a `#[path = …]` module compiles a file outside `src/`, which this scan never "
            "reads — the same unreadable-file class as `include!`"
        ),
    ),
    "extern_crate": (
        "extern crate",
        "`extern crate` introduces a second crate binding outside `use`",
    ),
}

#: The attribute every crate root of `blastit-lsp` must carry (design §1.3, row 3).
FORBIDDEN_UNSAFE = "#![forbid(unsafe_code)]"

#: The one line `crates/blastit/src/lib.rs` must keep (design §1.3, row 5).
DELEGATION = "pub use blastit_lsp as lsp;"

#: `src/main.rs`, byte for byte. Pinning the whole file is what makes the one
#: `std::process::exit` a checked fact rather than a string exemption.
MAIN_PINNED = (
    "#![forbid(unsafe_code)]\n"
    "\n"
    "fn main() {\n"
    "    std::process::exit(blastit_lsp::run() as i32)\n"
    "}\n"
)

#: How long `cargo metadata` may take before the graph counts as unreadable.
METADATA_TIMEOUT = 120.0

#: Fixed-point cap for chained crate/module aliases (`extern crate std as s; use s as t;`).
_MAX_ALIAS_PASSES = 16

#: Rust raw-string opener (`r"`, `r#"`, `br#"`, `cr"`, …) and identifier matcher.
_RAW_STRING = re.compile(r'(?:b|c)?r(?P<hashes>#*)"')
_IDENT_RE = re.compile(r"[^\W\d]\w*|_\w*")
_NUM_RE = re.compile(r"[0-9][0-9A-Za-z_]*")
_PUNCT = frozenset("{};,()[]=*&<>!|+-./:#@$?~^%")

#: `(kind, value, line)` where `kind` is `"ident"`, `"punct"`, or `"str"`.
Token = tuple[str, str, int]


@dataclass(frozen=True, slots=True)
class FileScan:
    """Everything the token scan extracts from one `.rs` source file."""

    std_paths: list[tuple[str, int]]
    foreign_hits: list[tuple[str, int]]
    cfg_hits: list[tuple[str, int]]
    unreadable_hits: list[tuple[str, int]]
    has_forbid_unsafe: bool


class Refusals:
    """The lines one run refused on, in the order the promises were checked."""

    def __init__(self) -> None:
        self.lines: list[str] = []

    def __call__(self, property_name: str, where: str, detail: str) -> None:
        self.lines.append(f"{where}: {property_name}: {detail}")


def reason(name: str) -> str:
    """The reason class a forbidden crate belongs to, in one clause."""
    if name in SERVING:
        return "a serving crate (a listener, a stage 0, or a role this binary never is)"
    if name in PRIVILEGE:
        return (
            "a capability crate (capset, setresuid, seccomp, fork, execve, or a C ABI)"
        )
    return "a third runtime dependency nobody reviewed for macOS and Windows"


def _quoted_string(text: str, quote: int, line: int) -> tuple[int, int, bool]:
    """Advance past a `"`-delimited literal opened at `quote`."""
    cursor = quote + 1
    length = len(text)
    while cursor < length:
        char = text[cursor]
        if char == "\\":
            if cursor + 1 < length and text[cursor + 1] == "\n":
                line += 1
            cursor += 2
            continue
        if char == '"':
            return cursor + 1, line, True
        if char == "\n":
            line += 1
        cursor += 1
    return length, line, False


def _char_literal(
    text: str, quote: int, line: int, *, byte_prefix: bool
) -> tuple[int, int, bool] | None:
    """Advance past `'…'` or `b'…'` if it is a char/byte literal, else `None` (lifetime)."""
    length = len(text)
    if quote + 1 >= length:
        return (length, line, False) if byte_prefix else None
    if text[quote + 1] == "\\":
        cursor = quote + 1
        while cursor < length and text[cursor] != "'":
            if text[cursor] == "\\":
                if cursor + 1 < length and text[cursor + 1] == "\n":
                    line += 1
                cursor += 2
            else:
                if text[cursor] == "\n":
                    line += 1
                cursor += 1
        if cursor >= length:
            return length, line, False
        return cursor + 1, line, True
    if quote + 2 < length and text[quote + 2] == "'" and text[quote + 1] != "'":
        if text[quote + 1] == "\n":
            line += 1
        return quote + 3, line, True
    if byte_prefix:
        return min(quote + 2, length), line, False
    return None


def code_tokens(text: str) -> list[Token]:
    """`(kind, value, line)` for the code of `text`.

    Comments and char literals are skipped as trivia; string literals become a
    single `"str"` token so code-like prose inside `"…"` never triggers an
    identifier or path rule while `#[cfg(target_os = "linux")]` retains its
    literal; raw identifiers (`r#ident`) strip `r#` so `r#std::r#env` is
    indistinguishable from `std::env`; unclosed or unrecognised lexical items
    become `"unclosed"` tokens so the scan fails closed.
    """
    tokens: list[Token] = []
    index, line, length = 0, 1, len(text)
    while index < length:
        character = text[index]
        if character == "\n":
            line += 1
            index += 1
            continue
        if character in " \t\r\f\v":
            index += 1
            continue
        if text.startswith("//", index):
            newline = text.find("\n", index)
            if newline < 0:
                break
            index = newline
            continue
        if text.startswith("/*", index):
            start_line = line
            depth, index = 1, index + 2
            while index < length and depth:
                if text.startswith("/*", index):
                    depth, index = depth + 1, index + 2
                elif text.startswith("*/", index):
                    depth, index = depth - 1, index + 2
                else:
                    if text[index] == "\n":
                        line += 1
                    index += 1
            if depth:
                tokens.append(("unclosed", "/*", start_line))
            continue
        raw = _RAW_STRING.match(text, index)
        if raw is not None:
            start_line = line
            closing = '"' + raw.group("hashes")
            found_end = text.find(closing, raw.end())
            end = length if found_end < 0 else found_end + len(closing)
            line += text.count("\n", index, end)
            kind = "unclosed" if found_end < 0 else "str"
            tokens.append((kind, text[index:end], start_line))
            index = end
            continue
        if character == "r" and text.startswith("#", index + 1):
            raw_ident = _IDENT_RE.match(text, index + 2)
            if raw_ident is not None:
                tokens.append(("ident", raw_ident.group(), line))
                index = raw_ident.end()
                continue
        if character == '"' or (character in "bc" and text.startswith('"', index + 1)):
            start_line = line
            quote = index if character == '"' else index + 1
            end, line, closed = _quoted_string(text, quote, line)
            tokens.append(
                ("str" if closed else "unclosed", text[index:end], start_line)
            )
            index = end
            continue
        if character == "'" or (character == "b" and text.startswith("'", index + 1)):
            start_line = line
            quote = index if character == "'" else index + 1
            consumed = _char_literal(text, quote, line, byte_prefix=(character == "b"))
            if consumed is not None:
                index, line, closed = consumed
                if not closed:
                    tokens.append(
                        ("unclosed", text[quote : min(quote + 2, length)], start_line)
                    )
                continue
            if character == "'":
                index += 1
                continue
        if text.startswith("::", index):
            tokens.append(("punct", "::", line))
            index += 2
            continue
        ident = _IDENT_RE.match(text, index)
        if ident is not None:
            tokens.append(("ident", ident.group(), line))
            index = ident.end()
            continue
        number = _NUM_RE.match(text, index)
        if number is not None:
            tokens.append(("num", number.group(), line))
            index = number.end()
            continue
        if character in _PUNCT:
            tokens.append(("punct", character, line))
            index += 1
            continue
        tokens.append(("unclosed", repr(character), line))
        index += 1
    return tokens


def _joined(prefix: str, current: str) -> str:
    """`prefix::current`, with `::self` folded back to the module it repeats."""
    if current == "self":
        current = ""
    elif current.endswith("::self"):
        current = current.removesuffix("::self")
    if not current:
        return prefix
    if not prefix:
        return current
    return f"{prefix}::{current}"


def parse_use_tree(
    tokens: list[Token],
    index: int,
    prefix: str,
    *,
    in_group: bool = False,
    depth: int = 0,
) -> tuple[list[tuple[str, str | None, int]], int, bool]:
    """Flatten one `use` tree from `index` into `(full_path, alias, line)` leaves."""
    if depth > 32:
        return [], index, False
    paths: list[tuple[str, str | None, int]] = []
    current = ""
    alias: str | None = None
    leaf_line = 0
    ok = True
    while index < len(tokens):
        kind, value, line = tokens[index]
        if kind == "punct" and value in ("}", ";"):
            if (value == "}" and not in_group) or (value == ";" and in_group):
                ok = False
            if current:
                paths.append((_joined(prefix, current), alias, leaf_line or line))
            return paths, index + 1, ok
        if kind == "punct" and value == ",":
            if not in_group:
                ok = False
            if current:
                paths.append((_joined(prefix, current), alias, leaf_line or line))
            current, alias, leaf_line = "", None, 0
            index += 1
            continue
        if kind == "punct" and value == "::":
            index += 1
            continue
        if kind == "ident" and value == "as":
            if index + 1 < len(tokens) and tokens[index + 1][0] == "ident":
                alias = tokens[index + 1][1]
                index += 2
            else:
                ok = False
                index += 1
            continue
        if kind == "punct" and value == "{":
            inner, index, inner_ok = parse_use_tree(
                tokens,
                index + 1,
                _joined(prefix, current),
                in_group=True,
                depth=depth + 1,
            )
            ok = ok and inner_ok
            paths.extend(inner)
            current, alias, leaf_line = "", None, 0
            continue
        if kind == "ident" or (kind == "punct" and value == "*"):
            current = f"{current}::{value}" if current else value
            leaf_line = line
            index += 1
            continue
        ok = False
        index += 1
    if current:
        fallback = leaf_line or (tokens[-1][2] if tokens else 1)
        paths.append((_joined(prefix, current), alias, fallback))
    return paths, index, False


def _rewrite_root(path: str, aliases: dict[str, str]) -> str:
    """Replace `path`'s leading segment through `aliases` when bound."""
    head, sep, tail = path.partition("::")
    mapped = aliases.get(head)
    if mapped is None:
        return path
    return f"{mapped}::{tail}" if sep else mapped


def _balanced_group(
    tokens: list[Token], open_index: int
) -> tuple[list[Token], int] | None:
    """Return `(inner_tokens, index_after_close)` for a `(…)`, `[…]`, or `{…}` group."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    if open_index >= len(tokens) or tokens[open_index][0] != "punct":
        return None
    opener = tokens[open_index][1]
    closer = pairs.get(opener)
    if closer is None:
        return None
    depth = 0
    cursor = open_index
    while cursor < len(tokens):
        kind, value, _line = tokens[cursor]
        if kind == "punct" and value == opener:
            depth += 1
        elif kind == "punct" and value == closer:
            depth -= 1
            if depth == 0:
                return tokens[open_index + 1 : cursor], cursor + 1
        cursor += 1
    return None


def _in_spans(index: int, spans: list[tuple[int, int]]) -> bool:
    return any(start <= index < end for start, end in spans)


def scan_file_tokens(text: str) -> FileScan:
    """Scan one file's token stream for `std` paths, foreign rules, `cfg`s and `forbid`."""
    tokens = code_tokens(text)
    foreign: list[tuple[str, int]] = []
    cfg_hits: list[tuple[str, int]] = []
    unreadable: list[tuple[str, int]] = []
    has_forbid_unsafe = False

    for kind, value, line in tokens:
        if kind == "unclosed":
            unreadable.append((f"unclosed or unrecognised token `{value}`", line))

    extern_crates: list[tuple[str, str | None, int]] = []
    use_leaves: list[tuple[str, str | None, int]] = []
    item_spans: list[tuple[int, int]] = []

    # Pass 1: `extern crate`, FFI `extern`, and `use` trees.
    index = 0
    while index < len(tokens):
        kind, value, line = tokens[index]
        if kind == "ident" and value == "extern":
            if index + 1 < len(tokens) and tokens[index + 1][:2] == ("ident", "crate"):
                foreign.append(("extern_crate", line))
                end = index + 2
                while end < len(tokens) and tokens[end][:2] != ("punct", ";"):
                    end += 1
                span_end = min(end + 1, len(tokens))
                item_spans.append((index, span_end))
                if index + 2 < span_end and tokens[index + 2][0] == "ident":
                    target = tokens[index + 2][1]
                    alias: str | None = None
                    if (
                        index + 4 < span_end
                        and tokens[index + 3][:2] == ("ident", "as")
                        and tokens[index + 4][0] == "ident"
                    ):
                        alias = tokens[index + 4][1]
                    extern_crates.append((target, alias, line))
                index = span_end
                continue
            foreign.append(("extern_abi", line))
            index += 1
            continue
        if (
            kind == "ident"
            and value == "use"
            and not (
                index + 1 < len(tokens) and tokens[index + 1][:2] == ("punct", "<")
            )
        ):
            leaves, next_index, use_ok = parse_use_tree(tokens, index + 1, "")
            if not use_ok:
                unreadable.append(("unclassifiable `use` item", line))
            item_spans.append((index, next_index))
            use_leaves.extend(leaves)
            index = next_index
            continue
        index += 1

    # Pass 2: fixed-point alias resolution across `extern crate` and `use`.
    aliases: dict[str, str] = {}
    for _ in range(_MAX_ALIAS_PASSES):
        changed = False
        for target, alias, _line in extern_crates:
            resolved = _rewrite_root(target, aliases)
            bound = alias if alias is not None else target
            if (
                bound != "_"
                and bound != resolved
                and bound not in aliases
                and resolved not in STD_ALLOWLIST
                and resolved.partition("::")[0] in TRACKED_ROOTS
            ):
                aliases[bound] = resolved
                changed = True
        for raw_path, alias, _line in use_leaves:
            resolved = _rewrite_root(raw_path, aliases)
            if (
                resolved.partition("::")[0] not in TRACKED_ROOTS
                or resolved in STD_ALLOWLIST
            ):
                continue
            if alias is not None:
                bound = alias
            else:
                last = raw_path.rpartition("::")[2]
                bound = resolved.rpartition("::")[2] if last == "self" else last
            if (
                bound
                and bound not in ("_", "*")
                and bound != resolved
                and bound not in aliases
            ):
                aliases[bound] = resolved
                changed = True
        if not changed:
            break

    # Pass 3: attributes (`#[…]`, `#![…]`) and macro calls (`include!`, `cfg!`).
    index = 0
    brace_depth = 0
    while index < len(tokens):
        kind, value, line = tokens[index]
        if kind == "punct" and value == "{":
            brace_depth += 1
            index += 1
            continue
        if kind == "punct" and value == "}":
            brace_depth = max(0, brace_depth - 1)
            index += 1
            continue
        if kind == "punct" and value == "#":
            is_inner = index + 1 < len(tokens) and tokens[index + 1][:2] == (
                "punct",
                "!",
            )
            bracket = index + 2 if is_inner else index + 1
            if bracket < len(tokens) and tokens[bracket][:2] == ("punct", "["):
                grouped = _balanced_group(tokens, bracket)
                if grouped is None:
                    unreadable.append(("unclosed attribute `#[…]`", line))
                    body, next_index = tokens[bracket + 1 :], len(tokens)
                else:
                    body, next_index = grouped
                if (
                    is_inner
                    and brace_depth == 0
                    and len(body) >= 4
                    and body[0][:2] == ("ident", "forbid")
                ):
                    inner_parens = _balanced_group(body, 1)
                    if inner_parens is not None and any(
                        tok[:2] == ("ident", "unsafe_code") for tok in inner_parens[0]
                    ):
                        has_forbid_unsafe = True
                for pos in range(len(body) - 1):
                    if body[pos][:2] == ("ident", "path") and body[pos + 1][:2] == (
                        "punct",
                        "=",
                    ):
                        foreign.append(("path_attr", line))
                head_idents: list[str] = []
                arg_index: int | None = None
                for pos, tok in enumerate(body):
                    if tok[0] == "ident":
                        head_idents.append(tok[1])
                    elif tok[:2] == ("punct", "::"):
                        continue
                    else:
                        arg_index = pos
                        break
                if head_idents and head_idents[-1] in ("cfg", "cfg_attr"):
                    shape = [(tok[0], tok[1]) for tok in body]
                    is_plain_cfg_test = not is_inner and shape == [
                        ("ident", "cfg"),
                        ("punct", "("),
                        ("ident", "test"),
                        ("punct", ")"),
                    ]
                    if not is_plain_cfg_test:
                        pred_group = (
                            _balanced_group(body, arg_index)
                            if arg_index is not None
                            else None
                        )
                        predicate = (
                            "".join(tok[1] for tok in pred_group[0])
                            if pred_group is not None
                            else "?"
                        )
                        cfg_hits.append((predicate or "?", line))
                index = next_index
                continue
        if (
            kind == "ident"
            and index + 1 < len(tokens)
            and tokens[index + 1][:2] == ("punct", "!")
        ):
            resolved_macro = _rewrite_root(value, aliases)
            if value == "include" or resolved_macro in (
                "std::include",
                "core::include",
            ):
                foreign.append(("include", line))
            elif value == "cfg" or resolved_macro in ("std::cfg", "core::cfg"):
                macro_group = _balanced_group(tokens, index + 2)
                predicate = (
                    "".join(tok[1] for tok in macro_group[0])
                    if macro_group is not None
                    else "?"
                )
                cfg_hits.append((predicate or "?", line))
        index += 1

    # Pass 4: resolved paths from `use`, `extern crate`, and standalone code tokens.
    candidate_paths: list[tuple[str, int]] = []
    for target, _alias, line in extern_crates:
        resolved = _rewrite_root(target, aliases)
        if resolved.partition("::")[0] in FOREIGN_CRATES:
            candidate_paths.append((resolved, line))
    for raw_path, _alias, line in use_leaves:
        candidate_paths.append((_rewrite_root(raw_path, aliases), line))

    path_spans: list[tuple[int, int]] = []
    index = 0
    while index < len(tokens):
        if _in_spans(index, item_spans):
            index += 1
            continue
        kind, value, line = tokens[index]
        if (
            kind == "ident"
            and index + 1 < len(tokens)
            and tokens[index + 1][:2] == ("punct", "::")
        ):
            segments = [value]
            cursor = index + 1
            while cursor < len(tokens) and tokens[cursor][:2] == ("punct", "::"):
                following = tokens[cursor + 1] if cursor + 1 < len(tokens) else None
                if following is None:
                    segments.append("?")
                    cursor += 1
                    break
                if following[0] == "ident" or following[:2] == ("punct", "*"):
                    segments.append(following[1])
                    cursor += 2
                elif following[:2] == ("punct", "<"):
                    break
                else:
                    segments.append("?")
                    cursor += 1
                    break
            candidate_paths.append((_rewrite_root("::".join(segments), aliases), line))
            path_spans.append((index, cursor))
            index = cursor
            continue
        index += 1

    for index, (kind, value, line) in enumerate(tokens):
        if kind != "ident":
            continue
        resolved_tok = _rewrite_root(value, aliases)
        root_name = resolved_tok.partition("::")[0]
        if root_name in ("rustix", "pyo3", "blastit_sys"):
            foreign.append((root_name, line))
        elif (
            root_name in STD_ROOTS
            and not _in_spans(index, item_spans)
            and not _in_spans(index, path_spans)
        ):
            candidate_paths.append((resolved_tok, line))
    for resolved, line in candidate_paths:
        root_name = resolved.partition("::")[0]
        if root_name in FOREIGN_CRATES:
            foreign.append((root_name, line))

    seen_foreign: set[tuple[str, int]] = set()
    deduped_foreign: list[tuple[str, int]] = []
    for item in sorted(foreign, key=lambda pair: (pair[1], pair[0])):
        if item not in seen_foreign:
            seen_foreign.add(item)
            deduped_foreign.append(item)

    std_paths = [
        (resolved, line)
        for resolved, line in candidate_paths
        if resolved.partition("::")[0] in STD_ROOTS
    ]
    return FileScan(
        std_paths=std_paths,
        foreign_hits=deduped_foreign,
        cfg_hits=cfg_hits,
        unreadable_hits=unreadable,
        has_forbid_unsafe=has_forbid_unsafe,
    )


def dep_tables(data: dict) -> list[tuple[str, dict]]:
    """Every dependency table Cargo reads, as `(dotted name, table)`."""
    tables = []
    for name in DEP_TABLES:
        table = data.get(name)
        if isinstance(table, dict):
            tables.append((name, table))
    targets = data.get("target")
    for target, table in (targets or {}).items():
        if not isinstance(table, dict):
            continue
        for name in DEP_TABLES:
            if isinstance(table.get(name), dict):
                tables.append((f"target.'{target}'.{name}", table[name]))
    return tables


def _check_target_path(
    crate_dir: Path,
    src_dir: Path,
    where: str,
    section: str,
    raw_path: object,
    refuse: Refusals,
) -> Path | None:
    """Require an explicit `[lib]`/`[[bin]]` `path` to resolve inside `crates/blastit-lsp/src/`."""
    if raw_path is None:
        return None
    if not isinstance(raw_path, str) or not raw_path:
        refuse(
            "manifest",
            where,
            f"{section} sets an invalid `path` ({raw_path!r}) — every compiled target "
            f"must resolve to a `.rs` file inside `{CRATE / 'src'}`",
        )
        return None
    candidate = Path(raw_path)
    resolved = (crate_dir / candidate).resolve()
    if (
        candidate.is_absolute()
        or resolved.suffix != ".rs"
        or not resolved.is_relative_to(src_dir)
    ):
        refuse(
            "manifest",
            where,
            f"{section} sets `path = {raw_path!r}`, which resolves outside `{CRATE / 'src'}` "
            "— every library and binary target must live inside `src/` so the source scan "
            "reads every compiled root",
        )
        return None
    return resolved


def scan_manifest(root: Path, refuse: Refusals) -> tuple[list[str], set[Path], bool]:
    """The `[dependencies]` ceiling, target-path containment, and no conditional edge."""
    crate_dir = (root / CRATE).resolve()
    src_dir = (crate_dir / "src").resolve()
    path = root / CRATE / "Cargo.toml"
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        refuse(
            "manifest",
            str(CRATE / "Cargo.toml"),
            f"cannot parse the crate manifest ({error}) — a manifest this guard "
            "cannot read is red, never a skip",
        )
        return [], set(), False
    before = len(refuse.lines)
    where = str(CRATE / "Cargo.toml")
    names: list[str] = []
    for table_name, table in dep_tables(data):
        if table_name == "dependencies":
            names = sorted(table)
            for name in sorted(set(names) - set(ALLOWED)):
                refuse(
                    "manifest",
                    where,
                    f"[dependencies] asks for `{name}`, {reason(name)}; the "
                    f"portable server has exactly {set(ALLOWED)} and no more",
                )
        elif table:
            refuse(
                "manifest",
                where,
                f"[{table_name}] is not empty ({sorted(table)}) — a target- or "
                "build-scoped dependency is an edge the portability claim is not "
                "about, so it cannot be added",
            )
    for name, spec in (data.get("dependencies") or {}).items():
        if isinstance(spec, dict) and spec.get("optional"):
            refuse(
                "manifest",
                where,
                f"[dependencies] makes `{name}` optional — a feature that turns a "
                "dependency on and off is a second build of a binary that must "
                "have exactly one",
            )
    raw_src = root / CRATE / "src"
    if raw_src.is_symlink():
        refuse(
            "manifest",
            where,
            f"`{CRATE / 'src'}` is a symlink — the scanned source directory must live inside "
            f"`{CRATE}`",
        )
    target_roots: set[Path] = {src_dir / "lib.rs", src_dir / "main.rs"}
    lib_table = data.get("lib")
    if lib_table is not None:
        if not isinstance(lib_table, dict):
            refuse("manifest", where, "[lib] is not a TOML table")
        else:
            resolved_lib = _check_target_path(
                crate_dir, src_dir, where, "[lib]", lib_table.get("path"), refuse
            )
            if resolved_lib is not None:
                target_roots.add(resolved_lib)
    bin_tables = data.get("bin")
    if bin_tables is not None:
        if not isinstance(bin_tables, list) or not all(
            isinstance(entry, dict) for entry in bin_tables
        ):
            refuse("manifest", where, "[[bin]] is not an array of TOML tables")
        else:
            for entry in bin_tables:
                label = f"[[bin]] ({entry.get('name', '?')})"
                resolved_bin = _check_target_path(
                    crate_dir, src_dir, where, label, entry.get("path"), refuse
                )
                if resolved_bin is not None:
                    target_roots.add(resolved_bin)
    package = data.get("package") or {}
    if (root / CRATE / "build.rs").is_file() or package.get("build") not in (
        None,
        False,
    ):
        refuse(
            "manifest",
            where,
            "has a build script — `build.rs` is the one place a build can acquire "
            "a platform edge outside every dependency table",
        )
    features = data.get("features") or {}
    if any(features.values()):
        refuse(
            "manifest",
            where,
            f"[features] has entries ({sorted(features)}) — the portable server is "
            "one build with no conditional dependency",
        )
    return names, target_roots, before == len(refuse.lines)


def scan_sources(
    root: Path, target_roots: set[Path], refuse: Refusals
) -> tuple[int, set[str]]:
    """The `std` allowlist, foreign token rules, `cfg`s and `#![forbid(unsafe_code)]`."""
    # A row for `std::env` would switch the `environment` promise off from inside
    # the allowlist, so the allowlist itself is checked before reading any file.
    for allowed_path in STD_ALLOWLIST:
        if allowed_path == "std::env" or allowed_path.startswith("std::env::"):
            refuse(
                "environment",
                "scripts/check_lsp_portable.py",
                f"the allowlist holds `{allowed_path}` — no `std::env` path may be "
                "allowed, because the server reads the client's messages and never "
                "the environment",
            )
    tree = root / CRATE / "src"
    resolved_tree = tree.resolve()
    files: list[Path] = []
    symlink_dirs: list[Path] = []
    if tree.is_dir():
        for dirpath, dirnames, filenames in os.walk(tree, followlinks=False):
            parent = Path(dirpath)
            dirnames.sort()
            kept_dirs: list[str] = []
            for dirname in dirnames:
                subdir = parent / dirname
                if subdir.is_symlink() or not subdir.resolve().is_relative_to(
                    resolved_tree
                ):
                    symlink_dirs.append(subdir)
                else:
                    kept_dirs.append(dirname)
            dirnames[:] = kept_dirs
            for filename in sorted(filenames):
                if filename.endswith(".rs"):
                    files.append(parent / filename)
    files.sort()
    for subdir in sorted(symlink_dirs):
        where = subdir.relative_to(root)
        refuse(
            "source",
            str(where),
            f"resolves outside `{CRATE / 'src'}` — symlinked files and directories out of "
            "`src/` are refused",
        )
    if not files:
        refuse(
            "source",
            str(CRATE / "src"),
            "holds no Rust source — a moved or emptied tree must not turn the rule off",
        )
        return 0, set()
    used: set[str] = set()
    forbid_by_path: dict[Path, bool] = {}
    for path in files:
        where = path.relative_to(root)
        resolved_file = path.resolve()
        if path.is_symlink() or not resolved_file.is_relative_to(resolved_tree):
            refuse(
                "source",
                str(where),
                f"resolves outside `{CRATE / 'src'}` — symlinked files and directories out of "
                "`src/` are refused",
            )
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            refuse(
                "source",
                str(where),
                f"cannot be read ({error}) — an unreadable source file is red, never a skip",
            )
            continue
        if path.name == "main.rs" and text != MAIN_PINNED:
            refuse(
                "source",
                str(where),
                "is not `src/main.rs`'s pinned three-line shape "
                "(#![forbid(unsafe_code)], one fn main, one "
                "std::process::exit) — the artifact's entry point is a checked "
                "fact, not a convention",
            )
        scanned = scan_file_tokens(text)
        forbid_by_path[resolved_file] = scanned.has_forbid_unsafe
        for detail, number in scanned.unreadable_hits:
            refuse(
                "source",
                f"{where}:{number}",
                f"cannot classify {detail} — a construct this scan cannot resolve is "
                "red, never a skip",
            )
        for rule_key, number in scanned.foreign_hits:
            label, why = FOREIGN_RULES[rule_key]
            refuse("source", f"{where}:{number}", f"contains `{label}` — {why}")
        for predicate, number in scanned.cfg_hits:
            refuse(
                "source",
                f"{where}:{number}",
                f"carries `cfg` predicate `{predicate}` — the only "
                "conditional compilation this crate may have is a "
                "`#[cfg(test)]` test module",
            )
        for found, number in scanned.std_paths:
            if found in STD_ALLOWLIST:
                used.add(found)
                continue
            property_name = (
                "environment"
                if found == "std::env" or found.startswith("std::env::")
                else "source"
            )
            glob = (
                " (a glob puts every `std` module in scope at once, so no reading "
                "of the rest of the file can bound it)"
                if found.endswith("::*")
                else ""
            )
            refuse(
                property_name,
                f"{where}:{number}",
                f"names `{found}`, which is not one of the {len(STD_ALLOWLIST)} "
                f"pinned `std` paths{glob} — use an allowlisted path, or add a "
                "row to `STD_ALLOWLIST` in `scripts/check_lsp_portable.py` with "
                "the reason this crate needs it",
            )
    roots_to_check = sorted(
        target_roots or {resolved_tree / "lib.rs", resolved_tree / "main.rs"}
    )
    for required in roots_to_check:
        if not forbid_by_path.get(required, False):
            rel = (
                CRATE / "src" / required.relative_to(resolved_tree)
                if required.is_relative_to(resolved_tree)
                else required
            )
            refuse(
                "source",
                str(rel),
                f"lacks `{FORBIDDEN_UNSAFE}` — the attribute is what refuses the "
                '`extern "C"` shortcut to a privilege call',
            )
    return len(files), used


def scan_identity(root: Path, refuse: Refusals) -> None:
    """One home for the server, and one delegation edge to it."""
    stray = root / "crates/blastit/src/lsp"
    if stray.exists():
        refuse(
            "identity",
            "crates/blastit/src/lsp",
            "exists — a second copy of the server passes every transcript test "
            "with two identical implementations; the crate is its only home",
        )
    delegation = root / "crates/blastit/src/lib.rs"
    try:
        text = delegation.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        text = ""
    if DELEGATION not in text:
        refuse(
            "identity",
            "crates/blastit/src/lib.rs",
            f"does not contain `{DELEGATION}` — `blastit lsp` must delegate to "
            "the crate rather than host a copy",
        )
    edge = root / "crates/blastit/Cargo.toml"
    try:
        data = tomllib.loads(edge.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        data = None
    edges = [
        table_name
        for table_name, table in dep_tables(data or {})
        if "blastit-lsp" in table
    ]
    if edges != ["dependencies"]:
        refuse(
            "identity",
            "crates/blastit/Cargo.toml",
            f"names `blastit-lsp` in {edges or 'no dependency table'}, not exactly "
            "once in `[dependencies]` — the binary has one delegation edge to the "
            "portable crate",
        )


def prerequisites(lines: list[str], target: str) -> list[str] | None:
    """A target's prerequisite tokens, with `\\`-continuations joined."""
    for index, line in enumerate(lines):
        if not line.startswith(target + ":"):
            continue
        parts = [line]
        cursor = index
        while parts[-1].rstrip().endswith("\\") and cursor + 1 < len(lines):
            cursor += 1
            parts.append(lines[cursor])
        joined = " ".join(part.rstrip().rstrip("\\") for part in parts)
        return joined.partition(":")[2].partition("##")[0].split()
    return None


def scan_wiring(root: Path, refuse: Refusals) -> None:
    """`make check` runs `lsp-portable`, and that target runs the guard and its twins."""
    path = root / "Makefile"
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        refuse("wiring", "Makefile", f"cannot be read ({error})")
        return
    lines = text.splitlines()
    check_prereqs = prerequisites(lines, "check")
    target: list[str] | None = None
    for index, line in enumerate(lines):
        if line.startswith("lsp-portable:"):
            recipe = []
            for following in lines[index + 1 :]:
                if following.startswith("\t"):
                    recipe.append(following)
                elif following.strip():
                    break
            target = recipe
            break
    if check_prereqs is None:
        refuse("wiring", "Makefile", "has no `check:` target")
    elif "lsp-portable" not in check_prereqs:
        refuse(
            "wiring",
            "Makefile",
            "the `check` target does not list `lsp-portable` — a guard `make check` "
            "does not run is a comment with extra steps",
        )
    if target is None:
        refuse("wiring", "Makefile", "has no `lsp-portable:` target")
    else:
        recipe = "\n".join(target)
        for script in (
            "scripts/check_lsp_portable.py",
            "scripts/test_check_lsp_portable.py",
        ):
            if script not in recipe:
                refuse(
                    "wiring",
                    "Makefile",
                    f"the `lsp-portable` target does not run `{script}` — the "
                    "guard and its negative twins are gated together",
                )
    phony = prerequisites(lines, ".PHONY") or []
    if "lsp-portable" not in phony:
        refuse("wiring", "Makefile", "does not list `lsp-portable` in `.PHONY`")


def closure(root: Path, refuse: Refusals) -> int:
    """The crate's normal dependency closure, via `cargo metadata --offline`."""
    manifest = root / CRATE / "Cargo.toml"
    cargo = shutil.which("cargo")
    if cargo is None:
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            "cannot be checked: no `cargo` on PATH — a graph this guard cannot "
            "read is red, never a skip",
        )
        return 0
    command = [
        cargo,
        "metadata",
        "--offline",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        str(manifest),
    ]
    try:
        done = subprocess.run(
            command,
            cwd=root,
            capture_output=True,
            text=True,
            timeout=METADATA_TIMEOUT,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as error:
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            f"`cargo metadata --offline --locked` could not run ({error})",
        )
        return 0
    if done.returncode != 0:
        tail = " / ".join((done.stderr or "").strip().splitlines()[-3:])
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            f"`cargo metadata --offline --locked` exited {done.returncode}: {tail} "
            "— a stale or unreadable graph is red, never a skip",
        )
        return 0
    try:
        graph = json.loads(done.stdout)
    except json.JSONDecodeError as error:
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            f"`cargo metadata` did not print JSON ({error})",
        )
        return 0
    names = {
        package["id"]: package.get("name") for package in graph.get("packages") or []
    }
    roots = [identifier for identifier, name in names.items() if name == "blastit-lsp"]
    if len(roots) != 1:
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            f"the workspace graph holds {len(roots)} `blastit-lsp` package(s), not "
            "exactly 1 — the closure this guard walks is ambiguous",
        )
        return 0
    resolve = graph.get("resolve") or {}
    nodes = {node["id"]: node for node in resolve.get("nodes") or []}
    if roots[0] not in nodes or not resolve.get("nodes"):
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            "the metadata run resolved no dependency graph (a `--no-deps` read is "
            "not this guard's input)",
        )
        return 0
    parent: dict[str, str | None] = {roots[0]: None}
    queue = deque([roots[0]])
    while queue:
        current = queue.popleft()
        for dependency in nodes[current].get("deps") or []:
            kinds = {kind.get("kind") for kind in dependency.get("dep_kinds") or [{}]}
            if not kinds & {None, "normal"}:
                continue
            target = dependency.get("pkg")
            if target in parent:
                continue
            parent[target] = current
            queue.append(target)
    in_closure = {names.get(identifier) for identifier in parent} - {None}
    for name in sorted(in_closure & set(SERVING + PRIVILEGE)):
        identifier = next(i for i in parent if names.get(i) == name)
        chain = []
        cursor: str | None = identifier
        while cursor is not None:
            chain.append(names.get(cursor) or cursor)
            cursor = parent[cursor]
        refuse(
            "closure",
            str(CRATE / "Cargo.toml"),
            f"`{name}`, {reason(name)}, is in the normal closure: "
            + " -> ".join(reversed(chain)),
        )
    return len(parent)


def check(root: Path) -> int:
    """Every promise, one line each when one breaks. `0` when all six hold."""
    refusals = Refusals()

    names, target_roots, manifest_clean = scan_manifest(root, refusals)
    files, std_paths = scan_sources(root, target_roots, refusals)
    scan_identity(root, refusals)
    scan_wiring(root, refusals)
    packages = 0
    # The closure walk reads the manifest this guard has just refused: with a
    # third dependency in it the graph describes the crate under test, not a
    # portable one, so the manifest refusal is the answer and metadata is not run.
    if manifest_clean:
        packages = closure(root, refusals)
    if refusals.lines:
        for line in refusals.lines:
            print(line, file=sys.stderr)
        print(
            f"lsp-portable: FAIL — {len(refusals.lines)} refusal(s); the portable "
            "server is not portable (see design §1.3)",
            file=sys.stderr,
        )
        return 1
    print(
        f"lsp-portable: ok — {CRATE}: [dependencies] = {names} with no build script, "
        f"no target table, no feature and target paths under `src/`; {files} source "
        f"file(s) naming {len(std_paths)} of {len(STD_ALLOWLIST)} pinned `std` paths "
        f"and none of {len(FOREIGN_RULES)} foreign constructs, with no `cfg` but "
        f"`#[cfg(test)]`; {packages} packages in the normal closure, none of "
        f"{len(SERVING + PRIVILEGE)} forbidden; one home; `make check` runs this target"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        default=str(REPO),
        help="the repository root to scan (the self-test hook; default: this checkout)",
    )
    arguments = parser.parse_args(argv)
    root = Path(arguments.root).resolve()
    if not root.is_dir():
        print(f"lsp-portable: FAIL — {root} is not a directory", file=sys.stderr)
        return 1
    return check(root)


if __name__ == "__main__":
    sys.exit(main())
