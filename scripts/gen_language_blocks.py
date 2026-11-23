#!/usr/bin/env python3
"""Regenerate the language blocks in `docs/install.md` and `docs/install.sh`.

`packaging/languages.tsv` is the one declaration of the languages Blastit ships
and at which versions; `scripts/languages.py` is its one Python reader. Two of
Blastit's user-facing files cannot run either at their use site — the install
page renders in a browser, and `docs/install.sh` runs on a host that may have no
Python — so their version-bearing lines are generated from the table, committed,
and checked for staleness here.

    scripts/gen_language_blocks.py            # the same as --check
    scripts/gen_language_blocks.py --write    # rewrite both files
    scripts/gen_language_blocks.py --check    # exit 1 + a unified diff when stale

The marked blocks (`<!-- BEGIN generated:languages:<name> -->` … in Markdown,
`# BEGIN generated:languages` … in the installer) are regenerated whole, never
hand-edited: adding a language version is a one-line edit to
`packaging/languages.tsv`. `make docs-check` and `scripts/test_install.sh` run
`--check`, so a stale block fails the same gate that builds the site.

The markers around the install page's two shell snippets sit *outside* the
fenced blocks: an HTML comment inside a fence renders as text, so each snippet
is generated whole (its repository definition included) and the reader never
sees a marker.
"""

from __future__ import annotations

import difflib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

import languages  # noqa: E402

INSTALL_MD = ROOT / "docs" / "install.md"
INSTALL_SH = ROOT / "docs" / "install.sh"

#: How a language is named in prose and in the package table.
NAMES = {"python": "CPython", "php": "PHP"}

#: The table's "What it adds" cell for a shim row.
ADDS = {
    "python": "the WSGI shim for CPython {version}",
    "php": "the PHP shim for NTS `libphp` {version}",
}

#: The one row-less package, first in the table.
CORE_ROW = (
    "| `blastit` | server, `blastit-lsp`, pure-Python CLI, `/usr/bin` links | "
    "measured `libc6 (>= …)`, plus `libgcc-s1` only when a shipped ELF needs it | — |"
)

#: The shell variable a language's version is read into, and its probe line.
VARS = {"python": "py", "php": "php"}
PROBES = {
    "python": """py=$(python3 -c 'import sys; print("%d.%d" % sys.version_info[:2])')""",
    "php": (
        """php=$(php -r 'echo PHP_MAJOR_VERSION, ".", PHP_MINOR_VERSION;' """
        "2>/dev/null || true)"
    ),
}

#: What a refused version reads: never a typed range, always the table. Both
#: probes are guarded, because a host with no `python3`/`php` at all answers
#: empty and the sentence still has to name what was asked for.
REFUSALS = {
    "python": (
        "no WSGI shim for CPython ${py:-no python3}: the table below lists the "
        "versions Blastit ships"
    ),
    "php": (
        "no PHP shim for ${php:-no php}: the table below lists the versions Blastit ships"
    ),
}

# The repository definitions are verbatim parts of the install page: the marker
# that delimits a generated snippet has to sit outside its fence, so the whole
# snippet is generated from here. A change to either file is a change here plus
# `--write`.
APT_SETUP = """\
install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://apt.offworld-nexus.app/keyring.gpg -o /etc/apt/keyrings/offworld-nexus.gpg
cat >/etc/apt/sources.list.d/offworld-nexus.sources <<'EOF'
Types: deb
URIs: https://apt.offworld-nexus.app
Suites: any-distro
Components: main
Signed-By: /etc/apt/keyrings/offworld-nexus.gpg
EOF
apt-get update"""

DNF_SETUP = """\
cat >/etc/yum.repos.d/offworld-nexus.repo <<'EOF'
[offworld-nexus]
name=Offworld Nexus packages
baseurl=https://rpm.offworld-nexus.app/stable/$basearch/
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://rpm.offworld-nexus.app/gpg.key
EOF"""

