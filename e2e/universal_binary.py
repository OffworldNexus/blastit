"""One artifact, four runtimes: the universal binary's acceptance (`MB.42.2.4`).

    uv run --no-project e2e/universal_binary.py \
        --binary /tmp/opencode/install-x/bin/blastit \
        --python 3.13 --python 3.14 --php 8.4 --static --handoff --check

Every leg drives the *same file*: the digest the driver hashes before the first
leg is re-hashed after each one, and every `blastit dev` leg asserts the
interpreter that served it (`sys.version_info` in the body) and that
`--version` still answers from that same file. `--binary` is the bundle
`make install` staged (design §6.1): `bin/blastit` -> `share/blastit/lib/blastit`
with every shim beside it, so no `BLASTIT_PY_SHIM_DIR`/`BLASTIT_PHP_SHIM_DIR` is
set and a bundle that shipped no shim fails here instead of passing on the build
tree's copy.

Legs (one PASS/FAIL/SKIP line each, and one summary):

* `python 3.13`, `python 3.14` — a real WSGI project in its own venv answers 200
  with the interpreter that ran it;
* `php 8.4` — the PHP fixture answers 200 through the bundle's shim and the
  host's `libphp8.4`. A host without either SKIPs with the exact command
  (`make php-sapi` / `apt install libphp8.4-embed`);
* `static` — a mount answers 200 with the file's own bytes;
* `handoff` — a project's `.venv/bin/blastit` marker runs for `dev` while the
  global binary prints nothing, and never runs for production, `--version` or
  `help`;
* `check 3.12` — a CPython 3.12 venv with no per-minor `.so` runs
  `python -m blastit check --json`, which is the whole pure-Python (`>=3.12`)
  half of the one-artifact claim.

A SKIP means "this host cannot prove the claim", never "the claim holds": the
summary exits 1 on any SKIP unless `--allow-skip` is passed, and the gates that
call this harness (`make check`, `make e2e-universal`) never pass it — so a
bundle with no PHP shim cannot leave the epic gate green.

Nothing is mocked: every leg is a real socket, a real `blastit dev` and a real
interpreter; the driver owns only fixtures and a free port. Zero `Mock*Server`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

#: The NTS embed packages the PHP leg may prove, in discovery order: the
#: Debian/Ubuntu name, then Alpine 8.4's per-minor directory. The *shim* always
#: comes from the bundle (`<exe_dir>/libblastit_php<mm>-*.so`), never from the
#: build tree; only `libphp` itself is a host fact the driver has to name.
LIBPHP_84 = (
    "/usr/lib/libphp8.4.so",
    "/usr/local/lib/libphp8.4.so",
    "/usr/lib/php84/libphp.so",
)

WSGI_MANIFEST = """app "universal"

entrypoint python "app:application"

view home GET "/" {}
"""

WSGI_APP = '''"""The app this e2e serves: it says which interpreter and venv ran it.

