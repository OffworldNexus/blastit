#!/usr/bin/env python3
"""Generate `scripts/rpm/blastit.spec` from `packaging/languages.tsv` (`MB.67.2`).

`rpmbuild` cannot read the canonical table, so the spec is a *generated* file
rather than a second hand-kept copy: the tag list in the header, the per-package
`blastit_glibc_<tag>`/`blastit_libgcc_<tag>` macro defaults, the `%package -n` /
`Summary:` / `Requires:` / `Recommends:` stanzas and the `%files -n` globs are
all derived from one row of `packaging/languages.tsv` per package. The plumbing
that is not per-language — `Name:`/`Version:`/`Release:`, the measured-floor
rationale, `%install`, the core `%files` — stays verbatim in this file.

Stdlib only, and it reads the table through the one loader (`scripts/languages.py`)
like every other consumer:

    scripts/rpm/gen_spec.py             # print the spec
    scripts/rpm/gen_spec.py --stdout    # the same, spelled out
    scripts/rpm/gen_spec.py --write     # rewrite scripts/rpm/blastit.spec
    scripts/rpm/gen_spec.py --check     # exit 1 + a unified diff when stale

`scripts/platform_matrix.py --check` runs `--check`, and the package workflows
run that `--check`, so a stale committed spec fails the same gate that validates
the matrix.
"""

from __future__ import annotations

import difflib
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR.parent))

import languages  # noqa: E402

SPEC = SCRIPT_DIR / "blastit.spec"

_USAGE = """usage: scripts/rpm/gen_spec.py [--stdout|--write|--check]

  --stdout   print the generated spec (the default)
  --write    rewrite scripts/rpm/blastit.spec from packaging/languages.tsv
  --check    exit 1 with a unified diff when the committed spec is stale, and
             exit 3 when this table cannot be rendered at all (a language with
             no template, or no row for the prose's worked example)
"""


# The header's second half: the measured-floor rationale, with its worked
# examples rendered from the table's own rows. A withdrawn version therefore
# cannot leave a freshly generated spec naming a package or macro that no
# longer exists, which a hand-spelled `blastit-php-8.4`/`blastit_glibc_php84`
# did (`MB.67.2` review).
class CannotRender(Exception):
    """The declared table cannot be rendered into a spec (no template, no example)."""


def _worked_example(
    rows: list[languages.Language], language: str
) -> languages.Language:
    """The row the prose names: the newest declared row of `language`.

    One rule for every language, so the `Recommends:` paragraph and the floor
    paragraph both render a version the table actually ships, and withdrawing a
    row simply moves the example to the next newest (`MB.67.2` review).
    """
    versions = languages.versions_of(rows, language)
    if not versions:
        raise CannotRender(
            f"{language}: packaging/languages.tsv declares no {language} row, so the "
            "spec's worked example has no version to render"
        )
    version = versions[0]
    return next(
        row for row in rows if row.language == language and row.version == version
    )


def _prose_one_floor(rows: list[languages.Language]) -> str:
    py = _worked_example(rows, "python")
    php = _worked_example(rows, "php")
    return (
        "# # One floor per package, never the bundle's maximum\n"
        "#\n"
        "# `scripts/build-deb.sh` measures `blastit`+`blastit-lsp` for the core and "
        "each\n"
        "# shim for its own package, and this spec is the RPM mirror of that: the "
        "core\n"
        "# `Requires: glibc >= %{blastit_glibc_core}` (2.34 on today's artifacts) "
        "and\n"
        f"# `{php.package}` `Requires: glibc >= %{{blastit_glibc_{php.slug}}}` "
        "(2.17), so a\n"
        "# shim does not demand a newer glibc than its own bytes need. `libgcc` is\n"
        "# declared only where the measured `DT_NEEDED` set carries "
        "`libgcc_s.so.1`\n"
        "# (the binaries and the CPython shims; not the PHP shims).\n"
        "#\n"
        "# `AutoReqProv: no` is deliberate: RPM's automatic dependency generator "
        "would\n"
        "# walk the shims and invent `libphp.so.*` / `libpython3.*` requirements "
        "for\n"
        "# libraries Blastit `dlopen`s at run time and does not link. It is a "
        "*package*\n"
        "# header, so every `%package -n` below restates it as `AutoReq: "
        "no`/`AutoProv:\n"
        "# no` — without that, the subpackages (and only they) get generated\n"
        "# `libc.so.6(GLIBC_2.x)` requirements that nobody measured.\n"
        "#\n"
        "# # The interpreter relationship is weak, and has to be\n"
        "#\n"
        "# Every shim package — `blastit-python-*` and `blastit-php-*` — carries "
        "the\n"
        "# matching distro runtime as a **`Recommends:`**, exactly as\n"
        "# `scripts/build-deb.sh`'s control files do (`Recommends: "
        f"{py.deb_runtime}` /\n"
        f"# `Recommends: {php.deb_runtime}` there, `Recommends: {py.rpm_runtime}` /\n"
        f"# `Recommends: {php.rpm_runtime}` here, because RPM distributions name no "
        "interpreter\n"
        "# package per minor). RPM distributions cannot back a hard requirement: "
        "Rocky 9\n"
        f"# has **no** `{py.rpm_runtime}` (it ships 3.9) and its "
        f"`{php.rpm_runtime}` is 8.0, so\n"
        f"# `Requires: {py.rpm_runtime}` or `Requires: {php.rpm_runtime}` would "
        "make the shim package\n"
        "# uninstallable on a distribution the release still supports. "
        "`Recommends:`\n"
        "# states the intent, shows in `dnf info`, is pulled by a plain `dnf "
        "install` when\n"
        "# available and skipped when it is not, and leaves the operator in "
        "control. A\n"
        "# host that follows the docs installs the matching interpreter first.\n"
        "#\n"
        "# `__os_install_post` is disabled so the payload stays byte-identical to "
        "the\n"
        "# bundle that was measured: a strip or a build-id relink would make the "
        "floor a\n"
        "# fact about a file nobody measured."
    )


