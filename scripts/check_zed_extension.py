"""The static guard for `editors/**`: the Zed extension and its grammar (MB.22.5).

`make zed-check` runs this, and `make check` runs `zed-check`, because an
extension that is merely *present* installs nothing: Zed shallow-fetches the
grammar `rev`, resolves `path =` inside that commit, compiles the grammar wasm
from `parser.c`, and loads exactly the captures and `.scm` files committed here.
A typo in any of those is an install-time failure, or a silently plain-text
editor, so every property of the accepted design
(`docs/design/blastfile-lsp-zed.md` §8.1–§8.4, §8.3a, §10.1–§10.3, §12.4) is a line
this script checks. `src/lib.rs`'s half is §8.3a's resolution order — settings,
`blastit-lsp`, `blastit lsp`, one Release download — and the properties there are
literal: the five asset names, their extraction types, the one joined relaunch
path, the `PATH`/`SYSTEMROOT` allowlist, and no `[[capabilities]]` block.

Two things it deliberately does **not** do: it never reads the network (the
shallow-fetch reproduction is `MB.22.5`'s acceptance command, not a `make check`
step), and it never *generates* with `tree-sitter` (the generator gate is
§9.4's own command). It is stdlib-only and takes a `--root` so its self-tests
can point it at crafted trees.

It does **parse** the design's twenty-three `Blastfile`s, because §9.2a's
"never stricter than the raw string" is the one property a static read of
`grammar.js` cannot prove: a `sql_string` that rejects a character the plain
`'…'` accepts would paint a red node in a file `blastit` runs. That check needs
the pinned generator binary, which CI does not install, so **`--require-parse`**
is what turns a missing binary (or a generator that cannot load the grammar)
into a failure instead of a `note`; the default keeps the local fallback that a
`make check` without the dev tool needs, and every caller that knows the pin is
installed -- the self-tests, `MB.34.6`'s acceptance run -- passes the flag, so
the loop cannot pass without having run.

"Committed `target/`, `extension.wasm` or `grammars/`" is a **git** fact, not a
filesystem one: Zed writes all three into the extension directory at install
time, and `MB.22.4`'s wasm build leaves `editors/zed/target/` behind. The guard
asks `git ls-files` which paths are tracked; in a tree that is not a git
checkout (the self-tests' temp trees) every existing path counts as committed,
so a doctored `extension.wasm` still goes red.

`LICENSE` is committed as a symlink to the repository root license (§8.1: a
symlink inside the extension directory is accepted, `readlink -f` is
`$PWD/LICENSE`). The guard requires the entry to be a file entry and its target
to resolve to **the repository's** `LICENSE` — a symlink out of the repository
(for example `/etc/passwd`) is the failure the check exists for.

Usage (stdlib only; `make zed-check` runs it):

    scripts/check_zed_extension.py
    scripts/check_zed_extension.py --root /path/to/fixture  # the self-tests' hook

Exit 0 when every property holds; exit 1 with one `FAIL <property>: <why>` line
per property that does not.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

#: `scripts/check_zed_extension.py` -> the repository root.
REPO = Path(__file__).resolve().parent.parent

#: The fifteen capture names of §10.1: the fourteen `One`/`Ayu`/`Gruvbox`
#: intersection names plus `variable.special`, which is used only as the first
#: half of the right-to-left fallback pair `@variable.special @variable`.
CAPTURES = frozenset(
    {
        "keyword",
        "function",
        "constant",
        "type",
        "variable.special",
        "variable",
        "string",
        "string.escape",
        "string.special",
        "string.regex",
        "attribute",
        "property",
        "punctuation.bracket",
        "punctuation.delimiter",
        "comment",
    }
)

#: §8.3a's five release assets in table order: `(os, architecture, asset)`. The
#: variable name is `architecture` and the type `Architecture` in `src/lib.rs`;
#: here both are the WIT spelling, so the tuple is the design's table read
#: literally.
ASSETS = (
    ("Linux", "X8664", "blastit-lsp-x86_64-unknown-linux-gnu.tar.gz"),
    ("Linux", "Aarch64", "blastit-lsp-aarch64-unknown-linux-gnu.tar.gz"),
    ("Mac", "Aarch64", "blastit-lsp-aarch64-apple-darwin.tar.gz"),
    ("Mac", "X8664", "blastit-lsp-x86_64-apple-darwin.tar.gz"),
    ("Windows", "X8664", "blastit-lsp-x86_64-pc-windows-msvc.zip"),
)

#: The resolution order, as the three calls `language_server_command` makes: a
#: download-first extension is the regression `lib: order` exists for (§8.3a).
RESOLUTION_STEPS = ("from_settings(", "from_path(", "from_release(")

#: The names the child may inherit (§8.3a). Anything else in the shell
#: environment — `LD_PRELOAD`, `PGPASSWORD`, `BLASTIT_*` — stays behind.
ENV_ALLOWLIST = 'const ENV_ALLOWLIST: [&str; 2] = ["PATH", "SYSTEMROOT"];'

#: Where the download lands and where the relaunch path starts (§8.3a). One
#: name, and deliberately version-free: `<version>` is knowable only from GitHub,
#: so a versioned directory would put the network in front of a binary that is
#: already on disk.
EXTRACT_DIR = 'const EXTRACT_DIR: &str = "blastit-lsp";'

#: The cache hit, pinned verbatim (the way `ENV_ALLOWLIST` is): a real early
#: return before any network call, not a decorative `metadata` probe beside an
#: unconditional download. `is_file()` rules out a directory of that name.
CACHE_HIT = "if std::fs::metadata(&command).is_ok_and(|entry| entry.is_file()) {"

#: The argument vectors any arm may build: none, and `["lsp"]` for the Linux
#: `blastit` binary. Anything else is an argument the server never agreed to
#: take (`blastit-lsp` reads stdin only).
ALLOWED_ARGV = ("Vec::new()", 'vec!["lsp".to_string()]')

#: What `uv` in the editor path was spelled as (the defect `MB.33` removes).
#: The patterns are regular expressions so that this file does not itself carry
#: the two literals `MB.33`'s acceptance greps the tree for: a guard that trips
#: that grep is the wrong kind of green. The escaped hyphen is a literal hyphen.
UV_PATTERNS = (
    r'which\("uv"\)',
    r'"--frozen"',
    r'"--no\-sync"',
    r'"run"',
)

#: The three build artefacts Zed writes into the extension directory (§8.1).
BUILD_ARTEFACTS = ("extension.wasm", "target", "grammars")

#: The `.gitignore` entries that must match them (§8.1, §12.4).
GITIGNORE_ENTRIES = ("/target", "/extension.wasm", "/grammars")

#: The three directories whose `Blastfile`s must parse with no `ERROR`/`MISSING`
#: (§9.2a's "no false red"): eight grammar fixtures, six `e2e/` projects and nine
#: `python/tests/` projects. `crates/blastit-manifest/tests/fixtures/*.blastfile`
#: is deliberately out: those are the parser's own unit fixtures and a future
#: error-case fixture there is *meant* to be unparseable.
BLASTFILE_DIRS = (
    "editors/tree-sitter-blastfile/test/fixtures",
    "e2e",
    "python/tests",
)

#: The design counts twenty-three (§9.2a); a loop whose glob silently matched
#: nothing would otherwise pass.
BLASTFILE_MIN = 23

#: The rules `sql_string` owns (§9.2a). `sql_string` is the wrapper and the last
#: seven are its interior, so a regenerated parser exposes all eight.
SQL_RULES = (
    "query_decl",
    "sql_string",
    "sql_keyword",
    "sql_parameter",
    "sql_ellipsis",
    "sql_number",
    "sql_word",
    "sql_operator",
    "sql_other",
)

#: The captures `highlights.scm` must carry for those nodes (§10.2). The two
#: variable captures are the same right-to-left pair `$IDENT` uses.
SQL_CAPTURES = (
    r'\(query_decl\s+"query"\s*@keyword\)',
    r"\(sql_string\)\s*@string\.special\b",
    r"\(sql_keyword\)\s*@keyword\b",
    r"\(sql_parameter\)\s*@variable\.special\s+@variable\b",
    r"\(sql_ellipsis\)\s*@variable\.special\s+@variable\b",
)


class Guard:
    """Collects one `ok`/`FAIL` line per checked property."""

    def __init__(self) -> None:
        self.checked = 0
        self.failed = 0

    def ok(self, name: str) -> None:
        """Records a property that holds."""
        self.checked += 1
        print(f"ok   {name}")

    def fail(self, name: str, why: str) -> None:
        """Records a property that does not hold, naming the fix."""
        self.checked += 1
        self.failed += 1
        print(f"FAIL {name}: {why}", file=sys.stderr)

    def note(self, text: str) -> None:
        """Explains a property without counting it; never `ok`/`FAIL`-shaped."""
        print(f"note {text}")

    def expect(self, name: str, got: Any, want: Any) -> None:
        """Records a property whose value must equal `want`."""
        if got == want:
            self.ok(name)
        else:
            self.fail(name, f"expected {want!r}, got {got!r}")

    def require(self, name: str, condition: bool, why: str) -> None:
        """Records a property that must be true, with `why` as the failure."""
        if condition:
            self.ok(name)
        else:
            self.fail(name, why)


def load_toml(guard: Guard, path: Path, name: str) -> dict[str, Any] | None:
    """Parses one TOML file, recording a FAIL and returning `None` on error."""
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        guard.fail(name, f"{path} is missing")
    except (OSError, tomllib.TOMLDecodeError) as error:
        guard.fail(name, f"{path} does not parse: {error}")
    return None


def tracked(root: Path) -> set[str] | None:
    """The paths `git` tracks under `root`, or `None` outside a work tree."""
    try:
        inside = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
            capture_output=True,
            text=True,
            check=False,
        )
        if inside.returncode != 0 or inside.stdout.strip() != "true":
            return None
        listed = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            capture_output=True,
            check=False,
        )
        if listed.returncode != 0:
            return None
        return {
            entry.decode("utf-8", "surrogateescape")
            for entry in listed.stdout.split(b"\0")
            if entry
        }
    except (OSError, subprocess.SubprocessError):
        return None


def is_committed(tracked_paths: set[str] | None, root: Path, relative: str) -> bool:
    """Whether `relative` is tracked, or exists in a tree that is not a checkout."""
    if tracked_paths is None:
        return (root / relative).exists()
    return any(
        path == relative or path.startswith(relative + "/") for path in tracked_paths
    )


def check_extension(guard: Guard, root: Path) -> dict[str, Any] | None:
    """§8.2's manifest rules, one property per line."""
    data = load_toml(
        guard, root / "editors" / "zed" / "extension.toml", "extension: parse"
    )
    if data is None:
        return None

    required = (
        "id",
        "name",
        "version",
        "schema_version",
        "authors",
        "description",
        "repository",
    )
    missing = [key for key in required if key not in data]
    guard.require("extension: keys", not missing, f"missing {missing}")

    guard.expect("extension: id", data.get("id"), "blastfile")
    guard.expect("extension: schema_version", data.get("schema_version"), 1)

    version = data.get("version")
    guard.require(
        "extension: version",
        isinstance(version, str)
        and re.fullmatch(r"\d+\.\d+\.\d+", version) is not None,
        f"version {version!r} is not strict semver",
    )

    name = data.get("name")
    description = data.get("description")
    guard.require(
        "extension: description",
        isinstance(name, str)
        and isinstance(description, str)
        and len(description) > len(name),
        "description must be longer (in chars) than name",
    )

    grammars = data.get("grammars")
    guard.require(
        "grammars: key",
        isinstance(grammars, dict) and "blastfile" in grammars,
        f"[grammars.blastfile] is missing (got {sorted(grammars) if isinstance(grammars, dict) else grammars!r})",
    )
    grammar = grammars.get("blastfile") if isinstance(grammars, dict) else None
    if not isinstance(grammar, dict):
        grammar = {}

    guard.expect(
        "grammars: repository",
        grammar.get("repository"),
        "https://github.com/OffworldNexus/blastit",
    )
    rev = grammar.get("rev")
    guard.require(
        "grammars: rev",
        isinstance(rev, str)
        and re.fullmatch(r"[0-9a-f]{40}", rev) is not None
        and rev != "0" * 40,
        f"rev {rev!r} must be one 40-hex commit, never a branch, a tag, HEAD or the placeholder",
    )
    guard.expect("grammars: path", grammar.get("path"), "editors/tree-sitter-blastfile")

    servers = data.get("language_servers")
    server = servers.get("blastit-lsp") if isinstance(servers, dict) else None
    if not isinstance(server, dict):
        server = {}
    guard.expect("language_servers: languages", server.get("languages"), ["Blastfile"])
    guard.expect(
        "language_servers: language_ids",
        server.get("language_ids"),
        {"Blastfile": "blastfile"},
    )
    guard.require(
        "language_servers: no name",
        "name" not in server,
        "LanguageServerManifestEntry has no `name` field; the table key is the server id",
    )
    return data


