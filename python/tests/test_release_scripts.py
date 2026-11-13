"""Regression tests for the release scripts' hygiene.

Three release-hygiene behaviours were fixed by hand in `MB.1.7` and had no test,
so any later edit could undo them silently (`docs/testing/AUDIT-MB1.md` `G6`):

* `scripts/build-wheel.sh` rewrites the README to absolute links for the duration
  of the build and puts it back on **every** exit path — a dirty tree after
  `make check` (which reaches this script through `docs-test`) is the bug, and a
  wheel whose long description links 404 on PyPI is the other half;
* every `CARGO_NET_OFFLINE=` in a path a maturin run can inherit is the string
  `true`/`false`; cargo also accepts `1`, maturin >= 1.16 does not, so the old
  spelling only fails once the build has already succeeded;
* `crates/blastit-events` no longer depends on `serde` (it only ever used
  `serde_json`), and a re-add is invisible to clippy and `--locked`.

Plus `scripts/dev-version.sh --check`: a read-only mode that names a lagging
intra-workspace requirement as `file:line` without rewriting a manifest, running
`cargo metadata`, or needing a `Cargo.lock`.

Everything here is offline and sub-second except the script runs, which write a
README or a manifest tree into `tmp_path`; nothing builds a Rust target, and the
wheel assertion only reads a wheel that a previous `make docs-test` left in
`dist/` (with `BLASTIT_REQUIRE_WHEEL=1` it refuses to run without one).
"""

from __future__ import annotations

import email
import os
import re
import shutil
import subprocess
import tomllib
import zipfile
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
SCRIPTS = REPO / "scripts"
BUILD_WHEEL = SCRIPTS / "build-wheel.sh"
DEV_VERSION = SCRIPTS / "dev-version.sh"
DIST = REPO / "dist"

#: Where `CARGO_NET_OFFLINE` can reach a maturin run: the gate, the scripts it
#: calls, CI, and the e2e harness that also builds a wheel.
OFFLINE_SCOPE = ("Makefile", "scripts", ".github", "e2e")
OFFLINE = re.compile(r"CARGO_NET_OFFLINE=(?P<value>\S*)")
STRICT_BOOL = frozenset({"true", "false"})

#: A Markdown/HTML target that is already absolute, or an in-page fragment.
ABSOLUTE = re.compile(r"^(?:[a-zA-Z][a-zA-Z0-9+.-]*:|//|/|#|$)")
LINK = re.compile(
    r"\]\((?P<markdown>[^)\s]+)\)"
    r"""|(?:src|href)=["'](?P<attribute>[^"'\s>]+)"""
    r"""|srcset=["'](?P<srcset>[^"']*)"""
)


# --------------------------------------------------------------------------- #
# CARGO_NET_OFFLINE: only the strict boolean, everywhere a build inherits it
# --------------------------------------------------------------------------- #


def offline_offenders(text: str, name: str) -> list[str]:
    """Every `CARGO_NET_OFFLINE=<value>` in `text` whose value is not true/false.

    A whole-line comment is skipped: it documents the variable, it does not set
    it. A value inside a trailing comment is still reported — cheap, and the fix
    is to stop spelling the old value at all.
    """
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("#"):
            continue
        for match in OFFLINE.finditer(line):
            value = match.group("value").strip("'\"")
            if value not in STRICT_BOOL:
                found.append(f"{name}:{number}: {match.group(0)}")
    return found


def scoped_files() -> list[Path]:
    """Every text file of the gate, the scripts and CI, hidden files included."""
    files = [REPO / "Makefile"]
    for entry in OFFLINE_SCOPE[1:]:
        base = REPO / entry
        if not base.is_dir():
            continue
        files.extend(
            path
            for path in sorted(base.rglob("*"))
            if path.is_file() and "__pycache__" not in path.parts
        )
    return files