_GLOBALS = """\
%global _build_id_links none
%global debug_package %{nil}
%global __os_install_post %{nil}"""

_VARIABLES = """\
%{!?blastit_version:%global blastit_version 0.0.0}
%{!?blastit_release:%global blastit_release 1}
%{!?blastit_bundle:%global blastit_bundle %{_builddir}/bundle}"""

_HEADER_FIELDS = """\
Name: blastit
Version: %{blastit_version}
Release: %{blastit_release}
Summary: Zero-trust application harness and sandbox"""

_CORE_METADATA = """\
License: Apache-2.0
URL: https://github.com/offworld-nexus/blastit
AutoReqProv: no
Requires: glibc >= %{blastit_glibc_core}
%if "%{blastit_libgcc_core}" == "1"
Requires: libgcc
%endif"""

_CORE_DESCRIPTION = """\
%description
Blastit runs 12-factor web applications as isolated per-route sandboxes, validates
every inbound byte, and keeps credentials behind protocol-aware capability proxies.
This package ships the interpreter-free server, the native language server, the
licence and the pure-Python helper package; the language shims are the
`blastit-python-*` and `blastit-php-*` subpackages beside it."""

_INSTALL = """\
%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a %{blastit_bundle}/usr %{buildroot}/
# `cp -a` keeps the staging user; a package's files belong to root whoever built
# it (and `scripts/test_packages.sh` asserts `root:root` from the RPM header).
chown -R 0:0 %{buildroot}"""

_CORE_FILES = """\
%files
%{_bindir}/blastit
%{_bindir}/blastit-lsp
%{_datadir}/blastit/LICENSE
%{_datadir}/blastit/python
%{_datadir}/blastit/lib/blastit
%{_datadir}/blastit/lib/blastit-lsp"""

# How a language names its package in prose: what the shim *is* (`Summary:` /
# `%description -n`) for each language the TSV may declare. A language without a
# template is an error, not a guessed sentence — the spec is read by operators.
_TEMPLATES = {
    "python": {
        "summary": "CPython {version} view shim for blastit",
        "description": (
            "The libblastit_{slug} shim lets blastit run Python WSGI views on "
            "CPython {version}. It is\n"
            "loaded into the host interpreter by the blastit binary; install it "
            "together with\n"
            "blastit, never instead of it."
        ),
    },
    "php": {
        "summary": "PHP {version} embedded SAPI shim for blastit",
        "description": (
            "The libblastit_{slug} shim lets blastit run PHP view scripts through "
            "the host's\n"
            "embedded NTS libphp {version}. It is loaded by the blastit binary; "
            "install it together\n"
            "with blastit and a PHP {version} embedded build, never instead of it."
        ),
    },
}

_NUMBERS = (
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
)


def _count_word(count: int) -> str:
    return _NUMBERS[count] if count < len(_NUMBERS) else str(count)


def _tags(rows: list[languages.Language]) -> str:
    """The per-package macro tags, `core` first: `` `core`, `py313`, ... ``."""
    tags = ("core", *(row.slug for row in rows))
    return ", ".join(f"`{tag}`" for tag in tags)


def _intro(rows: list[languages.Language]) -> str:
    return (
        "# Packs a staged Blastit bundle (`make install PREFIX=<dir>`) as "
        f"{_count_word(len(rows) + 1)} RPM\n"
        "# packages (`universal-distribution.md` §6.1, `MB.65`).\n"
        "#\n"
        "# `scripts/build-rpm.sh` drives this spec; it defines `blastit_version`,\n"
        "# `blastit_release`, `blastit_bundle` (the staged `/usr` tree) and — one "
        "pair per\n"
        "# package — `blastit_glibc_<tag>`/`blastit_libgcc_<tag>`: the floor\n"
        "# `scripts/check-portable-elf.sh --max-glibc` measured **for that "
        "package's own\n"
        "# ELFs** and whether its measured `DT_NEEDED` set needs `libgcc_s.so.1`. "
        "The tags\n"
        f"# are {_tags(rows)}. The spec's\n"
        "# only job is to move that tree into the buildroot and declare those "
        "measured\n"
        "# relationships; it never compiles anything."
    )


