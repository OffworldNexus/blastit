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
"""

from __future__ import annotations

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
import tempfile
import threading
import time
import tomllib
import urllib.request
import zipfile
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path

from fences import Command, Fence, console_fences
from pgstub import PgStub

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
FIXTURES = HERE / "fixtures"
DIST = REPO / "dist"
PYTEST_VERSION = "9.2.0"
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
# A mode that can return "not executed" must say why in the inventory.
NEEDS_REASON = ACCOUNTED | SUBSTITUTED
MODES = EXECUTING | ACCOUNTED


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


def wheel_path() -> Path:
    wheels = sorted(DIST.glob("blastit-*.whl"), key=lambda item: item.stat().st_mtime)
    if not wheels:
        raise Failure(
            f"{DIST} has no blastit wheel; run scripts/build-wheel.sh (make docs-test does)"
        )
    return wheels[-1]


def interpreter_for(wheel: Path) -> str:
    match = re.search(r"-cp3(\d+)-", wheel.name)
    if match is None:
        raise Failure(f"{wheel.name}: no cp3NN tag to choose an interpreter from")
    return subprocess.check_output(
        ["uv", "python", "find", f"==3.{match.group(1)}"], text=True
    ).strip()


def uv_cache_dir() -> str:
    return subprocess.check_output(["uv", "cache", "dir"], text=True).strip()


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
        self.wheel = wheel_path()
        self.wheel_sha256 = hashlib.sha256(self.wheel.read_bytes()).hexdigest()
        self.python = interpreter_for(self.wheel)
        self.version = self.python.split("python")[-1]
        # `pytest` comes from the global cache (offline); everything that
        # installs the wheel uses a cache this run created empty, so uv cannot
        # serve a *rebuilt* same-version wheel it cached during an earlier run.
        self.global_cache = uv_cache_dir()
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
        env["UV_OFFLINE"] = "1"
        for command in (
            ["uv", "venv", "--python", self.version, ".venv"],
            ["uv", "lock"],
            ["uv", "sync"],
        ):
            self._tool(self.base, env, command, quiet=True)
        self.shared_python = self.base / ".venv" / "bin" / "python"
        self.verify_wheel_install(self.base / ".venv")
        self.install_pytest(self.base, env)

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

    def install_pytest(self, root: Path, env: dict[str, str]) -> None:
        """Install `pytest` from the global uv cache, or fail.

        The gate is offline by contract: reaching the package index inside
        `make check` would make a dangling network a red build and a supply-chain
        hole. A missing cache entry names the one command that fills it. This is
        the only command that reads the global cache, because the wheel install
        must never see it.
        """
        argv = [
            "uv",
            "pip",
            "install",
            "--python",
            str(root / ".venv" / "bin" / "python"),
            f"pytest=={PYTEST_VERSION}",
        ]
        pytest_env = {
            **env,
            "UV_OFFLINE": "1",
            "UV_CACHE_DIR": self.global_cache,
        }
        try:
            self._tool(root, pytest_env, argv, quiet=True)
        except Failure as failure:
            raise Failure(
                f"pytest=={PYTEST_VERSION} is not in the uv cache, and this gate "
                "never reaches the network. Fill the cache once with "
                f"`uv pip install pytest=={PYTEST_VERSION}` and re-run: {failure}"
            ) from failure

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
        return handler(fence, spec)

    # -- modes ------------------------------------------------------------

    def mode_install(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        root = Path(tempfile.mkdtemp(prefix="install-", dir=self.work)) / "app"
        root.mkdir()
        env = self.base_env(root / "home", shared=False)
        env["UV_OFFLINE"] = "1"
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
        reason = spec.reason or "no reason recorded"
        cli = self.cli_help()
        for command in fence.commands():
            _static_command(command.text, cli)
        _static_toml(fence, REPO / fence.page)
        _static_json_lines(fence)
        return f"statically checked ({reason})", False

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
        if not spec.reason:
            raise Failure("an `example` fence needs a written reason in the inventory")
        return f"accounted: {spec.reason}", False

    def mode_install_external(self, fence: Fence, spec: FenceSpec) -> tuple[str, bool]:
        if not spec.reason:
            raise Failure("an `install-external` fence needs a written reason")
        installs_blastit = False
        for command in fence.commands():
            if "uv add" in command.text and "blastit" in command.text:
                installs_blastit = True
        if not installs_blastit:
            raise Failure(
                "an `install-external` fence must name blastit in one of its `uv add` "
                "commands; otherwise it cannot be accounted as an install of this wheel"
            )
        return f"accounted: {spec.reason}", False

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
DOCKER_SUBCOMMANDS = frozenset(
    {"run", "build", "push", "pull", "compose", "logs", "exec", "image"}
)
COMPOSE_SUBCOMMANDS = frozenset({"up", "down", "logs", "build", "run", "ps", "pull"})
COMPOSE_VALUE_FLAGS = frozenset(
    {
        "--env-file",
        "-f",
        "--file",
        "--project-name",
        "-p",
        "--profile",
        "--project-directory",
        "--ansi",
        "--progress",
        "--parallel",
    }
)
DOCTL_SUBCOMMANDS = frozenset(
    {"apps", "registry", "auth", "compute", "databases", "account", "kubernetes"}
)
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


def _static_command(command: str, cli: CliHelp) -> None:
    """Check every executable, script path and flag a non-executed fence names.

    Nothing is ignored for being unfamiliar: an executable the harness cannot
    identify, a `uv run` target that does not exist, or a flag `--help` does not
    list is a failure, because a deployment recipe that cannot be followed is a
    documentation bug even when it cannot be run here.
    """
    for line in _logical_lines(command):
        head = line.split("\n", 1)[0]
        head = re.sub(r"(?<!\S)#.*$", "", head).strip()
        head = _unwrap_substitution(head)
        if head is None:
            continue
        for words in _segments(head):
            _static_segment(words, cli, line)


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


def _static_segment(words: list[str], cli: CliHelp, line: str) -> None:
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
                return
            continue
        break
    if not words:
        return
    executable = words[0]
    if executable == "blastit":
        _static_blastit(words, cli, line)
    elif executable == "blastit-manifest":
        _static_manifest(words, cli, line)
    elif executable == "docker":
        _static_docker(words, line)
    elif executable == "doctl":
        if len(words) < 2 or words[1] not in DOCTL_SUBCOMMANDS:
            raise Failure(
                f"`{line}`: `doctl {words[1:2] or ''}` is not a known command"
            )
    elif executable == "psql":
        if not any(not word.startswith("-") for word in words[1:]) or len(words) < 2:
            raise Failure(f"`{line}`: `psql` without a database")
    elif executable == "git":
        if len(words) < 2 or words[1] not in GIT_SUBCOMMANDS:
            raise Failure(f"`{line}`: `git {words[1:2] or ''}` is not a known command")
    elif executable in FILTERS:
        return
    else:
        raise Failure(
            f"`{line}` runs `{executable}`, which this harness cannot check. Use a "
            "command it knows (`blastit`, `blastit-manifest`, `docker`, `doctl`, "
            "`psql`, `git`, `setpriv`, `env`, `uv`) or make the fence executable"
        )


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


def _static_manifest(words: list[str], cli: CliHelp, line: str) -> None:
    if words[1:2] != ["fold"]:
        raise Failure(f"`{line}` does not use `blastit-manifest fold`")
    args = words[2:]
    _check_flags(args, cli.fold, line, "blastit-manifest fold")
    if not any(word.split("=", 1)[0] == "--manifest" for word in args):
        raise Failure(
            f"`{line}` has no `--manifest PATH`; the page says it is required"
        )


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


def _static_docker(words: list[str], line: str) -> None:
    if len(words) < 2 or words[1] not in DOCKER_SUBCOMMANDS:
        raise Failure(f"`{line}`: `docker {words[1:2] or ''}` is not a known command")
    if words[1] != "compose":
        return
    rest = list(words[2:])
    while rest and rest[0].startswith("-"):
        word = rest[0]
        name = word.split("=", 1)[0]
        takes_value = name in COMPOSE_VALUE_FLAGS and "=" not in word
        rest = rest[1:]
        if takes_value and rest:
            rest = rest[1:]
    if not rest or rest[0] not in COMPOSE_SUBCOMMANDS:
        raise Failure(
            f"`{line}`: `docker compose {rest[0] if rest else ''}` is not a known "
            "command"
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


def _static_toml(fence: Fence, page: Path) -> None:
    if "deploy/" not in fence.page:
        return
    text = page.read_text(encoding="utf-8")
    for block in re.finditer(
        r"^```toml[^\n]*\n(.*?)^```$", text, re.DOTALL | re.MULTILINE
    ):
        try:
            tomllib.loads(block.group(1))
        except tomllib.TOMLDecodeError as error:
            raise Failure(
                f"{fence.page}: a TOML block does not parse: {error}"
            ) from error


def _static_json_lines(fence: Fence) -> None:
    for line in fence.body.splitlines():
        stripped = line.strip()
        if not stripped.startswith("{"):
            continue
        try:
            document = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        if "event" in document:
            for key in ("event", "stage", "seccomp", "privilege", "bounding_set"):
                if key not in document:
                    raise Failure(
                        f"the documented boot line has no {key!r}: {stripped}"
                    )