def test_the_offline_scan_flags_the_old_spelling():
    """The detector itself: `1` is reported, `true`/`false` and comments are not."""
    assert offline_offenders("CARGO_NET_OFFLINE=true cargo build", "Makefile") == []
    assert offline_offenders("\tCARGO_NET_OFFLINE=false uv run x", "x.sh") == []
    assert offline_offenders("# was CARGO_NET_OFFLINE=1 once", "x.sh") == []
    assert offline_offenders(
        "CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check", "Makefile"
    ) == ["Makefile:1: CARGO_NET_OFFLINE=1"]


def test_no_cargo_net_offline_is_spelled_one():
    """maturin >= 1.16 parses the variable as a strict bool; cargo's `1` breaks it."""
    files = scoped_files()
    assert len(files) > 2, f"the scan found almost nothing under {OFFLINE_SCOPE}"
    offenders = []
    for path in files:
        text = path.read_text(errors="replace")
        offenders.extend(offline_offenders(text, str(path.relative_to(REPO))))
    assert not offenders, (
        "CARGO_NET_OFFLINE must be `true` or `false`, never cargo's `1`: maturin "
        "rejects it (`docs/knowledge/LESSONS.md`).\n" + "\n".join(offenders)
    )


# --------------------------------------------------------------------------- #
# blastit-events: no unused serde dependency
# --------------------------------------------------------------------------- #


def test_blastit_events_does_not_depend_on_serde():
    """`serde_json` is a different crate; `serde` is the one that was unused."""
    manifest = REPO / "crates/blastit-events/Cargo.toml"
    data = tomllib.loads(manifest.read_text())
    assert data["package"]["name"] == "blastit-events"
    dependencies = data.get("dependencies", {})
    assert "serde_json" in dependencies, (
        "blastit-events has no serde_json dependency: this test is parsing the "
        "wrong table and would accept any serde re-add"
    )
    searched = [("dependencies", dependencies)]
    searched += [
        (section, data.get(section, {}))
        for section in ("dev-dependencies", "build-dependencies")
    ]
    for section, table in searched:
        assert "serde" not in table, (
            "crates/blastit-events/Cargo.toml re-gained `serde` under "
            f"[{section}]: nothing in the crate uses `serde::` or its derives, "
            "and only Cargo.lock notices the extra edge"
        )


# --------------------------------------------------------------------------- #
# The wheel's long description: absolute links, or PyPI shows 404s
# --------------------------------------------------------------------------- #


def long_description(wheel: Path) -> str:
    """The `Description` payload of a wheel's `*.dist-info/METADATA`."""
    with zipfile.ZipFile(wheel) as archive:
        metadata = [
            name for name in archive.namelist() if name.endswith(".dist-info/METADATA")
        ]
        assert metadata, f"{wheel.name} has no .dist-info/METADATA"
        message = email.message_from_bytes(archive.read(metadata[0]))
    body = message.get_payload()
    assert isinstance(body, str) and body.strip(), (
        f"{wheel.name}: empty long description"
    )
    return body


def relative_links(text: str) -> list[str]:
    """Every link, image or `srcset` candidate in `text` that is not absolute."""
    targets = []
    for match in LINK.finditer(text):
        if match.group("srcset") is not None:
            candidates = [
                candidate.split()[0]
                for candidate in match.group("srcset").split(",")
                if candidate.split()
            ]
        elif match.group("markdown"):
            candidates = [match.group("markdown")]
        else:
            candidates = [match.group("attribute")]
        targets.extend(target for target in candidates if not ABSOLUTE.match(target))
    return targets


def wheel_problems(wheel: Path) -> list[str]:
    """Why `wheel`'s long description would 404 on PyPI, if it would."""
    body = long_description(wheel)
    problems = []
    if "github.com/OffworldNexus/blastit/blob/" not in body:
        problems.append(
            f"{wheel.name}: no absolute repository link, so the README rewrite "
            "did not run before the build"
        )
    offenders = relative_links(body)
    if offenders:
        problems.append(f"{wheel.name}: relative targets {', '.join(offenders)}")
    return problems