MD_BEGIN = "<!-- BEGIN generated:languages:{name} -->"
MD_END = "<!-- END generated:languages:{name} -->"
SH_BEGIN = "# BEGIN generated:languages"
SH_END = "# END generated:languages"


class BlockError(Exception):
    """A marked block cannot be found or rendered."""


def ordered_languages(rows: list[languages.Language]) -> list[str]:
    """The languages in the table's own order, once each."""
    seen: list[str] = []
    for row in rows:
        if row.language not in seen:
            seen.append(row.language)
    return seen


def versions_of(rows: list[languages.Language], language: str) -> list[str]:
    """One language's versions, oldest first: the order a sentence reads."""
    return sorted(languages.versions_of(rows, language), key=languages.version_key)


def sentence(items: list[str]) -> str:
    """`["8.3", "8.4", "8.5", "8.6"]` -> `"8.3, 8.4, 8.5 and 8.6"`."""
    if len(items) < 2:
        return "".join(items)
    return ", ".join(items[:-1]) + " and " + items[-1]


def render_apt(rows: list[languages.Language]) -> str:
    return _manager_snippet("apt-get", APT_SETUP, rows)


def render_dnf(rows: list[languages.Language]) -> str:
    return _manager_snippet("dnf", DNF_SETUP, rows)


def _manager_snippet(manager: str, setup: str, rows: list[languages.Language]) -> str:
    lines = [
        "```bash",
        setup,
        f"{manager} install blastit                      "
        "# server: static mounts, fmt, check work now",
    ]
    for language in ordered_languages(rows):
        versions = versions_of(rows, language)
        var = VARS[language]
        lines.append(PROBES[language])
        lines.append(
            f'case "${var}" in {"|".join(versions)}) '
            f'{manager} install "blastit-{language}-${var}" ;; '
            f'*) echo "{REFUSALS[language]}" ;; esac'
        )
    lines.append("```")
    return "\n".join(lines)


def render_table(rows: list[languages.Language]) -> str:
    lines = [
        "| Package | What it adds | `Depends` | `Recommends` (deb / rpm) |",
        "|---|---|---|---|",
        CORE_ROW,
    ]
    for row in rows:
        adds = ADDS.get(row.language)
        if adds is None:
            raise BlockError(
                f"{row.language}: no table template for this language "
                f"(known: {', '.join(sorted(ADDS))})"
            )
        lines.append(
            f"| `{row.package}` | {adds.format(version=row.version)} | "
            f"`blastit (= <version>)`, measured `libc6` | "
            f"`{row.deb_runtime}` / `{row.rpm_runtime}` |"
        )
    return "\n".join(lines)


def render_sentence(rows: list[languages.Language]) -> str:
    listed = ", and ".join(
        f"{NAMES.get(language, language)} {sentence(versions_of(rows, language))}"
        for language in ordered_languages(rows)
    )
    return (
        f"Shims ship for **{listed}**, by design: an older host interpreter is\n"
        "supported-but-unavailable, and core `blastit` still serves static mounts, `check`."
    )


def render_shims(rows: list[languages.Language], indent: int = 4) -> str:
    """The `for minor in …` loops of `detect_shims`, at that function's indent."""
    pad = " " * indent
    lines: list[str] = []
    for language in ordered_languages(rows):
        versions = versions_of(rows, language)
        lines.append(f"{pad}for minor in {' '.join(versions)}; do")
        if language == "python":
            lines.extend(
                [
                    f'{pad}    if runtime_installed "$fmt" "python$minor"; then',
                    f'{pad}        shims="$shims blastit-python-$minor"',
                    f"{pad}    else",
                    f'{pad}        missing_python="$missing_python${{missing_python:+/}}$minor"',
                    f'{pad}        python_lib_hint "$(printf \'%s\' "$minor" | tr -d .)" &&',
                    f'{pad}            hinted_python="$hinted_python${{hinted_python:+/}}$minor"',
                    f"{pad}    fi",
                ]
            )
        else:
            lines.extend(
                [
                    f'{pad}    if php_runtime_installed "$fmt" "$minor"; then',
                    f'{pad}        shims="$shims blastit-{language}-$minor"',
                    f"{pad}    else",
                    f'{pad}        missing_{language}="$missing_{language}${{missing_{language}:+/}}$minor"',
                    f"{pad}    fi",
                ]
            )
        lines.append(f"{pad}done")
    return "\n".join(lines)