def check_config(guard: Guard, root: Path) -> None:
    """§8.4's `languages/blastfile/config.toml`, key by key."""
    path = root / "editors" / "zed" / "languages" / "blastfile" / "config.toml"
    data = load_toml(guard, path, "config: parse")
    if data is None:
        return
    guard.expect("config: name", data.get("name"), "Blastfile")
    guard.expect("config: grammar", data.get("grammar"), "blastfile")
    guard.expect(
        "config: path_suffixes", data.get("path_suffixes"), ["Blastfile", "blastfile"]
    )
    guard.expect("config: line_comments", data.get("line_comments"), ["# "])
    guard.expect("config: tab_size", data.get("tab_size"), 4)
    guard.expect("config: hard_tabs", data.get("hard_tabs"), False)


def check_grammar(guard: Guard, root: Path) -> None:
    """§9.1's committed generator output and the scanner-free rule."""
    grammar_dir = root / "editors" / "tree-sitter-blastfile"
    src = grammar_dir / "src"
    generated = (
        "parser.c",
        "grammar.json",
        "node-types.json",
        "tree_sitter/parser.h",
    )
    missing = [name for name in generated if not (src / name).is_file()]
    guard.require("grammar: files", not missing, f"missing {missing}")

    parser = src / "parser.c"
    text = (
        parser.read_text(encoding="utf-8", errors="replace") if parser.is_file() else ""
    )
    guard.require(
        "grammar: export",
        "tree_sitter_blastfile" in text,
        "parser.c does not define tree_sitter_blastfile (Zed links -Wl,--export=tree_sitter_blastfile)",
    )

    grammar_js = grammar_dir / "grammar.js"
    source = grammar_js.read_text(encoding="utf-8") if grammar_js.is_file() else ""
    match = re.search(r"\bname:\s*'([^']+)'", source)
    guard.expect("grammar: name", match.group(1) if match else None, "blastfile")

    scanners = [name for name in ("scanner.c", "scanner.cc") if (src / name).exists()]
    guard.require(
        "grammar: no scanner",
        not scanners,
        f"{scanners} would be C running in the editor's grammar wasm",
    )
    guard.require(
        "grammar: no externals",
        re.search(r"\bexternals\s*:", source) is None,
        "grammar.js declares externals, so a hand-written scanner would be required",
    )

    package = grammar_dir / "package.json"
    try:
        pinned = json.loads(package.read_text(encoding="utf-8"))["devDependencies"][
            "tree-sitter-cli"
        ]
    except (OSError, ValueError, KeyError, TypeError):
        pinned = None
    guard.require(
        "grammar: generator pin",
        isinstance(pinned, str) and re.fullmatch(r"\d+\.\d+\.\d+", pinned) is not None,
        f"package.json's tree-sitter-cli {pinned!r} must be an exact version, not a range (§9.4)",
    )


