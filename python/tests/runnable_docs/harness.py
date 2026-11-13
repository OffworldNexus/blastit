"""The runnable-docs harness: execute the public console fences for real.

One wheel build and one throwaway virtualenv for the whole run, then each fence's
own commands run in a fresh project directory with a clean environment (the
documented one: no `BLASTIT_*` knob is injected). `blastit dev`, `blastit dev
--learn`, `blastit check` and `--learn` run against a deterministic in-process
PostgreSQL wire stub (or `BLASTIT_TEST_PG_URL` when a real server is supplied);
deployment recipes are statically checked, and their executables, script paths
and flags are checked against the installed CLI's own `--help`.

Nothing here can turn a fence into a pass without running it: a fence whose
inventory entry says `example`, `static` or `install-external` carries a written
reason and is counted as accounted, never as executed, and a substituted command
(`pgstub`) says so on its own line. A command that fails reports the command,
the expected lines and the output it got. A fence whose command cannot run here
because the host, the product or the page disagrees with the documented contract
fails: every promise a page makes (a denial, a learned query) is asserted
directly, so a regression goes red at the fence that lost it.

The wheel is built once and installed through a fresh per-run `UV_CACHE_DIR`, so
a rebuilt same-version wheel can never be served from the global cache: the
installed package is then compared, file by file, with the wheel that was built.

**Offline and HOME contract.** Nothing here reaches the network. Every `uv`
invocation runs with `UV_OFFLINE=1` (and `UV_PYTHON_DOWNLOADS=never` for the
fences), the wheel comes from `dist/`, and the one pin the gate needs
(`pytest==9.2.0`, for the `blastit check` fences) is read from the *caller's* uv
cache. That cache is the only thing this gate asks of the machine it runs on: fill
it once with `python python/tests/runnable_docs/harness.py --warm-cache`, or point
`BLASTIT_DOCS_UV_CACHE` at a cache that already holds the pin. A missing entry
fails in the first second, before the wheel is built, with the command that fixes
it — see `docs/testing/README.md`. `HOME` is never read otherwise, and a CPython
the uv cache does not manage is found on `PATH` instead.

**Wheel provenance.** The wheel under test is `dist/blastit-<version>-*.whl` where
`<version>` is `[workspace.package] version` in `Cargo.toml`, which must equal the
newest release `CHANGELOG.md` announces. Two wheels for the same version and the
host's interpreter is a failure, not a coin toss on mtime.

**Every mode is honest about what it ran.** A mode that executes a fence's
commands raises `Failure` when the fence shows lines but no `$ ` prompt, so a
fence that lost its prompts can never report a pass. An accounted mode must carry
its written reason, and the `static` mode parses what a reader would copy: every
`yaml`, `ini`, `toml` and `sql` block the page shows, every command's executable,
subcommand, flags and file paths, and every JSON line in the fence.

**The flag tables are the tools' own.** A `static` fence passes only if the flag
it shows is one the real command accepts, so the tables above are checked against
the installed binaries rather than maintained by hand: `assert_flag_tables()` runs
before anything is built (`psql --help` is complete, so it is checked on every
run) and `harness.py --audit-flags` checks every table, accepting a flag `--help`
does not print only when the tool itself parses it. `psql -C` — in this table,
never in psql — is the bug that check exists for.
"""

from __future__ import annotations

import configparser
import hashlib
import json
import os
import queue
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import urllib.request
import zipfile
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path

from fences import Command, Fence, console_fences, scan_page
from pgstub import PgStub

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
FIXTURES = HERE / "fixtures"
DIST = REPO / "dist"
PYTEST_VERSION = "9.2.0"
# The environment variable that points the gate at a uv cache which already holds
# `pytest==9.2.0` (a shared read-only cache in CI), instead of the caller's HOME.
CACHE_OVERRIDE = "BLASTIT_DOCS_UV_CACHE"

EXECUTING = {
    "install",
    "dev",
    "dev-curl",
    "summary",
    "page-manifest",
    "pgstub",
    "learn-git",
    "dev-learn",
    "denial",
    "check",
    "findings",
    "json",
    "learn-sequence",
    "learn-then-check",
    "dev-flags",
    "check-flags",
    "serve-flags",
    "usage",
    "presets",
}
ACCOUNTED = {"static", "example", "install-external"}
# Modes that run something but honestly report `executed=False`: the fence's own
# command is substituted (Docker for the in-process PostgreSQL stub), so the
# fence is accounted, never counted as executed.
SUBSTITUTED = {"pgstub"}
# A mode whose fence is the *expected output*, not a command: `denial` starts the
# server itself and asserts the `sql.denied` line the page prints. Such a fence
# has no `$ ` prompt on purpose, so the empty-command rule below lets it through
# and `mode_denial` requires a non-blank body of its own.
PROSE_MODES = {"denial"}
# A mode that can return "not executed" must say why in the inventory.
NEEDS_REASON = ACCOUNTED | SUBSTITUTED
MODES = EXECUTING | ACCOUNTED
# The five fields `docs/reference/log-events.md` documents for a boot line.
BOOT_KEYS = ("event", "stage", "seccomp", "privilege", "bounding_set")


DEV_TIMEOUT = 30.0
FENCE_TIMEOUT = 180.0
SUCCESS, FINDINGS, START_FAILURE, SUITE_FAILURE = 0, 1, 2, 3


_STUB_DETAIL = (
    "substituted: `docker` is not run; the harness's PostgreSQL wire stub "
    "answered StartupMessage and SELECT 1 instead"
)


class Failure(Exception):
    """One fence does not match what the page promises."""


@dataclass(frozen=True, slots=True)
class FenceSpec:
    index: int
    mode: str
    fixture: str | None = None
    reason: str | None = None
    port_fallback: str | None = None


@dataclass(frozen=True, slots=True)
class PageSpec:
    path: str
    fences: tuple[FenceSpec, ...]


@dataclass
class Outcome:
    page: str
    index: int
    mode: str
    ok: bool
    executed: bool
    detail: str = ""
    seconds: float = 0.0
    transcript: str = ""


@dataclass
class Report:
    outcomes: list[Outcome] = field(default_factory=list)

    def add(self, outcome: Outcome) -> None:
        self.outcomes.append(outcome)


def load_inventory(inventory: Path) -> list[PageSpec]:
    document = tomllib.loads(inventory.read_text(encoding="utf-8"))
    pages = []
    for page in document.get("page", []):
        fences = []
        for entry in page.get("fence", []):
            fences.append(
                FenceSpec(
                    index=int(entry["index"]),
                    mode=str(entry["mode"]),
                    fixture=entry.get("fixture"),
                    reason=entry.get("reason"),
                    port_fallback=entry.get("port_fallback"),
                )
            )
        pages.append(PageSpec(str(page["path"]), tuple(fences)))
    return pages


def _require_reason(spec: FenceSpec, where: str) -> str:
    """An accounted mode must say, in writing, why it runs nothing."""
    reason = (spec.reason or "").strip()
    if not reason:
        raise Failure(
            f"{where}: `{spec.mode}` does not run the fence's command, so the "
            "inventory entry must carry a written `reason` "
            "(docs/testing/README.md)"
        )
    return reason


def _require_commands(fence: Fence, spec: FenceSpec) -> None:
    """No mode may report a pass for a fence it read nothing from.

    A `console` fence whose `$ ` prompts were lost yields zero commands while
    still showing output lines: an executing mode then asserted nothing and
    reported success. The complement matters too — a mode that substitutes or
    accounts for the command says so in the inventory, and the modes that only
    read the body (`static`, `example`) may accept one without a command.
    """
    where = f"{fence.page} fence {fence.index}"
    if spec.mode in ACCOUNTED:
        _require_reason(spec, where)
        return
    if spec.mode in SUBSTITUTED:
        _require_reason(spec, where)
    shown = [line for line in fence.body.splitlines() if line.strip()]
    if not shown:
        raise Failure(
            f"{where}: `{spec.mode}` runs something, but the fence shows nothing"
        )
    if spec.mode in PROSE_MODES:
        # The body *is* the assertion here (`mode_denial` matches the printed
        # line), so a fence without `$ ` prompts is the documented shape.
        return
    if not fence.commands():
        raise Failure(
            f"{where}: `{spec.mode}` runs the fence's commands, but none of its "
            f"{len(shown)} non-blank lines starts with `$ `, so this mode would "
            "run nothing and report a pass. Restore the `$ ` prompt, or declare "
            "the fence `example`/`static` with a written reason"
        )


def _version_of(path: Path, key: tuple[str, ...]) -> str:
    try:
        document = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise Failure(f"{path} cannot be read for its version: {error}") from error
    value: object = document
    for part in key:
        if not isinstance(value, dict) or part not in value:
            raise Failure(f"{path} has no {']['.join(key)} version")
        value = value[part]
    if not isinstance(value, str) or not re.fullmatch(r"\d+\.\d+\.\d+", value):
        raise Failure(f"{path} names the version {value!r}, not `X.Y.Z`")
    return value


def planned_version() -> str:
    """`[workspace.package] version`: the version of every artifact of this tree."""
    return _version_of(REPO / "Cargo.toml", ("workspace", "package", "version"))


def documented_version() -> str:
    """The release the user docs claim: the newest `## X.Y.Z` in `CHANGELOG.md`."""
    text = (REPO / "CHANGELOG.md").read_text(encoding="utf-8")
    match = re.search(r"^## (\d+\.\d+\.\d+) ", text, re.MULTILINE)
    if match is None:
        raise Failure(
            "CHANGELOG.md has no `## X.Y.Z (date)` release heading, so the version "
            "the documentation claims cannot be read"
        )
    return match.group(1)


def assert_provenance() -> str:
    """One version for the source tree, the release the docs announce, the wheel."""
    planned = planned_version()
    documented = documented_version()
    if planned != documented:
        raise Failure(
            f"Cargo.toml says the workspace version is {planned}, but the newest "
            f"release CHANGELOG.md announces is {documented}: the wheel this gate "
            f"builds would be {planned} while the docs tell a reader to expect "
            f"{documented}. Bump both (scripts/dev-version.sh) or add the missing "
            "CHANGELOG entry, whichever is true"
        )
    return planned


def _wheel_python(name: str) -> str:
    """`blastit-0.1.0-cp314-…whl` -> `3.14`; the ABI tag names the interpreter."""
    match = re.search(r"-cp3(\d+)-", name)
    if match is None:
        raise Failure(
            f"{name} carries no `cp3NN` ABI tag, so the interpreter to install it "
            "into cannot be chosen"
        )
    return "3." + match.group(1)


HOST_PYTHON = f"{sys.version_info.major}.{sys.version_info.minor}"


def wheel_path() -> Path:
    """The one wheel for the workspace version that this host can install.

    `dist/` is a build directory: it may hold one wheel per CPython minor (CI
    builds 3.13 and 3.14), and it may hold a wheel of an older release. Neither
    the newest mtime nor the alphabet decides what is tested — the workspace
    version does, and two wheels competing for the host's interpreter are a
    failure rather than a guess.
    """
    version = planned_version()
    candidates = sorted(DIST.glob(f"blastit-{version}-*.whl"))
    if not candidates:
        found = sorted(item.name for item in DIST.glob("blastit-*.whl"))
        raise Failure(
            f"{DIST} has no blastit-{version} wheel; it holds "
            f"{', '.join(found) if found else 'no wheel'}. The workspace version is "
            f"{version}, so run scripts/build-wheel.sh (`make docs-test` does)"
        )
    native = [wheel for wheel in candidates if _wheel_python(wheel.name) == HOST_PYTHON]
    if native:
        candidates = native
    if len(candidates) > 1:
        raise Failure(
            f"{DIST} holds {len(candidates)} wheels for version {version}: "
            f"{', '.join(wheel.name for wheel in candidates)}. The harness cannot "
            "tell which one it documents: delete the ones this run did not build "
            "(`dist/` is generated) and re-run"
        )
    return candidates[0]