def _floors(rows: list[languages.Language]) -> str:
    lines = [
        "# Per-package measured floors, one pair each (see the header). The "
        "defaults are",
        "# the oldest glibc a gnu artifact may claim; `build-rpm.sh` always passes the",
        "# measured value, so a default only shows up in a hand-run rpmbuild.",
        # The core package's own pair, then one pair per TSV row.
        "%{!?blastit_glibc_core:%global blastit_glibc_core 2.17}",
        "%{!?blastit_libgcc_core:%global blastit_libgcc_core 0}",
    ]
    for row in rows:
        lines.append(
            f"%{{!?blastit_glibc_{row.slug}:%global blastit_glibc_{row.slug} 2.17}}"
        )
        lines.append(
            f"%{{!?blastit_libgcc_{row.slug}:%global blastit_libgcc_{row.slug} 0}}"
        )
    return "\n".join(lines)


def _template(row: languages.Language) -> dict:
    template = _TEMPLATES.get(row.language)
    if template is None:
        raise CannotRender(
            f"{row.language}: no RPM package template for this language "
            f"(known: {', '.join(sorted(_TEMPLATES))})"
        )
    return template


def _package(row: languages.Language) -> str:
    template = _template(row)
    return "\n".join(
        [
            f"%package -n {row.package}",
            "Summary: " + template["summary"].format(**row._asdict()),
            f"Requires: glibc >= %{{blastit_glibc_{row.slug}}}",
            f'%if "%{{blastit_libgcc_{row.slug}}}" == "1"',
            "Requires: libgcc",
            "%endif",
            "Requires: blastit = %{version}-%{release}",
            "AutoReq: no",
            "AutoProv: no",
            f"Recommends: {row.rpm_runtime}",
        ]
    )


def _description(row: languages.Language) -> str:
    template = _template(row)
    return "\n".join(
        [
            f"%description -n {row.package}",
            template["description"].format(**row._asdict()),
        ]
    )


def _files(row: languages.Language) -> str:
    return "\n".join(
        [
            f"%files -n {row.package}",
            f"%{{_datadir}}/blastit/lib/libblastit_{row.slug}-*.so",
        ]
    )


def render(rows: list[languages.Language]) -> str:
    """The whole spec, byte for byte, from the declared language rows."""
    blocks = [
        # The header is one contiguous comment run: the generated intro, a `#`
        # separator, then the measured-floor rationale (lines 1-47 of the file).
        _intro(rows) + "\n#\n" + _prose_one_floor(rows),
        _GLOBALS,
        _VARIABLES,
        _floors(rows),
        _HEADER_FIELDS,
        _CORE_METADATA,
        _CORE_DESCRIPTION,
    ]
    for row in rows:
        blocks.append(_package(row))
        blocks.append(_description(row))
    blocks.extend(["%prep", "%build", _INSTALL, _CORE_FILES])
    for row in rows:
        blocks.append(_files(row))
    return "\n\n".join(blocks) + "\n"


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    modes = {"--stdout": "stdout", "--write": "write", "--check": "check"}
    mode = "stdout"
    for arg in args:
        if arg in ("-h", "--help"):
            sys.stdout.write(_USAGE)
            return 0
        if arg not in modes:
            sys.stderr.write(f"gen_spec: unknown argument {arg}\n{_USAGE}")
            return 2
        mode = modes[arg]

    try:
        rows = languages.load()
    except languages.LanguageError as error:
        sys.stderr.write(f"gen_spec: {error}\n")
        return 1

    try:
        generated = render(rows)
    except CannotRender as error:
        # A distinct exit code: this is "the table cannot be rendered", not
        # "the committed file is stale", and `--write` cannot fix it either.
        sys.stderr.write(f"gen_spec: cannot render packaging/languages.tsv: {error}\n")
        return 3
    if mode == "stdout":
        sys.stdout.write(generated)
        return 0
    if mode == "write":
        SPEC.write_text(generated, encoding="utf-8")
        sys.stdout.write(
            f"gen_spec: wrote {SPEC.relative_to(SCRIPT_DIR.parent.parent)}\n"
        )
        return 0

    try:
        committed = SPEC.read_text(encoding="utf-8")
    except OSError as error:
        sys.stderr.write(f"gen_spec: {SPEC}: {error.strerror or error}\n")
        return 1
    if committed == generated:
        return 0
    shown = SPEC.relative_to(SCRIPT_DIR.parent.parent)
    diff = difflib.unified_diff(
        committed.splitlines(keepends=True),
        generated.splitlines(keepends=True),
        fromfile=f"a/{shown}",
        tofile=f"b/{shown}",
    )
    sys.stderr.writelines(diff)
    sys.stderr.write(
        f"gen_spec: {shown} is stale: run `scripts/rpm/gen_spec.py --write`\n"
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
