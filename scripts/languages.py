"""The one Python loader for `packaging/languages.tsv` (`MB.67`, `MB.75.1`).

Stdlib only (run it with ``uv run --no-project scripts/languages.py``) and
importable as a module (later slices do ``import languages``), with no argparse
side effects at import time.

Derivation rules, implemented here once:

* package name  = ``blastit-<language>-<version>``
* shim file     = ``libblastit_<slug>-<arch>-<libc>.so``; the table globs the
  architecture and names the libc exactly as `packaging/package-set.sh` does
* the core package ``blastit`` is the row-less base, spelled ``(none)`` in the
  package-set vocabulary, and always comes first
* ``deb-runtime`` / ``rpm-runtime`` are the ``Recommends:`` values
* order is the file's order; a version is a string compared component by
  component, never a float (``8.10`` sorts after ``8.9``)
* ``channels`` says which release channel(s) a row ships in; the listings
  default to ``stable``, so a stable build never names a nightly-only row. The
  Rust runtime is *not* filtered (the same binary serves every channel), which is
  why only the packaging/docs generators take a channel here.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import NamedTuple

HEADER = ("language", "version", "slug", "deb-runtime", "rpm-runtime", "channels")
NONE = "(none)"
CORE_PACKAGE = "blastit"
CORE_ROW = (CORE_PACKAGE, "core", NONE, NONE, NONE)
DEFAULT_TSV = Path(__file__).resolve().parent.parent / "packaging" / "languages.tsv"
#: The declared channels, in the order a `channels` cell spells them.
CHANNELS = ("stable", "nightly")
#: The only legal `channels` cell values: one channel, or both with `stable` first.
CHANNEL_COMBINATIONS = ("stable", "nightly", "stable nightly")
#: The channel a listing selects unless it is told otherwise.
DEFAULT_CHANNEL = "stable"

_VERSION = re.compile(r"^[0-9]+\.[0-9]+$")
_SLUG = re.compile(r"^[a-z0-9_]+$")
_RESERVED_LANGUAGES = {"core", "blastit", "language"}


class LanguageError(Exception):
    """The TSV cannot be read as the supported-language declaration."""


class ChannelError(Exception):
    """A caller asked for a channel the TSV does not declare."""


class Language(NamedTuple):
    language: str
    version: str
    slug: str
    deb_runtime: str
    rpm_runtime: str
    channels: str

    @property
    def package(self) -> str:
        return package_name(self.language, self.version)

    @property
    def shim(self) -> str:
        return shim_glob(self.slug)

    @property
    def kind(self) -> str:
        return "shim"

    def in_channel(self, channel: str) -> bool:
        """Whether this row ships in `channel` (`"stable"` or `"nightly"`)."""
        return channel in self.channels.split(" ")

    def package_row(self) -> tuple[str, str, str, str, str]:
        return (self.package, self.kind, self.shim, self.deb_runtime, self.rpm_runtime)


def check_channel(channel: str) -> str:
    """`channel`, or `ChannelError` when the TSV does not declare it."""
    if channel not in CHANNELS:
        raise ChannelError(
            f"unknown channel {channel!r}; expected one of {', '.join(CHANNELS)}"
        )
    return channel


def rows_in_channel(
    rows: list[Language], channel: str = DEFAULT_CHANNEL
) -> list[Language]:
    """The rows that ship in `channel` (the one selection rule)."""
    check_channel(channel)
    return [row for row in rows if row.in_channel(channel)]


def package_name(language: str, version: str) -> str:
    return f"blastit-{language}-{version}"


def shim_glob(slug: str) -> str:
    return f"libblastit_{slug}-*-gnu.so"


def version_key(version: str) -> tuple[int, ...]:
    """Sort key for a version: components as integers, never a float."""
    return tuple(int(part) for part in version.split("."))


def _lines(text: str) -> list[tuple[int, str]]:
    lines = []
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.rstrip("\r")
        if line.strip() and not line.lstrip().startswith("#"):
            lines.append((number, line))
    return lines


def validate(text: str, *, source: str = "packaging/languages.tsv") -> list[str]:
    """Every problem in the TSV, in file order; an empty list means valid."""
    problems: list[str] = []
    rows: list[Language] = []
    names: dict[str, str] = {}
    slugs: dict[str, str] = {}
    header_seen = False
    for number, line in _lines(text):
        fields = line.split("\t")
        if not header_seen:
            header_seen = True
            if tuple(fields) != HEADER:
                problems.append(
                    f"{source}:{number}: header is {line!r}, expected "
                    f"{chr(9).join(HEADER)!r}"
                )
            continue
        if len(fields) != 6:
            problems.append(
                f"{source}:{number}: {len(fields)} fields, a row is exactly 6 "
                "(language<TAB>version<TAB>slug<TAB>deb-runtime<TAB>rpm-runtime<TAB>channels)"
            )
            continue
        row = Language(*fields)
        name = row.package
        language, version, slug = row.language, row.version, row.slug
        if not _VERSION.fullmatch(version):
            problems.append(
                f"{source}:{number}: {name} has version {version!r}; expected "
                "<major>.<minor> like 8.10 (not 8 or 8.1.0)"
            )
        if not _SLUG.fullmatch(slug):
            problems.append(
                f"{source}:{number}: {name} has slug {slug!r}; expected lowercase "
                "letters, digits or '_'"
            )
        if language in _RESERVED_LANGUAGES:
            problems.append(
                f"{source}:{number}: {language!r} is not a language "
                f"({', '.join(sorted(_RESERVED_LANGUAGES))} are reserved)"
            )
        if row.deb_runtime == NONE or row.rpm_runtime == NONE:
            problems.append(
                f"{source}:{number}: shim {name} needs a deb-runtime and rpm-runtime "
                f"Recommends value ({NONE} is only for the core package)"
            )
        # The channels cell is a selection key, so it is one of exactly three
        # spellings: an empty one would ship nowhere, a repeat is ambiguous and a
        # reordered one would make `packaging/languages.sh` disagree.
        if row.channels not in CHANNEL_COMBINATIONS:
            expected = ", ".join(repr(value) for value in CHANNEL_COMBINATIONS)
            problems.append(
                f"{source}:{number}: {name} has channels {row.channels!r}; expected one "
                f"of {expected} (space-separated, `stable` first, never empty, never "
                "repeated)"
            )
        # The runtime columns are interpolated into `Recommends:` in a Debian
        # control file and an RPM spec (and read by `docs/install.sh`), so they
        # are a package name, never spec/control syntax: `%`/`#`/`{`/`}` would
        # either expand a macro or start a comment in one of the two consumers,
        # and surrounding whitespace would silently change what is asked for.
        for field, value in (
            ("deb-runtime", row.deb_runtime),
            ("rpm-runtime", row.rpm_runtime),
        ):
            if value != value.strip():
                problems.append(
                    f"{source}:{number}: {name} has {field} {value!r} with leading or "
                    "trailing whitespace: it is a `Recommends:` value, not prose"
                )
            forbidden = sorted({char for char in "%#{}" if char in value})
            if forbidden:
                problems.append(
                    f"{source}:{number}: {name} has {field} {value!r} containing "
                    f"{' '.join(forbidden)}: a runtime column is a package name for "
                    "`Recommends:`, never spec/control syntax"
                )
        if name in names:
            problems.append(
                f"{source}:{number}: package {name} is declared twice "
                f"(first on line {names[name]})"
            )
        else:
            names[name] = str(number)
        if slug in slugs:
            problems.append(
                f"{source}:{number}: slug {slug} is declared twice "
                f"(first on line {slugs[slug]})"
            )
        else:
            slugs[slug] = str(number)
        rows.append(row)
    if not header_seen:
        problems.append(f"{source}: no header line")
    if header_seen and not rows:
        problems.append(f"{source}: declares no language rows")
    return problems


def parse(text: str, *, source: str = "packaging/languages.tsv") -> list[Language]:
    problems = validate(text, source=source)
    if problems:
        raise LanguageError("\n".join(problems))
    return _rows_of(text)


def _rows_of(text: str) -> list[Language]:
    rows: list[Language] = []
    header_seen = False
    for _, line in _lines(text):
        if not header_seen:
            header_seen = True
            continue
        rows.append(Language(*line.split("\t")))
    return rows


def load(path: Path | str = DEFAULT_TSV) -> list[Language]:
    path = Path(path)
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise LanguageError(f"{path}: {error.strerror or error}") from error
    return parse(text, source=str(path))


def package_rows(
    rows: list[Language], channel: str = DEFAULT_CHANNEL
) -> list[tuple[str, ...]]:
    """The five-field package-set rows of `channel`, core first."""
    return [CORE_ROW, *(row.package_row() for row in rows_in_channel(rows, channel))]


def names(rows: list[Language], channel: str = DEFAULT_CHANNEL) -> list[str]:
    """The binary package names of `channel`, core first."""
    return [CORE_PACKAGE, *(row.package for row in rows_in_channel(rows, channel))]


def shims(
    rows: list[Language], channel: str = DEFAULT_CHANNEL
) -> list[tuple[str, str]]:
    """`(package, shim glob)` of `channel`."""
    return [(row.package, row.shim) for row in rows_in_channel(rows, channel)]


def versions_of(
    rows: list[Language], language: str, channel: str = DEFAULT_CHANNEL
) -> list[str]:
    """Versions of one language in `channel`, newest first (never a float)."""
    found = [
        row.version
        for row in rows_in_channel(rows, channel)
        if row.language == language
    ]
    return sorted(found, key=version_key, reverse=True)


def as_json(rows: list[Language]) -> list[dict[str, object]]:
    core = {
        "package": CORE_PACKAGE,
        "kind": "core",
        "shim": NONE,
        "deb_runtime": NONE,
        "rpm_runtime": NONE,
        "language": None,
        "version": None,
        "slug": None,
        "channels": NONE,
    }
    tail = [
        {
            "package": row.package,
            "kind": row.kind,
            "shim": row.shim,
            "deb_runtime": row.deb_runtime,
            "rpm_runtime": row.rpm_runtime,
            "language": row.language,
            "version": row.version,
            "slug": row.slug,
            "channels": row.channels,
        }
        for row in rows
    ]
    return [core, *tail]


_COMMANDS = (
    "--rows",
    "--names",
    "--shims",
    "--php-minors",
    "--py-minors",
    "--json",
    "--check",
)
_USAGE = f"""usage: scripts/languages.py [--tsv PATH] [--channel stable|nightly] {"|".join(_COMMANDS)}

  --channel CH  the release channel the listings select: stable (the default) or
                nightly. Every row is validated whatever this is.
  --rows        name<TAB>kind<TAB>shim-glob<TAB>deb-runtime<TAB>rpm-runtime, core first
  --names       the binary package names, one per line (core first)
  --shims       <package><TAB><glob> for every package that ships a shim
  --php-minors  PHP minors, newest first
  --py-minors   CPython minors, newest first
  --json        one JSON object per row, core first
  --check       validate packaging/languages.tsv (every row, both channels) and
                print nothing on success