def check_queries(guard: Guard, root: Path) -> None:
    """§10.1–§10.3: the capture allowlist and each `.scm` file's required captures."""
    queries = root / "editors" / "zed" / "languages" / "blastfile"

    def read(name: str) -> str:
        try:
            return (queries / name).read_text(encoding="utf-8")
        except OSError:
            return ""

    highlights = read("highlights.scm")
    used = set(re.findall(r"@([A-Za-z0-9_.]+)", highlights))
    outside = sorted(used - CAPTURES)
    missing = sorted(CAPTURES - used)
    guard.require(
        "highlights: captures",
        not outside and not missing,
        f"outside the fifteen-name allowlist: {outside}; unused: {missing}",
    )
    guard.require(
        "highlights: variable pair",
        re.search(r"\(env_var\)\s*@variable\.special\s+@variable\b", highlights)
        is not None,
        "`$IDENT` needs the right-to-left fallback pair `@variable.special @variable`",
    )

    absent_captures = [
        pattern for pattern in SQL_CAPTURES if re.search(pattern, highlights) is None
    ]
    guard.require(
        "highlights: sql captures",
        not absent_captures,
        "the `query '…'` interior needs `(sql_string) @string.special`, "
        '`(sql_keyword) @keyword`, `(query_decl "query" @keyword)` and the '
        f"`$?`/`...` pair (§10.2); missing {absent_captures}",
    )

    guard.require(
        "indents: capture",
        "@indent" in read("indents.scm"),
        "indents.scm has no @indent: Enter keeps the previous line's indentation",
    )
    brackets = read("brackets.scm")
    guard.require(
        "brackets: captures",
        "@open" in brackets and "@close" in brackets,
        "brackets.scm needs @open and @close",
    )
    outline = read("outline.scm")
    guard.require(
        "outline: captures",
        "@item" in outline and "@name" in outline,
        "outline.scm needs @item + @name for the section navigator",
    )


