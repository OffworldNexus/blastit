#!/usr/bin/env python3
"""Negative twins for the portability guard (`scripts/check_lsp_portable.py`).

A guard that cannot fail is worse than no guard, so every promise is proved by
breaking it on a throwaway copy and asserting **which** promise went red — a
refusal line is `path:line: property: detail`, and each case here compares the
set of property names a run reported with the one it should have reported.

The tree each case mutates is a miniature workspace, not this checkout: the two
crates the guard reads (`blastit-lsp`, `blastit-manifest`), the delegating
`blastit` package, and a copy of this repository's `Makefile`, so the wiring
cases mutate the file `make check` actually reads. `cargo generate-lockfile
--offline` writes the lock each guard run needs, and no case reaches the network
(`docs/knowledge/LESSONS.md`, `MB.3.64`).

The cases, covering every promise and its adversarial escapes:

1. `manifest` — a third runtime dependency, a `[target.'cfg(windows)']` table
   (the edge a `[dependencies]` count misses), a `build.rs`, and `[lib].path` /
   `[[bin]].path` pointing outside `crates/blastit-lsp/src/`.
2. `source` — literal paths, brace groups, brace/root/module/`self` aliases,
   globs, split `use` items, single-line and split `extern crate` aliases
   (resolving `s::…` to `std::…`), single-line and split `include!`/`#[path]`,
   single-line and split `#[cfg(…)]`/`cfg!(…)`, a symlink out of `src/`, an
   invented raw-identifier + comment-interleaved spelling, and a deleted
   `#![forbid(unsafe_code)]` (even when quoted in a doc comment).
3. `environment` — a direct `std::env` read, a split `extern crate std as s;
   s::env::var(..)` alias read, and the guard's own self-refusal when a
   `std::env` row is injected into `STD_ALLOWLIST`.
4. `closure` — `rustix` reached **transitively** through `blastit-manifest`, so
   the manifest is legal and only the graph is red, naming the dependency chain.
5. `identity` — a second copy under `crates/blastit/src/lsp/`, dropped
   delegation, and a second `blastit-lsp` edge.
6. `wiring` — `lsp-portable` dropped from `make check`, from `.PHONY`, and the
   negative twins dropped from the `lsp-portable` recipe.

Two cases prove the guard's **boundary**: a dependency the code still uses but
the manifest no longer names is the compiler's red (`E0433`), not this guard's
(the guard enforces the ceiling, never the floor); and a tree with no source or
an unreadable manifest is refused rather than passed vacuously.

Run by `make lsp-portable` (stdlib only, no pytest needed, offline).
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
GUARD = HERE / "check_lsp_portable.py"

#: The library the fixture's `blastit-lsp` exposes, so `main.rs` can call it.
LIB = (
    "//! A fixture; quoting `#![forbid(unsafe_code)]` in prose is not the attribute.\n"
    "#![forbid(unsafe_code)]\n"
    "\n"
    "pub fn run() -> u8 {\n"
    "    0\n"
    "}\n"
)

MAIN = (
    "#![forbid(unsafe_code)]\n"
    "\n"
    "fn main() {\n"
    "    std::process::exit(blastit_lsp::run() as i32)\n"
    "}\n"
)

#: Escapes a **compiler accepts** (`cargo check` exit 0) that `source` must refuse.
SOURCE_ESCAPES: dict[str, tuple[str, str]] = {
    "literal-path": (
        'pub fn read_it() {\n    let _ = std::fs::read("x");\n}\n',
        "names `std::fs::read`",
    ),
    "brace-group": (
        "#[allow(unused_imports)]\nuse std::{fs, net};\n",
        "names `std::fs`",
    ),
    "brace-alias": (
        (
            "use std::{process as p};\n\n"
            'pub fn spawn() {\n    let _ = p::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "root-alias": (
        (
            "use std as s;\n\n"
            'pub fn spawn() {\n    let _ = s::process::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "glob": (
        (
            "#[allow(unused_imports)]\nuse std::*;\n\n"
            'pub fn spawn() {\n    let _ = process::Command::new("ls");\n}\n'
        ),
        "names `std::*`",
    ),
    "split-root-alias": (
        (
            "use std\nas\ns;\n\n"
            'pub fn spawn() {\n    let _ = s::process::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "split-glob": (
        (
            "#[allow(unused_imports)]\nuse std::\n*;\n\n"
            'pub fn spawn() {\n    let _ = process::Command::new("ls");\n}\n'
        ),
        "names `std::*`",
    ),
    "os-alias": (
        (
            "use std::os as o;\n\n"
            'pub fn link() {\n    let _ = o::unix::fs::symlink("a", "b");\n}\n'
        ),
        "names `std::os::unix::fs::symlink`",
    ),
    "self-item": (
        (
            "use std::{self as s};\n\n"
            'pub fn spawn() {\n    let _ = s::process::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "star-item": (
        "#[allow(unused_imports)]\nuse std::{*, fmt};\n",
        "names `std::*`",
    ),
    "extern-crate": (
        (
            "extern crate std as s;\n\n"
            'pub fn spawn() {\n    let _ = s::process::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "split-extern-crate-spawn": (
        (
            "extern\ncrate std as s;\n\n"
            'pub fn spawn() {\n    let _ = s::process::Command::new("ls");\n}\n'
        ),
        "names `std::process::Command::new`",
    ),
    "split-extern-crate-os": (
        (
            "extern\ncrate std as s;\n\n"
            'pub fn link() {\n    let _ = s::os::unix::fs::symlink("a", "b");\n}\n'
        ),
        "names `std::os::unix::fs::symlink`",
    ),
    "three-line-extern-crate": (
        (
            "extern\n/* trivia */\ncrate\nself\nas\nme;\n\n"
            "pub fn touch() -> u8 {\n    me::run()\n}\n"
        ),
        "contains `extern crate`",
    ),
    "split-extern-c": (
        'pub extern\n"C"\nfn c_entry() {}\n',
        'contains `extern "C"`',
    ),
    "split-libc": (
        (
            "mod libc {\n    pub fn c() {}\n}\n\n"
            "pub fn call_it() {\n"
            '    let _ = "continued \\\n        string";\n'
            "    libc\n    ::c();\n"
            "}\n"
        ),
        "crates/blastit-lsp/src/lib.rs:15: source: contains `libc::`",
    ),
    "split-nix": (
        (
            "mod nix {\n    pub fn c() {}\n}\n\n"
            "pub fn call_it() {\n    nix\n    ::c();\n}\n"
        ),
        "contains `nix::`",
    ),
    # A single Rust identifier (`rustix`, `pyo3`, `blastit_sys`) cannot contain a
    # newline (`rus\ntix` is two identifiers and does not compile — asserted below),
    # while `r#<ident>` + a newline before `::` is the valid multi-line spelling.
    "foreign-rustix": (
        (
            "mod rustix {\n    pub fn c() {}\n}\n\n"
            "pub fn call_it() {\n    r#rustix\n    ::c();\n}\n"
        ),
        "contains `rustix`",
    ),
    "foreign-pyo3": (
        (
            "mod pyo3 {\n    pub fn c() {}\n}\n\n"
            "pub fn call_it() {\n    r#pyo3\n    ::c();\n}\n"
        ),
        "contains `pyo3`",
    ),
    "foreign-blastit-sys": (
        (
            "mod blastit_sys {\n    pub fn c() {}\n}\n\n"
            "pub fn call_it() {\n    r#blastit_sys\n    ::c();\n}\n"
        ),
        "contains `blastit_sys`",
    ),
    "exact-row": (
        (
            "use std::io::Write;\n\n"
            "pub fn w<W: Write>(w: &mut W) -> std::io::Result<()> {\n"
            '    w.write_all(b"x")\n}\n'
        ),
        "names `std::io::Result`",
    ),
    "allowlist-drift": (
        ("use std::time::Instant;\n\npub fn now() {\n    let _ = Instant::now();\n}\n"),
        "names `std::time::Instant`",
    ),
    "cfg-unix": (
        "#[cfg(unix)]\npub fn only_unix() {}\n",
        "carries `cfg` predicate `unix`",
    ),
    "split-cfg-unix": (
        "#[cfg\n(unix)]\npub fn only_unix() {}\n",
        "carries `cfg` predicate `unix`",
    ),
    "split-hash-cfg": (
        "#\n[cfg(unix)]\npub fn only_unix() {}\n",
        "carries `cfg` predicate `unix`",
    ),
    "inner-cfg-unix": (
        "pub mod inner {\n    #!\n    [cfg(unix)]\n}\n",
        "carries `cfg` predicate `unix`",
    ),
    "split-cfg-attr": (
        "#[cfg_attr\n(unix, allow(dead_code))]\npub fn gated() {}\n",
        "carries `cfg` predicate `unix,allow(dead_code)`",
    ),
    "split-cfg-macro": (
        "pub fn check_os() -> bool {\n    cfg\n    !\n    [unix]\n}\n",
        "carries `cfg` predicate `unix`",
    ),
    "macro-std-escape": (
        (
            "macro_rules! u {\n"
            "    ($r:ident) => {\n"
            "        use $r::process::Command;\n"
            "    };\n"
            "}\n\n"
            "pub fn spawn() {\n"
            "    u!(std);\n"
            '    let _ = Command::new("ls");\n'
            "}\n"
        ),
        "names `std`",
    ),
    "invented-spelling": (
        (
            "use ::r#std/* trivia */::r#os as o;\n\n"
            "pub fn link() {\n"
            '    let _ = o :: /* mid-path */ r#unix :: fs :: r#symlink("a", "b");\n'
            "}\n"
        ),
        "names `std::os::unix::fs::symlink`",
    ),
}

#: One refusal line is `path:line: property: detail` (or `path: property: detail`).
_PROPERTY = re.compile(r"^\S+: ([a-z-]+): ", re.MULTILINE)


@dataclass(slots=True)
class Suite:
    """Assertion counter without module-level mutable state."""

    checks: int = 0

    def expect(self, message: str, condition: bool) -> None:
        self.checks += 1
        if not condition:
            raise SystemExit(f"lsp-portable self-test: {message}")

    def red(
        self,
        root: Path,
        message: str,
        *,
        expected: set[str],
        contains: str = "",
        guard_script: Path = GUARD,
    ) -> None:
        done = guard(root, guard_script=guard_script)
        found = properties(done.stderr)
        self.expect(
            f"{message}: expected the guard to fail, it exited {done.returncode} "
            f"with {done.stdout.strip()!r}",
            done.returncode == 1,
        )
        self.expect(
            f"{message}: expected exactly {sorted(expected)} to go red, got "
            f"{sorted(found)} — {done.stderr.strip()}",
            found == expected,
        )
        if contains:
            self.expect(
                f"{message}: the refusal does not say {contains!r} — {done.stderr.strip()}",
                contains in done.stderr,
            )


def build(
    root: Path,
    *,
    lib: str = LIB,
    main: str = MAIN,
    lib_path: str = "src/lib.rs",
    bin_path: str = "src/main.rs",
    lsp_tables: str = "",
    dependencies: str = (
        'blastit-manifest = { version = "0.2.0", path = "../blastit-manifest" }\n'
        'serde_json = "1.0.154"\n'
    ),
    manifest_dependencies: str = "",
    makefile: str | None = None,
    stray_server: bool = False,
    delegation: bool = True,
    second_edge: bool = False,
) -> Path:
    """Write a miniature workspace the guard can read, and lock it offline."""
    (root / "Cargo.toml").write_text(
        "[workspace]\n"
        'members = ["crates/blastit", "crates/blastit-lsp", "crates/blastit-manifest"]\n'
        'resolver = "2"\n',
        encoding="utf-8",
    )
    crate = root / "crates/blastit-lsp"
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text(
        "[package]\n"
        'name = "blastit-lsp"\n'
        'version = "0.2.0"\n'
        'edition = "2021"\n'
        "\n"
        "[lib]\n"
        'name = "blastit_lsp"\n'
        f'path = "{lib_path}"\n'
        "\n"
        "[[bin]]\n"
        'name = "blastit-lsp"\n'
        f'path = "{bin_path}"\n'
        "\n"
        f"[dependencies]\n{dependencies}"
        f"{lsp_tables}"
        "\n",
        encoding="utf-8",
    )
    (crate / "src/lib.rs").write_text(lib, encoding="utf-8")
    (crate / "src/main.rs").write_text(main, encoding="utf-8")
    if stray_server:
        (crate.parent / "blastit/src/lsp").mkdir(parents=True)
        (crate.parent / "blastit/src/lsp/mod.rs").write_text(lib, encoding="utf-8")

    manifest = root / "crates/blastit-manifest"
    (manifest / "src").mkdir(parents=True)
    (manifest / "Cargo.toml").write_text(
        "[package]\n"
        'name = "blastit-manifest"\n'
        'version = "0.2.0"\n'
        'edition = "2021"\n'
        "\n"
        f"[dependencies]\n{manifest_dependencies}"
        "\n",
        encoding="utf-8",
    )
    (manifest / "src/lib.rs").write_text(
        "pub fn fold() -> u8 {\n    0\n}\n", encoding="utf-8"
    )

    delegating = root / "crates/blastit"
    (delegating / "src").mkdir(parents=True, exist_ok=True)
    (delegating / "Cargo.toml").write_text(
        "[package]\n"
        'name = "blastit"\n'
        'version = "0.2.0"\n'
        'edition = "2021"\n'
        "\n"
        "[dependencies]\n"
        'blastit-lsp = { version = "0.2.0", path = "../blastit-lsp" }\n'
        + (
            "\n[dev-dependencies]\n"
            'blastit-lsp = { version = "0.2.0", path = "../blastit-lsp" }\n'
            if second_edge
            else ""
        ),
        encoding="utf-8",
    )
    (delegating / "src/lib.rs").write_text(
        "pub use blastit_lsp as lsp;\n" if delegation else "// no delegation here\n",
        encoding="utf-8",
    )
    (root / "Makefile").write_text(
        makefile
        if makefile is not None
        else (REPO / "Makefile").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    subprocess.run(
        ["cargo", "generate-lockfile", "--offline"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return root


def guard(
    root: Path, *, guard_script: Path = GUARD
) -> subprocess.CompletedProcess[str]:
    """The guard as `make check` runs it, against `root`."""
    return subprocess.run(
        [sys.executable, str(guard_script), "--root", str(root)],
        capture_output=True,
        text=True,
        check=False,
    )


def compiles(root: Path, target_dir: Path) -> subprocess.CompletedProcess[str]:
    """`cargo check` one fixture; `serde_json` stays cached while `blastit-lsp` is cleaned."""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
    subprocess.run(
        ["cargo", "clean", "--offline", "-p", "blastit-lsp"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    return subprocess.run(
        ["cargo", "check", "--offline", "-p", "blastit-lsp"],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )


def with_lib_outside(root: Path) -> None:
    """`[lib].path` points outside `src/` at a spawning file while `src/` stays clean."""
    (root / "outside.rs").write_text(
        "#![forbid(unsafe_code)]\n"
        "pub fn run() -> u8 {\n"
        '    let _ = std::process::Command::new("ls");\n'
        "    0\n"
        "}\n",
        encoding="utf-8",
    )
    build(root, lib_path="../../outside.rs")


def with_bin_outside(root: Path) -> None:
    """`[[bin]].path` points outside `src/` at a spawning file while `src/` stays clean."""
    (root / "outside_bin.rs").write_text(
        "#![forbid(unsafe_code)]\n"
        "fn main() {\n"
        '    let _ = std::process::Command::new("ls");\n'
        "    std::process::exit(blastit_lsp::run() as i32)\n"
        "}\n",
        encoding="utf-8",
    )
    build(root, bin_path="../../outside_bin.rs")


def with_path_attribute(
    root: Path, *, split: bool = False, inner: bool = False
) -> None:
    """A fixture whose `#[path]` / `#![path]` module lives outside `src/` and spawns."""
    (root / "outside.rs").write_text(
        'pub fn spawn() {\n    let _ = std::process::Command::new("ls");\n}\n',
        encoding="utf-8",
    )
    if inner:
        body = f'\npub mod outside {{\n    #!\n    [path = "{root}/outside.rs"]\n}}\n'
    elif split:
        body = f'\n#\n[\npath\n=\n"{root}/outside.rs"\n]\npub mod outside;\n'
    else:
        body = f'\n#[path = "{root}/outside.rs"]\npub mod outside;\n'
    build(root, lib=LIB + body)


def with_include(root: Path, *, split: bool = False) -> None:
    """A fixture that compiles another file's text in with `include!`."""
    call = (
        'include\n!\n("../../../outside.rs");'
        if split
        else 'include!("../../../outside.rs");'
    )
    (root / "outside.rs").write_text(
        'pub fn spawn() {\n    let _ = std::process::Command::new("ls");\n}\n',
        encoding="utf-8",
    )
    build(root, lib=LIB + f"\n{call}\n")