`universal_marker` exists in this project's `.venv` site-packages and nowhere
else, so importing it is what proves the project's own environment served the
request. `sys.prefix` names the CPython install the shim loaded (stage 0 sets
`PYTHONHOME` to it) and `sys.executable` is the name the embedded runtime was
configured with, so neither one alone answers "which venv?".
"""

import json
import sys

import universal_marker


def application(environ, start_response):
    body = json.dumps(
        {
            "minor": "%d.%d" % sys.version_info[:2],
            "marker": universal_marker.MINOR,
            "executable": sys.executable,
            "prefix": sys.prefix,
            "path": environ.get("PATH_INFO"),
        }
    ).encode()
    start_response(
        "200 OK",
        [("Content-Type", "application/json"), ("Content-Length", str(len(body)))],
    )
    return [body]
'''

STATIC_MANIFEST = 'static "/" {\n    root "public"\n}\n'
STATIC_INDEX = b"<!doctype html><h1>one binary, four runtimes</h1>\n"


class Skip(Exception):
    """A leg this host cannot run, naming the command that would let it."""


class LegFailure(Exception):
    """A leg that ran and did not prove its claim."""


def expect(condition: bool, what: str) -> None:
    """Ends a leg with `what` when `condition` is false."""
    if not condition:
        raise LegFailure(what)


# ---------------------------------------------------------------------------
# processes, the one HTTP client, and the summary
# ---------------------------------------------------------------------------


def clean_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """The environment a leg runs under.

    `VIRTUAL_ENV` is dropped on purpose: `blastit dev` prefers it over the
    `<manifest_dir>/.venv` a leg built, and this driver itself runs from a uv
    environment that exports one. `PYTHONPATH`/`PYTHONHOME` go too, so leg *n*
    cannot pass on the shim or interpreter of leg *n-1*.
    """
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": os.environ.get("HOME", ""),
        "BLASTIT_MEM_HIGH_WATERMARK": "100",
        "BLASTIT_MAX_VIEW_PROCESSES": "8",
    }
    for name in ("TMPDIR", "LD_LIBRARY_PATH"):
        if os.environ.get(name):
            env[name] = os.environ[name]
    env.update(extra or {})
    return env


def run(
    argv: list[str],
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: float = 180,
) -> subprocess.CompletedProcess[str]:
    """Runs one command to completion, never raising on its exit status."""
    return subprocess.run(
        argv,
        cwd=str(cwd) if cwd else None,
        env=clean_env(env),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def sha256(path: Path) -> str:
    """The digest of the one artifact every leg must be running."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def free_port() -> int:
    """A port nothing is listening on right now (the kernel hands it back)."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _unchunk(sock, buffer: bytes) -> bytes:
    """The body of a `Transfer-Encoding: chunked` answer, chunk framing removed."""
    body = b""
    while True:
        while b"\r\n" not in buffer:
            chunk = sock.recv(65536)
            if not chunk:
                return body
            buffer += chunk
        line, _, buffer = buffer.partition(b"\r\n")
        try:
            size = int(line.split(b";", 1)[0].strip(), 16)
        except ValueError as error:
            raise LegFailure(f"bad chunk size {line[:40]!r}") from error
        if size == 0:
            return body
        while len(buffer) < size + 2:
            chunk = sock.recv(65536)
            if not chunk:
                break
            buffer += chunk
        body += buffer[:size]
        buffer = buffer[size + 2 :]


def http(
    port: int,
    target: str,
    method: str = "GET",
    body: bytes = b"",
    timeout: float = 10,
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, str], bytes]:
    """One HTTP/1.1 exchange on a raw socket; the target is sent exactly."""
    lines = [
        f"{method} {target} HTTP/1.1",
        "Host: 127.0.0.1",
        "Connection: close",
        f"Content-Length: {len(body)}",
    ]
    lines.extend(f"{name}: {value}" for name, value in (headers or {}).items())
    payload = ("\r\n".join(lines) + "\r\n\r\n").encode("latin-1") + body
    with socket.create_connection(("127.0.0.1", port), timeout=timeout) as sock:
        sock.settimeout(timeout)
        sock.sendall(payload)
        buffer = b""
        while b"\r\n\r\n" not in buffer:
            chunk = sock.recv(65536)
            if not chunk:
                raise LegFailure(f"{method} {target}: closed before a response head")
            buffer += chunk
        head, _, rest = buffer.partition(b"\r\n\r\n")
        header_lines = head.decode("latin-1").split("\r\n")
        protocol, _, remainder = header_lines[0].partition(" ")
        status, _, _reason = remainder.partition(" ")
        if protocol != "HTTP/1.1" or not status.isdigit():
            raise LegFailure(f"{method} {target}: bad status line {header_lines[0]!r}")
        headers: dict[str, str] = {}
        for line in header_lines[1:]:
            name, separator, value = line.partition(":")
            if separator:
                headers[name.strip().lower()] = value.strip()
        data = rest
        chunked = "chunked" in headers.get("transfer-encoding", "").lower()
        length = headers.get("content-length")
        if chunked:
            data = _unchunk(sock, data)
        elif length is not None:
            wanted = int(length)
            while len(data) < wanted:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
            data = data[:wanted]
        else:
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
        return int(status), headers, data


class Dev:
    """One `blastit dev`, its transcript, and a request helper."""

    def __init__(self, binary: Path, project: Path, env: dict[str, str] | None = None):
        self.binary = binary
        self.project = project
        self.env = env or {}
        self.port = 0
        self.process: subprocess.Popen[str] | None = None
        self.out: list[str] = []
        self.err: list[str] = []
        self._ready = threading.Event()
        self._pumps: list[threading.Thread] = []

    def _pump(self, stream, sink: list[str]) -> None:
        for line in stream:
            sink.append(line)
            if "INFO  ready" in line:
                self._ready.set()

    def __enter__(self) -> Dev:
        self.port = free_port()
        self.process = subprocess.Popen(
            [str(self.binary), "dev", "--port", str(self.port), "--no-reload"],
            cwd=str(self.project),
            env=clean_env(self.env),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._pumps = [
            threading.Thread(
                target=self._pump, args=(self.process.stdout, self.out), daemon=True
            ),
            threading.Thread(
                target=self._pump, args=(self.process.stderr, self.err), daemon=True
            ),
        ]
        for pump in self._pumps:
            pump.start()
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if self._ready.wait(0.1):
                return self
            if self.process.poll() is not None:
                raise LegFailure(
                    "`blastit dev` exited before its ready line "
                    f"(status {self.process.returncode}):\n{self.tail()}"
                )
        raise LegFailure(f"`blastit dev` never printed `INFO  ready`:\n{self.tail()}")

    def tail(self, lines: int = 25) -> str:
        """The last lines of both streams, for a failure that names no cause."""
        return (
            "stdout:\n"
            + "".join(self.out[-lines:])
            + "stderr:\n"
            + "".join(self.err[-lines:])
        )

    def get(
        self, target: str, deadline: float = 60
    ) -> tuple[int, dict[str, str], bytes]:
        """`target`, retrying while the view has not been forked yet."""
        end = time.monotonic() + deadline
        last: OSError | None = None
        while time.monotonic() < end:
            try:
                return http(self.port, target)
            except OSError as error:
                last = error
                if self.process is not None and self.process.poll() is not None:
                    raise LegFailure(
                        f"`blastit dev` exited (status {self.process.returncode}) "
                        f"while waiting for {target}:\n{self.tail()}"
                    ) from error
                time.sleep(0.1)
        raise LegFailure(
            f"GET {target} on 127.0.0.1:{self.port} never answered ({last}):"
            f"\n{self.tail()}"
        )

    def __exit__(self, *_exc) -> None:
        if self.process is None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        for pump in self._pumps:
            pump.join(timeout=5)


class Summary:
    """One PASS/FAIL/SKIP line per leg, then one summary line."""

    def __init__(self) -> None:
        self.rows: list[tuple[str, str]] = []

    def record(self, status: str, name: str, detail: str) -> None:
        self.rows.append((status, name))
        print(f"{status:<5} {name:<14} {detail}", flush=True)

    def finish(self, allow_skip: bool) -> int:
        """The exit status: any FAIL fails, and so does a SKIP unless allowed.

        A SKIP is "this host cannot prove the claim", never "the claim holds": the
        gate that calls this harness (`make check`, `make e2e-universal`) passes no
        `--allow-skip`, so a bundle that ships no PHP shim — or a host without
        `libphp-embed` — cannot turn into a green epic gate.
        """
        passed = sum(1 for status, _ in self.rows if status == "PASS")
        skipped = sum(1 for status, _ in self.rows if status == "SKIP")
        failed = sum(1 for status, _ in self.rows if status == "FAIL")
        print(
            f"\n{passed} passed, {skipped} skipped, {failed} failed",
            flush=True,
        )
        if failed:
            return 1
        if skipped and not allow_skip:
            print(
                f"error: {skipped} requested leg(s) could not run: this host does not "
                "prove the bundle's claim; fix the SKIP above or pass --allow-skip",
                flush=True,
            )
            return 1
        return 0


def leg(summary: Summary, name: str, function) -> None:
    """Runs one leg; a refusal or a crash is that leg's FAIL, not the run's."""
    started = time.monotonic()
    try:
        detail = function()
    except Skip as skipped:
        summary.record("SKIP", name, str(skipped))
    except LegFailure as failure:
        summary.record("FAIL", name, str(failure))
    except Exception as error:  # noqa: BLE001 - one bad leg must not hide the rest
        summary.record("FAIL", name, f"{type(error).__name__}: {error}")
    else:
        elapsed = time.monotonic() - started
        summary.record("PASS", name, f"{detail} ({elapsed:.1f}s)")


# ---------------------------------------------------------------------------
# fixtures
# ---------------------------------------------------------------------------


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def private(path: Path) -> None:
    """`0755`: the handoff's tree rule refuses a directory another uid may write."""
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o755)