def check_sql_grammar(guard: Guard, root: Path) -> None:
    """§9.2a: the `sql_string` rules, `query`'s shape, the node types, the corpus.

    Every clause here is readable from the source tree, because the clauses the
    corpus alone could catch go unchecked on a host without the pinned
    generator: `query` leaving `keyword` and `query_decl`'s *optional* value are
    both text patterns, and `node-types.json` is the generator's own proof that
    `parser.c` was rebuilt from this `grammar.js`.
    """
    grammar_dir = root / "editors" / "tree-sitter-blastfile"
    source = (
        (grammar_dir / "grammar.js").read_text(encoding="utf-8")
        if (grammar_dir / "grammar.js").is_file()
        else ""
    )
    missing = [
        name for name in SQL_RULES if re.search(rf"\b{name}\s*:", source) is None
    ]
    # The measured mechanism of §9.2a decision 1: while `query` is one of the
    # shared directive keywords, the state after it is shared with every other
    # directive, the one-token `raw_string` always wins the longest match, and
    # `sql_string` is unreachable -- the rule exists and never fires.
    keyword_rule = re.search(
        r"\bkeyword\s*:\s*\$ => choice\((.*?)\),\n", source, re.DOTALL
    )
    still_a_keyword = keyword_rule is None or re.search(
        r"""['"]query['"]""", keyword_rule.group(1)
    )
    if still_a_keyword:
        missing.append("keyword: 'query' removed")
    # `query`'s *value is optional*, and that is not a style choice: `query` is a
    # legal name (`functions query`, `select query`), so a required value turns
    # five files `blastit fmt` accepts into ERROR nodes (MB.34.6's reviewer
    # finding 1). The corpus catches that only where the pinned generator runs,
    # so the shape is pinned here as text: a host without the binary would
    # otherwise report this green while the regression came back.
    if re.search(r"query_decl:\s*\$\s*=>\s*seq\('query',\s*optional\(", source) is None:
        missing.append(
            "query_decl: seq('query', optional(choice($.sql_string, $.string)))"
            " (a required value turns `functions query` into an ERROR)"
        )
    # `node-types.json` is generator output: it proves `parser.c` was rebuilt from
    # the amended `grammar.js`, not only that the JavaScript says so.
    try:
        node_types = json.loads(
            (grammar_dir / "src" / "node-types.json").read_text(encoding="utf-8")
        )
        named = {
            entry.get("type")
            for entry in node_types
            if isinstance(entry, dict) and entry.get("named")
        }
        interior = {
            child.get("type")
            for entry in node_types
            if isinstance(entry, dict) and entry.get("type") == "sql_string"
            for child in entry.get("children", {}).get("types", [])
            if isinstance(child, dict)
        }
    except (OSError, ValueError, AttributeError):
        named, interior = set(), set()
    absent = {"sql_string", "sql_keyword", "sql_parameter", "sql_ellipsis"} - named
    if absent or not {"sql_keyword", "sql_parameter"} <= interior:
        missing.append(f"node-types.json {sorted(absent)} (regenerate with the pin)")
    corpus = grammar_dir / "test" / "corpus" / "query-sql.txt"
    corpus_text = corpus.read_text(encoding="utf-8") if corpus.is_file() else ""
    if "query 'SELECT" not in corpus_text or "select " not in corpus_text:
        missing.append("test/corpus/query-sql.txt (an upper- and a lowercase case)")
    guard.require(
        "grammar: sql_string",
        not missing,
        "the `query` SQL grammar is incomplete: "
        + "; ".join(missing)
        + " (§9.2a: `query` must leave `keyword`, or `sql_string` never fires)",
    )


def blastfiles(root: Path) -> list[Path]:
    """Every `Blastfile` under `BLASTFILE_DIRS`, in a stable order."""
    found: list[Path] = []
    for relative in BLASTFILE_DIRS:
        base = root / relative
        if not base.is_dir():
            continue
        found.extend(
            path
            for path in sorted(base.rglob("*"))
            if path.is_file()
            and (path.name == "Blastfile" or path.name.endswith(".Blastfile"))
        )
    return found


def pinned_generator(root: Path) -> str | None:
    """`package.json`'s exact `tree-sitter-cli` version (§9.4), or `None`."""
    try:
        package = json.loads(
            (root / "editors" / "tree-sitter-blastfile" / "package.json").read_text(
                encoding="utf-8"
            )
        )
    except (OSError, ValueError):
        return None
    pinned = package.get("devDependencies", {}).get("tree-sitter-cli")
    return pinned if isinstance(pinned, str) else None


