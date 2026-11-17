"""The static guard for `editors/**`: the Zed extension and its grammar (MB.22.5).

`make zed-check` runs this, and `make check` runs `zed-check`, because an
extension that is merely *present* installs nothing: Zed shallow-fetches the
grammar `rev`, resolves `path =` inside that commit, compiles the grammar wasm
from `parser.c`, and loads exactly the captures and `.scm` files committed here.
A typo in any of those is an install-time failure, or a silently plain-text
editor, so every property of the accepted design
(`docs/design/blastfile-lsp-zed.md` §8.1–§8.4, §10.1–§10.3, §12.4) is a line
this script checks.

Two things it deliberately does **not** do: it never reads the network (the
shallow-fetch reproduction is `MB.22.5`'s acceptance command, not a `make check`
step), and it never runs `tree-sitter` (the generator gate is §9.4's own
command). It is stdlib-only and takes a `--root` so its self-tests can point it
at crafted trees.

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
import json
import re
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

#: `src/lib.rs`'s `uv` argv: `uv run --frozen --no-sync --quiet blastit lsp`
#: (§8.3). The whitespace between the items is the source's, so it is matched
#: loosely; the order is exact and `--no-sync` must be one of them.
UV_ARGV = (
    '"run"',
    '"--frozen"',
    '"--no-sync"',
    '"--quiet"',
    '"blastit"',
    '"lsp"',
)

#: The three build artefacts Zed writes into the extension directory (§8.1).
BUILD_ARTEFACTS = ("extension.wasm", "target", "grammars")

#: The `.gitignore` entries that must match them (§8.1, §12.4).
GITIGNORE_ENTRIES = ("/target", "/extension.wasm", "/grammars")


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


def check_extension(guard: Guard, root: Path) -> None:
    """§8.2's manifest rules, one property per line."""
    data = load_toml(
        guard, root / "editors" / "zed" / "extension.toml", "extension: parse"
    )
    if data is None:
        return

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


def check_lib(guard: Guard, root: Path) -> None:
    """§8.3's `src/lib.rs`: the resolution order, the argv, and no process exec."""
    lib_rs = root / "editors" / "zed" / "src" / "lib.rs"
    try:
        source = lib_rs.read_text(encoding="utf-8")
    except OSError:
        guard.fail("lib: which order", f"{lib_rs} is missing")
        return

    blastit_at = source.find('which("blastit")')
    uv_at = source.find('which("uv")')
    guard.require(
        "lib: which order",
        blastit_at != -1 and uv_at != -1 and blastit_at < uv_at,
        'which("blastit") must be resolved before which("uv")',
    )

    argv = re.search(
        r"\[\s*" + r"\s*,\s*".join(re.escape(item) for item in UV_ARGV) + r"\s*,?\s*\]",
        source,
    )
    guard.require(
        "lib: argv",
        argv is not None,
        'the uv fallback argv must be ["run", "--frozen", "--no-sync", '
        '"--quiet", "blastit", "lsp"] in that order (§8.3)',
    )

    guard.expect("lib: shell_env", source.count("shell_env()"), 1)
    guard.require(
        "lib: no process",
        "std::process" not in source and "Command::new" not in source,
        "the wasm must never execute anything: Zed spawns the returned Command",
    )
    guard.require(
        "lib: error string",
        "uv sync" in source and "uv add blastit" in source,
        "the not-installed error must name `uv add blastit` and `uv sync`",
    )

    capabilities = []
    for path in sorted(ext_rs_files(root / "editors" / "zed")):
        if re.search(
            r"^\s*\[capabilities\]",
            path.read_text(encoding="utf-8", errors="replace"),
            re.MULTILINE,
        ):
            capabilities.append(path.relative_to(root).as_posix())
    guard.require(
        "lib: no capabilities",
        not capabilities,
        f"[capabilities] under editors/zed turns a zero-config install into a permission prompt: {capabilities}",
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


def check(root: Path) -> int:
    """Runs every property against `root`; returns the process exit status."""
    guard = Guard()
    check_extension(guard, root)
    check_config(guard, root)
    check_grammar(guard, root)
    check_queries(guard, root)
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
    """Parses `--root` and runs the guard; returns the process exit status."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        default=str(REPO),
        help="the tree to check (default: this repository)",
    )
    args = parser.parse_args(argv)
    return check(Path(args.root).resolve())


if __name__ == "__main__":
    raise SystemExit(main())