def interpreter(minor: str) -> str | None:
    """The CPython `minor` this host can run, or `None`."""
    found = shutil.which(f"python{minor}")
    if found:
        return found
    uv = shutil.which("uv")
    if uv is None:
        return None
    result = subprocess.run(
        [uv, "python", "find", "--no-project", minor],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode == 0 and result.stdout.strip():
        return result.stdout.strip()
    return None


def make_venv(project: Path, base: str) -> Path:
    """A venv at `project/.venv`, and the interpreter inside it."""
    target = project / ".venv"
    uv = shutil.which("uv")
    if uv is not None:
        result = run([uv, "venv", "--python", base, str(target)], timeout=300)
        expect(
            result.returncode == 0,
            f"`uv venv --python {base}` failed: {result.stderr.strip()}",
        )
    else:
        result = run(
            [base, "-m", "venv", "--without-pip", str(target)],
            timeout=300,
        )
        expect(
            result.returncode == 0,
            f"`{base} -m venv` failed: {result.stderr.strip()}",
        )
    for name in ("python3", "python"):
        candidate = target / "bin" / name
        if candidate.exists():
            return candidate
    raise LegFailure(f"{target} holds no interpreter under bin/")


# ---------------------------------------------------------------------------
# the legs
# ---------------------------------------------------------------------------


class Context:
    """What every leg shares: the one artifact, its digest, and where to build."""

    def __init__(self, binary: Path, digest: str, version: str, work: Path):
        self.binary = binary
        self.digest = digest
        self.version = version
        self.work = work
        #: Directories a leg had to create outside `work` (the handoff's root).
        self.scratch: list[Path] = []

    def still_one_file(self) -> None:
        """The artifact has not been replaced between legs."""
        expect(
            sha256(self.binary) == self.digest,
            f"{self.binary} changed during the run (digest is no longer {self.digest})",
        )

    def assert_version(self, cwd: Path) -> None:
        result = run([str(self.binary), "--version"], cwd=cwd)
        expect(result.returncode == 0, f"`--version` exited {result.returncode}")
        expect(
            result.stdout.strip() == self.version,
            f"`--version` says {result.stdout.strip()!r}, not {self.version!r}",
        )


def leg_python(ctx: Context, minor: str) -> str:
    """A WSGI project in a CPython `minor` venv answers 200 with that interpreter."""
    binary_dir = ctx.binary.resolve().parent
    digits = minor.replace(".", "")
    if not any(binary_dir.glob(f"libblastit_py{digits}-*.so")):
        raise Skip(
            f"the bundle ships no libblastit_py{digits}-*.so beside {ctx.binary.resolve()}; "
            "run `make py-sapi` and re-stage the bundle"
        )
    base = interpreter(minor)
    if base is None:
        raise Skip(
            f"no CPython {minor} on this host; install it with `uv python install {minor}`"
        )
    project = ctx.work / f"python-{minor}"
    write(project / "Blastfile", WSGI_MANIFEST)
    write(project / "app.py", WSGI_APP)
    make_venv(project, base)
    sites = sorted((project / ".venv" / "lib").glob("python*/site-packages"))
    expect(
        bool(sites),
        f"{project}/.venv holds no lib/python*/site-packages to import a marker from",
    )
    write(sites[0] / "universal_marker.py", f'MINOR = "{minor}"\n')

    with Dev(ctx.binary, project) as server:
        status, headers, body = server.get("/")
    content_type = headers.get("content-type", "")
    expect(status == 200, f"GET / answered {status}, not 200")
    expect(
        content_type.startswith("application/json"),
        f"GET / answered {content_type!r}, not application/json",
    )
    try:
        document = json.loads(body)
    except ValueError as error:
        raise LegFailure(f"GET / answered no JSON ({error}): {body[:400]!r}") from error
    expect(
        document.get("minor") == minor,
        f"CPython {minor} was expected to serve it, and the body says "
        f"{document.get('minor')!r} ({json.dumps(document)})",
    )
    expect(
        document.get("marker") == minor,
        "the project's own .venv site-packages did not serve it: the marker module "
        f"says {document.get('marker')!r} ({json.dumps(document)})",
    )
    expect(
        document.get("path") == "/",
        f"PATH_INFO reached the app as {document.get('path')!r}",
    )
    ctx.still_one_file()
    ctx.assert_version(project)
    return f"200 from CPython {minor} ({project.name}/.venv), same artifact"


def leg_php(ctx: Context, minor: str) -> str:
    """The PHP fixture answers 200 through the bundle's shim and the host libphp.

    No `BLASTIT_PHP_SHIM_DIR`: `dev` with `exe_dir = …/share/blastit/lib` already
    finds the shim the bundle ships, so a bundle that shipped none fails here
    instead of passing on `target/php-sapi` (the build tree's copy).
    """
    digits = minor.replace(".", "")
    shims = sorted(ctx.binary.resolve().parent.glob(f"libblastit_php{digits}-*.so"))
    libphp = next((Path(path) for path in LIBPHP_84 if Path(path).is_file()), None)
    missing = []
    if not shims:
        missing.append(
            f"the bundle ships no libblastit_php{digits}-*.so beside "
            f"{ctx.binary.resolve()}"
        )
    if libphp is None:
        missing.append(
            f"no libphp{minor} (Debian/Ubuntu: apt install libphp{minor}-embed, "
            f"Alpine: apk add php{digits}-embed)"
        )
    if missing:
        raise Skip(
            "; ".join(missing)
            + f"; run `make php-sapi PHP_MINORS={minor}` on a host with php{minor}-dev "
            f"and libphp{minor}-embed, or `make php-sapi-e2e`"
        )

    project = ctx.work / f"php-{minor}"
    shutil.copytree(ROOT / "e2e" / "apps" / "php", project, dirs_exist_ok=True)
    # `Blastfile.php2shell` is the app suite's own store-less manifest for this
    # front controller, so the leg needs no PostgreSQL: what it proves is that
    # *one artifact* serves PHP `minor`, not the shop's grants (`run_apps.py`).
    env = {
        "BLASTIT_LIBPHP": str(libphp),
        "BLASTIT_MANIFEST": str(project / "Blastfile.php2shell"),
    }
    with Dev(ctx.binary, project, env) as server:
        status, _headers, body = server.get("/")
    expect(status == 200, f"GET / answered {status}, not 200")
    expect(
        b"front-controller" in body,
        f"GET / did not reach the fixture's front controller: {body[:400]!r}",
    )
    ctx.still_one_file()
    ctx.assert_version(project)
    return f"200 from PHP {minor} ({shims[0].name})"


def leg_static(ctx: Context) -> str:
    """A mount answers 200 with the file's own bytes."""
    project = ctx.work / "static"
    write(project / "Blastfile", STATIC_MANIFEST)
    (project / "public").mkdir(parents=True, exist_ok=True)
    (project / "public" / "index.html").write_bytes(STATIC_INDEX)

    with Dev(ctx.binary, project) as server:
        status, headers, body = server.get("/index.html")
    expect(status == 200, f"GET /index.html answered {status}, not 200")
    expect(body == STATIC_INDEX, f"GET /index.html is not the file's bytes: {body!r}")
    expect(
        headers.get("content-type", "").startswith("text/html"),
        f"GET /index.html answered {headers.get('content-type')!r}, not text/html",
    )
    ctx.still_one_file()
    ctx.assert_version(project)
    return "200 with the file's own bytes"


def tree_is_protected(root: Path) -> bool:
    """Whether design §4's tree rule accepts `root`.

    Every ancestor must be owned by this user (or root) and writable by its owner
    only. A root-owned sticky directory (`/tmp`: mode `1777`) is the one
    exception, and then its immediate child must be ours.
    """
    uid = os.geteuid()
    entries = [root, *root.parents]
    for index, entry in enumerate(entries):
        info = entry.stat()
        if info.st_uid == 0 and info.st_mode & 0o1000:
            if index == 0:
                return True
            if entries[index - 1].stat().st_uid not in (uid, 0):
                return False
            continue
        if info.st_uid not in (uid, 0) or info.st_mode & 0o022:
            return False
    return True


def handoff_root(ctx: Context) -> Path:
    """A scratch directory §4's tree rule accepts, or a SKIP naming `chmod`.

    A checkout another uid may write (`0775`) refuses every handoff, so the
    fixture moves to the one place this user certainly owns: a fresh `0700`
    directory under `$TMPDIR` (or `/tmp`, whose sticky bit the rule allows).
    """
    candidate = Path(tempfile.mkdtemp(prefix="handoff-", dir=ctx.work))
    if tree_is_protected(candidate):
        return candidate
    shutil.rmtree(candidate, ignore_errors=True)
    scratch = Path(
        tempfile.mkdtemp(prefix="blastit-handoff-", dir=tempfile.gettempdir())
    )
    ctx.scratch.append(scratch)
    if not tree_is_protected(scratch):
        raise Skip(
            "no directory on this host passes the §4 handoff tree rule (an ancestor "
            "is writable by another user); run `chmod 755` on the checkout and retry"
        )
    return scratch


def leg_handoff(ctx: Context) -> str:
    """`dev` runs the project's `.venv/bin/blastit`; production and `--version` never do."""
    project = handoff_root(ctx)
    private(project)
    private(project / ".venv")
    private(project / ".venv" / "bin")
    write(project / "Blastfile", STATIC_MANIFEST)
    write(project / "public" / "index.html", STATIC_INDEX.decode())

    evidence = project / "handoff.txt"
    marker = project / ".venv" / "bin" / "blastit"
    marker.write_text(f'#!/bin/sh\nprintf \'%s\\n\' "$@" > "{evidence}"\n')
    os.chmod(marker, 0o755)

    # 1. `dev` is handed to the project's own binary, and the global one is silent.
    result = run([str(ctx.binary), "dev", "--port", str(free_port())], cwd=project)
    expect(
        result.returncode == 0,
        f"`blastit dev` exited {result.returncode}, not 0: {result.stderr.strip()}",
    )
    expect(
        evidence.is_file(), "`blastit dev` never ran the project's .venv/bin/blastit"
    )
    ran = evidence.read_text().splitlines()
    expect("dev" in ran, f"the marker saw {ran!r}, not a `dev …` command line")
    expect(ran.count("dev") == 1, f"the marker saw {ran!r}")
    expect(
        result.stdout == "" and result.stderr == "",
        f"a valid handoff is silent, and it printed {result.stdout!r} {result.stderr!r}",
    )

    # 2. Production is the artifact the operator installed: no handoff, and
    #    unprivileged it refuses (design §4, `R6`–`R8`).
    evidence.unlink()
    production = run([str(ctx.binary)], cwd=project)
    expect(
        production.returncode != 0,
        "a production boot without privileges exited 0",
    )
    expect(
        not evidence.exists(),
        "a production boot ran the project's .venv/bin/blastit",
    )

    # 3. `--version` is answered by the global binary too.
    version = run([str(ctx.binary), "--version"], cwd=project)
    expect(version.returncode == 0, f"`--version` exited {version.returncode}")
    expect(
        version.stdout.strip() == ctx.version, f"`--version` says {version.stdout!r}"
    )
    expect(not evidence.exists(), "`--version` ran the project's .venv/bin/blastit")
    ctx.still_one_file()
    return "`dev` handed off silently; production and `--version` did not"


def leg_check(ctx: Context, minor: str) -> str:
    """CPython `minor` (no per-minor `.so`) runs `check --json`."""
    bundle_python = ctx.binary.resolve().parent.parent / "python"
    if not (bundle_python / "blastit" / "_blastit.py").is_file():
        raise Skip(
            f"`--binary` is not an installed bundle: {bundle_python} holds no "
            "blastit package; stage one with `make install PREFIX=<dir>`"
        )
    base = interpreter(minor)
    if base is None:
        raise Skip(
            f"no CPython {minor} on this host; install it with `uv python install {minor}`"
        )
    digits = minor.replace(".", "")
    expect(
        not any(ctx.binary.resolve().parent.glob(f"libblastit_py{digits}-*.so")),
        f"the bundle ships a libblastit_py{digits}-*.so, so this leg proves nothing",
    )

    # `$DATABASE_URL` is a manifest env reference, so the lowered model needs it
    # to exist; nothing ever connects, because the static route stores nothing.
    env = {"PYTHONPATH": str(bundle_python)}
    check_project = ctx.work / f"check-{minor}"
    write(check_project / "Blastfile", STATIC_MANIFEST)
    write(check_project / "public" / "index.html", STATIC_INDEX.decode())
    venv_python = make_venv(check_project, base)

    check = run(
        [str(venv_python), "-m", "blastit", "check", "--json"],
        cwd=check_project,
        env=env,
    )
    expect(
        check.returncode == 0,
        f"`python -m blastit check --json` exited {check.returncode}: "
        f"{check.stderr.strip() or check.stdout.strip()}",
    )
    try:
        document = json.loads(check.stdout)
    except ValueError as error:
        raise LegFailure(
            f"`check --json` printed no JSON document ({error}): {check.stdout[:400]!r}"
        ) from error
    expect(
        isinstance(document, dict) and document.get("ok") is True,
        f"`check --json` did not say ok: {document!r}",
    )

    # The bridge found the bundle's own binary through the §6.1 layout
    # (`<pkg>/../../lib/blastit`), not through PATH or `BLASTIT_BIN`.
    probe = run(
        [
            str(venv_python),
            "-c",
            "from blastit import _blastit; print(_blastit.bin_path())",
        ],
        cwd=check_project,
        env=env,
    )
    expect(probe.returncode == 0, f"`bin_path()` failed: {probe.stderr.strip()}")
    expect(
        os.path.exists(probe.stdout.strip())
        and os.path.samefile(probe.stdout.strip(), ctx.binary),
        f"`bin_path()` answered {probe.stdout.strip()!r}, not {ctx.binary}",
    )

    expect(
        not list((check_project / ".venv").rglob("libblastit_py*.so")),
        f"the CPython {minor} venv holds a per-minor shim, so the leg proves nothing",
    )
    ctx.still_one_file()
    return f"check --json ok on CPython {minor}, no per-minor .so"


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="One blastit artifact, four runtimes, one PASS/FAIL summary."
    )
    parser.add_argument(
        "--binary",
        required=True,
        type=Path,
        help="the staged bundle's bin/blastit (`make install PREFIX=<dir>`)",
    )
    parser.add_argument(
        "--python",
        action="append",
        default=[],
        metavar="MINOR",
        help="serve a WSGI app under this CPython (repeatable: 3.13, 3.14)",
    )
    parser.add_argument(
        "--php", metavar="MINOR", help="serve the PHP fixture under this minor (8.4)"
    )
    parser.add_argument("--static", action="store_true", help="serve a static mount")
    parser.add_argument(
        "--handoff", action="store_true", help="a project's .venv/bin/blastit marker"
    )
    parser.add_argument(
        "--check", action="store_true", help="check --json on an old CPython"
    )
    parser.add_argument(
        "--check-python", default="3.12", metavar="MINOR", help="the --check minor"
    )
    parser.add_argument(
        "--work",
        type=Path,
        help="where the fixtures go (default: a fresh directory under target/)",
    )
    parser.add_argument(
        "--keep", action="store_true", help="keep the fixtures after a green run"
    )
    parser.add_argument(
        "--allow-skip",
        action="store_true",
        help="exit 0 when a requested leg was SKIPped (a host fact, not a proof); "
        "`make check` and `make e2e-universal` never pass this",
    )
    arguments = parser.parse_args(argv)

    binary = arguments.binary
    if not os.access(binary, os.X_OK):
        parser.error(f"{binary} is not an executable file")

    base = ROOT / "target" / "e2e-universal"
    base.mkdir(parents=True, exist_ok=True)
    work = arguments.work or Path(tempfile.mkdtemp(prefix="work-", dir=base))
    work.mkdir(parents=True, exist_ok=True)

    version_run = run([str(binary), "--version"])
    version = version_run.stdout.strip()
    if version_run.returncode != 0 or not version.startswith("blastit "):
        parser.error(
            f"{binary} --version exited {version_run.returncode} printing {version!r}, "
            "not a blastit version (pass the bundle's bin/blastit)"
        )
    ctx = Context(binary, sha256(binary), version, work)
    summary = Summary()

    print("== one artifact, four runtimes ==")
    print(f"binary  {binary}")
    print(f"file    {binary.resolve()}")
    print(f"sha256  {ctx.digest}")
    print(f"version {version}")
    print(f"work    {work}\n")

    if not (arguments.python or arguments.php or arguments.static or arguments.handoff):
        parser.error(
            "nothing to prove: pass --python, --php, --static and/or --handoff"
        )

    try:
        for minor in arguments.python:
            leg(summary, f"python {minor}", lambda minor=minor: leg_python(ctx, minor))
        if arguments.php:
            leg(summary, f"php {arguments.php}", lambda: leg_php(ctx, arguments.php))
        if arguments.static:
            leg(summary, "static", lambda: leg_static(ctx))
        if arguments.handoff:
            leg(summary, "handoff", lambda: leg_handoff(ctx))
        if arguments.check:
            leg(
                summary,
                f"check {arguments.check_python}",
                lambda: leg_check(ctx, arguments.check_python),
            )
    finally:
        status = summary.finish(arguments.allow_skip)
        for scratch in ctx.scratch:
            if status == 0 and not arguments.keep:
                shutil.rmtree(scratch, ignore_errors=True)
            else:
                print(f"scratch kept at {scratch}")
        if status == 0 and not arguments.keep:
            shutil.rmtree(work, ignore_errors=True)
        else:
            print(f"\nfixtures kept at {work}")

    if status == 0:
        print(
            f"\none artifact: {ctx.digest[:12]}… served every passing leg",
            flush=True,
        )
    else:
        print(
            f"\none artifact: {ctx.digest[:12]}… did not pass every leg",
            flush=True,
        )
    return status


if __name__ == "__main__":
    sys.exit(main())