def with_symlink_source(root: Path) -> None:
    """A symlink inside `src/` pointing at an un-scanned file outside `src/`."""
    (root / "outside.rs").write_text(
        'pub fn spawn() {\n    let _ = std::process::Command::new("ls");\n}\n',
        encoding="utf-8",
    )
    build(root, lib=LIB + "\npub mod sneaky;\n")
    (root / "crates/blastit-lsp/src/sneaky.rs").symlink_to(root / "outside.rs")


def with_symlink_dir(root: Path) -> None:
    """A symlinked directory `src/sneaky -> ../vendor_mod` holding an un-scanned `mod.rs`."""
    build(
        root,
        lib=LIB
        + "\npub mod sneaky;\npub fn call_sneaky() {\n    sneaky::spawn();\n}\n",
    )
    vendor = root / "crates/blastit-lsp/vendor_mod"
    vendor.mkdir(parents=True)
    (vendor / "mod.rs").write_text(
        'pub fn spawn() {\n    let _ = std::process::Command::new("/bin/true");\n}\n',
        encoding="utf-8",
    )
    (root / "crates/blastit-lsp/src/sneaky").symlink_to("../vendor_mod")


def properties(report: str) -> set[str]:
    """The property names a red run refused on."""
    return set(_PROPERTY.findall(report))