def test_the_relative_link_scan_flags_a_github_relative_target():
    """The detector: the three shapes a wheel's description must never contain."""
    assert relative_links("[`LICENSE`](LICENSE)") == ["LICENSE"]
    assert relative_links(
        "See [`docs`](docs/deploy.md) and ![x](.github/brand/a.svg)"
    ) == [
        "docs/deploy.md",
        ".github/brand/a.svg",
    ]
    assert relative_links('<img srcset=".github/a.svg 1x, .github/b.svg 2x">') == [
        ".github/a.svg",
        ".github/b.svg",
    ]
    absolute = "https://github.com/OffworldNexus/blastit/blob/develop/LICENSE"
    assert relative_links(f"[`LICENSE`]({absolute}) #anchor /site") == []


def write_wheel(path: Path, description: str) -> Path:
    """A wheel whose only payload is a `*.dist-info/METADATA` long description."""
    metadata = "Metadata-Version: 2.1\nName: blastit\nVersion: 0.0.0\n\n" + description
    with zipfile.ZipFile(path, "w") as wheel:
        wheel.writestr("blastit-0.0.0.dist-info/METADATA", metadata)
    return path


def test_the_long_description_check_rejects_a_relative_link(tmp_path):
    """The wheel reader and the check, on wheels this test writes itself."""
    relative = write_wheel(
        tmp_path / "blastit-0.0.0-cp313-cp313-linux_x86_64.whl",
        "See [`docs`](docs/local.md) and [`LICENSE`](LICENSE).",
    )
    assert wheel_problems(relative) == [
        (
            "blastit-0.0.0-cp313-cp313-linux_x86_64.whl: no absolute repository link, "
            "so the README rewrite did not run before the build"
        ),
        "blastit-0.0.0-cp313-cp313-linux_x86_64.whl: relative targets docs/local.md, LICENSE",
    ]
    absolute = write_wheel(
        tmp_path / "blastit-0.0.1-cp313-cp313-linux_x86_64.whl",
        "See [`LICENSE`](https://github.com/OffworldNexus/blastit/blob/develop/LICENSE).",
    )
    assert wheel_problems(absolute) == []


def test_built_wheel_long_description_has_absolute_links():
    """`make docs-test` builds the wheel first; this reads it, it does not build."""
    wheels = sorted(DIST.glob("blastit-*.whl"))
    if not wheels:
        if os.environ.get("BLASTIT_REQUIRE_WHEEL") == "1":
            pytest.fail(
                "no dist/blastit-*.whl for the long-description check: "
                "`make docs-test` builds one with scripts/build-wheel.sh first"
            )
        # Nothing to read yet. The enforcement is `make docs-test`, so this branch
        # checks that wiring rather than passing quietly: if the flag or the node
        # id is dropped from the Makefile, no run would ever exercise the wheel.
        makefile = (REPO / "Makefile").read_text()
        assert (
            "BLASTIT_REQUIRE_WHEEL=1" in makefile
            and "test_built_wheel_long_description_has_absolute_links" in makefile
        ), (
            "nothing enforces this check: `make docs-test` must run "
            "test_built_wheel_long_description_has_absolute_links with "
            "BLASTIT_REQUIRE_WHEEL=1 after scripts/build-wheel.sh"
        )
        pytest.skip(
            "no built wheel in dist/; `make docs-test` builds one and sets BLASTIT_REQUIRE_WHEEL=1"
        )
    problems = [problem for wheel in wheels for problem in wheel_problems(wheel)]
    assert not problems, (
        "PyPI renders the long description with no repository around it, so these "
        "targets 404:\n" + "\n".join(problems)
    )


# --------------------------------------------------------------------------- #
# build-wheel.sh: rewrite for the build, restore on every exit
# --------------------------------------------------------------------------- #