def interpreter_for(wheel: Path) -> str:
    """The CPython the wheel's ABI tag names, through uv or else `PATH`."""
    wanted = _wheel_python(wheel.name)
    try:
        return subprocess.check_output(
            ["uv", "python", "find", f"=={wanted}"],
            text=True,
            stderr=subprocess.PIPE,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        found = shutil.which(f"python{wanted}")
        if found:
            return found
        raise Failure(
            f"no CPython {wanted} to install {wheel.name}: `uv python find "
            f"=={wanted}` found none and python{wanted} is not on PATH. Install it "
            f"once (`uv python install {wanted}`) — docs/testing/README.md names "
            "every prerequisite of this gate"
        ) from None


def uv_cache_dir() -> str:
    try:
        return subprocess.check_output(
            ["uv", "cache", "dir"], text=True, stderr=subprocess.PIPE
        ).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise Failure(
            f"`uv cache dir` failed ({error}). This gate is offline and reads "
            f"pytest=={PYTEST_VERSION} from the uv cache, so `uv` must be on PATH "
            "— docs/testing/README.md lists the prerequisites"
        ) from error


def warm_pytest_cache() -> str:
    """Fill the uv cache with the one pin the gate needs (this step may use the network)."""
    cache = os.environ.get(CACHE_OVERRIDE) or uv_cache_dir()
    target = Path(tempfile.mkdtemp(prefix="warm-")) / "wheels"
    argv = [
        "uv",
        "pip",
        "install",
        "--python",
        sys.executable,
        "--target",
        str(target),
        f"pytest=={PYTEST_VERSION}",
    ]
    completed = subprocess.run(
        argv,
        env={**os.environ, "UV_CACHE_DIR": cache},
        capture_output=True,
        text=True,
        check=False,
    )
    shutil.rmtree(target.parent, ignore_errors=True)
    if completed.returncode != 0:
        raise Failure(
            f"`{' '.join(argv)}` failed with {completed.returncode}:\n"
            f"{completed.stderr[-2000:]}"
        )
    return f"{cache} now holds pytest=={PYTEST_VERSION}"


class Shell:
    """One persistent bash, so `export` lines persist inside a fence."""

    def __init__(self, cwd: Path, env: dict[str, str]) -> None:
        self.cwd = cwd
        self.env = env
        self._lines: queue.Queue[str] = queue.Queue()
        self._process = subprocess.Popen(
            ["/bin/bash", "--noprofile", "--norc"],
            cwd=str(cwd),
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        self._reader = threading.Thread(target=self._read, daemon=True)
        self._reader.start()

    def _read(self) -> None:
        assert self._process.stdout is not None
        for line in self._process.stdout:
            self._lines.put(line.rstrip("\n"))
        self._lines.put("\0eof")

    def run(
        self, command: str, timeout: float = FENCE_TIMEOUT
    ) -> tuple[int, list[str]]:
        if self._process.poll() is not None:
            raise Failure("the fence's shell exited before the command ran")
        marker = f"__blastit_done_{time.monotonic_ns()}__"
        assert self._process.stdin is not None
        self._process.stdin.write(f"{command}\nprintf '%s %d\\n' '{marker}' \"$?\"\n")
        self._process.stdin.flush()
        lines: list[str] = []
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.close()
                raise Failure(
                    f"`{command}` timed out after {timeout:.0f}s; output so far:\n"
                    + "\n".join(lines[-40:])
                )
            try:
                line = self._lines.get(timeout=remaining)
            except queue.Empty:
                continue
            if line == "\0eof":
                raise Failure(
                    f"`{command}` ended the shell; output so far:\n"
                    + "\n".join(lines[-40:])
                )
            if line.startswith(marker + " "):
                return int(line.split(" ", 1)[1]), lines
            lines.append(line)

    def close(self) -> None:
        if self._process.poll() is None:
            try:
                self._process.stdin.close()  # type: ignore[union-attr]
            except OSError:
                pass
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._kill()
        self._process.wait()
        self._reader.join(timeout=2)

    def _kill(self) -> None:
        try:
            os.killpg(self._process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


@dataclass
class DevProcess:
    process: subprocess.Popen[str]
    stdout: list[str]
    stderr: list[str]
    port: int

    def wait_ready(self, timeout: float = DEV_TIMEOUT) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if any("ready" in line for line in self.stdout):
                return
            if self.process.poll() is not None:
                text = "\n".join(self.stdout[-40:] + self.stderr[-40:])
                hint = ""
                if "BLASTIT_MEM_HIGH_WATERMARK" in text:
                    hint = (
                        "\nthis host is above the `BLASTIT_MEM_HIGH_WATERMARK` "
                        "default, so no view process may start and the documented "
                        "command cannot serve here (product gap, MA.3.7 (a)) until "
                        "the host frees memory"
                    )
                raise Failure(
                    "blastit dev exited before ready; stdout:\n"
                    + "\n".join(self.stdout[-40:])
                    + "\nstderr:\n"
                    + "\n".join(self.stderr[-40:])
                    + hint
                )
            time.sleep(0.02)
        raise Failure(
            f"blastit dev did not print a ready line in {timeout:.0f}s; stdout:\n"
            + "\n".join(self.stdout[-40:])
            + "\nstderr:\n"
            + "\n".join(self.stderr[-40:])
        )

    def stop(self, sig: int = signal.SIGTERM, timeout: float = 15.0) -> int:
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, sig)
            except ProcessLookupError:
                pass
        try:
            return self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            return self.process.wait()


@dataclass(frozen=True, slots=True)
class CliHelp:
    """The flags each documented command's own `--help` prints."""

    blastit: frozenset[str]
    dev: frozenset[str]
    check: frozenset[str]
    fold: frozenset[str]
    presets: frozenset[str]


class Harness:
    def __init__(self, pages: list[PageSpec], report: Report) -> None:
        self.pages = pages
        self.report = report
        # The flag tables the static deploy checks trust are compared with the
        # installed tools' own `--help` before anything is built: a table entry a
        # tool does not accept would let an impossible command pass (see
        # `FlagAudit` for why `psql` is the table the gate itself re-checks).
        assert_flag_tables(gate_only=True)
        # The source tree, the release the docs announce and the artifact must
        # agree before anything is built or run.
        assert_provenance()
        self.wheel = wheel_path()
        self.wheel_sha256 = hashlib.sha256(self.wheel.read_bytes()).hexdigest()
        self.python = interpreter_for(self.wheel)
        # The wheel's `cp3NN` tag names the interpreter it was built for, and the
        # resolved path is what `uv venv --python` gets: no discovery, so the venv
        # does not depend on the caller's HOME holding a uv-managed interpreter.
        self.version = _wheel_python(self.wheel.name)
        # `pytest` comes from a cache this gate did not create (offline);
        # everything that installs the wheel uses a cache this run created empty,
        # so uv cannot serve a *rebuilt* same-version wheel it cached during an
        # earlier run. The cache is the caller's HOME or `BLASTIT_DOCS_UV_CACHE`.
        self.global_cache = os.environ.get(CACHE_OVERRIDE) or uv_cache_dir()
        self.pg_stub: PgStub | None = None
        self.pg_url = os.environ.get("BLASTIT_TEST_PG_URL", "")
        self.substitutions: list[tuple[str, str]] = []
        self._cli: CliHelp | None = None
        self.work = Path(
            tempfile.mkdtemp(
                prefix="brd-",
                dir="/tmp/opencode" if Path("/tmp/opencode").is_dir() else None,
            )
        )
        self.tmpdir = Path(
            tempfile.mkdtemp(
                prefix="t-",
                dir="/tmp/opencode" if Path("/tmp/opencode").is_dir() else None,
            )
        )
        self.cache = self.work / "uv-cache"
        self.cache.mkdir()
        self.base = self.work / "base"
        try:
            # Fail on a missing cached pin before the (slow) wheel install, so a
            # cold cache costs a second rather than a build.
            self._require_cached_pytest()
            self._build_base()
            self._start_postgres()
        except BaseException:
            shutil.rmtree(self.work, ignore_errors=True)
            shutil.rmtree(self.tmpdir, ignore_errors=True)
            raise

    def _build_base(self) -> None:
        """One throwaway venv: the wheel and `pytest`, installed exactly once.

        Every fence project carries the same `pyproject.toml`/`uv.lock` and
        points `UV_PROJECT_ENVIRONMENT` at this environment, so `uv run` uses it
        instead of creating another virtualenv.
        """
        self.base.mkdir(parents=True, exist_ok=True)
        (self.base / "pyproject.toml").write_text(
            '[project]\nname = "blastit-fixture"\nversion = "0.0.0"\n'
            'requires-python = ">=3.13"\ndependencies = ["blastit"]\n',
            encoding="utf-8",
        )
        env = self.base_env(self.base / "home", shared=False)
        for command in (
            ["uv", "venv", "--python", self.python, ".venv"],
            ["uv", "lock"],
            ["uv", "sync"],
        ):
            self._tool(self.base, env, command, quiet=True)
        self.shared_python = self.base / ".venv" / "bin" / "python"
        self.verify_wheel_install(self.base / ".venv")
        self.install_pytest()

    def _start_postgres(self) -> None:
        self.substitutions = [
            ("postgresql://shop:secret@127.0.0.1:5432/shop", self.pg_url)
        ]
        if self.pg_url:
            return
        try:
            self.pg_stub = PgStub("127.0.0.1", 5432)
            self.pg_stub.start()
        except OSError:
            # A developer's PostgreSQL may own 5432; the stub is still
            # deterministic, just on a port the harness chooses. Every fence
            # that names the documented URL is translated to this one.
            self.pg_stub = PgStub("127.0.0.1", 0)
            self.pg_stub.start()
        self.pg_url = f"postgresql://shop:secret@127.0.0.1:{self.pg_stub.port}/shop"
        self.substitutions = [
            ("postgresql://shop:secret@127.0.0.1:5432/shop", self.pg_url)
        ]

    def close(self) -> None:
        if self.pg_stub is not None:
            self.pg_stub.stop()
        shutil.rmtree(self.work, ignore_errors=True)
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    # -- environments -----------------------------------------------------

    def base_env(self, home: Path, shared: bool = True) -> dict[str, str]:
        """The documented environment, built from scratch.

        No `BLASTIT_*` knob is injected: a fence runs with what the page tells a
        developer to set, so a host where `blastit dev` cannot serve its first
        view is a red fence naming the product, never a green one masking it.

        `UV_OFFLINE=1` is the one thing every `uv` call here shares, so the
        offline contract is a property of the environment rather than of the call
        sites: a fence that would need the network fails naming `uv`, offline,
        instead of quietly downloading. The only step outside this environment is
        `--warm-cache`, documented as the one command that may reach the index.
        """
        uv_dir = str(Path(shutil.which("uv") or "uv").parent)
        env = {
            "HOME": str(home),
            "PATH": f"{uv_dir}:/usr/local/bin:/usr/bin:/bin",
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "TZ": "UTC",
            "TMPDIR": str(self.tmpdir),
            "UV_CACHE_DIR": str(self.cache),
            "UV_OFFLINE": "1",
            "UV_PYTHON_DOWNLOADS": "never",
            "UV_LINK_MODE": "copy",
            "UV_FIND_LINKS": str(DIST),
            "GIT_AUTHOR_NAME": "blastit docs",
            "GIT_AUTHOR_EMAIL": "docs@blastit.invalid",
            "GIT_COMMITTER_NAME": "blastit docs",
            "GIT_COMMITTER_EMAIL": "docs@blastit.invalid",
        }
        if shared:
            env["UV_PROJECT_ENVIRONMENT"] = str(self.base / ".venv")
            env["PATH"] = f"{self.base / '.venv' / 'bin'}:{env['PATH']}"
        (home / "tmp").mkdir(parents=True, exist_ok=True)
        return env

    # -- project setup ----------------------------------------------------

    def prepare(
        self,
        fence: Fence,
        spec: FenceSpec,
        git: bool = False,
        manifest: str | None = None,
    ) -> tuple[Path, dict[str, str]]:
        root = Path(tempfile.mkdtemp(prefix=f"f{fence.index}-", dir=self.work))
        env = self.base_env(root / "home")
        env["DATABASE_URL"] = self.pg_url
        env["ANALYTICS_URL"] = self.pg_url
        fixture = spec.fixture or "empty"
        if fixture != "empty":
            source = FIXTURES / fixture
            if not source.is_dir():
                raise Failure(f"inventory names the missing fixture {fixture!r}")
            shutil.copytree(source, root, dirs_exist_ok=True)
        if manifest is not None:
            # The page's own TOML is the manifest under test; the fixture only
            # supplies the app code it names.
            (root / "blastit.toml").write_text(manifest, encoding="utf-8")
        shutil.copyfile(self.base / "pyproject.toml", root / "pyproject.toml")
        shutil.copyfile(self.base / "uv.lock", root / "uv.lock")
        if git:
            self._init_git(root, env)
        return root, env

    def verify_wheel_install(self, venv: Path) -> None:
        """Prove the environment holds the wheel this run built, byte for byte.

        A fresh per-run `UV_CACHE_DIR` already keeps uv from reusing a wheel it
        cached in an earlier run; this comparison makes that a checked fact
        rather than a configuration intention. `RECORD` is excluded because the
        installer rewrites it; every other file must match the built wheel, or a
        cached wheel (or a stale install) is being tested.
        """
        python = venv / "bin" / "python"
        site = Path(
            subprocess.check_output(
                [
                    str(python),
                    "-c",
                    "import sysconfig; print(sysconfig.get_paths()['purelib'])",
                ],
                text=True,
            ).strip()
        )
        checked = 0
        with zipfile.ZipFile(self.wheel) as archive:
            for info in archive.infolist():
                name = info.filename
                if info.is_dir() or name.endswith("/RECORD"):
                    continue
                installed = site / name
                if not installed.is_file():
                    raise Failure(
                        f"the installed environment has no {name} from "
                        f"{self.wheel.name}: it did not install the built wheel"
                    )
                if installed.read_bytes() != archive.read(name):
                    raise Failure(
                        f"{name} in the installed environment differs from "
                        f"{self.wheel.name} (sha256 {self.wheel_sha256[:12]}): a "
                        "wheel cached by an earlier run was installed, not the "
                        "one this run built"
                    )
                checked += 1
        if not checked:
            raise Failure(f"{self.wheel.name} holds no installable files")

    def _pytest_argv(self, python: str) -> list[str]:
        return [
            "uv",
            "pip",
            "install",
            "--python",
            python,
            f"pytest=={PYTEST_VERSION}",
        ]

    def _pytest_env(self) -> dict[str, str]:
        return {
            **self.base_env(self.work / "pytest-cache", shared=False),
            "UV_CACHE_DIR": self.global_cache,
        }

    def _pytest_missing(self, failure: Failure) -> Failure:
        return Failure(
            f"pytest=={PYTEST_VERSION} is not in the uv cache this gate reads "
            f"({self.global_cache}), and this gate never reaches the network. Run "
            "the one documented prerequisite once for this HOME, then re-run:\n"
            "  uv run --no-project python python/tests/runnable_docs/harness.py "
            "--warm-cache\n"
            "(or set BLASTIT_DOCS_UV_CACHE to a cache that already holds the pin; "
            "docs/testing/README.md has the whole contract)\n"
            f"the offline install said: {failure}"
        )

    def _require_cached_pytest(self) -> None:
        """Fail in a second, with the fix, instead of after the wheel install."""
        argv = [*self._pytest_argv(self.python), "--dry-run"]
        try:
            self._tool(self.work, self._pytest_env(), argv, quiet=True)
        except Failure as failure:
            raise self._pytest_missing(failure) from failure

    def install_pytest(self) -> None:
        """Install `pytest` from the uv cache, or fail with the fix.

        The gate is offline by contract: reaching the package index inside
        `make check` would make a dangling network a red build and a supply-chain
        hole. A missing cache entry names the one command that fills it. This is
        the only install that reads the cache the caller's HOME owns, because the
        wheel install must never see it.
        """
        argv = self._pytest_argv(str(self.base / ".venv" / "bin" / "python"))
        try:
            self._tool(self.base, self._pytest_env(), argv, quiet=True)
        except Failure as failure:
            raise self._pytest_missing(failure) from failure

    def _init_git(self, root: Path, env: dict[str, str]) -> None:
        for command in (
            ["git", "init", "-q"],
            ["git", "add", "blastit.toml"],
            ["git", "commit", "-q", "-m", "fixture"],
        ):
            self._tool(root, env, command, quiet=True)

    def _tool(
        self,
        cwd: Path,
        env: dict[str, str],
        argv: list[str],
        quiet: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        completed = subprocess.run(
            argv,
            cwd=str(cwd),
            env=env,
            capture_output=True,
            text=True,
            timeout=FENCE_TIMEOUT,
            check=False,
        )
        if completed.returncode != 0:
            raise Failure(
                f"`{' '.join(argv)}` failed with {completed.returncode}:\n"
                f"stdout:\n{completed.stdout[-2000:]}\nstderr:\n{completed.stderr[-2000:]}"
            )
        if not quiet:
            return completed
        return completed

    def shell(self, root: Path, env: dict[str, str]) -> Shell:
        return Shell(root, env)

    def run_split(
        self, root: Path, env: dict[str, str], command: str
    ) -> tuple[int, list[str], list[str]]:
        """Run one command with stdout and stderr apart (the `--json` contract)."""
        completed = subprocess.run(
            ["/bin/bash", "--noprofile", "--norc", "-c", self.translated(command)],
            cwd=str(root),
            env=env,
            capture_output=True,
            text=True,
            timeout=FENCE_TIMEOUT,
            check=False,
        )
        return (
            completed.returncode,
            completed.stdout.splitlines(),
            completed.stderr.splitlines(),
        )

    def translated(self, command: str) -> str:
        for old, new in self.substitutions:
            if old in command:
                command = command.replace(old, new)
        return command

    # -- fences -----------------------------------------------------------

    def execute(self) -> None:
        for page in self.pages:
            text = (REPO / page.path).read_text(encoding="utf-8")
            fences = console_fences(page.path, text)
            by_index = {fence.index: fence for fence in fences}
            for spec in page.fences:
                fence = by_index.get(spec.index)
                if fence is None:
                    self.report.add(
                        Outcome(
                            page.path,
                            spec.index,
                            spec.mode,
                            False,
                            False,
                            "the inventory names a console fence this page does not have",
                        )
                    )
                    continue
                started = time.monotonic()
                try:
                    detail, executed = self.dispatch(page.path, spec, fence)
                    if spec.mode in ACCOUNTED and executed:
                        raise Failure(
                            f"{page.path} fence {spec.index}: `{spec.mode}` is an "
                            "accounted mode and must not claim execution"
                        )
                    if (
                        spec.mode not in ACCOUNTED
                        and spec.mode not in SUBSTITUTED
                        and not executed
                    ):
                        raise Failure(
                            f"{page.path} fence {spec.index}: `{spec.mode}` runs the "
                            "fence's command, so it cannot report a substitution; "
                            "declare the substitution in the inventory"
                        )
                    self.report.add(
                        Outcome(
                            page.path,
                            spec.index,
                            spec.mode,
                            True,
                            executed,
                            detail,
                            time.monotonic() - started,
                        )
                    )
                except Failure as failure:
                    self.report.add(
                        Outcome(
                            page.path,
                            spec.index,
                            spec.mode,
                            False,
                            True,
                            str(failure),
                            time.monotonic() - started,
                        )
                    )

    def dispatch(self, page: str, spec: FenceSpec, fence: Fence) -> tuple[str, bool]:
        handler = getattr(self, "mode_" + spec.mode.replace("-", "_"), None)
        if handler is None:
            raise Failure(f"unknown inventory mode {spec.mode!r}")
        _require_commands(fence, spec)
        return handler(fence, spec)

    # -- modes ------------------------------------------------------------

    def mode_install(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root = Path(tempfile.mkdtemp(prefix="install-", dir=self.work)) / "app"
        root.mkdir()
        env = self.base_env(root / "home", shared=False)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                self.expect(command, code, lines, "exact")
        finally:
            shell.close()
        completed = subprocess.run(
            [
                str(root / ".venv" / "bin" / "python"),
                "-c",
                "import blastit, pathlib; print(pathlib.Path(blastit.__file__).resolve())",
            ],
            cwd=str(root),
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        if completed.returncode != 0 or str(root) not in completed.stdout:
            raise Failure(
                "`uv add blastit` did not install the wheel under test:\n"
                + completed.stdout
                + completed.stderr
            )
        self.verify_wheel_install(root / ".venv")
        return (
            f"installed {self.wheel.name} and verified it against the built wheel",
            True,
        )

    def mode_dev(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        commands = list(fence.commands())
        exports = [
            command for command in commands if command.text.startswith("export ")
        ]
        servers = [
            command for command in commands if not command.text.startswith("export ")
        ]
        if len(servers) != 1:
            raise Failure(
                f"expected one `blastit dev` command, found {len(servers)}: "
                + "; ".join(command.text.splitlines()[0] for command in servers)
            )
        for export in exports:
            for name, value in _exports(self.translated(export.text)):
                env[name] = value
        server = servers[0]
        port = _port_of(server.text)
        dev = self.start_dev(root, env, server.text, port)
        try:
            self.expect(server, None, dev.stdout, "exact")
        finally:
            code = dev.stop()
        if code != 0:
            raise Failure(
                f"`{server.text}` exited {code} after SIGTERM; stdout:\n"
                + "\n".join(dev.stdout[-40:])
                + "\nstderr:\n"
                + "\n".join(dev.stderr[-40:])
            )
        return f"served on 127.0.0.1:{port}", True

    def mode_dev_curl(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        server, client = self.dev_and_client(fence)
        dev = self.start_dev(root, env, server.text, _port_of(server.text))
        try:
            dev.wait_ready()
            shell = self.shell(root, env)
            try:
                code, lines = shell.run(self.translated(client.text))
            finally:
                shell.close()
            try:
                self.expect(client, code, lines, "exact")
            except Failure:
                unserved = self.unserved(server.text, dev.stdout)
                if unserved is not None:
                    raise unserved from None
                raise
        finally:
            dev.stop()
        return "served and answered", True

    def mode_summary(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        commands = list(fence.commands())
        dev_command = next(
            command for command in commands if _dev_argv(command.text) is not None
        )
        dev = self.start_dev(root, env, dev_command.text, _port_of(dev_command.text))
        shell = self.shell(root, env)
        try:
            dev.wait_ready()
            for command in commands:
                if command is dev_command:
                    self.expect(command, None, dev.stdout, "exact")
                    continue
                code, lines = shell.run(self.translated(command.text))
                if _serve_argv(command.text) is not None:
                    if code != 1:
                        raise Failure(
                            f"`{command.text}` exited {code}, expected 1: production "
                            "refuses to run unprivileged:\n" + "\n".join(lines[-30:])
                        )
                    continue
                if "check" in command.text:
                    self.assert_clean(command, lines)
                self.expect(command, code, lines, "exact")
        finally:
            shell.close()
            dev.stop()
        return "dev, check and preset agree", True

    def mode_pgstub(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        """The page's `docker run … postgres` line, substituted, never run.

        Docker is not a dependency of this unprivileged gate, so the container
        command is not executed; the harness probes its own in-process wire stub
        (or `BLASTIT_TEST_PG_URL`). That is a substitution the inventory declares,
        so the fence is **accounted**, not counted as executed.
        """
        if self.pg_stub is not None:
            self.pg_stub.ping()
            return _STUB_DETAIL, False
        parts = self.pg_url
        match = re.match(r"postgresql://[^@]*@([^/:]+)(?::(\d+))?/(\w+)", parts)
        if match is None:
            raise Failure(
                "BLASTIT_TEST_PG_URL is not a `postgresql://user:password@host:port/db` "
                "URL (its value is not printed)"
            )
        with socket.create_connection(
            (match.group(1), int(match.group(2) or 5432)), timeout=5
        ):
            pass
        return (
            "substituted: `docker` is not run; BLASTIT_TEST_PG_URL answers TCP instead",
            False,
        )

    def mode_learn_git(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        return self.learn_flow(fence, spec, git=True)

    def mode_learn_then_check(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        return self.learn_flow(fence, spec, git=False)

    def learn_flow(self, fence: Fence, spec: FenceSpec, git: bool) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec, git=git)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                if command.text.startswith("git diff"):
                    if code != 0 or not lines:
                        raise Failure(
                            "`git diff blastit.toml` showed nothing after `--learn`; "
                            f"exit {code}, output:\n" + "\n".join(lines[-20:])
                        )
                    continue
                self.expect(command, code, lines, "shape")
        finally:
            shell.close()
        return "learn wrote the manifest, then check was clean", True

    def mode_learn_sequence(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                if code != 0:
                    raise Failure(
                        f"`{command.text}` exited {code}:\n" + "\n".join(lines[-40:])
                    )
                self.assert_clean(command, lines)
                self.expect(command, code, lines, "shape")
        finally:
            shell.close()
        return "learn, dry-run and check agree", True

    def mode_dev_learn(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        command = fence.commands()[0]
        port = _port_of(command.text)
        dev = self.start_dev(root, env, command.text, port)
        try:
            dev.wait_ready()
            statuses: list[int] = []
            for path in ("/extra", "/"):
                statuses.append(self._request(port, path))
                if statuses[-1] == 503:
                    break
            code = dev.stop(signal.SIGINT)
        finally:
            if dev.process.poll() is None:
                dev.stop(signal.SIGKILL)
        if 503 in statuses:
            unserved = self.unserved(command.text, dev.stdout)
            if unserved is not None:
                raise unserved
        manifest = tomllib.loads((root / "blastit.toml").read_text(encoding="utf-8"))
        routes = {route["name"] for route in manifest.get("route", [])}
        if code != 0:
            raise Failure(
                f"`{command.text}` exited {code} after SIGINT (expected 0); stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        # The page promises one pass adds the routes it saw *and their exact
        # queries*, printing the resolved path. Assert all three, so a product
        # that adds a route without its grants (or writes nothing) goes red here.
        summary = next(
            (line.strip() for line in dev.stdout if line.lstrip().startswith("learn:")),
            None,
        )
        if summary is None:
            raise Failure(
                "no `learn:` summary after SIGINT; stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        match = re.match(
            r"^learn: (\d+) routes?, (\d+) queries? added → (.+)$", summary
        )
        if match is None:
            raise Failure(
                "the `learn:` summary does not match the documented shape "
                "`learn: N routes, M queries added → <path>`:\n" + summary
            )
        added_routes, added_queries, printed_path = (
            int(match.group(1)),
            int(match.group(2)),
            Path(match.group(3).strip()),
        )
        if added_routes < 1 or added_queries < 1:
            raise Failure(
                "one `dev --learn` pass must add the route and its queries; the "
                f"summary says {added_routes} routes, {added_queries} queries:\n"
                + summary
            )
        if printed_path.resolve() != (root / "blastit.toml").resolve():
            raise Failure(
                "the `learn:` summary must print the manifest it wrote; it names "
                f"{printed_path}, expected {(root / 'blastit.toml').resolve()}"
            )
        if "extra" not in routes:
            raise Failure(
                "`--learn` did not add the unrouted route; manifest:\n"
                + (root / "blastit.toml").read_text(encoding="utf-8")
            )
        learned = next(route for route in manifest["route"] if route["name"] == "extra")
        queries = (learned.get("db") or {}).get("queries") or []
        if not queries:
            raise Failure(
                "`--learn` added the route but wrote no `queries`; the page says "
                "one pass adds the exact queries it saw. Manifest:\n"
                + (root / "blastit.toml").read_text(encoding="utf-8")
            )
        if "SELECT 1" not in queries:
            raise Failure(
                "the learned query is not the statement the app issued "
                f"(`SELECT 1`): {queries!r}"
            )
        return (
            f"one pass added {added_routes} route(s) and {added_queries} "
            "query(ies), written to the printed manifest"
        ), True

    def mode_page_manifest(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        """Run the fence against the page's own TOML block, then its prose.

        `docs/local.md` shows a `blastit.toml` with `queries = []` and then the
        command that starts the app with it. Running a fixture manifest instead
        would prove nothing about that block, so this mode writes the page's own
        TOML into the project and asserts the two promises that follow it: the
        app keeps reading `DATABASE_URL` (the request is answered at all), and an
        ungranted statement is denied by PostgreSQL's `42501` through a listener
        — visible as a `sql.denied` line and, uncaught, a `500`.
        """
        manifest_text = _page_manifest(fence)
        root, env = self.prepare(fence, spec, manifest=manifest_text)
        document = tomllib.loads(manifest_text)
        target = str(document.get("app", ""))
        module = target.split(":", 1)[0].replace(".", "/")
        if not module or not (root / f"{module}.py").is_file():
            raise Failure(
                f"{fence.page}: the manifest block names `app = {target!r}`, which "
                f"the `{spec.fixture or 'empty'}` fixture does not provide"
            )
        commands = list(fence.commands())
        exports = [
            command for command in commands if command.text.startswith("export ")
        ]
        servers = [
            command for command in commands if not command.text.startswith("export ")
        ]
        if len(servers) != 1:
            raise Failure(
                f"expected one `blastit dev` command, found {len(servers)}: "
                + "; ".join(command.text.splitlines()[0] for command in servers)
            )
        exported: set[str] = set()
        for export in exports:
            for name, value in _exports(self.translated(export.text)):
                env[name] = value
                exported.add(name)
        if "DATABASE_URL" not in exported:
            raise Failure(
                f"{fence.page}: the fence no longer exports the database URL the "
                "page shows; the harness cannot prove the app reads it"
            )
        server = servers[0]
        port = _port_of(server.text)
        dev = self.start_dev(root, env, server.text, port)
        try:
            status = self._request(port, "/products/")
        finally:
            code = dev.stop()
        if status == 503:
            unserved = self.unserved(server.text, dev.stdout)
            if unserved is not None:
                raise unserved
        if code != 0:
            raise Failure(
                f"`{server.text}` exited {code} after SIGTERM; stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        if status != 500:
            raise Failure(
                "the page says an uncaught denial is a `500`; the request "
                f"answered {status}. stdout:\n" + "\n".join(dev.stdout[-40:])
            )
        # The paragraph promises this exact listener: `queries = []` denies every
        # statement with PostgreSQL's 42501 and says so on one `sql.denied` line.
        denied = [
            line for line in dev.stdout if re.search(r"\bWARN\s+sql\.denied\b", line)
        ]
        if not denied:
            raise Failure(
                "the page's `queries = []` route has no denying listener: the "
                "request failed without a `sql.denied` line. stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        pattern = re.compile(r"\bWARN\s+sql\.denied\s+products\s+db\s+\w+\s+SELECT 1\b")
        if not any(pattern.search(line) for line in denied):
            raise Failure(
                "the denial does not name the page's route and statement "
                "(`products db <phase> SELECT 1`):\n" + "\n".join(denied)
            )
        if not any("SQLSTATE 42501" in line for line in dev.stdout):
            raise Failure(
                "the app never received PostgreSQL's `42501`; the page says the "
                "empty grant denies with that code. stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        return (
            "the page's `queries = []` route answered 500 with a `sql.denied` "
            "line and SQLSTATE 42501"
        ), True

    def mode_denial(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        command = Command(fence.line, "uv run blastit dev", ())
        port = 8080
        dev = self.start_dev(root, env, command.text, port)
        try:
            dev.wait_ready()
            status = self._request(port, "/products/")
            if status == 503:
                unserved = self.unserved(command.text, dev.stdout)
                if unserved is not None:
                    raise unserved
        finally:
            dev.stop()
        if status != 500:
            raise Failure(
                "the page says an uncaught denial is a `500`; the request "
                f"answered {status}. stdout:\n" + "\n".join(dev.stdout[-40:])
            )
        expected = [line for line in fence.body.splitlines() if line.strip()]
        if not expected:
            raise Failure("the denial fence is empty")
        pattern = re.compile(
            r"^\d{2}:\d{2}:\d{2}\.\d{3}\s+WARN\s+sql\.denied\s+products\s+db\s+(\w+)\s+SELECT\b"
        )
        actual = [line for line in dev.stdout if "sql.denied" in line]
        if not actual:
            raise Failure(
                "no `sql.denied` line was printed; stdout:\n"
                + "\n".join(dev.stdout[-40:])
            )
        match = pattern.match(actual[0].strip())
        if match is None:
            raise Failure(
                "the printed denial does not match the documented line:\n"
                f"expected: {expected[0]}\n"
                f"got:      {actual[0]}"
            )
        documented = expected[0].strip()
        if match.group(1) != "parse" and "  parse  " in documented:
            raise Failure(
                "the documented denial names the `parse` phase, the printed line "
                f"names `{match.group(1)}`:\n{actual[0]}"
            )
        return "the printed denial matches the documented fields", True

    def mode_check(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                self.assert_clean(command, lines)
                self.expect(command, code, lines, "shape")
        finally:
            shell.close()
        return "check reported a clean suite", True

    def mode_findings(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        env["ANALYTICS_URL"] = self.pg_url
        shell = self.shell(root, env)
        command = fence.commands()[0]
        try:
            code, lines = shell.run(self.translated(command.text))
        finally:
            shell.close()
        if code != FINDINGS:
            raise Failure(
                f"`{command.text}` exited {code}, expected {FINDINGS} for a manifest "
                "with findings:\n" + "\n".join(lines[-40:])
            )
        for expected in command.expected:
            if expected.startswith("    "):
                continue  # the illustrative SQL detail line is a shape, below
            if not any(_shape_matches(expected, line) for line in lines):
                raise Failure(
                    "the documented line is missing:\n"
                    f"expected: {expected}\n"
                    "got:\n" + "\n".join(lines)
                )
        if not any(line.startswith("    SELECT") for line in lines):
            raise Failure(
                "a missing grant did not print its statement detail:\n"
                + "\n".join(lines)
            )
        if not any("blastit check: 6 problems" in line for line in lines):
            raise Failure(
                "expected six findings (the page shows six):\n" + "\n".join(lines)
            )
        return "six documented findings, one line each", True

    def mode_json(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        env["ANALYTICS_URL"] = self.pg_url
        command = fence.commands()[0]
        code, lines, errors = self.run_split(root, env, command.text)
        if code != FINDINGS:
            raise Failure(
                f"`{command.text}` exited {code}, expected {FINDINGS}:\n"
                + "\n".join(errors[-40:])
            )
        try:
            document = json.loads("\n".join(lines))
        except json.JSONDecodeError as error:
            raise Failure(
                f"stdout is not one JSON document ({error}):\n"
                + "\n".join(lines)
                + "\nstderr:\n"
                + "\n".join(errors[-40:])
            ) from error
        for key in (
            "schema_version",
            "ok",
            "requests",
            "statements",
            "routes",
            "proxies",
            "findings",
        ):
            if key not in document:
                raise Failure(f"the document has no {key!r}: {document}")
        if document["ok"] is not False:
            raise Failure(f"a run with findings says ok: {document['ok']!r}")
        kinds = {entry.get("kind") for entry in document["findings"]}
        documented = {
            "missing_grant",
            "dead_grant",
            "unexercised_route",
            "missing_ingress",
            "unrouted_request",
            "proxy_unused",
        }
        if not documented <= kinds:
            raise Failure(
                f"findings missing from the document: {sorted(documented - kinds)}; "
                f"got {sorted(kinds)}"
            )
        for entry in document["findings"]:
            if entry.get("severity") not in ("error", "warning"):
                raise Failure(f"a finding has no severity: {entry}")
        return f"one JSON document with {len(document['findings'])} findings", True

    def mode_dev_flags(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        seen: list[tuple[int, str]] = []
        notes: list[str] = []
        for command in fence.commands():
            port = _port_of(command.text)
            text = command.text
            if _is_busy(port):
                if spec.port_fallback is None:
                    raise Failure(
                        f"port {port} is busy on this host; this page cannot be "
                        "followed here as written. Declare `port_fallback` on this "
                        "fence in inventory.toml with the reason the flag, not the "
                        "number, is the contract, or free the port"
                    )
                fallback = _free_port_near(port)
                text = re.sub(r"--port\s+\d+", f"--port {fallback}", text)
                notes.append(
                    f"port {port} was busy; ran the documented flag on {fallback} "
                    "(declared in inventory.toml)"
                )
                port = fallback
            dev = self.start_dev(root, env, text, port)
            try:
                dev.wait_ready()
                banner = [
                    line for line in dev.stdout if "single-uid simulation" in line
                ]
                if not banner:
                    raise Failure(
                        f"`{command.text}` printed no banner:\n"
                        + "\n".join(dev.stdout[-40:])
                    )
                seen.append((port, banner[0].strip()))
            finally:
                code = dev.stop(
                    signal.SIGINT if "--learn" in command.text else signal.SIGTERM
                )
            if code != 0:
                raise Failure(f"`{command.text}` exited {code} after the stop signal")
        for port, banner in seen:
            if f":{port}" not in banner:
                raise Failure(
                    f"the banner does not name the port {port} the command chose: {banner}"
                )
        return "each dev flag boots and stops" + (
            " (" + "; ".join(notes) + ")" if notes else ""
        ), True

    def mode_check_flags(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        for command in fence.commands():
            code, stdout, stderr = self.run_split(root, env, command.text)
            if code != 0:
                raise Failure(
                    f"`{command.text}` exited {code}:\n" + "\n".join(stderr[-40:])
                )
            if "--json" in command.text:
                try:
                    json.loads("\n".join(stdout))
                except json.JSONDecodeError as error:
                    raise Failure(
                        f"`{command.text}` did not print one JSON document:\n"
                        + "\n".join(stdout)
                    ) from error
            else:
                self.assert_clean(command, stdout + stderr)
        return "each check flag runs", True

    def mode_serve_flags(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                if "--help" in command.text:
                    if code != 0:
                        raise Failure(f"`{command.text}` exited {code}")
                    text = "\n".join(lines)
                    for word in ("blastit dev", "blastit check", "digitalocean-paas"):
                        if word not in text:
                            raise Failure(
                                f"`{command.text}` does not name {word!r}:\n{text[-2000:]}"
                            )
                elif code != 1:
                    raise Failure(
                        f"`{command.text}` exited {code}, expected 1 (production "
                        "refuses to run unprivileged):\n" + "\n".join(lines[-40:])
                    )
                if code == 1 and not any(
                    "blastit dev" in line or "cap_" in line for line in lines
                ):
                    raise Failure(
                        f"`{command.text}` did not say why it refused:\n"
                        + "\n".join(lines[-40:])
                    )
        finally:
            shell.close()
        return "production refuses closed; help lists the commands and presets", True

    def mode_usage(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        shell = self.shell(root, env)
        try:
            code, lines = shell.run("blastit-manifest fold --help")
            if code != 0:
                raise Failure(f"`blastit-manifest fold --help` exited {code}")
            text = "\n".join(lines)
            for option in (
                "--manifest",
                "--write",
                "--prefer-tables",
                "--allow-dangerous",
            ):
                if option not in text:
                    raise Failure(
                        f"the documented option {option} is not in the command's help:\n{text}"
                    )
        finally:
            shell.close()
        return "the documented fold signature exists", True

    def mode_presets(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root, env = self.prepare(fence, spec)
        help_shell = self.shell(root, env)
        try:
            code, lines = help_shell.run("blastit --help")
        finally:
            help_shell.close()
        if code != 0:
            raise Failure("`blastit --help` failed")
        help_text = "\n".join(lines)
        shell = self.shell(root, env)
        try:
            for command in fence.commands():
                code, lines = shell.run(self.translated(command.text))
                if code != 1:
                    raise Failure(
                        f"`{command.text}` exited {code}, expected 1 (no capabilities):\n"
                        + "\n".join(lines[-40:])
                    )
                text = "\n".join(lines)
                if "blastit dev" not in text or "cap_" not in text:
                    raise Failure(
                        f"`{command.text}` did not point at `blastit dev` and the "
                        "missing capabilities:\n" + "\n".join(lines[-40:])
                    )
                preset = command.text.split("--preset", 1)[-1].strip().split()[0]
                if preset not in help_text:
                    raise Failure(
                        f"`{command.text}` names preset {preset!r}, which `blastit "
                        "--help` does not list"
                    )
        finally:
            shell.close()
        return "each preset exists and fails closed without capabilities", True

    def mode_static(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        """Check a deployment recipe that cannot run in this unprivileged gate.

        Three things are read, not eyeballed: every command's executable,
        subcommand, flags and file paths (against the real CLI's `--help` and this
        table), every `yaml`/`ini`/`toml`/`sql` block the page shows, and every
        JSON line in the fence itself.
        """
        reason = _require_reason(spec, f"{fence.page} fence {fence.index}")
        cli = self.cli_help()
        titles = _static_blocks(fence)
        skipped = 0
        for command in fence.commands():
            skipped += _static_command(command.text, cli, titles)
        _static_json_lines(fence)
        detail = f"statically checked every command, flag and snippet ({reason})"
        if skipped:
            detail += (
                f" · {skipped} absolute path(s) left to the reader's host, unchecked"
            )
        return detail, False

    def cli_help(self) -> CliHelp:
        """Parse the flags the installed CLI really accepts, once."""
        if self._cli is None:
            blastit = str(self.base / ".venv" / "bin" / "blastit")
            manifest = str(self.base / ".venv" / "bin" / "blastit-manifest")
            env = self.base_env(self.work / "help")
            texts = {}
            for name, argv in {
                "blastit": [blastit, "--help"],
                "dev": [blastit, "dev", "--help"],
                "check": [blastit, "check", "--help"],
                "fold": [manifest, "fold", "--help"],
            }.items():
                texts[name] = self._tool(self.work, env, argv).stdout
            self._cli = CliHelp(
                blastit=_flags(texts["blastit"]),
                dev=_flags(texts["dev"]),
                check=_flags(texts["check"]),
                fold=_flags(texts["fold"]),
                presets=_presets(texts["blastit"]),
            )
        return self._cli

    def mode_example(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        return (
            f"accounted: {_require_reason(spec, f'{fence.page} fence {fence.index}')}",
            False,
        )

    def mode_install_external(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        reason = _require_reason(spec, f"{fence.page} fence {fence.index}")
        installs_blastit = False
        for command in fence.commands():
            if "uv add" in command.text and "blastit" in command.text:
                installs_blastit = True
        if not installs_blastit:
            raise Failure(
                "an `install-external` fence must name blastit in one of its `uv add` "
                "commands; otherwise it cannot be accounted as an install of this wheel"
            )
        return f"accounted: {reason}", False

    # -- helpers ----------------------------------------------------------

    def start_dev(
        self, root: Path, env: dict[str, str], command: str, port: int
    ) -> DevProcess:
        _require_free(port)
        argv = _dev_argv(command)
        if argv is None:
            raise Failure(f"`{command}` is not a `blastit dev` command")
        shell_command = "exec " + " ".join(shlex.quote(part) for part in argv)
        process = subprocess.Popen(
            ["/bin/bash", "--noprofile", "--norc", "-c", shell_command],
            cwd=str(root),
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        stdout: list[str] = []
        stderr: list[str] = []
        threading.Thread(
            target=_drain, args=(process.stdout, stdout), daemon=True
        ).start()
        threading.Thread(
            target=_drain, args=(process.stderr, stderr), daemon=True
        ).start()
        dev = DevProcess(process, stdout, stderr, port)
        dev.wait_ready()
        return dev

    def unserved(self, command: str, stdout: list[str]) -> Failure | None:
        """A failure naming the product when the dev server never spawned a view.

        `unserved` returns `None` when the log holds no `queue_timeout`, so a
        fence that failed for its own reason keeps its own message. No environment
        knob is injected to avoid this: a host-sized `BLASTIT_MEM_HIGH_WATERMARK`
        problem is the product's to fix (MA.3.7 (a)), not the harness's to hide.
        """
        if not any("queue_timeout" in line for line in stdout):
            return None
        boot = next((line for line in stdout if "max_view_processes=" in line), "")
        return Failure(
            f"`{command}` answered 503 queue_timeout: the scheduler never spawned "
            "a view. The harness injects no BLASTIT_* knob, and above the 85% "
            "`BLASTIT_MEM_HIGH_WATERMARK` default the first spawn waits for an "
            "evictable process that does not exist yet (product gap, MA.3.7 (a))."
            + (f"\nboot: {boot.strip()}" if boot else "")
        )

    def _request(self, port: int, path: str) -> int:
        """One GET; returns the status, and never hides a refused connection."""
        url = f"http://127.0.0.1:{port}{path}"
        try:
            with urllib.request.urlopen(url, timeout=20) as response:
                response.read()
                return response.status
        except urllib.error.HTTPError as error:
            error.read()
            return error.code
        except urllib.error.URLError as error:
            raise Failure(f"the dev server did not answer {url}: {error}") from error

    def dev_and_client(self, fence: Fence) -> tuple[Command, Command]:
        commands = list(fence.commands())
        if len(commands) != 1:
            raise Failure(f"expected one client command, found {len(commands)}")
        # The client fence has no server command of its own: the page keeps the
        # server in the previous terminal, so the harness starts the same
        # documented `dev` command in the fence's own fresh project.
        return Command(fence.line, "uv run blastit dev", ()), commands[0]

    def assert_clean(self, command: Command, lines: Iterable[str]) -> None:
        """A plain `check` fence promises a clean audit; `--json` and `--learn`
        have their own contracts (checked by their modes).

        The counts are read, not normalised away: `0 requests, 0 statements,
        0/0 routes, clean` is not a clean audit, it is a suite that observed
        nothing, and the page's `412 requests, 977 statements, 3/3 routes` must
        not be allowed to match it.
        """
        if "--json" in command.text or "--learn" in command.text:
            return
        got = list(lines)
        text = "\n".join(got)
        matching = [line for line in got if _CLEAN.match(line.strip())]
        if not matching:
            raise Failure(
                f"`{command.text}` was expected to report a clean audit:\n{text[-3000:]}"
            )
        _require_nonzero(matching[0])

    def expect(
        self,
        command: Command,
        code: int | None,
        lines: Iterable[str],
        policy: str,
    ) -> None:
        got = list(lines)
        if code is not None and code != 0:
            raise Failure(f"`{command.text}` exited {code}:\n" + "\n".join(got[-60:]))
        if not command.expected:
            return
        if policy == "exact":
            for expected in command.expected:
                if not any(expected.rstrip() == line.rstrip() for line in got):
                    raise Failure(
                        "the documented output is missing:\n"
                        f"expected: {expected}\n"
                        "got:\n" + "\n".join(got[-60:])
                    )
        else:
            for expected in command.expected:
                if not any(_shape_matches(expected, line) for line in got):
                    raise Failure(
                        "the documented output is missing (numbers and absolute "
                        "paths may differ):\n"
                        f"expected: {expected}\n"
                        "got:\n" + "\n".join(got[-60:])
                    )
                if _CLEAN.match(expected.strip()):
                    # The page's counts are illustrative, but a real clean line
                    # still has to have observed something.
                    _require_nonzero(
                        next(line for line in got if _CLEAN.match(line.strip()))
                    )


def _drain(stream, sink: list[str]) -> None:
    if stream is None:
        return
    for line in stream:
        sink.append(line.rstrip("\n"))


def _exports(command: str) -> list[tuple[str, str]]:
    pairs = []
    for line in command.splitlines():
        line = line.strip()
        if line.startswith("export "):
            assignment = line[len("export ") :]
            if "=" in assignment:
                name, value = assignment.split("=", 1)
                pairs.append((name, value.strip("'\"")))
    return pairs


def _dev_argv(command: str) -> list[str] | None:
    text = command.replace("\\\n", " ")
    text = re.sub(r"(?<!\S)#.*$", "", text).strip()
    try:
        words = shlex.split(text)
    except ValueError:
        return None
    if words[:2] == ["uv", "run"]:
        words = words[2:]
    if words[:2] == ["blastit", "dev"]:
        return words
    return None


def _serve_argv(command: str) -> list[str] | None:
    text = command.replace("\\\n", " ")
    text = re.sub(r"(?<!\S)#.*$", "", text).strip()
    try:
        words = shlex.split(text)
    except ValueError:
        return None
    if words[:2] == ["uv", "run"]:
        words = words[2:]
    if words[:1] == ["blastit"] and words[1:2] not in (["dev"], ["check"], ["test"]):
        return words
    return None


def _port_of(command: str) -> int:
    argv = _dev_argv(command) or shlex.split(command)
    if "--port" in argv:
        return int(argv[argv.index("--port") + 1])
    return 8080


def _require_free(port: int) -> None:
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(("127.0.0.1", port))
    except OSError as error:
        raise Failure(
            f"127.0.0.1:{port} is already in use, so the documented command cannot "
            f"bind it ({error})"
        ) from error
    finally:
        probe.close()


def _is_busy(port: int) -> bool:
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(("127.0.0.1", port))
    except OSError:
        return True
    finally:
        probe.close()
    return False


def _free_port_near(preferred: int) -> int:
    for port in range(preferred + 1, preferred + 21):
        if not _is_busy(port):
            return port
    return _free_port()


def _free_port() -> int:
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return port


_NUMBER = re.compile(r"\d+")
_ABSOLUTE_PATH = re.compile(r"(?<![\w.])/(?:[^\s`'\"]+/)+[^\s`'\"]+")
_CLEAN = re.compile(
    r"^blastit check: (\d+) requests, (\d+) statements, (\d+)/(\d+) routes"
    r"(?:, \d+ proxies)?, clean$"
)


def _require_nonzero(line: str) -> None:
    """A clean line that observed nothing is not a clean audit."""
    match = _CLEAN.match(line.strip())
    if match is None:
        raise Failure(f"`{line}` is not a clean audit line")
    requests, statements, exercised, total = (int(group) for group in match.groups())
    if not (requests and statements and exercised and total):
        raise Failure(
            "a clean audit must observe something, and this one reports "
            f"{requests} requests, {statements} statements, {exercised}/{total} "
            f"routes: `0` means the suite reached nothing, not a complete "
            f"manifest: {line}"
        )
    if exercised > total:
        raise Failure(f"more routes exercised than declared: {line}")


def _normalise(line: str) -> str:
    line = _ABSOLUTE_PATH.sub("<path>", line)
    return _NUMBER.sub("#", line)


def _shape_matches(expected: str, actual: str) -> bool:
    if expected.rstrip() == actual.rstrip():
        return True
    return _normalise(expected) == _normalise(actual) or _normalise(
        expected
    ) in _normalise(actual)


def _flags(help_text: str) -> frozenset[str]:
    """Every `--flag`/`-x` a `--help` prints, plus the help probe's own."""
    found = set(re.findall(r"(?<![\w-])(--?[A-Za-z][A-Za-z0-9-]*)", help_text))
    found.update({"-h", "--help"})
    return frozenset(found)


def _presets(help_text: str) -> frozenset[str]:
    return frozenset(re.findall(r"^  ([a-z0-9][a-z0-9-]+): ", help_text, re.MULTILINE))


_ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
_SUBSTITUTION = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=\$\((.*)\)$")

# A deployment fence may pipe a command through a text filter; those are not
# commands to check, and every other executable has to be one this harness knows.
FILTERS = frozenset({"head", "tail", "grep", "cat", "wc", "sort"})
GIT_SUBCOMMANDS = frozenset({"rev-parse", "status", "diff", "log", "show"})

UV_SUBCOMMANDS = frozenset(
    {
        "run",
        "add",
        "remove",
        "init",
        "venv",
        "sync",
        "lock",
        "pip",
        "export",
        "tree",
        "python",
        "tool",
        "version",
        "self",
        "build",
        "publish",
    }
)
UV_RUN_BOOLEAN_FLAGS = frozenset(
    {
        "--no-project",
        "--no-sync",
        "--frozen",
        "--locked",
        "--offline",
        "--no-dev",
        "--all",
        "--isolated",
        "--no-cache",
        "--refresh",
        "--all-extras",
        "--quiet",
        "--verbose",
        "--no-build-isolation",
        "--active",
        "--no-active",
    }
)
UV_RUN_VALUE_FLAGS = frozenset(
    {
        "--env-file",
        "--python",
        "--with",
        "--directory",
        "--project",
        "--package",
        "--group",
        "--only-group",
        "--extra",
        "--index",
        "--index-url",
        "--extra-index-url",
        "--find-links",
        "--config-file",
        "--exclude-newer",
    }
)
SETPRIV_VALUE_FLAGS = frozenset(
    {
        "--inh-caps",
        "--ambient-caps",
        "--bounding-set",
        "--reuid",
        "--regid",
        "--ruid",
        "--rgid",
        "--euid",
        "--egid",
        "--groups",
        "--securebits",
        "--selinux-label",
        "--apparmor-profile",
        "--landlock-access",
    }
)
ENV_VALUE_FLAGS = frozenset({"-u", "--unset", "-C", "--chdir"})


@dataclass(frozen=True, slots=True)
class ToolFlags:
    """What one command of an external tool accepts, and which values are files.

    The tables are deliberately explicit: a flag a page shows and this table does
    not have is a failure, because the page and the check have to agree on what
    the real command accepts (`docker run --help` is the referee).
    """

    booleans: frozenset[str] = frozenset()
    values: frozenset[str] = frozenset()
    paths: frozenset[str] = frozenset()

    def plus(self, other: ToolFlags) -> ToolFlags:
        return ToolFlags(
            self.booleans | other.booleans,
            self.values | other.values,
            self.paths | other.paths,
        )


DOCKER_GLOBAL = ToolFlags(
    booleans=frozenset(
        {"-D", "--debug", "--tls", "--tlsverify", "--help", "-h", "--version", "-v"}
    ),
    values=frozenset(
        {
            "--config",
            "-H",
            "--host",
            "--context",
            "--log-level",
            "-l",
            "--tlscacert",
            "--tlscert",
            "--tlskey",
        }
    ),
)
DOCKER_RUN = ToolFlags(
    booleans=frozenset(
        {
            "-d",
            "--detach",
            "--rm",
            "-i",
            "--interactive",
            "-t",
            "--tty",
            "--init",
            "--privileged",
            "--read-only",
            "--publish-all",
            "-P",
            "--no-healthcheck",
            "--oom-kill-disable",
            "--disable-content-trust",
            "--help",
            "--sig-proxy",
            "--quiet",
            "-q",
        }
    ),
    values=frozenset(
        {
            "-a",
            "--attach",
            "--add-host",
            "--annotation",
            "--blkio-weight",
            "--cap-add",
            "--cap-drop",
            "--cgroup-parent",
            "--cgroupns",
            "--cidfile",
            "--cpu-period",
            "--cpu-quota",
            "--cpu-shares",
            "-c",
            "--cpus",
            "--cpuset-cpus",
            "--cpuset-mems",
            "--device",
            "--device-cgroup-rule",
            "--dns",
            "--dns-option",
            "--dns-search",
            "--domainname",
            "--entrypoint",
            "-e",
            "--env",
            "--env-file",
            "--expose",
            "--gpus",
            "--group-add",
            "--health-cmd",
            "--health-interval",
            "--health-retries",
            "--health-start-interval",
            "--health-start-period",
            "--health-timeout",
            # `-h` is `--hostname` here, not help.
            "--hostname",
            "-h",
            "--ip",
            "--ip6",
            "--ipc",
            "--isolation",
            "--label",
            "-l",
            "--label-file",
            "--link",
            "--link-local-ip",
            "--log-driver",
            "--log-opt",
            "--mac-address",
            "-m",
            "--memory",
            "--memory-reservation",
            "--memory-swap",
            "--memory-swappiness",
            "--mount",
            "--name",
            "--network",
            "--net",
            "--network-alias",
            "--oom-score-adj",
            "--pid",
            "--pids-limit",
            "--platform",
            "--publish",
            "-p",
            "--pull",
            "--restart",
            "--runtime",
            "--security-opt",
            "--shm-size",
            "--stop-signal",
            "--stop-timeout",
            "--storage-opt",
            "--sysctl",
            "--tmpfs",
            "--ulimit",
            "--user",
            "-u",
            "--userns",
            "--uts",
            "--volume",
            "-v",
            "--volumes-from",
            "--workdir",
            "-w",
            "--detach-keys",
        }
    ),
)
DOCKER_BUILD = ToolFlags(
    booleans=frozenset(
        {
            "--no-cache",
            "--pull",
            "--rm",
            "--force-rm",
            "--quiet",
            "-q",
            "--compress",
            "--squash",
            "--check",
            "--help",
        }
    ),
    values=frozenset(
        {
            "-t",
            "--tag",
            "-f",
            "--file",
            "--build-arg",
            "--platform",
            "--target",
            "--network",
            "--progress",
            "--label",
            "--iidfile",
            "--cache-from",
            "--cache-to",
            "--secret",
            "--ssh",
            "--add-host",
            "--build-context",
            "--output",
            "-o",
            "--metadata-file",
            "--shm-size",
            "--ulimit",
            "--allow",
            "--annotation",
            "--builder",
            "--cpu-shares",
            "-c",
            "--memory",
            "-m",
            "--memory-swap",
            "--isolation",
            "--no-cache-filter",
        }
    ),
    paths=frozenset({"-f", "--file"}),
)
DOCKER_PUSH = ToolFlags(
    booleans=frozenset(
        {"-a", "--all-tags", "-q", "--quiet", "--disable-content-trust"}
    ),
    values=frozenset({"--platform"}),
)
DOCKER_PULL = ToolFlags(
    booleans=frozenset(
        {"-a", "--all-tags", "-q", "--quiet", "--disable-content-trust"}
    ),
    values=frozenset({"--platform"}),
)
DOCKER_LOGS = ToolFlags(
    booleans=frozenset({"-f", "--follow", "-t", "--timestamps", "--details"}),
    values=frozenset({"-n", "--tail", "--since", "--until"}),
)
DOCKER_EXEC = ToolFlags(
    booleans=frozenset(
        {"-d", "--detach", "-i", "--interactive", "-t", "--tty", "--privileged"}
    ),
    values=frozenset(
        {
            "-e",
            "--env",
            "--env-file",
            "--user",
            "-u",
            "--workdir",
            "-w",
            "--detach-keys",
        }
    ),
)
DOCKER_IMAGE = ToolFlags(
    booleans=frozenset({"-a", "--all", "-q", "--quiet", "--help"}),
    values=frozenset({"--format", "--filter", "--no-trunc"}),
)
DOCKER_COMMANDS = {
    "run": DOCKER_RUN,
    "build": DOCKER_BUILD,
    "push": DOCKER_PUSH,
    "pull": DOCKER_PULL,
    "logs": DOCKER_LOGS,
    "exec": DOCKER_EXEC,
    "image": DOCKER_IMAGE,
}
COMPOSE_GLOBAL = ToolFlags(
    booleans=frozenset(
        {"-h", "--help", "--compatibility", "--dry-run", "--verbose", "-v", "--version"}
    ),
    values=frozenset(
        {
            "-f",
            "--file",
            "-p",
            "--project-name",
            "--project-directory",
            "--env-file",
            "--profile",
            "--ansi",
            "--progress",
            "--parallel",
        }
    ),
    paths=frozenset({"-f", "--file"}),
)
COMPOSE_UP = ToolFlags(
    booleans=frozenset(
        {
            "-d",
            "--detach",
            "--build",
            "--no-build",
            "--force-recreate",
            "--no-recreate",
            "--no-deps",
            "--remove-orphans",
            "--abort-on-container-exit",
            "--wait",
            "--no-start",
            "--quiet-pull",
            "--always-recreate-deps",
            "--renew-anon-volumes",
            "-V",
            "--attach-dependencies",
            "--timestamps",
            "--menu",
        }
    ),
    values=frozenset(
        {
            "-t",
            "--timeout",
            "--scale",
            "--pull",
            "--exit-code-from",
            "--attach",
            "--no-attach",
        }
    ),
)
COMPOSE_DOWN = ToolFlags(
    booleans=frozenset({"-v", "--volumes", "--remove-orphans"}),
    values=frozenset({"-t", "--timeout", "--rmi"}),
)
COMPOSE_LOGS = ToolFlags(
    booleans=frozenset(
        {"-f", "--follow", "-t", "--timestamps", "--no-color", "--no-log-prefix"}
    ),
    values=frozenset({"--tail", "-n", "--since", "--until", "--index"}),
)
COMPOSE_BUILD = ToolFlags(
    booleans=frozenset(
        {"--no-cache", "--pull", "--push", "-q", "--quiet", "--with-dependencies"}
    ),
    values=frozenset(
        {
            "--build-arg",
            "--progress",
            "--ssh",
        }
    ),
)
COMPOSE_RUN = ToolFlags(
    booleans=frozenset(
        {
            "--rm",
            "-d",
            "--detach",
            "--no-deps",
            "-T",
            "--no-TTY",
            "--service-ports",
            "--use-aliases",
            "--quiet-pull",
        }
    ),
    values=frozenset(
        {
            "-e",
            "--env",
            "--name",
            "-u",
            "--user",
            "-w",
            "--workdir",
            "--entrypoint",
            "--volume",
            "-v",
            "--publish",
            "-p",
            "--cap-add",
            "--cap-drop",
            "--label",
            "-l",
            "--env-from-file",
        }
    ),
)
COMPOSE_PS = ToolFlags(
    booleans=frozenset({"-q", "--quiet", "-a", "--all", "--services"}),
    values=frozenset({"--format", "--filter", "--status", "--orphans"}),
)
COMPOSE_PULL = ToolFlags(
    booleans=frozenset(
        {
            "--ignore-pull-failures",
            "--include-deps",
            "-q",
            "--quiet",
            "--ignore-buildable",
        }
    ),
    values=frozenset({"--policy"}),
)
COMPOSE_COMMANDS = {
    "up": COMPOSE_UP,
    "down": COMPOSE_DOWN,
    "logs": COMPOSE_LOGS,
    "build": COMPOSE_BUILD,
    "run": COMPOSE_RUN,
    "ps": COMPOSE_PS,
    "pull": COMPOSE_PULL,
}
# Regenerated from the real `psql --help` (`postgres:18`, the image every deploy
# recipe names; identical in the 15/16/17 clients a distro ships). psql's help
# prints its *complete* option list — getopt_long rejects anything else — so
# this table is checked against that text on every gate run by
# `assert_flag_tables()`. Do not add a flag here without pasting the `--help`
# line that documents it: `-C`, `-g` and `--on-error-stop` were here before and
# psql has never accepted any of them (it prints `invalid option -- 'C'`).
PSQL = ToolFlags(
    booleans=frozenset(
        {
            "-a",
            "--echo-all",
            "-A",
            "--no-align",
            "-b",
            "--echo-errors",
            "--csv",
            "-e",
            "--echo-queries",
            "-E",
            "--echo-hidden",
            "-H",
            "--html",
            "-l",
            "--list",
            "-n",
            "--no-readline",
            "-0",
            "--record-separator-zero",
            "-q",
            "--quiet",
            "-1",
            "--single-transaction",
            "-s",
            "--single-step",
            "-S",
            "--single-line",
            "-t",
            "--tuples-only",
            "-w",
            "--no-password",
            "-W",
            "--password",
            "-x",
            "--expanded",
            "-X",
            "--no-psqlrc",
            "-z",
            "--field-separator-zero",
            "-?",
            "--help",
            "-V",
            "--version",
        }
    ),
    values=frozenset(
        {
            "-c",
            "--command",
            "-d",
            "--dbname",
            "-f",
            "--file",
            "-h",
            "--host",
            "-p",
            "--port",
            "-U",
            "--username",
            "--set",
            "-v",
            "--variable",
            "-o",
            "--output",
            "-L",
            "--log-file",
            "-P",
            "--pset",
            "-T",
            "--table-attr",
            "-F",
            "--field-separator",
            "-R",
            "--record-separator",
        }
    ),
    paths=frozenset({"-f", "--file"}),
)
DOCTL_GLOBAL = ToolFlags(
    booleans=frozenset({"-v", "--verbose", "--trace", "-h", "--help"}),
    values=frozenset(
        {"-t", "--access-token", "--api-url", "--config", "--output", "-o"}
    ),
)
DOCTL_COMMANDS = {
    ("apps", "create"): ToolFlags(
        booleans=frozenset({"--wait", "--no-header"}),
        values=frozenset({"--spec", "--format"}),
        paths=frozenset({"--spec"}),
    ),
    ("apps", "update"): ToolFlags(
        booleans=frozenset({"--wait", "--no-header"}),
        values=frozenset({"--spec", "--format"}),
        paths=frozenset({"--spec"}),
    ),
    ("apps", "list"): ToolFlags(
        booleans=frozenset({"--no-header"}), values=frozenset({"--format"})
    ),
    ("apps", "get"): ToolFlags(values=frozenset({"--format"})),
    ("apps", "logs"): ToolFlags(
        booleans=frozenset({"-f", "--follow"}),
        values=frozenset({"--type", "--deployment"}),
    ),
    ("apps", "delete"): ToolFlags(booleans=frozenset({"--force", "--wait"})),
    ("apps", "spec"): ToolFlags(values=frozenset({"--format"})),
    ("registry", "login"): ToolFlags(
        booleans=frozenset({"--read-only"}), values=frozenset({"--expiry-seconds"})
    ),
    ("registry", "logout"): ToolFlags(),
    ("registry", "create"): ToolFlags(
        values=frozenset({"--region", "--subscription-tier"})
    ),
    ("registry", "delete"): ToolFlags(booleans=frozenset({"--force"})),
    ("auth", "init"): ToolFlags(booleans=frozenset({"--interactive"})),
    ("auth", "switch"): ToolFlags(),
    ("auth", "list"): ToolFlags(),
    ("account", "get"): ToolFlags(values=frozenset({"--format"})),
    ("compute", "ssh"): ToolFlags(
        booleans=frozenset({"-L"}),
        values=frozenset({"--ssh-key-path", "--ssh-user", "--remote-command"}),
    ),
}


def _flag_name(word: str) -> str:
    return word.split("=", 1)[0]


@dataclass(frozen=True, slots=True)
class FlagAudit:
    """One table, and the real command whose `--help` is the referee for it.

    `gate` marks the tables the gate re-checks on every run: `psql` prints its
    *complete* option list (getopt_long accepts nothing it does not print) and
    its flag set is the same in psql 15/16/17/18, so `--help` containment is
    exact there — a flag that table holds and psql does not print is invented,
    which is how `-C`, `-g` and `--on-error-stop` passed every command but could
    never run. Docker's help hides compatibility aliases (`--net`,
    `--disable-content-trust`, `-h`) and gains or loses flags between CLI
    releases, so containment alone would red the gate for a host's version
    rather than for the table; those tables are audited by `--audit-flags`
    instead, where a flag missing from `--help` must at least be *accepted* by
    the installed binary.
    """

    label: str
    argv: tuple[str, ...]
    table: ToolFlags
    gate: bool = False


FLAG_AUDIT: tuple[FlagAudit, ...] = (
    FlagAudit("docker", ("docker",), DOCKER_GLOBAL),
    FlagAudit("docker run", ("docker", "run"), DOCKER_RUN),
    FlagAudit("docker build", ("docker", "build"), DOCKER_BUILD),
    FlagAudit("docker push", ("docker", "push"), DOCKER_PUSH),
    FlagAudit("docker pull", ("docker", "pull"), DOCKER_PULL),
    FlagAudit("docker logs", ("docker", "logs"), DOCKER_LOGS),
    FlagAudit("docker exec", ("docker", "exec"), DOCKER_EXEC),
    FlagAudit("docker image ls", ("docker", "image", "ls"), DOCKER_IMAGE),
    FlagAudit("docker compose", ("docker", "compose"), COMPOSE_GLOBAL),
    FlagAudit("docker compose up", ("docker", "compose", "up"), COMPOSE_UP),
    FlagAudit("docker compose down", ("docker", "compose", "down"), COMPOSE_DOWN),
    FlagAudit("docker compose logs", ("docker", "compose", "logs"), COMPOSE_LOGS),
    FlagAudit("docker compose build", ("docker", "compose", "build"), COMPOSE_BUILD),
    FlagAudit("docker compose run", ("docker", "compose", "run"), COMPOSE_RUN),
    FlagAudit("docker compose ps", ("docker", "compose", "ps"), COMPOSE_PS),
    FlagAudit("docker compose pull", ("docker", "compose", "pull"), COMPOSE_PULL),
    FlagAudit("psql", ("psql",), PSQL, gate=True),
)

# A flag in a help text: `-a`, `--echo-all`, but not the `-x` in `--no-align`.
_HELP_FLAG = re.compile(r"(?<![\w-])(--?[A-Za-z0-9][\w-]*)")
_VALUE_FILLER = "x"
_AUDIT_TIMEOUT = 30.0


def _audit_env(home: Path) -> dict[str, str]:
    """A scrubbed environment for one audit command: nothing runs, nothing is read.

    `DOCKER_HOST` points at a socket that cannot exist, so even a flag the audit
    only *parses* can never reach a daemon and start, pull or run a container.
    """
    return {
        "HOME": str(home),
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "TMPDIR": str(home),
        "DOCKER_HOST": "unix:///nonexistent/blastit-flag-audit.sock",
        "DOCKER_CLI_HINTS": "false",
        "NO_COLOR": "1",
    }


def _audit_run(argv: list[str], home: Path) -> subprocess.CompletedProcess[str] | None:
    """Run one audit command in `home`; `None` when the executable is not here."""
    try:
        return subprocess.run(
            argv,
            cwd=str(home),
            env=_audit_env(home),
            capture_output=True,
            text=True,
            timeout=_AUDIT_TIMEOUT,
            check=False,
        )
    except FileNotFoundError:
        return None


def _help_flags(audit: FlagAudit, home: Path) -> frozenset[str] | None:
    """The flags `<tool> --help` prints, or `None` when that help is not here."""
    completed = _audit_run([*audit.argv, "--help"], home)
    if completed is None or completed.returncode != 0:
        return None  # a tool the host does not have (`docker compose` w/o plugin)
    return frozenset(_HELP_FLAG.findall(completed.stdout + completed.stderr))


def _accepts(audit: FlagAudit, flag: str, takes_value: bool, home: Path) -> bool:
    """Does the real command parse `flag`? A trailing `--help` keeps it side-effect free."""
    words = [*audit.argv, flag, *([_VALUE_FILLER] if takes_value else []), "--help"]
    completed = _audit_run(words, home)
    return completed is not None and completed.returncode == 0


def assert_flag_tables(gate_only: bool = False) -> str:
    """Every flag the gate accepts must be one the real tool accepts.

    The referee is the installed binary: its `--help` text, plus one parsed
    command per flag the text does not print (a hidden compatibility alias) so a
    hand-typed table entry cannot survive a tool that never heard of it. This is
    the check that would have caught `psql -C`: `psql --help` has no `-C`, and
    `psql -C x --help` exits 1 with `invalid option -- 'C'`.
    """
    checked: list[str] = []
    missing: list[str] = []
    home = Path(tempfile.mkdtemp(prefix="flag-audit-"))
    try:
        for audit in FLAG_AUDIT:
            if gate_only and not audit.gate:
                continue
            printed = _help_flags(audit, home)
            if printed is None:
                missing.append(audit.argv[0])
                continue
            for flag in sorted(
                audit.table.booleans | audit.table.values | audit.table.paths
            ):
                if flag in printed:
                    continue
                if _accepts(audit, flag, flag in audit.table.values, home):
                    continue
                raise Failure(
                    f"`{audit.label} {flag}` is in the harness's `{audit.label}` "
                    f"flag table, but `{audit.label} --help` does not print it and "
                    f"`{' '.join([*audit.argv, flag])}` rejects it. Delete it from "
                    "the table (a command the page cannot run must fail here) or "
                    "replace it with the flag the tool really has"
                )
            checked.append(audit.label)
    finally:
        shutil.rmtree(home, ignore_errors=True)
    summary = f"flag tables checked against --help: {', '.join(checked)}"
    if missing:
        summary += f" (not installed here: {', '.join(sorted(set(missing)))})"
    return summary


# Files a page tells the reader to create in their own project, so the gate can
# neither have them nor expect the page to show them. `blastit.toml` is the one
# file the product is about (`./blastit.toml` is the documented default); every
# other file a flag names is either in this tree or shown on the page.
READER_FILES = frozenset({"blastit.toml"})


def _documented_path(value: str, titles: frozenset[str], line: str, flag: str) -> int:
    """Resolve a file a flag names; count the ones that belong to the reader.

    A workspace-relative path must exist, or be a file the page itself shows (the
    `compose.yaml` above the command). An absolute path outside the repository
    belongs to the reader's host and is not guessed at — the verdict line says
    how many were left unchecked. A shell variable is the reader's too.
    """
    if not value or "$" in value or value.startswith("-"):
        return 0
    path = Path(value)
    if path.is_absolute():
        try:
            inside = path.relative_to(REPO)
        except ValueError:
            return 1
        if not (REPO / inside).exists():
            raise Failure(f"`{line}`: `{flag} {value}` does not exist in the workspace")
        return 0
    if (
        (REPO / path).exists()
        or value in titles
        or (
            path.parent == Path(".")
            and (path.name in titles or path.name in READER_FILES)
        )
    ):
        return 0
    raise Failure(
        f"`{line}`: `{flag} {value}` names a file that neither exists in the "
        f"workspace nor is a file the page shows "
        f"({', '.join(sorted(titles)) or 'none'})"
    )


def _flag(
    word: str,
    following: list[str],
    spec: ToolFlags,
    line: str,
    tool: str,
    titles: frozenset[str],
) -> tuple[int, int]:
    """Validate one flag; return (words consumed as its value, absolute paths skipped)."""
    if "=" in word:
        name, value = word.split("=", 1)
        if name in spec.values:
            return (
                (0, _documented_path(value, titles, line, name))
                if name in spec.paths
                else (0, 0)
            )
        if name in spec.booleans:
            return 0, 0
        raise Failure(f"`{line}`: `{tool} {name}` is not a flag `{tool}` accepts")
    if len(word) > 2 and not word.startswith("--"):
        letters = ["-" + character for character in word[1:]]
        if all(letter in spec.booleans for letter in letters):
            return 0, 0  # `-it` is `-i -t`
    if word in spec.values:
        if not following:
            raise Failure(f"`{line}`: `{tool} {word}` has no value")
        skipped = (
            _documented_path(following[0], titles, line, word)
            if word in spec.paths
            else 0
        )
        return 1, skipped
    if word in spec.booleans:
        return 0, 0
    raise Failure(f"`{line}`: `{tool} {word}` is not a flag `{tool}` accepts")


def _walk_tool(
    words: list[str],
    spec: ToolFlags,
    line: str,
    tool: str,
    titles: frozenset[str] = frozenset(),
) -> tuple[list[str], int]:
    positionals: list[str] = []
    skipped = 0
    at = 0
    while at < len(words):
        word = words[at]
        if word == "--":
            positionals.extend(words[at + 1 :])
            break
        if not word.startswith("-") or word == "-":
            positionals.append(word)
            at += 1
            continue
        consumed, extra = _flag(word, words[at + 1 :], spec, line, tool, titles)
        at += 1 + consumed
        skipped += extra
    return positionals, skipped


def _leading(
    words: list[str],
    spec: ToolFlags,
    line: str,
    tool: str,
    titles: frozenset[str] = frozenset(),
) -> tuple[list[str], int]:
    """Consume the flags before the first positional word; return the rest."""
    skipped = 0
    at = 0
    while at < len(words) and words[at].startswith("-") and words[at] != "-":
        consumed, extra = _flag(words[at], words[at + 1 :], spec, line, tool, titles)
        at += 1 + consumed
        skipped += extra
    return words[at:], skipped


def _static_docker(words: list[str], line: str, titles: frozenset[str]) -> int:
    rest, skipped = _leading(words[1:], DOCKER_GLOBAL, line, "docker", titles)
    if not rest:
        raise Failure(f"`{line}`: `docker` names no command")
    command = rest[0]
    tail = rest[1:]
    if command == "compose":
        rest, more = _leading(tail, COMPOSE_GLOBAL, line, "docker compose", titles)
        skipped += more
        if not rest:
            raise Failure(f"`{line}`: `docker compose` names no command")
        subcommand = rest[0]
        if subcommand not in COMPOSE_COMMANDS:
            raise Failure(
                f"`{line}`: `docker compose {subcommand}` is not a command this "
                "harness checks"
            )
        _, more = _walk_tool(
            rest[1:],
            COMPOSE_COMMANDS[subcommand],
            line,
            f"docker compose {subcommand}",
            titles,
        )
        return skipped + more
    if command not in DOCKER_COMMANDS:
        raise Failure(
            f"`{line}`: `docker {command}` is not a command this harness checks"
        )
    positionals, more = _walk_tool(
        tail, DOCKER_COMMANDS[command], line, f"docker {command}", titles
    )
    if command in {"run", "build", "push", "pull", "exec"} and not positionals:
        raise Failure(f"`{line}`: `docker {command}` names no image or container")
    return skipped + more


def _static_psql(words: list[str], line: str, titles: frozenset[str]) -> int:
    positionals, skipped = _walk_tool(words[1:], PSQL, line, "psql", titles)
    named = any(_flag_name(word) in ("-d", "--dbname") for word in words[1:])
    if not positionals and not named:
        raise Failure(f"`{line}`: `psql` names no database")
    return skipped


def _static_doctl(words: list[str], line: str, titles: frozenset[str]) -> int:
    rest, skipped = _leading(words[1:], DOCTL_GLOBAL, line, "doctl", titles)
    if len(rest) < 2:
        raise Failure(
            f"`{line}`: `doctl {' '.join(rest)}` is not a command this harness checks"
        )
    command = (rest[0], rest[1])
    if command not in DOCTL_COMMANDS:
        raise Failure(
            f"`{line}`: `doctl {' '.join(command)}` is not a command this harness "
            "checks; add it to the harness's doctl table with the flags `doctl "
            "--help` prints"
        )
    tail = rest[2:]
    _, more = _walk_tool(
        tail,
        DOCTL_COMMANDS[command].plus(DOCTL_GLOBAL),
        line,
        f"doctl {' '.join(command)}",
        titles,
    )
    skipped += more
    if command == ("apps", "create") and not any(
        _flag_name(word) == "--spec" for word in tail
    ):
        raise Failure(f"`{line}`: `doctl apps create` needs `--spec <file>`")
    return skipped


def _static_command(
    command: str, cli: CliHelp, titles: frozenset[str] = frozenset()
) -> int:
    """Check every executable, script path, flag and file a static fence names.

    Nothing is ignored for being unfamiliar: an executable the harness cannot
    identify, a `uv run` target that does not exist, a flag the real command does
    not have, or a file that is neither in the tree nor on the page is a failure,
    because a deployment recipe that cannot be followed is a documentation bug
    even when it cannot be run here. Returns the number of absolute paths left to
    the reader's host.
    """
    skipped = 0
    for line in _logical_lines(command):
        head = line.split("\n", 1)[0]
        head = re.sub(r"(?<!\S)#.*$", "", head).strip()
        head = _unwrap_substitution(head)
        if head is None:
            continue
        for words in _segments(head):
            skipped += _static_segment(words, cli, line, titles)
    return skipped


def _unwrap_substitution(head: str) -> str | None:
    """`TAG=$(git rev-parse …)` is `git rev-parse …`; `FOO=bar` runs nothing."""
    while True:
        match = _SUBSTITUTION.match(head)
        if match is not None:
            head = match.group(1).strip()
            continue
        if _ASSIGNMENT.match(head) and "$(" not in head:
            return None
        return head


def _segments(head: str) -> list[list[str]]:
    """Split one shell line into simple commands (`&&`, `;`, `|`, `||`)."""
    try:
        lexer = shlex.shlex(head, posix=True, punctuation_chars=";&|")
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError as error:
        raise Failure(f"cannot parse `{head}`: {error}") from error
    segments: list[list[str]] = []
    current: list[str] = []
    for token in tokens:
        if token and set(token) <= {";", "&", "|"}:
            if current:
                segments.append(current)
                current = []
            continue
        current.append(token)
    if current:
        segments.append(current)
    return segments


def _skip_flags(words: list[str], value: frozenset[str]) -> list[str]:
    """Drop `words`' leading flags (and the values of the ones that take one)."""
    rest = list(words)
    while rest and rest[0].startswith("-"):
        word = rest[0]
        name = word.split("=", 1)[0]
        takes_value = name in value and "=" not in word
        rest = rest[1:]
        if takes_value:
            if not rest:
                raise Failure(f"flag `{word}` has no value in `{' '.join(words)}`")
            rest = rest[1:]
    return rest


def _static_segment(
    words: list[str], cli: CliHelp, line: str, titles: frozenset[str] = frozenset()
) -> int:
    words = list(words)
    while words:
        token = words[0]
        if token in {"export", "sudo"}:
            words = words[1:]
            continue
        if _ASSIGNMENT.match(token):
            words = words[1:]
            continue
        if token == "env":
            words = _skip_flags(words[1:], ENV_VALUE_FLAGS)
            continue
        if token == "setpriv":
            words = _skip_flags(words[1:], SETPRIV_VALUE_FLAGS)
            continue
        if token == "uv":
            words = _uv(words, line)
            if words is None:
                return 0
            continue
        break
    if not words:
        return 0
    executable = words[0]
    if executable == "blastit":
        _static_blastit(words, cli, line)
    elif executable == "blastit-manifest":
        return _static_manifest(words, cli, line, titles)
    elif executable == "docker":
        return _static_docker(words, line, titles)
    elif executable == "doctl":
        return _static_doctl(words, line, titles)
    elif executable == "psql":
        return _static_psql(words, line, titles)
    elif executable == "git":
        if len(words) < 2 or words[1] not in GIT_SUBCOMMANDS:
            raise Failure(f"`{line}`: `git {words[1:2] or ''}` is not a known command")
    elif executable in FILTERS:
        return 0
    else:
        raise Failure(
            f"`{line}` runs `{executable}`, which this harness cannot check. Use a "
            "command it knows (`blastit`, `blastit-manifest`, `docker`, `doctl`, "
            "`psql`, `git`, `setpriv`, `env`, `uv`) or make the fence executable"
        )
    return 0


def _uv(words: list[str], line: str) -> list[str] | None:
    """Unwrap `uv run …` to the command it runs; `None` when it runs nothing."""
    if len(words) < 2 or words[1] not in UV_SUBCOMMANDS:
        raise Failure(f"`{line}`: `uv {words[1:2] or ''}` is not a known uv command")
    if words[1] != "run":
        return None
    rest = words[2:]
    while rest and rest[0].startswith("-"):
        word = rest[0]
        name = word.split("=", 1)[0]
        if name in UV_RUN_VALUE_FLAGS:
            rest = rest[1:]
            if "=" not in word:
                if not rest:
                    raise Failure(f"`{line}`: `{word}` has no value")
                rest = rest[1:]
            continue
        if name in UV_RUN_BOOLEAN_FLAGS:
            rest = rest[1:]
            continue
        raise Failure(f"`{line}`: `uv run {word}` is not a flag this harness follows")
    if not rest:
        raise Failure(f"`{line}`: `uv run` names no command")
    target = rest[0]
    if target in {"blastit", "blastit-manifest"}:
        return rest
    if target.endswith(".py"):
        _require_script(target, line)
        return None
    if target == "python":
        return _uv_python(rest[1:], line)
    if (REPO / target).is_file():
        return None
    raise Failure(
        f"`{line}`: `uv run {target}` names a script that does not exist "
        f"({REPO / target}) and is not a command this harness knows"
    )


def _uv_python(args: list[str], line: str) -> list[str] | None:
    rest = [word for word in args if not word.startswith("-")]
    if args[:1] == ["-m"]:
        return None
    if not rest:
        raise Failure(f"`{line}`: `uv run python` names no script")
    _require_script(rest[0], line)
    return None


def _require_script(target: str, line: str) -> None:
    path = Path(target)
    candidate = path if path.is_absolute() else REPO / path
    if not candidate.is_file():
        raise Failure(f"`{line}` names the script `{target}`, which does not exist")


def _static_blastit(words: list[str], cli: CliHelp, line: str) -> None:
    rest = words[1:]
    if not rest:
        return
    command = rest[0]
    if command.startswith("-"):
        _check_flags(rest, cli.blastit, line, "blastit")
        _check_presets(rest, cli, line)
        return
    if command == "dev":
        _check_flags(rest[1:], cli.dev, line, "blastit dev")
        return
    if command in ("check", "test"):
        args = rest[1:]
        if "--" in args:
            args = args[: args.index("--")]
        _check_flags(args, cli.check, line, f"blastit {command}")
        return
    raise Failure(f"`{line}`: `blastit {command}` is not one of dev, check, test")


def _static_manifest(
    words: list[str], cli: CliHelp, line: str, titles: frozenset[str] = frozenset()
) -> int:
    if words[1:2] != ["fold"]:
        raise Failure(f"`{line}` does not use `blastit-manifest fold`")
    args = words[2:]
    _check_flags(args, cli.fold, line, "blastit-manifest fold")
    if not any(word.split("=", 1)[0] == "--manifest" for word in args):
        raise Failure(
            f"`{line}` has no `--manifest PATH`; the page says it is required"
        )
    skipped = 0
    for index, word in enumerate(args):
        if word == "--manifest" and index + 1 < len(args):
            skipped += _documented_path(args[index + 1], titles, line, "--manifest")
        elif word.startswith("--manifest="):
            skipped += _documented_path(
                word.split("=", 1)[1], titles, line, "--manifest"
            )
    return skipped


def _check_flags(
    args: list[str], allowed: frozenset[str], line: str, command: str
) -> None:
    for word in args:
        if not word.startswith("-") or word == "-":
            continue
        name = word.split("=", 1)[0]
        if name not in allowed:
            raise Failure(
                f"`{line}`: `{command} {name}` is not in `{command} --help` "
                f"(it lists {', '.join(sorted(allowed))})"
            )


def _check_presets(args: list[str], cli: CliHelp, line: str) -> None:
    for index, word in enumerate(args):
        if word == "--preset" and index + 1 < len(args):
            name = args[index + 1]
        elif word.startswith("--preset="):
            name = word.split("=", 1)[1]
        else:
            continue
        if name not in cli.presets:
            raise Failure(
                f"`{line}`: preset {name!r} is not in `blastit --help` "
                f"(it lists {', '.join(sorted(cli.presets))})"
            )


def _logical_lines(command: str) -> list[str]:
    """The command's physical lines, with `\\`-continuations joined.

    A heredoc is one unit: its body is data for another program (SQL), not a
    list of commands, so it stays with the line that opened it.
    """
    logical: list[str] = []
    pending = ""
    heredoc: str | None = None
    for raw in command.splitlines():
        line = raw.strip()
        if heredoc is not None:
            pending += "\n" + raw
            if line == heredoc:
                heredoc = None
                logical.append(pending.strip())
                pending = ""
            continue
        pending += (" " if pending else "") + raw
        marker = re.search(r"<<-?[ \t]*['\"]?([A-Za-z_][A-Za-z0-9_]*)['\"]?", pending)
        if marker is not None:
            heredoc = marker.group(1)
            continue
        if raw.rstrip().endswith("\\"):
            pending = pending[:-1]
            continue
        logical.append(pending.strip())
        pending = ""
    if pending.strip():
        logical.append(pending.strip())
    return [line for line in logical if line]


def _page_manifest(fence: Fence) -> str:
    """The `blastit.toml` block the page itself shows before this fence.

    The page's own manifest is what a developer copies; a fixture manifest would
    prove nothing about it. The block must still be the one this mode knows how
    to judge: a route whose `[route.db]` grants nothing, which the page says gets
    a listener that denies every statement.
    """
    text = (REPO / fence.page).read_text(encoding="utf-8")
    blocks = [
        (text.count("\n", 0, block.start()) + 1, block)
        for block in re.finditer(
            r"^```toml[^\n]*\n(.*?)^```$", text, re.DOTALL | re.MULTILINE
        )
    ]
    before = [(line, block) for line, block in blocks if line < fence.line]
    if not before:
        raise Failure(
            f"{fence.page} fence {fence.index}: no `toml` block precedes the fence; "
            "the page must show the manifest this command reads"
        )
    body = before[-1][1].group(1)
    try:
        document = tomllib.loads(body)
    except tomllib.TOMLDecodeError as error:
        raise Failure(
            f"{fence.page}: the manifest block does not parse: {error}"
        ) from error
    empty = [
        route.get("name", "?")
        for route in document.get("route", [])
        if not route.get("db", {}).get("queries")
    ]
    if not empty:
        raise Failure(
            f"{fence.page}: the manifest block no longer shows a route with "
            "`queries = []`, which is what this fence's next paragraph promises"
        )
    return body


@dataclass(frozen=True, slots=True)
class Snippet:
    """One fenced block of a page: its language, its `title=`, its body."""

    page: str
    language: str
    title: str
    line: int
    body: str

    @property
    def where(self) -> str:
        return f"{self.page}:{self.line}"


_BLOCK_TITLE = re.compile(r'title="([^"]+)"')


def documented_snippets(page: str, text: str) -> list[Snippet]:
    """Every fenced block of a page, whatever language, with its `title=`.

    The recognition is `fences.scan_page`'s, not a second regex's: a block indented
    inside a list item or an admonition (`docs/local.md`'s copy-me `toml` block) is
    dedented by its own indentation and parsed here like a column-0 one, and a
    block the scanner reports as unclosed cannot silently drop the snippets after
    it — the page's inventory check fails on that, before a fence runs. `where`
    names the block's first body line, which is the line a failure prints.
    """
    blocks, _ = scan_page(page, text)
    found = []
    for block in blocks:
        title = _BLOCK_TITLE.search(block.info)
        found.append(
            Snippet(
                page=page,
                language=block.language.lower(),
                title=title.group(1) if title else "",
                line=block.line + 1,
                body=block.body,
            )
        )
    return found


def _mapping_key(text: str) -> tuple[str, str] | None:
    """Split `key: value` at the first colon outside quotes and flow brackets."""
    quote: str | None = None
    depth = 0
    at = 0
    while at < len(text):
        character = text[at]
        if quote is not None:
            if character == "\\" and quote == '"':
                at += 2
                continue
            if character == quote:
                quote = None
        elif character in "\"'":
            quote = character
        elif character in "{[":
            depth += 1
        elif character in "}]":
            depth -= 1
        elif (
            character == ":"
            and depth == 0
            and (at + 1 == len(text) or text[at + 1] == " ")
        ):
            return text[:at].strip(), text[at + 1 :].strip()
        at += 1
    return None


def _scan_flow(text: str, block: Snippet, number: int, depth: int) -> int:
    """Track `{`/`[` depth across lines, ignoring quoted and escaped characters."""
    quote: str | None = None
    at = 0
    while at < len(text):
        character = text[at]
        if quote is not None:
            if character == "\\" and quote == '"':
                at += 2
                continue
            if character == quote:
                quote = None
        elif character in "\"'":
            quote = character
        elif character in "{[":
            depth += 1
        elif character in "}]":
            depth -= 1
            if depth < 0:
                raise Failure(
                    f"{block.where}: line {number} closes a `{character}` it never "
                    f"opened: {text.strip()}"
                )
        at += 1
    if quote is not None:
        raise Failure(
            f"{block.where}: line {number} has an unterminated {quote} quote: "
            f"{text.strip()}"
        )
    return depth


def _yaml_structure(block: Snippet) -> None:
    """A YAML *structure* lint, not a parser (this gate has no dependencies).

    It checks the shape a compose file, a workflow or a Kubernetes document has
    to have: space indentation only, `key: value` / `- item` / flow syntax on
    every content line, flow collections and quotes that close, block scalars
    that end, and a truncated top-level key with no children. Anything the
    documents here do is covered; anything YAML allows beyond that is not
    claimed.
    """
    body = block.body
    depth = 0
    pending: tuple[int, str, int] | None = None
    scalar: tuple[int, str] | None = None
    for offset, raw in enumerate(body.splitlines()):
        number = offset + 1
        stripped = raw.strip()
        indent = len(raw) - len(raw.lstrip())
        if scalar is not None:
            if not stripped or indent > scalar[0]:
                continue
            scalar = None
        if not stripped or stripped.startswith("#"):
            continue
        if "\t" in raw[:indent]:
            raise Failure(
                f"{block.where}: line {number} is indented with a tab: {stripped}"
            )
        if depth:
            depth = _scan_flow(raw, block, number, depth)
            continue
        if pending is not None:
            if indent <= pending[0]:
                raise Failure(
                    f"{block.where}: `{pending[1]}` on line {pending[2]} has no "
                    "value and no indented children"
                )
            pending = None
        if stripped.startswith("-"):
            # A sequence item is a bare scalar, a flow mapping or the first key
            # of a mapping, so its content is scanned but never demanded to be a key.
            content = stripped[1:].strip()
            depth = _scan_flow(content, block, number, 0)
            key = _mapping_key(content) if content else None
            if key is not None and key[1][:1] in ("|", ">"):
                scalar = (indent, key[1][0])
            elif not content and not indent:
                pending = (indent, "- item", number)
            elif key is not None and not key[1] and not depth and not indent:
                pending = (indent, key[0], number)
            continue
        key = _mapping_key(stripped)
        if key is None:
            raise Failure(
                f"{block.where}: line {number} is neither `key: value` nor "
                f"`- item`: {stripped}"
            )
        name, value = key
        depth = _scan_flow(stripped, block, number, 0)
        if value[:1] in ("|", ">"):
            scalar = (indent, value[0])
        elif not value and not depth and not indent:
            # A null value is legal YAML, so only a *top-level* key with neither
            # a value nor children is reported: that is the truncated document.
            pending = (indent, name, number)
    if pending is not None:
        raise Failure(
            f"{block.where}: `{pending[1]}` on line {pending[2]} has no value and "
            "no indented children: the document is truncated"
        )
    if depth:
        raise Failure(f"{block.where}: the block ends inside an unclosed `{{` or `[`")
    if scalar is not None:
        raise Failure(f"{block.where}: a `{scalar[1]}` block scalar never ends")


def _ini_document(block: Snippet) -> None:
    """Parse an INI document, or a dotenv file, the way the recipe uses it."""
    text = block.body.strip("\n")
    if re.search(r"^[ \t]*\[[^\]\n]+\][ \t]*$", text, re.MULTILINE):
        parser = configparser.ConfigParser(interpolation=None, strict=True)
        try:
            parser.read_string(text + "\n")
        except configparser.Error as error:
            raise Failure(
                f"{block.where}: the INI block does not parse: {error}"
            ) from error
        if not parser.sections():
            raise Failure(f"{block.where}: the INI block has no section")
        return
    for number, raw in enumerate(text.splitlines(), start=1):
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if "=" not in stripped:
            raise Failure(
                f"{block.where}: line {number} is neither `KEY=value` nor a "
                f"`[section]`: {stripped}"
            )
        name, value = stripped.split("=", 1)
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name.strip()):
            raise Failure(
                f"{block.where}: line {number} has no variable name: {stripped}"
            )
        if not value.strip():
            raise Failure(
                f"{block.where}: line {number} sets `{name.strip()}` to nothing"
            )


def _static_toml(block: Snippet) -> None:
    try:
        tomllib.loads(block.body)
    except tomllib.TOMLDecodeError as error:
        raise Failure(
            f"{block.where}: the TOML block does not parse: {error}"
        ) from error


def _sql_block(block: Snippet) -> None:
    """A SQL snippet a reader copies must be whole, not an ellipsis."""
    text = block.body.strip()
    if not text:
        raise Failure(f"{block.where}: the SQL block is empty")
    if "…" in block.body:
        raise Failure(
            f"{block.where}: the SQL block contains `…`, which a reader cannot copy "
            "and run"
        )
    statements = [
        line.strip()
        for line in text.splitlines()
        if line.strip() and not line.strip().startswith("--")
    ]
    if statements and not statements[-1].endswith(";"):
        raise Failure(
            f"{block.where}: the SQL block's last statement does not end with `;`: "
            f"{statements[-1]}"
        )


BLOCK_CHECKS = {
    "yaml": _yaml_structure,
    "yml": _yaml_structure,
    "ini": _ini_document,
    "toml": _static_toml,
    "sql": _sql_block,
}


def _static_blocks(fence: Fence) -> frozenset[str]:
    """Parse every snippet the page shows; return the file names it shows.

    The titles (`yaml title="compose.yaml"`) are how a command that reads a file
    the reader has not created yet is still checked: `doctl apps create --spec
    app.yaml` is only honest if the page shows that file, and that is what makes
    a workspace-relative path resolvable without the reader's directory.
    """
    text = (REPO / fence.page).read_text(encoding="utf-8")
    blocks = documented_snippets(fence.page, text)
    for block in blocks:
        check = BLOCK_CHECKS.get(block.language)
        if check is not None:
            check(block)
    titles = {block.title for block in blocks if block.title}
    for title in list(titles):
        titles.add(Path(title).name)
    return frozenset(titles)


def _static_json_lines(fence: Fence) -> None:
    """A `{`-line in a fence must be JSON, and a boot line must be complete."""
    for offset, raw in enumerate(fence.body.splitlines(), start=1):
        stripped = raw.strip()
        if not stripped.startswith("{"):
            continue
        try:
            document = json.loads(stripped)
        except json.JSONDecodeError as error:
            raise Failure(
                f"{fence.page} fence {fence.index}: the line {offset} starts with "
                f"`{{` but is not JSON ({error}): {stripped}"
            ) from error
        if not isinstance(document, dict) or "event" not in document:
            continue
        missing = [key for key in BOOT_KEYS if key not in document]
        if missing:
            raise Failure(
                f"{fence.page} fence {fence.index}: the documented event line has "
                f"no {', '.join(repr(key) for key in missing)}: {stripped} "
                "(docs/reference/log-events.md documents the five boot fields)"
            )


def main(argv: list[str]) -> int:
    """The three things this module offers outside a full run.

    `--warm-cache` fills this HOME's uv cache with the one pin the offline gate
    reads (it is the only step in this file that may use the network). It is the
    prerequisite `docs/testing/README.md` names. `--provenance` prints the version
    chain and the wheel the gate would test, which is what `make docs-test`'s
    verdict line and a CI log need to agree on. `--audit-flags` compares every
    flag table with the installed `docker`/`docker compose`/`psql`, which is how a
    hand-typed flag that no release accepts (`psql -C`) stays out of the gate.
    """
    import argparse

    parser = argparse.ArgumentParser(
        prog="harness.py",
        description="Prerequisites, provenance and flag tables of the runnable-docs gate.",
    )
    parser.add_argument(
        "--warm-cache",
        action="store_true",
        help=f"install pytest=={PYTEST_VERSION} once, to fill this HOME's uv cache",
    )
    parser.add_argument(
        "--provenance",
        action="store_true",
        help="print the version chain and the wheel under test",
    )
    parser.add_argument(
        "--audit-flags",
        action="store_true",
        help="compare every flag table with the installed tool's own --help",
    )
    options = parser.parse_args(argv)
    if not options.warm_cache and not options.provenance and not options.audit_flags:
        parser.print_help()
        return 2
    try:
        if options.warm_cache:
            print(f"harness: {warm_pytest_cache()}")
        if options.audit_flags:
            print(f"harness: {assert_flag_tables()}")
        if options.provenance:
            version = assert_provenance()
            wheel = wheel_path()
            print(
                f"harness: Cargo.toml {version} = CHANGELOG.md {version} = "
                f"{wheel.name} (sha256 {hashlib.sha256(wheel.read_bytes()).hexdigest()[:12]})"
            )
    except Failure as failure:
        print(f"harness: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