def wiring_makefile(
    suite: Suite, *, check_token: bool = True, twins: bool = True, phony: bool = True
) -> str:
    """This repository's `Makefile`, with one wiring fact removed."""
    text = (REPO / "Makefile").read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    if not check_token:
        for index, line in enumerate(lines):
            if line.startswith("check:"):
                suite.expect(
                    "the real `check:` line must name `lsp-portable` for this case to "
                    "mutate anything",
                    " lsp-portable" in line,
                )
                lines[index] = line.replace(" lsp-portable", "", 1)
                break
    if not twins:
        lines = [line for line in lines if "test_check_lsp_portable.py" not in line]
    if not phony:
        for index, line in enumerate(lines):
            if not line.startswith(".PHONY:"):
                continue
            block = []
            cursor = index
            while cursor < len(lines):
                block.append(cursor)
                if not lines[cursor].rstrip().endswith("\\"):
                    break
                cursor += 1
            suite.expect(
                "the real `.PHONY` list must name `lsp-portable` for this case to "
                "mutate anything",
                "lsp-portable" in "".join(lines[position] for position in block),
            )
            for position in block:
                lines[position] = lines[position].replace(" lsp-portable", "", 1)
            break
    return "".join(lines)


def escape_case(body: str) -> Callable[[Path], Path]:
    """Build a fixture tree with `body` appended to `LIB`."""

    def writer(root: Path) -> Path:
        return build(root, lib=LIB + "\n" + body)

    return writer