def tree_sitter(root: Path) -> str | None:
    """The pinned generator binary, or `None` when this host has no match.

    `$BLASTIT_TREE_SITTER` first (so an operator can point at their own copy),
    then the design's `/tmp/opencode/bin/tree-sitter`, then `PATH`. A binary that
    prints another version is skipped: a different generator can lex `sql_string`
    differently, and the corpus belongs to the pin.
    """
    pinned = pinned_generator(root)
    if pinned is None:
        return None
    for candidate in (
        os.environ.get("BLASTIT_TREE_SITTER"),
        "/tmp/opencode/bin/tree-sitter",
        shutil.which("tree-sitter"),
    ):
        if not candidate:
            continue
        try:
            version = subprocess.run(
                [candidate, "--version"],
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError:
            continue
        words = version.stdout.split()
        if version.returncode == 0 and words[:1] == ["tree-sitter"] and pinned in words:
            return candidate
    return None


def parse_cache(root: Path) -> str | None:
    """A `tree-sitter` library dir keyed by the grammar's own `parser.c` bytes.

    `tree-sitter`'s default cache is keyed by grammar *name*, and every copy of
    this grammar has the same name: the repository's, and each self-test's
    temporary tree. A `.so` left behind by one copy is then loaded by the next,
    and when the two disagree `parse` dies with `undefined symbol:
    tree_sitter_blastfile` instead of printing a tree. Keying the directory by
    the parser's content gives each distinct parser one library, so a
    regenerated `parser.c` never loads a stale `.so` and a copy reuses the one
    built from the repository. `None` when no cache directory can be named.
    """
    parser = root / "editors" / "tree-sitter-blastfile" / "src" / "parser.c"
    try:
        digest = hashlib.sha256(parser.read_bytes()).hexdigest()[:16]
        base = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    except (OSError, RuntimeError):
        return None
    cache = base / "blastit-zed-check" / digest
    try:
        cache.mkdir(parents=True, exist_ok=True)
    except OSError:
        return None
    return str(cache)


def check_fixtures(guard: Guard, root: Path, require_parse: bool = False) -> None:
    """§9.2a's "no false red": every in-repo `Blastfile` parses with no error.

    The grammar may be looser than `blastit-manifest`, never stricter, so a
    `query '…'` that the manifest accepts must produce no `ERROR`/`MISSING`
    node. `sql_string`'s interior is the union of subsets of `[^']`, which is the
    structural argument; this loop is the regression.

    Without the pinned binary the loop cannot run, and a "skipped" property that
    still prints `ok` is a green light nobody earned (reviewer finding 2). The
    fallback stays for a machine that has no dev tool -- CI is one -- but
    `require_parse` (`--require-parse`) turns both the missing binary and a
    generator that cannot load the grammar into a failure, so any caller that
    knows the pin is installed can insist the loop really ran.
    """
    found = blastfiles(root)
    guard.require(
        "grammar: fixture count",
        len(found) >= BLASTFILE_MIN,
        f"found {len(found)} Blastfile(s) under {list(BLASTFILE_DIRS)}; "
        f"the loop must cover the design's {BLASTFILE_MIN}",
    )
    binary = tree_sitter(root)
    if binary is None:
        missing = (
            "no tree-sitter "
            f"{pinned_generator(root) or '(pinned version)'} in "
            "$BLASTIT_TREE_SITTER, /tmp/opencode/bin/tree-sitter or PATH"
        )
        if require_parse:
            guard.fail(
                "grammar: fixtures parse",
                f"{missing}; install the pin (design §9.4) or drop --require-parse",
            )
            return
        guard.note(f"grammar: fixtures parse: skipped, {missing}")
        guard.ok("grammar: fixtures parse")
        return
    grammar_dir = root / "editors" / "tree-sitter-blastfile"
    environment = dict(os.environ)
    cache = parse_cache(root)
    if cache is not None:
        environment["TREE_SITTER_LIBDIR"] = cache
    broken: list[str] = []
    for path in found:
        relative = path.relative_to(root).as_posix()
        try:
            parsed = subprocess.run(
                [binary, "parse", str(path)],
                cwd=str(grammar_dir),
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            guard.fail("grammar: fixtures parse", f"{binary} did not run: {error}")
            return
        if "(source_file" not in parsed.stdout:
            # A loaded grammar always prints a tree, ERROR nodes included, so no
            # tree means the generator itself did not run here (no `cc`, an
            # unreadable `src/`) -- an environment fact, not a verdict on this
            # file. Name it and hold, exactly as for a missing binary; under
            # `--require-parse` it is a failure, because the loop did not run.
            tail = (parsed.stderr or parsed.stdout).strip().splitlines()[-1:]
            if require_parse:
                guard.fail(
                    "grammar: fixtures parse",
                    f"`{binary} parse {relative}` produced no tree ({tail}); "
                    "--require-parse means every file in the loop must parse",
                )
                return
            guard.note(
                "grammar: fixtures parse: skipped, "
                f"`{binary} parse {relative}` produced no tree ({tail})"
            )
            guard.ok("grammar: fixtures parse")
            return
        tree = parsed.stdout + parsed.stderr
        if "ERROR" in tree or "MISSING" in tree:
            broken.append(relative)
    guard.require(
        "grammar: fixtures parse",
        not broken,
        f"{len(broken)} of {len(found)} Blastfiles parse with ERROR/MISSING "
        f"({broken[:5]}); the grammar became stricter than the language",
    )


def capture_patterns(text: str) -> list[str]:
    """One `.scm`'s capture patterns, with comments and layout normalised.

    A `;` starts a comment in every pattern either file carries, so cutting at
    the first one drops the trailing notes and leaves the S-expression that
    decides a colour.
    """
    patterns: list[str] = []
    for line in text.splitlines():
        code = line.split(";", 1)[0].strip()
        if code and "@" in code:
            patterns.append(" ".join(code.split()))
    return patterns


def check_highlight_copies(guard: Guard, root: Path) -> None:
    """The grammar package's `highlights.scm` must not drift from Zed's copy.

    Two files describe one grammar: Zed installs
    `editors/zed/languages/blastfile/highlights.scm`, and
    `editors/tree-sitter-blastfile/highlights.scm` travels with the package so a
    consumer of `src/node-types.json` sees the same colours. The grammar copy is
    the smaller of the two by design, so the rule is one-directional: every
    pattern it carries must also appear in Zed's copy. A capture it invents (a
    renamed rule, a newer node) is drift, and drift is exactly what a second copy
    produces -- `MB.34.6` found this file still promising that `'…'` colours a
    `query`'s value after the grammar had split `sql_string` out of `raw_string`.
    """
    shipped_path = root / "editors" / "tree-sitter-blastfile" / "highlights.scm"
    zed_path = root / "editors" / "zed" / "languages" / "blastfile" / "highlights.scm"
    try:
        shipped = capture_patterns(shipped_path.read_text(encoding="utf-8"))
        zed = set(capture_patterns(zed_path.read_text(encoding="utf-8")))
    except OSError as error:
        guard.fail("highlights: copies agree", f"one copy is unreadable: {error}")
        return
    extra = [pattern for pattern in shipped if pattern not in zed]
    guard.require(
        "highlights: copies agree",
        bool(shipped) and not extra,
        f"{shipped_path.relative_to(root).as_posix()} carries {extra}, which Zed's "
        "copy does not; one grammar, two files, and they must not drift (§10.1)",
    )


def check_cargo(guard: Guard, root: Path) -> None:
    """§8.1's `Cargo.toml`: its own empty workspace and a cdylib."""
    data = load_toml(guard, root / "editors" / "zed" / "Cargo.toml", "cargo: parse")
    if data is None:
        return
    guard.expect("cargo: workspace", data.get("workspace"), {})
    lib = data.get("lib")
    guard.expect(
        "cargo: crate-type",
        lib.get("crate-type") if isinstance(lib, dict) else None,
        ["cdylib"],
    )


def check_license(guard: Guard, root: Path) -> None:
    """§8.1: `LICENSE` is a file entry inside the extension dir, resolving in-tree."""
    entry = root / "editors" / "zed" / "LICENSE"
    try:
        resolved = entry.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        guard.fail("license: inside", f"{entry} does not resolve: {error}")
        return
    root_resolved = root.resolve()
    inside = resolved.is_file() and resolved.is_relative_to(root_resolved)
    is_root_license = resolved == root_resolved / "LICENSE"
    guard.require(
        "license: inside",
        inside and is_root_license,
        f"{entry} resolves to {resolved}; it must be inside the extension directory's "
        f"repository and be that repository's LICENSE (§8.1 accepts the symlink)",
    )


def check_tree(guard: Guard, root: Path, tracked_paths: set[str] | None) -> None:
    """§8.1: no committed build artefact, and each is gitignored."""
    ext = root / "editors" / "zed"
    committed = [
        name
        for name in BUILD_ARTEFACTS
        if is_committed(tracked_paths, root, f"editors/zed/{name}")
    ]
    guard.require(
        "tree: no build artifacts",
        not committed,
        f"{committed} is committed; Zed writes it at install time",
    )
    try:
        lines = {
            line.strip()
            for line in (ext / ".gitignore").read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.strip().startswith("#")
        }
    except OSError:
        lines = set()
    absent = [entry for entry in GITIGNORE_ENTRIES if entry not in lines]
    guard.require(
        "tree: gitignore",
        not absent,
        f"{ext / '.gitignore'} is missing {absent}",
    )


def function_body(source: str, header: str) -> str:
    """The braced body of the first `header` in `source`, or `""`.

    A positional rule ("the settings read is above `which`") only means
    something inside the one function that orders the steps, so the guard reads
    that body and not the file: a helper defined above may legitimately call
    `which` before the settings helper is defined.
    """
    start = source.find(header)
    if start == -1:
        return ""
    opening = source.find("{", start)
    if opening == -1:
        return ""
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[opening + 1 : index]
    return ""


def asset_arms(source: str) -> list[tuple[str, str, str, str, str]]:
    """`(os, architecture, name, file type, executable)` per `asset_for` arm.

    The five arms are read out of the match rather than grepped file-wide, so a
    name that moved to another platform pair is a red and not a coincidence.
    """
    body = function_body(source, "fn asset_for(")
    arms = re.findall(
        r"\(zed::Os::(\w+), zed::Architecture::(\w+)\)\s*=>\s*Some\(Asset \{(.*?)\}\),",
        body,
        re.DOTALL,
    )
    parsed = []
    for os_name, architecture, fields in arms:
        name = re.search(r'name:\s*"([^"]+)"', fields)
        file_type = re.search(r"file_type:\s*zed::DownloadedFileType::(\w+)", fields)
        executable = re.search(r'executable:\s*"([^"]+)"', fields)
        parsed.append(
            (
                os_name,
                architecture,
                name.group(1) if name else "",
                file_type.group(1) if file_type else "",
                executable.group(1) if executable else "",
            )
        )
    return parsed


def check_lib(guard: Guard, root: Path) -> None:
    """§8.3a's `src/lib.rs`: the order, the five assets, and the allowlist."""
    lib_rs = root / "editors" / "zed" / "src" / "lib.rs"
    try:
        source = lib_rs.read_text(encoding="utf-8")
    except OSError:
        guard.fail("lib: order", f"{lib_rs} is missing")
        return

    # 1. The order itself: settings, then `$PATH`, then one download.
    steps = function_body(source, "fn language_server_command")
    positions = [steps.find(step) for step in RESOLUTION_STEPS]
    guard.require(
        "lib: order",
        all(position != -1 for position in positions)
        and positions == sorted(positions)
        and all(steps.count(step) == 1 for step in RESOLUTION_STEPS),
        "the three steps must be one call each, in this order: "
        f"{' -> '.join(RESOLUTION_STEPS)} (a download-first extension is red)",
    )

    # 2. The better `$PATH` name first: the portable archive beats `blastit`.
    portable_at = source.find('worktree.which("blastit-lsp")')
    cli_at = source.find('worktree.which("blastit")')
    guard.require(
        "lib: which order",
        portable_at != -1
        and cli_at != -1
        and portable_at < cli_at
        and source.count('worktree.which("blastit-lsp")') == 1
        and source.count('worktree.which("blastit")') == 1,
        'which("blastit-lsp") must be resolved before which("blastit"), once each',
    )

    # 3. `blastit` on `$PATH` is the Linux `blastit lsp`, so it needs the argv;
    #    every other arm must add none — the portable server reads stdin only.
    #    Three arms take none: the `blastit-lsp` on `$PATH`, the cached binary
    #    and the freshly unpacked one.
    argv = [value.strip() for value in re.findall(r"args:\s*([^,\n]+),", source)]
    invented = sorted(set(argv) - set(ALLOWED_ARGV))
    guard.require(
        "lib: arguments",
        not invented
        and argv.count('vec!["lsp".to_string()]') == 1
        and argv.count("Vec::new()") == 3,
        "every `args:` must be `Vec::new()` (three arms: `blastit-lsp` on "
        '`$PATH`, the cached binary, the unpacked one) or exactly ["lsp"] once '
        f'for `which("blastit")`; got {argv}',
    )

    # 4. `uv` left the editor path entirely (§8.3a, owner mandate).
    uv = [pattern for pattern in UV_PATTERNS if re.search(pattern, source)]
    guard.require(
        "lib: no uv",
        not uv,
        f"`uv` is back in the editor path ({uv}); the server is its own binary",
    )

    # 5. One shell environment read, and it is filtered, not assigned.
    guard.expect("lib: shell_env", source.count("shell_env("), 1)
    guard.require(
        "lib: env allowlist",
        ENV_ALLOWLIST in source
        and "ENV_ALLOWLIST.contains" in source
        and "env: worktree.shell_env()" not in source
        and re.search(r"\bbinary\.env\b", source) is None
        and "initialization_options" not in source
        and "workspace_configuration" not in source,
        "the child env must be the literal PATH/SYSTEMROOT allowlist: never "
        "`binary`'s env map, never `Command { env: worktree.shell_env() }`, "
        "and no settings payload (§8.3a)",
    )

    guard.require(
        "lib: no process",
        "std::process" not in source and "Command::new" not in source,
        "the wasm must never execute anything: Zed spawns the returned Command",
    )

    # 6. The platform table: five arms, read out of the match.
    arms = asset_arms(source)
    guard.require(
        "lib: platform",
        [(os_name, architecture) for os_name, architecture, *_ in arms]
        == [(os_name, architecture) for os_name, architecture, _ in ASSETS],
        "asset_for must match current_platform()'s five pairs, in §8.3a's order "
        f"(got {[(arm[0], arm[1]) for arm in arms]})",
    )

    # 7. The asset names, literally, and their extraction type.
    guard.require(
        "lib: asset map",
        [name for _, _, name, _, _ in arms] == [name for _, _, name in ASSETS],
        "the five asset names must be §8.3a's verbatim: "
        f"got {[arm[2] for arm in arms]}",
    )
    guard.require(
        "lib: zip windows",
        len(arms) == 5
        and all(
            (file_type == "Zip") == (os_name == "Windows")
            for os_name, _, _, file_type, _ in arms
        ),
        "`Zip` on `Os::Windows` only and `GzipTar` on the other four: a wrong "
        f"type fails at extraction on the user's machine (got {[arm[3] for arm in arms]})",
    )

    # 8. The executable: one joined shape, `.exe` on Windows only, and
    #    `make_file_executable` with no `cfg!` beside it (a no-op off Unix).
    exes = [executable for _, _, _, _, executable in arms]
    windows_exes = [
        executable
        for os_name, _, _, _, executable in arms
        if os_name == "Windows" and executable.endswith(".exe")
    ]
    path_of = function_body(source, "fn executable_path(")
    guard.require(
        "lib: executable",
        "zed::make_file_executable(&command)" in source
        and "cfg!(" not in source
        and len(windows_exes) == 1
        and sum(1 for executable in exes if executable.endswith(".exe")) == 1
        and '"{EXTRACT_DIR}/{directory}/{}"' in path_of
        and "asset.executable" in path_of,
        "one joined path `<extract dir>/<target>/<executable>`, "
        "`blastit-lsp.exe` only on Windows, and one unconditional "
        "make_file_executable (§8.3a)",
    )

    # 9. Where the download lands: one relative directory name, shared with the
    #    relaunch path, and never the worktree root.
    destination = re.search(
        r"zed::download_file\(\s*&asset_url\.download_url,\s*EXTRACT_DIR,\s*asset\.file_type\s*\)",
        source,
    )
    guard.require(
        "lib: download destination",
        EXTRACT_DIR in source
        and destination is not None
        and "root_path" not in source
        and path_of.count("EXTRACT_DIR") == 1,
        "download_file takes a directory relative to the extension's work "
        "directory — the one `EXTRACT_DIR` constant the relaunch path also "
        "starts with — never an absolute path (§8.3a)",
    )

    # 10. The cache answers before any network call, and it is a real early
    #     return: `metadata(..).is_ok_and(|entry| entry.is_file())`.
    step = function_body(source, "fn from_release(")
    cache_at = step.find(CACHE_HIT)
    release_at = step.find("zed::latest_github_release(")
    download_at = step.find("zed::download_file(")
    guard.require(
        "lib: cache",
        CACHE_HIT in source
        and cache_at != -1
        and release_at != -1
        and cache_at < release_at
        and "return Ok(" in step[cache_at:release_at]
        and download_at > release_at,
        "the unpacked `blastit-lsp/<target>/blastit-lsp` must be a hit BEFORE "
        "`latest_github_release`: an offline or rate-limited machine with the "
        "binary on disk gets a server (§8.3a)",
    )

    # 11. The two status words: `Downloading` before the fetch, `Failed` on every
    #     error -- the failure helper reports both, in that order.
    guard.require(
        "lib: status",
        source.count("LanguageServerInstallationStatus::Downloading") == 2
        and step.find("LanguageServerInstallationStatus::Downloading") != -1
        and step.find("LanguageServerInstallationStatus::Downloading") < release_at
        and source.find("LanguageServerInstallationStatus::Downloading")
        < source.find("LanguageServerInstallationStatus::Failed("),
        "report Downloading before the fetch, then Failed(<one sentence>) — "
        "a silent failure is an editor that never starts the server",
    )

    # 12. Every failure names the five targets and the way past it: one
    #     definition, one use per failure, and no failure without it.
    code = source.split("#[cfg(test)]")[0]
    hint = re.search(r'const FAILURE_HINT: &str = "(.*?)";', source, re.DOTALL)
    hint_text = " ".join(hint.group(1).replace("\\\n", "").split()) if hint else ""
    failures = [match.start() for match in re.finditer(r"return Err\(failed\(", source)]
    guard.require(
        "lib: failure",
        len(failures) == 5
        and code.count("FAILURE_HINT") == len(failures) + 1
        and "lsp.blastit-lsp.binary.path" in hint_text
        and "Linux x86_64/aarch64" in hint_text
        and "macOS x86_64/aarch64" in hint_text
        and "Windows x86_64" in hint_text,
        "each of the five failures (unsupported pair, no release, no asset, "
        "download, executable) must carry the supported targets and "
        "`lsp.blastit-lsp.binary.path` (design §8.3a)",
    )

    # 13. The one release, and no CDN host named anywhere.
    guard.require(
        "lib: asset url",
        source.count('"OffworldNexus/blastit"') == 1
        and "githubusercontent" not in source
        and "githubusercontent"
        not in (root / "editors" / "zed" / "extension.toml").read_text(
            encoding="utf-8", errors="replace"
        ),
        "the release URL is `OffworldNexus/blastit` once; the browser URL 302s to "
        "a CDN host and the download gate is checked before the redirect",
    )

    capabilities = []
    for path in sorted(ext_rs_files(root / "editors" / "zed")):
        if re.search(
            r"^\s*\[\[?capabilities\]\]?",
            path.read_text(encoding="utf-8", errors="replace"),
            re.MULTILINE,
        ):
            capabilities.append(path.relative_to(root).as_posix())
    guard.require(
        "lib: no capabilities",
        not capabilities,
        f"a [capabilities] block is inert for download_file and names a host that "
        f"fails closed: {capabilities}",
    )


def check_capabilities(guard: Guard, root: Path, data: dict[str, Any] | None) -> None:
    """The parsed manifest declares no capability at all (§8.3a).

    Structural, not textual: `[[capabilities]]` parses to a top-level
    `capabilities` array, and the host never consults it for `download_file`
    (`crates/extension/src/capability_granter.rs` gates `download_file` on the
    user's setting, checked before the redirect). A declared host therefore
    fails closed, so the empty manifest is the property, not a wider one.
    """
    if data is None:
        guard.fail("extension: capabilities", "the manifest does not parse")
        return
    declared = data.get("capabilities")
    guard.require(
        "extension: capabilities",
        declared is None,
        f"[[capabilities]] {declared!r} must not be declared: the download gate "
        'is the user\'s setting (`host = "*"` by default), never this manifest',
    )


def ext_rs_files(root: Path):
    """The source and manifest files of the extension, skipping build output."""
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        parts = path.relative_to(root).parts
        if any(part in {"target", "grammars"} for part in parts):
            continue
        if path.suffix in {".rs", ".toml"}:
            yield path


def settings(document: Any) -> Any:
    """`format_on_save` from a parsed `.zed/settings.json`, tolerating absence."""
    if not isinstance(document, dict):
        return None
    languages = document.get("languages")
    if not isinstance(languages, dict):
        return None
    blastfile = languages.get("Blastfile")
    if not isinstance(blastfile, dict):
        return None
    return blastfile.get("format_on_save")


def check_settings(guard: Guard, root: Path) -> None:
    """F6: the committed `.zed/settings.json` and the fence in `docs/local.md`."""
    committed = root / ".zed" / "settings.json"
    try:
        document = json.loads(committed.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        guard.fail("settings: format_on_save", f"{committed} does not parse: {error}")
    else:
        guard.expect("settings: format_on_save", settings(document), "on")

    page = root / "docs" / "local.md"
    try:
        text = page.read_text(encoding="utf-8")
    except OSError as error:
        guard.fail("docs: settings fence", f"{page} is missing: {error}")
        return
    fences = re.findall(r"```json\n(.*?)```", text, re.DOTALL)
    parsed = []
    broken = []
    for fence in fences:
        try:
            parsed.append(json.loads(fence))
        except ValueError as error:
            broken.append(str(error))
    guard.require(
        "docs: settings fence",
        not broken and any(settings(fence) == "on" for fence in parsed),
        f"{page}'s ```json fence must parse and turn format_on_save on"
        + (f" (broken: {broken})" if broken else ""),
    )


def check(root: Path, require_parse: bool = False) -> int:
    """Runs every property against `root`; returns the process exit status."""
    guard = Guard()
    manifest = check_extension(guard, root)
    check_capabilities(guard, root, manifest)
    check_config(guard, root)
    check_grammar(guard, root)
    check_sql_grammar(guard, root)
    check_fixtures(guard, root, require_parse)
    check_queries(guard, root)
    check_highlight_copies(guard, root)
    check_cargo(guard, root)
    check_license(guard, root)
    check_tree(guard, root, tracked(root))
    check_lib(guard, root)
    check_settings(guard, root)

    if guard.failed:
        print(
            f"zed-check: {guard.failed} of {guard.checked} properties failed; "
            "the extension would not install (design §8, §10, §12.4)",
            file=sys.stderr,
        )
        return 1
    print(f"zed-check: OK ({guard.checked} properties checked)")
    return 0


def main(argv: list[str] | None = None) -> int:
    """Parses `--root`/`--require-parse` and runs the guard."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        default=str(REPO),
        help="the tree to check (default: this repository)",
    )
    parser.add_argument(
        "--require-parse",
        action="store_true",
        help="fail instead of skipping when the pinned tree-sitter is missing, "
        "so §9.2a's 23-file loop cannot pass without running (design §9.4)",
    )
    args = parser.parse_args(argv)
    return check(Path(args.root).resolve(), require_parse=args.require_parse)


if __name__ == "__main__":
    raise SystemExit(main())