def _replace(path: Path, text: str, begin: str, end: str, body: str) -> str:
    lines = text.splitlines()
    start = _marker(lines, begin, path)
    stop = _marker(lines, end, path, after=start)
    trailing = "\n" if text.endswith("\n") else ""
    joined = "\n".join(lines[: start + 1] + body.splitlines() + lines[stop:])
    return joined + trailing


def _marker(lines: list[str], marker: str, path: Path, after: int = -1) -> int:
    """The line index of `marker`, ignoring indentation (a shell block is indented)."""
    for index in range(after + 1, len(lines)):
        if lines[index].strip() == marker:
            return index
    where = f"after line {after + 1}" if after >= 0 else "anywhere"
    raise BlockError(f"{path}: no `{marker}` line {where}")


def render_file(path: Path, rows: list[languages.Language]) -> str:
    """`path` with every one of its marked blocks rewritten from `rows`."""
    text = path.read_text(encoding="utf-8")
    if path == INSTALL_MD:
        rendered = (
            ("apt", render_apt(rows)),
            ("dnf", render_dnf(rows)),
            ("table", render_table(rows)),
            ("sentence", render_sentence(rows)),
        )
        for name, body in rendered:
            text = _replace(
                path,
                text,
                MD_BEGIN.format(name=name),
                MD_END.format(name=name),
                body,
            )
        return text
    if path == INSTALL_SH:
        return _replace(path, text, SH_BEGIN, SH_END, render_shims(rows))
    raise BlockError(f"{path}: no marked blocks are declared for this file")


def expected(rows: list[languages.Language]) -> dict[Path, str]:
    return {path: render_file(path, rows) for path in (INSTALL_MD, INSTALL_SH)}


def _usage() -> str:
    return (
        "usage: scripts/gen_language_blocks.py [--write|--check]\n"
        "\n"
        "  --write   rewrite the marked blocks in docs/install.md and\n"
        "            docs/install.sh from packaging/languages.tsv\n"
        "  --check   exit 1 with a unified diff when a committed block is stale\n"
        "            (the default)\n"
    )


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    mode = "check"
    for arg in args:
        if arg in ("-h", "--help"):
            sys.stdout.write(_usage())
            return 0
        if arg in ("--write", "--check"):
            mode = arg[2:]
            continue
        sys.stderr.write(f"generated-blocks: unknown argument {arg}\n{_usage()}")
        return 2

    try:
        rows = languages.load()
        files = expected(rows)
    except languages.LanguageError as error:
        sys.stderr.write(f"generated-blocks: {error}\n")
        return 1
    except BlockError as error:
        sys.stderr.write(f"generated-blocks: {error}\n")
        return 1

    if mode == "write":
        for path, text in files.items():
            shown = path.relative_to(ROOT)
            if path.read_text(encoding="utf-8") == text:
                sys.stdout.write(f"generated-blocks: {shown} is current\n")
                continue
            path.write_text(text, encoding="utf-8")
            sys.stdout.write(f"generated-blocks: wrote {shown}\n")
        return 0

    stale = False
    for path, text in files.items():
        committed = path.read_text(encoding="utf-8")
        if committed == text:
            continue
        stale = True
        shown = path.relative_to(ROOT)
        sys.stderr.writelines(
            difflib.unified_diff(
                committed.splitlines(keepends=True),
                text.splitlines(keepends=True),
                fromfile=f"a/{shown}",
                tofile=f"b/{shown}",
            )
        )
        sys.stderr.write(
            f"generated-blocks: {shown} is stale: run "
            "`scripts/gen_language_blocks.py --write`\n"
        )
    return 1 if stale else 0


if __name__ == "__main__":
    raise SystemExit(main())