def mutated_allowlist_guard(suite: Suite, scratch: Path) -> Path:
    """A temp copy of `check_lsp_portable.py` with `std::env::var` injected into `STD_ALLOWLIST`."""
    source = GUARD.read_text(encoding="utf-8")
    anchor = "STD_ALLOWLIST = {\n"
    suite.expect(
        "the guard must declare `STD_ALLOWLIST = {` for the allowlist-env twin to mutate it",
        anchor in source,
    )
    copy_path = scratch / "guard_with_env_allowlisted.py"
    copy_path.write_text(
        source.replace(anchor, anchor + '    "std::env::var": "smuggled",\n', 1),
        encoding="utf-8",
    )
    return copy_path


def main() -> int:
    """Runs every case; returns the process exit status."""
    suite = Suite()
    scratch = Path(tempfile.mkdtemp(prefix="lsp-portable-"))
    try:
        cases = {
            "clean": lambda root: build(root),
            "third-dependency": lambda root: build(
                root, dependencies='rustix = "1"\n', lsp_tables=""
            ),
            "target-table": lambda root: build(
                root,
                lsp_tables="\n[target.'cfg(windows)'.dependencies]\nlibc = \"0.2\"\n",
            ),
            "build-script": lambda root: (
                build(root),
                (root / "crates/blastit-lsp/build.rs").write_text(
                    "fn main() {}\n", encoding="utf-8"
                ),
            ),
            "lib-path-outside": with_lib_outside,
            "bin-path-outside": with_bin_outside,
            "main-not-pinned": lambda root: build(
                root,
                main=MAIN
                + '\nfn extra() {\n    let _ = std::process::Command::new("ls");\n}\n',
            ),
            "no-forbid": lambda root: build(
                root, lib=LIB.replace("\n#![forbid(unsafe_code)]\n", "\n")
            ),
            "submodule-forbid-only": lambda root: build(
                root,
                lib=LIB.replace(
                    "\n#![forbid(unsafe_code)]\n",
                    "\npub mod sub {\n    #![forbid(unsafe_code)]\n}\n",
                ),
            ),
            "environment": lambda root: build(
                root,
                lib=LIB + '\npub fn home() {\n    let _ = std::env::var("HOME");\n}\n',
            ),
            "split-extern-crate-env": lambda root: build(
                root,
                lib=LIB
                + "\nextern\ncrate std as s;\n\n"
                + 'pub fn home() {\n    let _ = s::env::var("HOME");\n}\n',
            ),
            "transitive-privilege": lambda root: build(
                root, manifest_dependencies='rustix = "1"\n'
            ),
            **{
                name: escape_case(body)
                for name, (body, _label) in SOURCE_ESCAPES.items()
            },
            "include-macro": lambda root: with_include(root, split=False),
            "split-include-macro": lambda root: with_include(root, split=True),
            "path-attribute": lambda root: with_path_attribute(root, split=False),
            "split-path-attribute": lambda root: with_path_attribute(root, split=True),
            "inner-path-attribute": lambda root: with_path_attribute(root, inner=True),
            "mid-ident-split": lambda root: build(
                root,
                lib=LIB + "\npub fn broken() {\n    let _ = rus\n    tix;\n}\n",
            ),
            "symlink-source": with_symlink_source,
            "symlink-dir": with_symlink_dir,
            "stray-server": lambda root: build(root, stray_server=True),
            "delegation-dropped": lambda root: build(root, delegation=False),
            "second-edge": lambda root: build(root, second_edge=True),
            "wiring-check-dropped": lambda root: build(
                root, makefile=wiring_makefile(suite, check_token=False)
            ),
            "wiring-twins-dropped": lambda root: build(
                root, makefile=wiring_makefile(suite, twins=False)
            ),
            "phony-dropped": lambda root: build(
                root, makefile=wiring_makefile(suite, phony=False)
            ),
            "no-source": lambda root: (
                build(root),
                shutil.rmtree(root / "crates/blastit-lsp/src"),
            ),
            "missing-manifest": lambda root: (
                build(root),
                (root / "crates/blastit-lsp/Cargo.toml").unlink(),
            ),
            "missing-dependency": lambda root: build(
                root,
                dependencies=(
                    'blastit-manifest = { version = "0.2.0", path = "../blastit-manifest" }\n'
                ),
                lib=LIB + "\npub fn json() {\n    let _ = serde_json::json!({});\n}\n",
            ),
        }
        trees: dict[str, Path] = {}
        for name, write in cases.items():
            root = scratch / name
            root.mkdir()
            write(root)
            trees[name] = root

        done = guard(trees["clean"])
        suite.expect(
            f"a clean fixture must be green, got {done.returncode}: {done.stderr.strip()}",
            done.returncode == 0 and "lsp-portable: ok" in done.stdout,
        )

        # Self-guard twin: injecting a `std::env` row into `STD_ALLOWLIST` must
        # refuse on `{environment}` even when the scanned tree itself is clean.
        bad_guard = mutated_allowlist_guard(suite, scratch)
        suite.red(
            trees["clean"],
            "a `std::env` row added to `STD_ALLOWLIST`",
            expected={"environment"},
            contains="the allowlist holds `std::env::var`",
            guard_script=bad_guard,
        )

        suite.red(
            trees["third-dependency"],
            "a third dependency",
            expected={"manifest"},
            contains="`rustix`, a capability crate",
        )
        suite.red(
            trees["target-table"],
            "a target-scoped dependency",
            expected={"manifest"},
            contains="[target.'cfg(windows)'.dependencies]",
        )
        suite.red(
            trees["build-script"],
            "a build script",
            expected={"manifest"},
            contains="build script",
        )
        suite.red(
            trees["main-not-pinned"],
            "a `main.rs` that is not the pinned shape",
            expected={"source"},
            contains="pinned three-line shape",
        )
        suite.red(
            trees["no-forbid"],
            "a deleted `forbid(unsafe_code)` (even when quoted in a doc comment)",
            expected={"source"},
            contains="#![forbid(unsafe_code)]",
        )
        suite.red(
            trees["transitive-privilege"],
            "a transitive privilege crate",
            expected={"closure"},
            contains="blastit-lsp -> blastit-manifest -> rustix",
        )
        suite.red(
            trees["stray-server"],
            "a second copy of the server",
            expected={"identity"},
            contains="crates/blastit/src/lsp",
        )

        # Every adversarial escape — manifest target paths outside `src/`, split
        # `extern crate`/`include!`/`#[path]`/`#[cfg]`/`cfg!`, symlinks, aliases,
        # globs, environment reads and the invented raw-identifier spelling — must
        # BOTH compile (`cargo check` exit 0) AND be refused with the exact promise
        # and resolved path/construct.
        shared_target = scratch / "shared-target"
        compiling_refusals: list[tuple[str, set[str], str]] = [
            ("lib-path-outside", {"manifest"}, "sets `path = '../../outside.rs'`"),
            ("bin-path-outside", {"manifest"}, "sets `path = '../../outside_bin.rs'`"),
            ("environment", {"environment"}, "names `std::env::var`"),
            (
                "split-extern-crate-env",
                {"source", "environment"},
                "names `std::env::var`",
            ),
            *[
                (name, {"source"}, label)
                for name, (_body, label) in SOURCE_ESCAPES.items()
            ],
            ("include-macro", {"source"}, "contains `include!`"),
            ("split-include-macro", {"source"}, "contains `include!`"),
            ("path-attribute", {"source"}, "contains `#[path`"),
            ("split-path-attribute", {"source"}, "contains `#[path`"),
            ("inner-path-attribute", {"source"}, "contains `#[path`"),
            ("symlink-source", {"source"}, "symlinked files and directories"),
            ("symlink-dir", {"source"}, "symlinked files and directories"),
            ("no-forbid", {"source"}, "#![forbid(unsafe_code)]"),
            ("submodule-forbid-only", {"source"}, "#![forbid(unsafe_code)]"),
        ]
        for name, expected_props, label in compiling_refusals:
            built = compiles(trees[name], shared_target)
            suite.expect(
                f"the `{name}` fixture must be one `cargo check` accepts — an escape "
                f"only matters when it compiles — got {built.returncode}: "
                f"{(built.stderr.strip().splitlines() or [''])[-1]}",
                built.returncode == 0,
            )
            suite.red(
                trees[name],
                f"the `{name}` escape",
                expected=expected_props,
                contains=label,
            )
        suite.red(
            trees["split-extern-crate-spawn"],
            "the `split-extern-crate-spawn` keyword refusal",
            expected={"source"},
            contains="contains `extern crate`",
        )
        mid_ident = compiles(trees["mid-ident-split"], shared_target)
        suite.expect(
            r"splitting a single identifier across a newline (`rus\ntix`) does not "
            f"compile in Rust — got {mid_ident.returncode}",
            mid_ident.returncode != 0,
        )

        suite.red(
            trees["delegation-dropped"],
            "a `lib.rs` that stopped delegating",
            expected={"identity"},
            contains="pub use blastit_lsp as lsp;",
        )
        suite.red(
            trees["second-edge"],
            "a second `blastit-lsp` edge",
            expected={"identity"},
            contains="dev-dependencies",
        )
        suite.red(
            trees["wiring-check-dropped"],
            "a `lsp-portable` outside `make check`",
            expected={"wiring"},
            contains="does not list `lsp-portable`",
        )
        suite.red(
            trees["wiring-twins-dropped"],
            "a guard with no negative twins",
            expected={"wiring"},
            contains="test_check_lsp_portable.py",
        )
        suite.red(
            trees["phony-dropped"],
            "a `lsp-portable` missing from `.PHONY`",
            expected={"wiring"},
            contains="`.PHONY`",
        )
        suite.red(
            trees["no-source"],
            "a tree with no source",
            expected={"source"},
            contains="holds no Rust source",
        )
        suite.red(
            trees["missing-manifest"],
            "an unreadable manifest",
            expected={"manifest"},
            contains="cannot parse the crate manifest",
        )

        # The boundary: deleting a dependency the code still uses is the
        # compiler's red, not this guard's (the ceiling is the guard's; the floor
        # is `cargo`'s).
        done = guard(trees["missing-dependency"])
        suite.expect(
            "the guard must stay green when a dependency the code needs is gone "
            f"(it is not a compiler), got {done.returncode}: {done.stderr.strip()}",
            done.returncode == 0,
        )
        checked = compiles(trees["missing-dependency"], shared_target)
        suite.expect(
            "`cargo check` must be the thing that catches the missing dependency: "
            f"expected E0433, got {checked.returncode}: {checked.stderr.strip()[:400]}",
            checked.returncode != 0 and "E0433" in checked.stderr,
        )

        # The real tree, which is the actual acceptance.
        done = guard(REPO)
        suite.expect(
            f"this checkout must be green, got {done.returncode}: {done.stderr.strip()}",
            done.returncode == 0 and "lsp-portable: ok" in done.stdout,
        )
        suite.expect(
            "every built tree must be counted in the fixture banner",
            len(trees) == len(cases),
        )
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    print(
        f"lsp-portable self-test: ok — {suite.checks} assertions over {len(trees)} "
        f"fixtures ({len(compiling_refusals)} cargo-checked)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