"""


def _compute(command: str, rows: list[Language], channel: str) -> tuple[str, int]:
    if command == "--rows":
        return "\n".join(
            "\t".join(row) for row in package_rows(rows, channel)
        ) + "\n", 0
    if command == "--names":
        return "\n".join(names(rows, channel)) + "\n", 0
    if command == "--shims":
        return (
            "\n".join(f"{name}\t{glob}" for name, glob in shims(rows, channel)) + "\n",
            0,
        )
    if command == "--php-minors":
        return "\n".join(versions_of(rows, "php", channel)) + "\n", 0
    if command == "--py-minors":
        return "\n".join(versions_of(rows, "python", channel)) + "\n", 0
    if command == "--json":
        return "\n".join(
            json.dumps(row, sort_keys=False) for row in as_json(rows)
        ) + "\n", 0
    if command == "--check":
        counts = ", ".join(
            f"{len([row for row in rows if row.language == language])} {language}"
            for language in sorted({row.language for row in rows})
        )
        channels = ", ".join(
            f"{len(rows_in_channel(rows, name))} {name}" for name in CHANNELS
        )
        return f"languages: ok ({len(rows)} language rows: {counts}; {channels})\n", 0
    return "", 2


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    path = DEFAULT_TSV
    channel = DEFAULT_CHANNEL
    commands: list[str] = []
    index = 0
    while index < len(args):
        arg = args[index]
        if arg in _COMMANDS:
            commands.append(arg)
        elif arg == "--tsv":
            index += 1
            if index >= len(args):
                sys.stderr.write("languages: --tsv needs a path\n")
                return 2
            path = Path(args[index])
        elif arg == "--channel" or arg.startswith("--channel="):
            if arg == "--channel":
                index += 1
                if index >= len(args):
                    sys.stderr.write("languages: --channel needs stable|nightly\n")
                    return 2
                channel = args[index]
            else:
                channel = arg.partition("=")[2]
        elif arg in ("-h", "--help"):
            sys.stdout.write(_USAGE)
            return 0
        elif arg.startswith("-"):
            sys.stderr.write(f"languages: unknown option {arg}\n{_USAGE}")
            return 2
        else:
            commands.append(arg)
        index += 1
    if channel not in CHANNELS:
        sys.stderr.write(
            f"languages: unknown channel {channel!r}; expected one of "
            f"{', '.join(CHANNELS)}\n{_USAGE}"
        )
        return 2
    if len(commands) != 1:
        sys.stderr.write(f"languages: exactly one command is required\n{_USAGE}")
        return 2
    command = commands[0]
    if command == "--check":
        try:
            text = Path(path).read_text(encoding="utf-8")
        except OSError as error:
            sys.stderr.write(f"languages: {path}: {error.strerror or error}\n")
            return 1
        problems = validate(text, source=str(path))
        if problems:
            for problem in problems:
                sys.stderr.write(problem + "\n")
            return 1
        rows = _rows_of(text)
    else:
        try:
            rows = load(path)
        except LanguageError as error:
            sys.stderr.write(f"languages: {error}\n")
            return 1
    output, status = _compute(command, rows, channel)
    sys.stdout.write(output)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