def executable(path: Path, body: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("#!/bin/sh\n" + body + "\n")
    path.chmod(0o755)
    return path


#: A README shaped like the real one, but owned by the test: `scripts/build-wheel.sh`
#: rewrites the *working tree's* `README.md` in place for the duration of a build,
#: so copying the live file would race a concurrent `make docs-test`/`make e2e` and
#: read the rewritten copy. The real README is covered by the wheel-metadata test.
README = """# blastit

![header](.github/brand/header-light.svg)

See [`ARCHITECTURE.md`](ARCHITECTURE.md#82-configuration-reference) and
[`docs`](docs/).

Apache 2.0 — see [`LICENSE`](LICENSE).
"""


def wheel_tree(tmp_path: Path) -> Path:
    """A throwaway copy of the script, its rewriter and a README to rewrite.

    `cargo` and `install` are stubs: this test is about the README, and a real
    `cargo build --release` would turn a sub-second check into a rebuild.
    """
    tree = tmp_path / "tree"
    (tree / "scripts").mkdir(parents=True)
    (tree / "python/blastit/_bin").mkdir(parents=True)
    (tree / "README.md").write_text(README)
    shutil.copy2(BUILD_WHEEL, tree / "scripts/build-wheel.sh")
    shutil.copy2(
        SCRIPTS / "readme-absolute-links.py", tree / "scripts/readme-absolute-links.py"
    )
    stubs = tmp_path / "stubs"
    for tool in ("cargo", "install"):
        executable(stubs / tool, "exit 0")
    (tmp_path / "scratch").mkdir()
    return tree


def run_wheel(
    tree: Path, tmp_path: Path, maturin: Path
) -> subprocess.CompletedProcess[str]:
    """`scripts/build-wheel.sh` in `tree`, with the build tools stubbed out."""
    environment = dict(os.environ)
    for key in [key for key in environment if key.startswith("GITHUB_")]:
        del environment[key]
    environment.update(
        {
            "PATH": f"{tmp_path / 'stubs'}{os.pathsep}{environment['PATH']}",
            "TMPDIR": str(tmp_path / "scratch"),
            "MATURIN": str(maturin),
            "UV_OFFLINE": "1",
            "PROBE_SNAPSHOT": str(tmp_path / "snapshot.md"),
        }
    )
    return subprocess.run(
        ["bash", "scripts/build-wheel.sh"],
        cwd=tree,
        env=environment,
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )


def test_a_failing_maturin_restores_the_readme_and_leaves_no_temp_file(tmp_path):
    """The `trap` fires on the failing path too — the one `make check` takes."""
    tree = wheel_tree(tmp_path)
    original = (tree / "README.md").read_bytes()
    assert relative_links(original.decode()) == [
        ".github/brand/header-light.svg",
        "ARCHITECTURE.md#82-configuration-reference",
        "docs/",
        "LICENSE",
    ], "the fixture README must give the rewriter something to do"

    result = run_wheel(tree, tmp_path, Path("/bin/false"))

    assert result.returncode != 0, (
        f"a failing maturin must fail the script:\n{result.stdout}"
    )
    assert (tree / "README.md").read_bytes() == original, (
        "the README is still rewritten after a failed build: `make check` would "
        f"end on a dirty tree\n{result.stdout}{result.stderr}"
    )
    leftovers = sorted((tmp_path / "scratch").glob("blastit-README.*"))
    assert not leftovers, f"the README backup was not removed: {leftovers}"


def test_the_readme_is_rewritten_while_the_wheel_is_built(tmp_path):
    """Restoring is only half: the wheel must be built from the absolute README."""
    probe = executable(
        tmp_path / "probe-maturin",
        'cp README.md "$PROBE_SNAPSHOT"\nexit 1',
    )
    tree = wheel_tree(tmp_path)
    original = (tree / "README.md").read_bytes()

    result = run_wheel(tree, tmp_path, probe)

    assert result.returncode != 0
    snapshot = (tmp_path / "snapshot.md").read_text()
    assert "https://github.com/OffworldNexus/blastit/blob/develop/" in snapshot, (
        "maturin saw the README without absolute links: the rewrite did not run "
        f"before the build\n{result.stdout}{result.stderr}"
    )
    assert relative_links(snapshot) == [], (
        "maturin saw a relative link, so PyPI will 404: "
        + ", ".join(relative_links(snapshot))
    )
    assert (tree / "README.md").read_bytes() == original


def test_a_successful_build_still_restores_the_readme(tmp_path):
    """The green path of `docs-test`, with a wheel the dbproxy check accepts."""
    wheel = tmp_path / "fake-maturin"
    executable(
        wheel,
        "out=\n"
        "while [ $# -gt 0 ]; do\n"
        "    case $1 in\n"
        "        --out) out=$2; shift 2 ;;\n"
        "        *) shift ;;\n"
        "    esac\n"
        "done\n"
        'mkdir -p "$out"\n'
        'printf blastit/_bin/blastit-dbproxy > "$out/blastit-0.0.0-cp313-cp313-linux_x86_64.whl"',
    )
    tree = wheel_tree(tmp_path)
    original = (tree / "README.md").read_bytes()

    result = run_wheel(tree, tmp_path, wheel)

    assert result.returncode == 0, (
        f"the stub build should succeed:\n{result.stdout}{result.stderr}"
    )
    assert (tree / "README.md").read_bytes() == original


# --------------------------------------------------------------------------- #
# dev-version.sh: the lagging-requirement guard, read-only, no cargo
# --------------------------------------------------------------------------- #


WORKSPACE = """[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.2.0"
edition = "2024"
"""

BETA = """[package]
name = "beta"
version.workspace = true
"""

ALPHA = """[package]
name = "alpha"
version.workspace = true

[dependencies]
beta = <requirement>
"""

LOCKFILE = """# This file is automatically @generated by Cargo.
version = 4

[[package]]
name = "alpha"
version = "0.2.0"

[[package]]
name = "beta"
version = "0.2.0"

[[package]]
name = "serde"
version = "1.0.0"
"""


def dev_version_tree(
    tmp_path: Path, *, requirement: str | None, lock: bool = True
) -> Path:
    """A workspace-shaped tree the real script is run against.

    Only the files the script reads exist: the workspace manifest, one member per
    directory, a `path` requirement on a sibling, and a lock that records both
    members. `lock=False` is a tree `--check` must still accept.
    """
    tree = tmp_path / "tree"
    (tree / "scripts").mkdir(parents=True)
    (tree / "crates/alpha").mkdir(parents=True)
    (tree / "crates/beta").mkdir(parents=True)
    shutil.copy2(DEV_VERSION, tree / "scripts/dev-version.sh")
    (tree / "Cargo.toml").write_text(WORKSPACE)
    (tree / "crates/beta/Cargo.toml").write_text(BETA)
    spelled = (
        f'version = "{requirement}", path = "../beta"'
        if requirement
        else 'path = "../beta"'
    )
    (tree / "crates/alpha/Cargo.toml").write_text(
        ALPHA.replace("<requirement>", spelled)
    )
    if lock:
        (tree / "Cargo.lock").write_text(LOCKFILE)
    return tree


def run_dev_version(
    tree: Path, argument: str, cargo: Path | None = None
) -> subprocess.CompletedProcess[str]:
    """`scripts/dev-version.sh <argument>` in `tree`, with `cargo` overridable."""
    environment = dict(os.environ)
    if cargo is not None:
        environment["PATH"] = f"{cargo.parent}{os.pathsep}{environment['PATH']}"
        environment["CARGO_SENTINEL"] = str(cargo.parent / "cargo-ran")
    return subprocess.run(
        ["bash", "scripts/dev-version.sh", argument],
        cwd=tree,
        env=environment,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )


def requirement_line(tree: Path) -> int:
    """The 1-based line of the `path` requirement in alpha's manifest."""
    manifest = (tree / "crates/alpha/Cargo.toml").read_text().splitlines()
    lines = [
        number for number, line in enumerate(manifest, 1) if 'path = "../beta"' in line
    ]
    assert len(lines) == 1, f"the fixture has {len(lines)} path requirements"
    return lines[0]


def test_check_names_a_lagging_requirement_and_changes_nothing(tmp_path):
    """`--check` reports `file:line`; it never rewrites the tree it is reading."""
    tree = dev_version_tree(tmp_path, requirement="0.1.0", lock=False)
    manifest = tree / "crates/alpha/Cargo.toml"
    before = manifest.read_bytes()
    cargo = executable(
        tmp_path / "stubs/cargo", 'echo "cargo ran" > "$CARGO_SENTINEL"\nexit 1'
    )

    result = run_dev_version(tree, "--check", cargo)

    assert result.returncode == 1, (
        f"--check must fail on a lagging requirement:\n{result.stdout}"
    )
    expected = f"crates/alpha/Cargo.toml:{requirement_line(tree)}:"
    assert expected in result.stderr, (
        f"the guard must name {expected}; it printed:\n{result.stderr}"
    )
    assert 'version = "0.1.0", path' in result.stderr
    assert "0.2.0" in result.stderr, "the message must name the version it expects"
    assert manifest.read_bytes() == before, (
        "--check rewrote the manifest it was checking"
    )
    assert not (cargo.parent / "cargo-ran").exists(), "--check ran cargo metadata"


def test_check_accepts_a_consistent_tree_without_cargo_or_a_lock(tmp_path):
    """The allowed path: consistent requirements, no `Cargo.lock`, no cargo at all."""
    tree = dev_version_tree(tmp_path, requirement="0.2.0", lock=False)
    cargo = executable(
        tmp_path / "stubs/cargo", 'echo "cargo ran" > "$CARGO_SENTINEL"\nexit 1'
    )

    result = run_dev_version(tree, "--check", cargo)

    assert result.returncode == 0, (
        f"a consistent tree must pass:\n{result.stdout}{result.stderr}"
    )
    assert "0.2.0" in result.stdout
    assert not (cargo.parent / "cargo-ran").exists(), "--check ran cargo metadata"


def test_check_flags_a_requirement_with_no_version_at_all(tmp_path):
    """A `path` requirement with no version never accepts a pre-release either."""
    tree = dev_version_tree(tmp_path, requirement=None, lock=False)

    result = run_dev_version(tree, "--check")

    assert result.returncode == 1
    assert f"crates/alpha/Cargo.toml:{requirement_line(tree)}:" in result.stderr


def test_rewrite_still_moves_the_version_its_requirements_and_the_lock(tmp_path):
    """The rewrite path is unchanged: all three files move together, lagging too."""
    tree = dev_version_tree(tmp_path, requirement="0.1.0")
    cargo = executable(tmp_path / "stubs/cargo", "exit 0")

    result = run_dev_version(tree, "42", cargo)

    assert result.returncode == 0, (
        f"the rewrite must succeed:\n{result.stdout}{result.stderr}"
    )
    assert result.stdout.strip() == "0.2.0-dev42"
    assert 'version = "0.2.0-dev42"' in (tree / "Cargo.toml").read_text()
    assert (
        'version = "0.2.0-dev42", path = "../beta"'
        in (tree / "crates/alpha/Cargo.toml").read_text()
    ), "a lagging requirement must be rewritten too"
    lock = (tree / "Cargo.lock").read_text()
    assert 'version = "0.2.0-dev42"' in lock
    assert 'version = "1.0.0"' in lock, "an unrelated lock entry must not move"
