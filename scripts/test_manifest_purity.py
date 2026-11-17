#!/usr/bin/env python3
"""Self-tests for the `std::env` guard (`scripts/check_manifest_purity.py`).

A guard that cannot fail is worse than no guard, so these are proved on every
`make check`:

1. **A dirty fixture is refused**, naming `file:line`, for both spellings —
   `std::env::var(…)` and `use std::env;`.
2. **The nested-use evasion is refused** (MB.21.5.5): the audit's exact text
   (`use std::{env};` plus `env::var("HOME")`) and every other spelling that
   hides the module behind braces, an absolute path, a `self`, an alias, a raw
   identifier (`r#env`, `env as r#e`), a nested group, a multi-line list, a
   comment inside the group, or a bare `env::var(..)` call.
2b. **A construct the scanner cannot classify is a refusal, not a pass**
   (review, MB.21.5.5): in a file that mentions `env`, a `use` window over the
   token/depth budget, a glob import (`use std::*;`), and a `crate::` alias
   whose origin is outside the file are refused with their `file:line`, because
   a gate that cannot read its input must not report success. Three rounds each
   found a new spelling, so the rule is fail-closed rather than enumerative;
   the deliberate over-approximation (a comment mentioning `env` plus a glob)
   is refused too.
2c. **An aliased `std` root is still a hit**: `use std::{self as s}; use
   s::env as e;` and the `extern crate` / chained forms resolve to `std`.
3. **The deliberate over-approximation still fires**: `use std::env::consts;`
   names the module (design §17.2 says a `std::env` *appearance* fails), and
   it failed before the nested spellings were added, so it stays a failure.
4. **The near-misses stay legal**: `env` in a comment or a string literal, a
   `let env = …` local, `env.len()`, and the other std modules.
5. **A clean fixture passes.**
6. **A tree with no Rust source is refused**, not passed vacuously (a moved
   directory must not turn the rule off).
7. **The repository itself is clean**, which is the actual acceptance.

Run by `make manifest-purity` (stdlib only, no pytest needed).
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
GUARD = HERE / "check_manifest_purity.py"

#: The tree the guard covers, relative to a scanned root.
COVERED = Path("crates/blastit-manifest/src")

#: Assertions made, so the closing line counts what actually ran.
_CHECKS = 0

#: Fixture files written, for the same reason.
_FIXTURES = 0

#: Every spelling that imports or calls through `std::env`, and the line the
#: hit must name. One fixture per spelling, so a miss names its own file.
NESTED = {
    # The audit's measured evasion (`docs/testing/AUDIT-MB.21.md` §5.1).
    "evasive.rs": (
        (
            "pub fn home() -> Option<String> {\n"
            "    use std::{env};\n"
            '    env::var("HOME").ok()\n'
            "}\n"
        ),
        [2, 3],
    ),
    # A nested list, not the module alone.
    "list.rs": ("use std::{fs, env};\n", [1]),
    # An alias inside the group.
    "alias.rs": ("use std::{fs, env as environment};\n", [1]),
    # A nested group with `self`, which re-exports the module itself.
    "group.rs": ("use std::{io, env::{self, var}};\n", [1]),
    # A multi-line group: the hit names the line the module is on, not `use`.
    "multi.rs": ("use std::{\n    fs,\n    env,\n};\n", [3]),
    # An absolute path in front of the group.
    "absolute.rs": ("use ::std::{env as e};\n", [1]),
    # A root-level group.
    "root_group.rs": ("use {std::env};\n", [1]),
    # The path spelled with spaces, which the old literal needle missed.
    "spaces.rs": ('fn f() { let _ = std :: env :: var("X"); }\n', [1]),
    # The call through a name some other import introduced.
    "call_only.rs": ('fn f() { env::var("HOME"); }\n', [1]),
    # A comment inside the group is trivia, not a parse failure.
    "comment_in_group.rs": (
        "use std::{\n    fs,\n    // the module this gate refuses\n    env,\n};\n",
        [4],
    ),
    # A comment before the root is trivia too.
    "comment_before.rs": ("use /* the module */ std::{env};\n", [1]),
    # A string holding the comment opener must not swallow the code after it.
    "string_then_use.rs": ('const OPEN: &str = "/*";\nuse std::{env};\n', [2]),
    # The literal spelling is textual: a string holding it is a hit.
    "string_literal.rs": ('const SPELLING: &str = "std::env";\n', [1]),
    # A raw identifier names the same module (`r#env` is `env`), and rustc
    # accepts the file, so the gate must see through the `r#` (review).
    "raw_alias.rs": (
        'use std::r#env as e;\npub fn home() -> Option<String> {\n    e::var("HOME").ok()\n}\n',
        [1],
    ),
    # The same inside a group.
    "raw_in_group.rs": (
        'use std::{r#env as e};\npub fn home() {\n    e::var("HOME");\n}\n',
        [1],
    ),
    # A raw identifier as the alias: `r#e::var(..)` reaches the module too.
    "raw_rename.rs": (
        'use std::{env as r#e};\npub fn home() -> Option<String> {\n    r#e::var("HOME").ok()\n}\n',
        [1],
    ),
    # A raw identifier with no alias at all.
    "raw_bare.rs": ("use std::{r#env};\n", [1]),
    # A visible re-export: `pub use` is the same `use` item.
    "pub_use.rs": ("pub use std::{env};\n", [1]),
    # `std::env as e`, with no group around it.
    "alias_direct.rs": ("use std::env as e;\n", [1]),
    # Two groups deep: `io::{self, Write}` next to `env`.
    "two_deep.rs": ("use std::{io::{self, Write}, env};\n", [1]),
    # A std alias root: `use std::{self as s}; use s::env as e;`.
    "alias_root.rs": (
        ('use std::{self as s};\nuse s::env as e;\nfn main() { e::var("HOME"); }\n'),
        [2],
    ),
    # `extern crate std as s` is another way to give the crate a name.
    "extern_root.rs": (
        ('extern crate std as s;\nuse s::env as e;\nfn main() { e::var("HOME"); }\n'),
        [2],
    ),
    # A chained alias: `std -> s -> t`, then `t::env`.
    "chained_root.rs": (
        (
            "use std::{self as s};\n"
            "use s as t;\n"
            "use t::env as e;\n"
            'fn main() { e::var("HOME"); }\n'
        ),
        [3],
    ),
    # A call through the aliased root, with no `env` import at all.
    "alias_call.rs": (
        'use std::{self as s};\nfn f() { s::env::var("HOME"); }\n',
        [2],
    ),
}

#: Fixtures that must be **refused**: their constructs cannot be classified in
#: a file that mentions `env`, and a refusal is never a pass.
_ALIASES = ",\n".join(f"    std::collections as c{n}" for n in range(150))
REFUSED = {
    # A `use` over the token budget that hides `std::{env as e}`.
    "alias_overlong.rs": (
        (
            f"use {{\n{_ALIASES},\n    std::{{env as e}},\n}};\n"
            'pub fn home() -> Option<String> {\n    e::var("HOME").ok()\n}\n'
        ),
        [1],
    ),
    # A glob imports the `env` module (and every std alias) unnameably.
    "glob_alias.rs": (
        'use std::*;\nuse env as e;\nfn f() { e::var("HOME"); }\n',
        [1],
    ),
    # Deliberate over-approximation: the comment only mentions the word, but a
    # file that talks about `env` and glob-imports std is refused.
    "comment_glob.rs": (
        "// the env names are resolved at boot\nuse std::*;\n",
        [2],
    ),
}

#: The cross-file re-export: `mod_a` publishes `std` as `s`, and `mod_b`
#: imports through `crate::` — resolvable only outside this file, so refused.
TWO_FILE = {
    "mod_a.rs": "pub use std::{self as s};\n",
    "mod_b.rs": (
        "use crate::mod_a::s::env as e;\n"
        'fn f() -> Option<String> { e::var("HOME").ok() }\n'
    ),
}
TWO_FILE_REFUSED = {"mod_b.rs": [1, 2]}

#: A file that mentions the environment without naming the module.
LEGAL = (
    "// The environment is resolved at boot; env names never become values.\n"
    "// The parser carries `env` as a name and `env.len()` is a String.\n"
    "// A comment cannot import anything: `use std::{env}` here is prose.\n"
    "use std::fs;\n"
    "use std::collections::HashMap;\n"
    "use std::process::Command;\n"
    "use std::{fmt, io};\n"
    'const TEMPLATE: &str = "env";\n'
    'const PHRASE: &str = "env::var is a phrase, not a call";\n'
    "fn names() -> Option<String> {\n"
    '    let env = "DATABASE_URL";\n'
    "    let count = 1;\n"
    "    let _ = count;\n"
    "    Some(env.to_string())\n"
    "}\n"
    "fn _unused(_: &Command, _: &fmt::Arguments<'_>, _: &mut dyn io::Write) {}\n"
    "fn _map() -> HashMap<String, String> { HashMap::new() }\n"
)

#: More files that must pass: a crate's own `env` module is not `std::env`
#: (R24's wording), an unreadable `use` with no `env` is not a refusal, and a
#: glob in a file that never mentions `env` is out of the rule's subject.
LEGAL_FILES = {
    "clean_overlong.rs": (f"use {{\n{_ALIASES},\n}};\nfn f() {{}}\n"),
    "mycrate_env.rs": (
        'use mycrate::env as e;\nfn f() -> Option<String> { e::lookup("HOME").ok() }\n'
    ),
    "glob_no_env.rs": ("use std::*;\nfn f() {}\n"),
}


def load_guard():
    """The guard module, imported by path (it is a script, not a package)."""
    spec = importlib.util.spec_from_file_location("check_manifest_purity", GUARD)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_guard(root: Path) -> subprocess.CompletedProcess[str]:
    """The guard as `make check` runs it, against `root`."""
    return subprocess.run(
        [sys.executable, str(GUARD), "--root", str(root)],
        capture_output=True,
        text=True,
        check=False,
    )


def write_source(root: Path, name: str, body: str) -> Path:
    """A fixture source file under the covered tree."""
    global _FIXTURES
    _FIXTURES += 1
    path = root / COVERED / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body, encoding="utf-8")
    return path


def check(condition: bool, message: str) -> None:
    """Fails the self-test with one sentence, never a bare traceback."""
    global _CHECKS
    _CHECKS += 1
    if not condition:
        raise SystemExit(f"manifest-purity self-test: {message}")


def main() -> int:
    """Runs every case; returns the process exit status."""
    guard = load_guard()
    check(guard.NEEDLE == "std::env", "the needle changed; update this self-test")
    check(
        guard.COVERED == COVERED,
        f"the covered tree changed to {guard.COVERED}; update this self-test",
    )

    # 1. A dirty fixture is refused, and the hit names file:line.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        body = 'fn main() { let _ = std::env::var("X"); }\n'
        write_source(root, "dirty.rs", body)
        relative = f"{COVERED.as_posix()}/dirty.rs"
        check(
            guard.scan(root) == [(relative, 1, body.strip())],
            f"the scan missed the hit: {guard.scan(root)}",
        )
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"a dirty tree must fail: {result.stdout}{result.stderr}",
        )
        check(f"{relative}:1:" in result.stderr, result.stderr)

        # `use std::env;` is the other spelling, and it is caught too.
        write_source(root, "use.rs", "use std::env;\n")
        check(
            (f"{COVERED.as_posix()}/use.rs", 1, "use std::env;") in guard.scan(root),
            f"the scan missed `use std::env;`: {guard.scan(root)}",
        )

    # 2. The nested-use evasion is refused, one fixture per spelling.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        for name, (body, _lines) in NESTED.items():
            write_source(root, name, body)
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"a nested `std::env` must fail: {result.stdout}{result.stderr}",
        )
        found = guard.scan(root)
        for name, (_body, lines) in NESTED.items():
            relative = f"{COVERED.as_posix()}/{name}"
            got = [line for path, line, _text in found if path == relative]
            check(
                got == sorted(lines),
                f"the scan gave {got} for {relative}, expected {sorted(lines)}",
            )
            for line in lines:
                check(
                    f"{relative}:{line}:" in result.stderr,
                    f"the refusal did not name {relative}:{line}: {result.stderr}",
                )
        check(
            "a Blastfile names variables and never reads them" in result.stderr,
            f"the refusal lost the rule's sentence: {result.stderr}",
        )

    # 2b. A construct the scanner cannot classify is refused, never a pass.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        for name, (body, _lines) in REFUSED.items():
            write_source(root, name, body)
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"an unclassifiable construct must fail: {result.stdout}{result.stderr}",
        )
        check(
            guard.scan(root) == [],
            f"a refusal is not a hit: {guard.scan(root)}",
        )
        found = guard.scan_refusals(root)
        for name, (_body, lines) in REFUSED.items():
            relative = f"{COVERED.as_posix()}/{name}"
            got = [line for path, line, _message in found if path == relative]
            check(
                got == sorted(lines),
                f"the refusal scan gave {got} for {relative}, expected {sorted(lines)}",
            )
            for line in lines:
                check(
                    f"{relative}:{line}:" in result.stderr,
                    f"the refusal did not name {relative}:{line}: {result.stderr}",
                )
        check(
            "cannot classify" in result.stderr,
            f"the refusal lost its construct sentence: {result.stderr}",
        )

    # 2c. The cross-file re-export: `mod_a` is clean on its own, `mod_b` cannot
    #     resolve `crate::mod_a::s` and is refused by line.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        for name, body in TWO_FILE.items():
            write_source(root, name, body)
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"a cross-file `std` alias must fail: {result.stdout}{result.stderr}",
        )
        found = guard.scan_refusals(root)
        for name, lines in TWO_FILE_REFUSED.items():
            relative = f"{COVERED.as_posix()}/{name}"
            got = [line for path, line, _message in found if path == relative]
            check(
                got == sorted(lines),
                f"the refusal scan gave {got} for {relative}, expected {sorted(lines)}",
            )
        check(
            guard.scan(root) == [],
            f"a cross-file alias is not a hit: {guard.scan(root)}",
        )
        mod_a = f"{COVERED.as_posix()}/mod_a.rs"
        check(
            all(path != mod_a for path, _line, _message in found),
            f"{mod_a} names no `env` and must not be refused: {found}",
        )

    # 3. The documented over-approximation: naming the module is the hit, so
    #    `std::env::consts` (target constants, not a read) fails as it did
    #    before the nested spellings were added. Design §17.2 is "if
    #    `std::env` appears", and a gate that guessed at the reader's intent
    #    would be the evasion again.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        write_source(root, "consts.rs", "use std::env::consts;\n")
        check(
            (f"{COVERED.as_posix()}/consts.rs", 1, "use std::env::consts;")
            in guard.scan(root),
            f"`std::env::consts` names the module and must fail: {guard.scan(root)}",
        )
        result = run_guard(root)
        check(
            result.returncode == 1,
            f"`std::env::consts` must fail: {result.stdout}{result.stderr}",
        )

    # 4. The near-misses stay legal: prose, a string, a local named `env`, and
    #    the std modules that do not read the environment.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        write_source(root, "legal.rs", LEGAL)
        check(
            guard.scan(root) == [],
            f"the scan flagged a legal file: {guard.scan(root)}",
        )
        result = run_guard(root)
        check(
            result.returncode == 0,
            f"a legal tree must pass: {result.stdout}{result.stderr}",
        )
        check("manifest-purity: ok" in result.stdout, result.stdout)

    # 4b. The allow corpus beyond one file: an honest `mycrate::env` module and
    #     an unreadable 150-alias `use` with no `env` must both stay green.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        for name, body in LEGAL_FILES.items():
            write_source(root, name, body)
        check(
            guard.scan(root) == [],
            f"the scan flagged a legal file: {guard.scan(root)}",
        )
        check(
            guard.scan_refusals(root) == [],
            f"the scan refused a legal file: {guard.scan_refusals(root)}",
        )
        result = run_guard(root)
        check(
            result.returncode == 0,
            f"a legal tree must pass: {result.stdout}{result.stderr}",
        )

    # 5. A clean fixture passes.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        root = Path(scratch)
        write_source(root, "clean.rs", "fn main() {}\n")
        result = run_guard(root)
        check(result.returncode == 0, f"a clean tree must pass: {result.stderr}")
        check("manifest-purity: ok" in result.stdout, result.stdout)

    # 6. No source at all is refused, never a vacuous pass.
    with tempfile.TemporaryDirectory(prefix="blastit-purity-") as scratch:
        result = run_guard(Path(scratch))
        check(
            result.returncode == 1,
            f"an empty tree must be refused, not pass: {result.stdout}",
        )

    # 7. The real tree is clean: the actual acceptance.
    result = run_guard(REPO)
    check(result.returncode == 0, f"the repository is not clean: {result.stderr}")
    print(result.stdout.strip())
    print(
        f"manifest-purity self-test: ok — {_CHECKS} assertions, "
        f"{_FIXTURES} fixtures proved, {len(guard.covered(REPO))} real source file(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
