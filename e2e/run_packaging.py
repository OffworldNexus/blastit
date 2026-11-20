"""The installation and packaging end-to-end suite (`TESTING_PLAN.md` §3).

    uv run --no-project e2e/run_packaging.py       # every leg, free ports 28080+
    uv run --no-project e2e/run_packaging.py --leg composer
    uv run --no-project e2e/run_packaging.py --deb dist/blastit_0.2.0_amd64.deb

One leg per install story, and every leg installs the artifact the story names
and then runs the story's own commands *where the artifact was installed*:

* `install` — `docs/install.sh --user` on this host, zero `sudo`, from a release
  archive served over `file://`; then `blastit --version`, `blastit fmt`,
  `blastit fmt --check`, `blastit check` and `blastit-lsp --version` out of
  `$prefix/bin`. A tampered archive and an overridden origin with no digest are
  refused with a sentence.
* `alpine` — the same installer in `alpine:3.21` as a non-root uid, fetching the
  musl archive over HTTP; the tampered archive is refused there too. A musl
  build on the host makes the leg serve it (otherwise the archive is a stand-in
  and the leg says so), and only with one does the leg also prove the root path
  (`ensure_libgcc` runs `apk add --no-cache libgcc` and the binary answers) and
  the `--user` refusal that names `run: apk add libgcc` once the library is
  gone.
* `deb` / `rpm` — `docs/install.sh` with `BLASTIT_DEB`/`BLASTIT_RPM` installs the
  built package in `debian:trixie` / `rockylinux:9`; §6.1's payload (two entry
  points, 13 paths under `/usr/share/blastit`, `root:root`),
  `blastit --version`, `blastit-lsp --version`, `blastit fmt`,
  `blastit fmt --check`, `blastit check` and a served page all come from inside
  the image. `--modes "install static wsgi"` also imports and serves the WSGI
  fixture through the package's own `python/blastit` (`MB.42.11`).
* `wheel` — the universal `py3-none` wheel installed with `pip` into
  `python:3.12-slim` (the CLI/`check` half) and `python:3.13-slim` /
  `python:3.14-slim` (WSGI served through the shipped shim).
* `composer` — `packages/composer-blastit` in `composer:2`: `composer install`
  puts `vendor/bin/blastit` on the project's PATH, the embedded digest table
  verifies the default origin (and a warm run stays silent), and a tampered
  archive, an origin with no digest and an unstamped (`pending`) table are all
  refused with a sentence.

An artifact that has not been built yet is a loud SKIP naming the command that
builds it — never a silent pass. `--require-legs` turns any SKIP into a failure,
which is what every CI call site passes: a green gate cannot be one that skipped.
Every container is `blastit-pkg-*`, every port comes from `28080`, every scratch
tree lives under `${TMPDIR:-/tmp/opencode}/blastit-pkg`, and one `finally`
removes what this run created. Stdlib only, no `sudo` on the host.
"""

from __future__ import annotations

import argparse
import functools
import hashlib
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http import server as http_server
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = HERE / "packaging"
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "scripts"))
from typing import Self

from platform_matrix import wheel_legs
from universal_binary import LegFailure, Skip, http

PORT_BASE = 28080
WORK_ROOT = Path(os.environ.get("TMPDIR", "/tmp/opencode")) / "blastit-pkg"
LEGS = ("composer", "install", "alpine", "wheel", "deb", "rpm")


# ---------------------------------------------------------------------------
# ports, processes, one container, one summary
# ---------------------------------------------------------------------------


class Ports:
    """Free loopback ports from 28080 upwards (each one is bound before use)."""

    def __init__(self) -> None:
        self.next = PORT_BASE

    def take(self) -> int:
        for port in range(self.next, PORT_BASE + 200):
            self.next = port + 1
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
                try:
                    sock.bind(("127.0.0.1", port))
                except OSError:
                    continue
                return port
        raise LegFailure(f"no free port between {PORT_BASE} and {PORT_BASE + 200}")


PORTS = Ports()


def clean_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    """The environment a host command runs under: no inherited venv or `BLASTIT_*`."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "TMPDIR": str(WORK_ROOT),
        "BLASTIT_MEM_HIGH_WATERMARK": "100",
        "BLASTIT_MAX_VIEW_PROCESSES": "8",
    }
    if os.environ.get("LD_LIBRARY_PATH"):
        env["LD_LIBRARY_PATH"] = os.environ["LD_LIBRARY_PATH"]
    env.update(extra or {})
    return env


def run(
    argv: list[str],
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: float = 600,
) -> subprocess.CompletedProcess[str]:
    """Runs one host command, never raising on its exit status."""
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


def docker(*args: str, timeout: float = 900) -> subprocess.CompletedProcess[str]:
    """One `docker` command, with the host's environment (daemon sockets)."""
    return subprocess.run(
        ["docker", *args],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )


def tail(text: str, lines: int = 16) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])


def joined(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + result.stderr


class Checks:
    """The assertions of one leg; a run names every promise it broke."""

    def __init__(self) -> None:
        self.failures: list[str] = []

    def that(self, condition: bool, promise: str) -> None:
        if not condition:
            self.failures.append(promise)

    def done(self, leg: str) -> str:
        if self.failures:
            raise LegFailure(
                f"{leg}: {len(self.failures)} promise(s) failed:\n  "
                + "\n  ".join(self.failures)
            )
        return leg


class Summary:
    def __init__(self) -> None:
        self.rows: list[tuple[str, str, str]] = []

    def add(self, leg: str, state: str, detail: str) -> None:
        self.rows.append((leg, state, detail))
        print(f"{state:4}  {leg:20}  {detail}", flush=True)

    def report(self, require: bool = False) -> int:
        passed = sum(1 for _, state, _ in self.rows if state == "PASS")
        skipped = sum(1 for _, state, _ in self.rows if state == "SKIP")
        failed = sum(1 for _, state, _ in self.rows if state == "FAIL")
        print(
            f"\n{passed} passed, {failed} failed, {skipped} skipped "
            "(a SKIP names the artifact it needs)",
            flush=True,
        )
        if require and skipped:
            print(
                f"{skipped} leg(s) skipped but this run requires them: a gate that "
                "skipped proves nothing",
                flush=True,
            )
            return 1
        return 1 if failed else 0


class Container:
    """One named container of a leg: started once, exec'd into, removed on exit."""

    def __init__(
        self,
        name: str,
        image: str,
        work: Path,
        mounts: list[tuple[Path, str]] | None = None,
        publish: list[int] | None = None,
        host_network: bool = False,
        user: str | None = None,
        environment: dict[str, str] | None = None,
    ) -> None:
        self.name = name
        self.image = image
        self.work = work
        self.mounts = mounts or []
        self.publish = publish or []
        self.host_network = host_network
        self.user = user
        self.environment = environment or {}

    def __enter__(self) -> Self:
        self.work.mkdir(parents=True, exist_ok=True)
        argv = ["run", "-d", "--name", self.name]
        for port in self.publish:
            argv += ["--publish", f"127.0.0.1:{port}:{port}"]
        if self.host_network:
            argv += ["--network", "host"]
        if self.user:
            argv += ["--user", self.user]
        argv += ["--volume", f"{self.work}:/work"]
        for host, inner in self.mounts:
            argv += ["--volume", f"{host}:{inner}:ro"]
        for key, value in self.environment.items():
            argv += ["--env", f"{key}={value}"]
        argv += [self.image, "sh", "-c", "while :; do sleep 3600; done"]
        started = docker(*argv)
        if started.returncode != 0:
            raise LegFailure(
                f"{self.name} ({self.image}) did not start: {tail(joined(started))}"
            )
        return self

    def exec(
        self,
        argv: list[str],
        workdir: str | None = None,
        env: dict[str, str] | None = None,
        timeout: float = 900,
        detach: bool = False,
        user: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        args = ["exec"]
        if detach:
            args.append("-d")
        if user:
            args += ["--user", user]
        if workdir:
            args += ["--workdir", workdir]
        for key, value in (env or {}).items():
            args += ["--env", f"{key}={value}"]
        args += [self.name, *argv]
        return docker(*args, timeout=timeout)

    def logs(self, lines: int = 60) -> str:
        return joined(docker("logs", "--tail", str(lines), self.name))

    def __exit__(self, *_exc) -> None:
        docker("rm", "-f", self.name, timeout=120)


class _Quiet(http_server.SimpleHTTPRequestHandler):
    def log_message(self, *_args) -> None:  # one PASS/FAIL line per leg, no access log
        return


class Serving:
    """A directory served over loopback HTTP, for a container that must fetch it."""

    def __init__(self, directory: Path, port: int) -> None:
        self.directory = directory
        self.port = port
        self.server: http_server.ThreadingHTTPServer | None = None
        self.thread: threading.Thread | None = None

    def __enter__(self) -> Self:
        handler = functools.partial(_Quiet, directory=str(self.directory))
        self.server = http_server.ThreadingHTTPServer(("127.0.0.1", self.port), handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        return self

    def __exit__(self, *_exc) -> None:
        if self.server is not None:
            self.server.shutdown()
            self.server.server_close()
        if self.thread is not None:
            self.thread.join(timeout=5)


def wait_for(port: int, target: str, needle: bytes, timeout: float = 180) -> str:
    """Polls one loopback URL until the body carries `needle`."""
    deadline = time.monotonic() + timeout
    last = "no answer"
    while time.monotonic() < deadline:
        try:
            status, _headers, body = http(port, target, timeout=5)
            if needle in body:
                return body.decode("utf-8", "replace")
            last = f"answered {status}: {body[:160]!r}"
        except (OSError, LegFailure) as error:
            last = str(error)
        time.sleep(0.25)
    raise LegFailure(f"127.0.0.1:{port}{target} never served {needle!r} ({last})")


# ---------------------------------------------------------------------------
# host facts, artifacts and archives
# ---------------------------------------------------------------------------


def require_docker() -> None:
    """A reachable Docker daemon, or a loud SKIP naming how to install it."""
    if shutil.which("docker") is None:
        raise Skip(
            "Docker is not installed; `sudo apt install docker.io` "
            "(or `sudo dnf install docker-ce`)"
        )
    version = docker("version", "--format", "{{.Server.Version}}", timeout=30)
    if version.returncode != 0:
        raise Skip(
            "the Docker daemon is not reachable; `sudo systemctl enable --now docker` "
            f"({tail(joined(version), 3)})"
        )


def arch() -> str:
    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        return "x86_64"
    if machine in ("aarch64", "arm64"):
        return "aarch64"
    raise Skip(
        f"unsupported CPU architecture {machine!r}: Blastit ships x86_64 and aarch64"
    )


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def workspace_version() -> str:
    """The version this checkout builds (`[workspace.package] version`)."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if match is None:
        raise LegFailure("Cargo.toml declares no [workspace.package] version")
    return match.group(1)


def binary_argument(given: Path | None) -> Path:
    for candidate in (
        given,
        os.environ.get("E2E_BLASTIT"),
        ROOT / "target" / "release" / "blastit",
        ROOT / "target" / "debug" / "blastit",
        shutil.which("blastit"),
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate).resolve()
    raise Skip(
        "no blastit binary; build one with `cargo build -p blastit --release --locked` "
        "or pass --binary /path/to/blastit"
    )


def lsp_argument(binary: Path, given: Path | None) -> Path | None:
    for candidate in (
        given,
        os.environ.get("E2E_BLASTIT_LSP"),
        binary.parent / "blastit-lsp",
    ):
        if candidate and Path(candidate).is_file():
            return Path(candidate).resolve()
    return None


def version_of(binary: Path) -> str:
    printed = run([str(binary), "--version"], timeout=120)
    fields = printed.stdout.split()
    if printed.returncode != 0 or len(fields) != 2 or fields[0] != "blastit":
        raise LegFailure(
            f"`{binary} --version` printed {printed.stdout.strip()!r} "
            f"(exit {printed.returncode}): {tail(printed.stderr, 6)}"
        )
    return fields[1]


def standin(directory: Path, name: str, text: str) -> Path:
    path = directory / name
    path.write_text(text, encoding="utf-8")
    path.chmod(0o755)
    return path


def release_archive(directory: Path, target: str, files: dict[str, Path]) -> Path:
    """One release tarball, `<dir>/<target>/{blastit,blastit-lsp}` (design §6.1)."""
    tree = directory / target
    tree.mkdir(parents=True, exist_ok=True)
    for name, source in files.items():
        shutil.copy2(source, tree / name)
        (tree / name).chmod(0o755)
    archive = directory / f"blastit-{target}.tar.gz"
    packed = run(["tar", "-C", str(directory), "-czf", str(archive), target])
    if packed.returncode != 0:
        raise LegFailure(f"tar could not build {archive.name}: {tail(joined(packed))}")
    return archive


def discover(given: Path | None, glob: str, what: str, command: str) -> Path:
    """`given`, else the newest match under `target/packages`/`dist`, else a SKIP."""
    if given is not None:
        if not given.is_file():
            raise LegFailure(f"{given} does not exist: build it with `{command}`")
        return given.resolve()
    for base in (ROOT / "target" / "packages", ROOT / "dist"):
        if not base.is_dir():
            continue
        found = sorted(base.glob(glob), key=lambda path: path.stat().st_mtime)
        if found:
            return found[-1].resolve()
    raise Skip(f"no {what} in target/packages/ or dist/: build it with `{command}`")


def from_environment(name: str) -> Path | None:
    value = os.environ.get(name)
    return Path(value) if value else None


def installed_files(box: Container, kind: str) -> list[str]:
    """The package's files, without the directory entries `dpkg -L`/`rpm -ql` list."""
    listing = "dpkg -L blastit" if kind == "deb" else "rpm -ql blastit"
    result = box.exec(
        [
            "sh",
            "-c",
            listing
            + ' | while IFS= read -r path; do [ -f "$path" ] || [ -L "$path" ] || continue;'
            " printf '%s\\n' \"$path\"; done",
        ]
    )
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


# ---------------------------------------------------------------------------
# the legs
# ---------------------------------------------------------------------------


def leg_install_user(
    scratch: Path, binary: Path, lsp: Path | None, version: str
) -> str:
    """`docs/install.sh --user`: the release archive, in user space, zero sudo."""
    checks = Checks()
    installer = ROOT / "docs" / "install.sh"
    target = f"{arch()}-unknown-linux-gnu"
    release = scratch / "release-gnu"
    files = {"blastit": binary}
    if lsp is not None:
        files["blastit-lsp"] = lsp
    archive = release_archive(release, target, files)
    digest = sha256_of(archive)
    origin = {"BLASTIT_RELEASE_BASE_URL": release.as_uri(), "BLASTIT_SHA256": digest}

    prefix = scratch / "prefix-user"
    installed = run(
        ["sh", str(installer), "--user", "--prefix", str(prefix)],
        env={**origin, "HOME": str(scratch / "home")},
    )
    checks.that(
        installed.returncode == 0,
        f"`install.sh --user` exits {installed.returncode}: {tail(joined(installed), 12)}",
    )
    entry = prefix / "bin" / "blastit"
    checks.that(os.path.islink(entry), f"{entry} is not the archive's bin symlink")
    if os.path.islink(entry):
        checks.that(
            os.readlink(entry) == "../share/blastit/lib/blastit",
            f"{entry} points at {os.readlink(entry)!r}, not ../share/blastit/lib/blastit",
        )
    placed = prefix / "share" / "blastit" / "lib" / "blastit"
    checks.that(
        placed.is_file() and sha256_of(placed) == sha256_of(binary),
        "share/blastit/lib/blastit is not the archive's binary",
    )
    printed = run([str(entry), "--version"], cwd=prefix)
    checks.that(
        printed.stdout.strip() == f"blastit {version}",
        f"`{entry} --version` printed {printed.stdout.strip()!r}",
    )
    if lsp is not None:
        lsp_entry = prefix / "bin" / "blastit-lsp"
        checks.that(lsp_entry.exists(), "the archive's blastit-lsp was not installed")
        printed = run([str(lsp_entry), "--version"], cwd=prefix)
        checks.that(
            printed.stdout.strip().startswith("blastit-lsp "),
            f"`{lsp_entry} --version` printed {printed.stdout.strip()!r}",
        )

    project = scratch / "project"
    shutil.copytree(FIXTURES / "fixture", project)
    formatted = run([str(entry), "fmt", "Blastfile"], cwd=project)
    checks.that(
        formatted.returncode == 0,
        f"`blastit fmt` exits {formatted.returncode}: {tail(joined(formatted), 10)}",
    )
    canonical = run([str(entry), "fmt", "--check", "Blastfile"], cwd=project)
    checks.that(
        canonical.returncode == 0,
        f"`blastit fmt --check` after `fmt` exits {canonical.returncode}: "
        f"{tail(joined(canonical), 10)}",
    )
    loose = scratch / "unformatted"
    loose.mkdir(parents=True, exist_ok=True)
    (loose / "Blastfile").write_text('static "/" { root "public" }\n', encoding="utf-8")
    diffed = run([str(entry), "fmt", "--check"], cwd=loose)
    checks.that(
        diffed.returncode != 0,
        "`blastit fmt --check` accepted an unformatted Blastfile",
    )
    checks.that(
        "--- " in joined(diffed) and "Blastfile" in joined(diffed),
        f"`blastit fmt --check` printed no diff naming the file: {tail(joined(diffed), 8)}",
    )
    checked = run([str(entry), "check"], cwd=project)
    checks.that(
        checked.returncode == 0,
        f"`blastit check` exits {checked.returncode}: {tail(joined(checked), 12)}",
    )

    # A mirror that can substitute the tarball is the threat the digest answers.
    tampered_dir = scratch / "release-gnu-tampered"
    shutil.copytree(release, tampered_dir)
    with (tampered_dir / archive.name).open("ab") as handle:
        handle.write(b"appended by the test\n")
    tampered_prefix = scratch / "prefix-tampered"
    refused = run(
        ["sh", str(installer), "--user", "--prefix", str(tampered_prefix)],
        env={**origin, "BLASTIT_RELEASE_BASE_URL": tampered_dir.as_uri()},
    )
    checks.that(refused.returncode != 0, "a tampered archive was installed")
    checks.that(
        "sha256 mismatch for" in refused.stderr
        and "refusing to install it" in refused.stderr,
        f"the tamper refusal is not a sentence: {tail(joined(refused), 8)}",
    )
    checks.that("Traceback" not in refused.stderr, "the tamper refusal is a traceback")
    checks.that(
        not (tampered_prefix / "bin" / "blastit").exists(),
        "a tampered archive left a blastit in the prefix",
    )

    unsigned_prefix = scratch / "prefix-unsigned"
    unsigned = run(
        ["sh", str(installer), "--user", "--prefix", str(unsigned_prefix)],
        env={"BLASTIT_RELEASE_BASE_URL": release.as_uri()},
    )
    checks.that(
        unsigned.returncode != 0, "an overridden origin with no digest was installed"
    )
    checks.that(
        "overridden release origin is unsigned" in unsigned.stderr,
        f"the unsigned-origin refusal is not a sentence: {tail(joined(unsigned), 8)}",
    )
    checks.that(
        not (unsigned_prefix / "bin" / "blastit").exists(),
        "an origin with no digest left a blastit in the prefix",
    )
    return checks.done("install.sh --user")


def leg_alpine(
    scratch: Path,
    version: str,
    musl_binary: Path | None,
    musl_archive: Path | None,
    fixture: Path,
) -> str:
    """The same installer, unprivileged, on Alpine against the musl archive."""
    require_docker()
    checks = Checks()
    target = f"{arch()}-unknown-linux-musl"
    release = scratch / "release-musl"
    real = musl_binary is not None or musl_archive is not None
    if musl_archive is not None:
        release.mkdir(parents=True, exist_ok=True)
        archive = release / f"blastit-{target}.tar.gz"
        shutil.copy2(musl_archive, archive)
    elif musl_binary is not None:
        archive = release_archive(release, target, {"blastit": musl_binary})
    else:
        tree = release / target
        tree.mkdir(parents=True, exist_ok=True)
        standin(
            tree,
            "blastit",
            f"#!/bin/sh\nprintf 'blastit {version} (musl archive fixture)\\n'\n",
        )
        archive = release / f"blastit-{target}.tar.gz"
        packed = run(["tar", "-C", str(release), "-czf", str(archive), target])
        checks.that(
            packed.returncode == 0,
            f"tar could not build {archive.name}: {tail(joined(packed))}",
        )
    digest = sha256_of(archive)
    tampered = scratch / "release-musl-tampered"
    shutil.copytree(release, tampered)
    with (tampered / archive.name).open("ab") as handle:
        handle.write(b"appended by the test\n")

    port = PORTS.take()
    name = f"blastit-pkg-alpine-{os.getpid()}"
    mounts: list[tuple[Path, str]] = [(ROOT, "/src")]
    if real:
        mounts.append((fixture, "/fixture"))
    (scratch / "alpine-home").mkdir(parents=True, exist_ok=True)
    prefix = "/work/alpine-home/.local"
    with (
        Serving(scratch, port),
        Container(
            name,
            "alpine:3.21",
            scratch,
            mounts=mounts,
            host_network=True,
            user=f"{os.getuid()}:{os.getgid()}",
            environment={"HOME": "/work/alpine-home", "TMPDIR": "/work"},
        ) as box,
    ):
        uid = box.exec(["id", "-u"]).stdout.strip()
        checks.that(uid != "0", f"the Alpine leg ran as root (uid {uid})")
        # A bare `alpine:3.21` carries neither `curl` nor `libgcc_s.so.1`. Only
        # the first is provisioned here — it is what the leg's own fetch step
        # needs — because the second is exactly what the installer's root path
        # installs itself (`ensure_libgcc`). Installed as root (`--user 0`)
        # because the leg's own uid may not run `apk`.
        prepared = box.exec(
            ["apk", "add", "--no-cache", "curl", "ca-certificates"],
            user="0",
            timeout=600,
        )
        checks.that(
            prepared.returncode == 0,
            f"the Alpine leg's prerequisites did not install "
            f"(exit {prepared.returncode}): {tail(joined(prepared), 10)}",
        )
        if real:
            # The root path: `main` reaches `ensure_libgcc` only as root, with no
            # `--user`/`--prefix`, and no `libgcc_s.so.1` present — which is
            # exactly this fresh container. It installs into `/usr/local`
            # *inside the container*, never on the host. The stand-in archive
            # cannot prove either half (it "runs" without the library), so these
            # assertions need a real musl archive.
            rooted = box.exec(
                ["sh", "/src/docs/install.sh"],
                user="0",
                env={
                    "BLASTIT_RELEASE_BASE_URL": f"http://127.0.0.1:{port}/release-musl",
                    "BLASTIT_SHA256": digest,
                },
                timeout=1200,
            )
            checks.that(
                rooted.returncode == 0,
                f"the root install exits {rooted.returncode}: {tail(joined(rooted), 14)}",
            )
            checks.that(
                "apk add --no-cache libgcc" in joined(rooted),
                f"the root install printed no 'apk add --no-cache libgcc': "
                f"{tail(joined(rooted), 12)}",
            )
            rooted_version = box.exec(["/usr/local/bin/blastit", "--version"])
            checks.that(
                rooted_version.returncode == 0,
                f"the root install's /usr/local/bin/blastit --version exits "
                f"{rooted_version.returncode}: {tail(joined(rooted_version), 8)}",
            )
        if real:
            # The fixture is mounted read-only at `/fixture`; `fmt`/`check` run
            # in a writable copy, the way every other leg provisions `<work>`.
            provisioned = box.exec(
                [
                    "sh",
                    "-c",
                    (
                        "rm -rf /work/fixture && mkdir -p /work/fixture"
                        " && cp -a /fixture/. /work/fixture/"
                    ),
                ]
            )
            checks.that(
                provisioned.returncode == 0,
                f"the Alpine leg could not copy /fixture to /work/fixture "
                f"(exit {provisioned.returncode}): {tail(joined(provisioned), 8)}",
            )
        installed = box.exec(
            ["sh", "/src/docs/install.sh", "--user"],
            env={
                "BLASTIT_RELEASE_BASE_URL": f"http://127.0.0.1:{port}/release-musl",
                "BLASTIT_SHA256": digest,
            },
        )
        checks.that(
            installed.returncode == 0,
            f"`install.sh --user` on Alpine exits {installed.returncode}: "
            f"{tail(joined(installed), 14)}",
        )
        entry = f"{prefix}/bin/blastit"
        link = box.exec(["readlink", entry])
        checks.that(
            link.stdout.strip() == "../share/blastit/lib/blastit",
            f"{entry} points at {link.stdout.strip()!r}, "
            "not ../share/blastit/lib/blastit",
        )
        printed = box.exec([entry, "--version"])
        checks.that(
            printed.stdout.strip().startswith(f"blastit {version}"),
            f"`{entry} --version` on Alpine printed {printed.stdout.strip()!r}",
        )
        if real:
            checks.that(
                printed.stdout.strip() == f"blastit {version}",
                f"the real musl binary printed {printed.stdout.strip()!r}, "
                f"not blastit {version}",
            )
            formatted = box.exec(
                [entry, "fmt", "--check", "Blastfile"], workdir="/work/fixture"
            )
            checks.that(
                formatted.returncode == 0,
                f"`blastit fmt --check` on Alpine exits {formatted.returncode}: "
                f"{tail(joined(formatted), 10)}",
            )
            checked = box.exec(
                [entry, "check"],
                workdir="/work/fixture",
                env={"BLASTIT_MEM_HIGH_WATERMARK": "100", "TMPDIR": "/work"},
            )
            checks.that(
                checked.returncode == 0,
                f"`blastit check` on Alpine exits {checked.returncode}: "
                f"{tail(joined(checked), 12)}",
            )
            # `--user` cannot install the library, so its refusal has to name the
            # command that can. Removing the package puts the container back in
            # the state a fresh Alpine host is in.
            removed = box.exec(["apk", "del", "libgcc"], user="0", timeout=300)
            checks.that(
                removed.returncode == 0,
                f"'apk del libgcc' exits {removed.returncode}: {tail(joined(removed), 8)}",
            )
            hinted = box.exec(
                [
                    "sh",
                    "/src/docs/install.sh",
                    "--user",
                    "--prefix",
                    "/work/hint-prefix",
                ],
                env={
                    "BLASTIT_RELEASE_BASE_URL": f"http://127.0.0.1:{port}/release-musl",
                    "BLASTIT_SHA256": digest,
                },
            )
            checks.that(
                hinted.returncode != 0,
                "the --user install on a musl host with no libgcc_s.so.1 exited 0",
            )
            checks.that(
                "run: apk add libgcc" in joined(hinted),
                f"the refusal printed no 'run: apk add libgcc': {tail(joined(hinted), 10)}",
            )
            restored = box.exec(
                ["apk", "add", "--no-cache", "libgcc"], user="0", timeout=300
            )
            checks.that(
                restored.returncode == 0,
                f"'apk add --no-cache libgcc' exits {restored.returncode}: "
                f"{tail(joined(restored), 8)}",
            )
        refused = box.exec(
            [
                "sh",
                "/src/docs/install.sh",
                "--user",
                "--prefix",
                "/work/tampered-prefix",
            ],
            env={
                "BLASTIT_RELEASE_BASE_URL": f"http://127.0.0.1:{port}/release-musl-tampered",
                "BLASTIT_SHA256": digest,
            },
        )
        checks.that(refused.returncode != 0, "Alpine installed a tampered archive")
        checks.that(
            "sha256 mismatch for" in joined(refused)
            and "refusing to install it" in joined(refused),
            f"the Alpine tamper refusal is not a sentence: {tail(joined(refused), 8)}",
        )
        left = box.exec(["test", "!", "-e", "/work/tampered-prefix/bin/blastit"])
        checks.that(left.returncode == 0, "a tampered archive left a blastit on Alpine")
        if checks.failures:
            print(tail(box.logs(80), 40), flush=True)
    if not real:
        print(
            "  note alpine: the archive holds a stand-in (no musl build on this host); "
            "build one with `scripts/build-standalone.sh --target "
            f"{target}` to run the real fmt/check and the root//--user libgcc "
            "assertions there",
            flush=True,
        )
    return checks.done(
        f"alpine:3.21 non-root ({'musl build' if real else 'fixture archive'})"
    )


def leg_package(
    scratch: Path, kind: str, package: Path, image: str, modes: list[str], version: str
) -> str:
    """`docs/install.sh` with `BLASTIT_DEB`/`BLASTIT_RPM` inside the target image."""
    require_docker()
    checks = Checks()
    name = f"blastit-pkg-{kind}-{os.getpid()}"
    port = PORTS.take()
    wsgi_port = PORTS.take() if "wsgi" in modes else None
    publishes = [port] + ([wsgi_port] if wsgi_port is not None else [])
    install_env = {
        "BLASTIT_DEB" if kind == "deb" else "BLASTIT_RPM": f"/pkg/{package.name}"
    }
    dev_env = {
        "BLASTIT_MEM_HIGH_WATERMARK": "100",
        "BLASTIT_DEV_EXPOSE": "1",
        "TMPDIR": "/work/tmp",
    }
    with Container(
        name,
        image,
        scratch,
        mounts=[
            (package.parent, "/pkg"),
            (ROOT, "/src"),
            (FIXTURES / "fixture", "/fixture"),
            (FIXTURES / "fixture-wsgi", "/fixture-wsgi"),
        ],
        publish=publishes,
    ) as box:
        installed = box.exec(
            ["sh", "/src/docs/install.sh"], env=install_env, timeout=1200
        )
        checks.that(
            installed.returncode == 0,
            f"`install.sh` with {package.name} exits {installed.returncode} in {image}: "
            f"{tail(joined(installed), 14)}",
        )
        payload = installed_files(box, kind)
        for path in (
            "/usr/bin/blastit",
            "/usr/bin/blastit-lsp",
            "/usr/share/blastit/LICENSE",
            "/usr/share/blastit/python/blastit/__main__.py",
        ):
            checks.that(path in payload, f"{path} is not in the installed payload")
        shared = [path for path in payload if path.startswith("/usr/share/blastit/")]
        checks.that(
            len(shared) == 13,
            f"{len(shared)} paths under /usr/share/blastit, §6.1 ships 13 "
            f"(LICENSE, 8 shims, 4 modules)",
        )
        for entry in ("blastit", "blastit-lsp"):
            link = box.exec(["readlink", f"/usr/bin/{entry}"])
            checks.that(
                link.stdout.strip() == f"../share/blastit/lib/{entry}",
                f"/usr/bin/{entry} points at {link.stdout.strip()!r}, "
                f"not ../share/blastit/lib/{entry}",
            )
        if kind == "deb":
            header = box.exec(["dpkg-deb", "-c", f"/pkg/{package.name}"])
            owned = [
                fields[1]
                for fields in (line.split() for line in header.stdout.splitlines())
                if len(fields) > 1 and fields[1] != "root/root"
            ]
        else:
            header = box.exec(
                [
                    "rpm",
                    "-qp",
                    "--qf",
                    "[%{FILEUSERNAME}:%{FILEGROUPNAME} %{FILENAMES}\n]",
                    f"/pkg/{package.name}",
                ]
            )
            owned = [
                line
                for line in header.stdout.splitlines()
                if line and not line.startswith("root:root ")
            ]
        checks.that(not owned, f"payload entries not owned root: {owned[:3]}")

        printed = box.exec(["blastit", "--version"])
        checks.that(
            printed.stdout.strip() == f"blastit {version}",
            f"`blastit --version` in {image} printed {printed.stdout.strip()!r}",
        )
        printed = box.exec(["blastit-lsp", "--version"])
        checks.that(
            printed.stdout.strip().startswith("blastit-lsp "),
            f"`blastit-lsp --version` in {image} printed {printed.stdout.strip()!r}",
        )

        box.exec(
            [
                "sh",
                "-c",
                (
                    "rm -rf /work/project && mkdir -p /work/project /work/tmp"
                    " && cp -a /fixture/. /work/project/"
                ),
            ]
        )
        formatted = box.exec(["blastit", "fmt", "Blastfile"], workdir="/work/project")
        checks.that(
            formatted.returncode == 0,
            f"`blastit fmt` in {image} exits {formatted.returncode}: "
            f"{tail(joined(formatted), 10)}",
        )
        canonical = box.exec(
            ["blastit", "fmt", "--check", "Blastfile"], workdir="/work/project"
        )
        checks.that(
            canonical.returncode == 0,
            f"`blastit fmt --check` in {image} exits {canonical.returncode}: "
            f"{tail(joined(canonical), 10)}",
        )
        checked = box.exec(["blastit", "check"], workdir="/work/project", env=dev_env)
        checks.that(
            checked.returncode == 0,
            f"`blastit check` in {image} exits {checked.returncode}: "
            f"{tail(joined(checked), 12)}",
        )
        box.exec(
            [
                "sh",
                "-c",
                (
                    f"cd /work/project && exec blastit dev --host 0.0.0.0 --port {port}"
                    " --no-reload >/work/dev.log 2>&1"
                ),
            ],
            env=dev_env,
            detach=True,
        )
        try:
            body = wait_for(port, "/", b"blastit package fixture")
            checks.that(
                "blastit package fixture" in body, "the served page is not the fixture"
            )
        except LegFailure as failure:
            checks.that(False, f"the installed package served nothing: {failure}")

        if "wsgi" in modes and wsgi_port is not None:
            # `MB.42.11`: the gate imports the app through the *package's*
            # `python/blastit`, and the view is served by the shipped shim.
            fetched = box.exec(
                [
                    "sh",
                    "-c",
                    (
                        "apt-get update -qq >/dev/null && apt-get install -y -qq "
                        "--no-install-recommends python3-pytest libpython3.13 >/dev/null"
                    ),
                ],
                timeout=1800,
            )
            checks.that(
                fetched.returncode == 0,
                f"the WSGI interpreter did not install in {image}: {tail(joined(fetched), 10)}",
            )
            box.exec(
                [
                    "sh",
                    "-c",
                    "rm -rf /work/wsgi && mkdir -p /work/wsgi && cp -a /fixture-wsgi/. /work/wsgi/",
                ]
            )
            gated = box.exec(
                [
                    "env",
                    "-i",
                    "PATH=/usr/local/bin:/usr/bin:/bin",
                    "HOME=/root",
                    "TMPDIR=/work/tmp",
                    "blastit",
                    "check",
                ],
                workdir="/work/wsgi",
            )
            checks.that(
                gated.returncode == 0,
                f"`blastit check` on the WSGI fixture exits {gated.returncode}: "
                f"{tail(joined(gated), 14)}",
            )
            box.exec(
                [
                    "sh",
                    "-c",
                    (
                        f"cd /work/wsgi && exec blastit dev --host 0.0.0.0 --port {wsgi_port}"
                        " --no-reload >/work/wsgi.log 2>&1"
                    ),
                ],
                env=dev_env,
                detach=True,
            )
            try:
                wait_for(wsgi_port, "/", b"hello from the packaged shim")
            except LegFailure as failure:
                checks.that(False, f"the package's WSGI view never served: {failure}")
        if checks.failures:
            print(tail(box.logs(80), 50), flush=True)
    return checks.done(f"{kind} ({image})")


def wheel_images(wheel: Path) -> list[tuple[str, str]]:
    """The matrix's `runtime_images` for this wheel's own (arch, libc).

    One `py3-none` wheel serves every CPython `>=3.12`, so the interpreters it is
    installed into are a fact of the leg that built it: `python:3.12/3.13/3.14-slim`
    for a `manylinux` wheel and the `-alpine` images for a `musllinux` one. They
    are read from `scripts/platform_matrix.py --matrix wheels`, never spelled here,
    so a wheel and the images it is smoked in cannot disagree (`MB.42.3.3`).
    """
    tag = wheel.name.rsplit("-", 1)[-1].removesuffix(".whl")
    arch = "aarch64" if tag.endswith("aarch64") else "x86_64"
    libc = "musl" if tag.startswith("musllinux") else "gnu"
    leg = next(
        (
            item
            for item in wheel_legs()
            if item["arch"] == arch and item["libc"] == libc
        ),
        None,
    )
    if leg is None:
        raise LegFailure(
            f"{wheel.name} is not a platform this matrix declares "
            "(`scripts/platform_matrix.py --matrix wheels`)"
        )
    return [
        (image.rsplit(":", 1)[-1].split("-")[0], image)
        for image in leg["runtime_images"].split(",")
    ]


def leg_wheel(scratch: Path, wheel: Path, version: str) -> str:
    """The universal wheel: `pip install` in the images its leg's matrix names."""
    require_docker()
    checks = Checks()
    images = wheel_images(wheel)
    for minor, image in images:
        name = f"blastit-pkg-wheel-{minor.replace('.', '')}-{os.getpid()}"
        port = PORTS.take()
        dev_env = {
            "BLASTIT_MEM_HIGH_WATERMARK": "100",
            "BLASTIT_DEV_EXPOSE": "1",
            "TMPDIR": "/work/tmp",
        }
        with Container(
            name,
            image,
            scratch / f"wheel-{minor}",
            mounts=[
                (wheel.parent, "/wheels"),
                (FIXTURES / "fixture", "/fixture"),
                (FIXTURES / "fixture-wsgi", "/fixture-wsgi"),
            ],
            publish=[port],
            environment={"PIP_DISABLE_PIP_VERSION_CHECK": "1", "PIP_NO_CACHE_DIR": "1"},
        ) as box:
            if "alpine" in image:
                # A dynamically linked musl Rust binary needs `libgcc_s.so.1`,
                # and a bare interpreter image does not carry it (the matrix's
                # musl leg says so). Installed here so the wheel's own binary —
                # not the image — is what answers.
                box.exec(
                    ["sh", "-c", "apk add --no-cache libgcc >/dev/null"],
                    timeout=300,
                )
            installed = box.exec(
                [
                    "python",
                    "-m",
                    "pip",
                    "install",
                    "--no-index",
                    "--find-links",
                    "/wheels",
                    "blastit",
                ],
                timeout=1800,
            )
            checks.that(
                installed.returncode == 0,
                f"`pip install` of {wheel.name} in {image} exits "
                f"{installed.returncode}: {tail(joined(installed), 14)}",
            )
            printed = box.exec(["blastit", "--version"])
            checks.that(
                printed.stdout.strip() == f"blastit {version}",
                f"`blastit --version` in {image} printed {printed.stdout.strip()!r}",
            )
            printed = box.exec(["blastit-lsp", "--version"])
            checks.that(
                printed.stdout.strip().startswith("blastit-lsp "),
                f"`blastit-lsp --version` in {image} printed {printed.stdout.strip()!r}",
            )
            box.exec(
                [
                    "sh",
                    "-c",
                    (
                        "rm -rf /work/project && mkdir -p /work/project /work/tmp"
                        " && cp -a /fixture/. /work/project/"
                    ),
                ]
            )
            formatted = box.exec(
                ["blastit", "fmt", "Blastfile"], workdir="/work/project"
            )
            checks.that(
                formatted.returncode == 0,
                f"`blastit fmt` in {image} exits {formatted.returncode}: "
                f"{tail(joined(formatted), 10)}",
            )
            checked = box.exec(
                ["blastit", "check"], workdir="/work/project", env=dev_env
            )
            checks.that(
                checked.returncode == 0,
                f"`blastit check` in {image} exits {checked.returncode}: "
                f"{tail(joined(checked), 12)}",
            )
            if minor != "3.12":
                # A WSGI view is served on 3.13 and 3.14 only; 3.12 is the
                # CLI/`check` half of the one-artifact claim.
                box.exec(
                    [
                        "sh",
                        "-c",
                        (
                            "rm -rf /work/wsgi && mkdir -p /work/wsgi"
                            " && cp -a /fixture-wsgi/. /work/wsgi/"
                        ),
                    ]
                )
                box.exec(
                    [
                        "sh",
                        "-c",
                        (
                            f"cd /work/wsgi && exec blastit dev --host 0.0.0.0 --port {port}"
                            " --no-reload >/work/wsgi.log 2>&1"
                        ),
                    ],
                    env=dev_env,
                    detach=True,
                )
                try:
                    wait_for(port, "/", b"hello from the packaged shim")
                except LegFailure as failure:
                    checks.that(
                        False, f"the wheel served no WSGI view on {minor}: {failure}"
                    )
            if checks.failures:
                print(tail(box.logs(80), 50), flush=True)
    return checks.done(f"wheel py3-none ({', '.join(minor for minor, _ in images)})")


def leg_composer(scratch: Path, version: str) -> str:
    """`packages/composer-blastit` in `composer:2`, with a local fixture release."""
    require_docker()
    checks = Checks()
    fixture = scratch / "composer-fixture"
    stage = fixture / "stage"
    for libc in ("gnu", "musl"):
        # The archive name is the release asset's (`blastit-<arch>-unknown-linux-<libc>`),
        # which is what `scripts/composer_sha256.py` keys its table rows on.
        target = f"blastit-{arch()}-unknown-linux-{libc}"
        (stage / target).mkdir(parents=True, exist_ok=True)
        standin(
            stage / target,
            "blastit",
            f"#!/bin/sh\nprintf 'blastit {version} (composer wrapper fixture)\\n'\n",
        )
        packed = run(
            ["tar", "-C", str(stage), "-czf", str(fixture / f"{target}.tar.gz"), target]
        )
        if packed.returncode != 0:
            raise LegFailure(
                f"tar could not build {target}.tar.gz: {tail(joined(packed))}"
            )
    (fixture / "SHA256SUMS").write_text(
        "".join(
            f"{sha256_of(path)}  {path.name}\n"
            for path in sorted(fixture.glob("*.tar.gz"))
        ),
        encoding="utf-8",
    )

    package = scratch / "composer-pkg"
    shutil.copytree(ROOT / "packages" / "composer-blastit", package)
    stamped = run(
        [
            sys.executable,
            str(ROOT / "scripts" / "composer_sha256.py"),
            "--version",
            version,
            "--wrapper",
            str(package / "bin" / "blastit"),
            "--archives",
            *[str(path) for path in sorted(fixture.glob("*.tar.gz"))],
        ],
        timeout=120,
    )
    checks.that(
        stamped.returncode == 0,
        f"the digest table did not stamp: {tail(joined(stamped), 10)}",
    )

    project = scratch / "composer-project"
    project.mkdir(parents=True, exist_ok=True)
    (project / "composer.json").write_text(
        f"""{{
  "repositories": [
    {{"type": "path", "url": "/work/composer-pkg",
      "options": {{"symlink": false,
                   "versions": {{"offworldnexus/blastit": "{version}"}}}}}},
    {{"packagist.org": false}}
  ],
  "require": {{"offworldnexus/blastit": "{version}"}},
  "config": {{"allow-plugins": false}}
}}
""",
        encoding="utf-8",
    )

    tampered = scratch / "composer-tampered"
    shutil.copytree(fixture, tampered)
    for tarball in tampered.glob("*.tar.gz"):
        with tarball.open("ab") as handle:
            handle.write(b"appended by the test\n")
    nosums = scratch / "composer-nosums"
    nosums.mkdir(parents=True, exist_ok=True)
    for tarball in fixture.glob("*.tar.gz"):
        shutil.copy2(tarball, nosums / tarball.name)

    name = f"blastit-pkg-composer-{os.getpid()}"
    with Container(
        name,
        "composer:2",
        scratch,
        mounts=[(ROOT, "/src")],
        user=f"{os.getuid()}:{os.getgid()}",
        environment={
            "COMPOSER_HOME": "/work/composer-home",
            "COMPOSER_ALLOW_SUPERUSER": "1",
        },
    ) as box:
        installed = box.exec(
            ["composer", "install", "--no-interaction", "--no-progress", "--quiet"],
            workdir="/work/composer-project",
            timeout=1200,
        )
        checks.that(
            installed.returncode == 0,
            f"`composer install` exits {installed.returncode}: {tail(joined(installed), 14)}",
        )
        built = box.exec(
            ["test", "-x", "vendor/bin/blastit"], workdir="/work/composer-project"
        )
        checks.that(
            built.returncode == 0,
            "`composer install` produced no vendor/bin/blastit (the `bin` wiring)",
        )
        musl = box.exec(["sh", "-c", "test -e /lib/ld-musl-*.so.1"]).returncode == 0
        archive = (
            f"/work/composer-fixture/blastit-{arch()}-unknown-linux-"
            f"{'musl' if musl else 'gnu'}.tar.gz"
        )

        # The default origin (the embedded table) is the only path a real
        # `composer require` takes: no `BLASTIT_RELEASE_BASE_URL`, archive pre-seeded.
        origin = {
            "BLASTIT_ARCHIVE": archive,
            "BLASTIT_COMPOSER_CACHE": "/work/cache-default",
        }
        first = box.exec(
            ["./vendor/bin/blastit", "--version"],
            workdir="/work/composer-project",
            env=origin,
        )
        checks.that(
            first.returncode == 0 and f"blastit {version}" in first.stdout,
            f"`vendor/bin/blastit --version` printed {first.stdout.strip()!r}: "
            f"{tail(joined(first), 8)}",
        )
        warm = box.exec(
            ["./vendor/bin/blastit", "--version"],
            workdir="/work/composer-project",
            env=origin,
        )
        checks.that(
            warm.returncode == 0 and "fetching" not in joined(warm),
            f"a warm run printed the cold-path fetch line: {joined(warm)[:160]!r}",
        )

        from_no_digest = box.exec(
            ["sh", "/work/composer-pkg/bin/blastit", "--version"],
            env={
                "BLASTIT_RELEASE_BASE_URL": "file:///work/composer-nosums",
                "BLASTIT_COMPOSER_CACHE": "/work/cache-nosums",
            },
        )
        checks.that(
            from_no_digest.returncode != 0
            and "a digest is required" in joined(from_no_digest),
            f"an origin with no digest was accepted: {tail(joined(from_no_digest), 8)}",
        )

        substituted = box.exec(
            ["sh", "/work/composer-pkg/bin/blastit", "--version"],
            env={
                "BLASTIT_RELEASE_BASE_URL": "file:///work/composer-tampered",
                "BLASTIT_COMPOSER_CACHE": "/work/cache-tampered",
            },
        )
        checks.that(
            substituted.returncode != 0
            and "does not match its digest" in joined(substituted),
            f"a tampered archive was executed: {tail(joined(substituted), 8)}",
        )

        pending = box.exec(
            ["sh", "/src/packages/composer-blastit/bin/blastit", "--version"],
            env={
                "BLASTIT_ARCHIVE": archive,
                "BLASTIT_COMPOSER_CACHE": "/work/cache-pending",
            },
        )
        checks.that(
            pending.returncode != 0
            and re.search(r"publishes no .*archive yet", joined(pending)) is not None,
            f"an unstamped (pending) table ran an archive: {tail(joined(pending), 8)}",
        )
        if checks.failures:
            print(tail(box.logs(60), 30), flush=True)
    return checks.done("composer wrapper (vendor/bin/blastit)")


# ---------------------------------------------------------------------------
# the run
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--leg", action="append", choices=LEGS, help="run only this leg (repeatable)"
    )
    parser.add_argument("--binary", type=Path, help="the blastit artifact under test")
    parser.add_argument(
        "--lsp", type=Path, help="blastit-lsp beside it (inferred when absent)"
    )
    parser.add_argument("--deb", type=Path, help="the built .deb")
    parser.add_argument("--rpm", type=Path, help="the built .rpm")
    parser.add_argument("--wheel", type=Path, help="the built py3-none wheel")
    parser.add_argument("--image", help="the image a --deb/--rpm leg installs in")
    parser.add_argument(
        "--modes",
        default="",
        help="space-separated phases for a package leg: install static [wsgi]",
    )
    parser.add_argument(
        "--musl-binary", type=Path, help="a musl build to serve the Alpine leg with"
    )
    parser.add_argument(
        "--musl-archive", type=Path, help="an existing musl release tarball"
    )
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    parser.add_argument(
        "--require-legs",
        action="store_true",
        help=(
            "fail when a leg skips: CI passes this, because a runner without "
            "Docker or the artifact would otherwise exit 0 having proved nothing"
        ),
    )
    arguments = parser.parse_args()

    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix="run-", dir=WORK_ROOT))
    summary = Summary()
    wanted = set(arguments.leg or LEGS)
    version = workspace_version()
    modes = arguments.modes.split() or ["install", "static"]

    try:
        legs: dict[str, object] = {
            "composer": lambda: leg_composer(workspace / "composer", version),
            "alpine": lambda: leg_alpine(
                workspace / "alpine",
                version,
                arguments.musl_binary,
                arguments.musl_archive,
                FIXTURES / "fixture",
            ),
            "wheel": lambda: leg_wheel(
                workspace / "wheel",
                discover(
                    arguments.wheel or from_environment("WHEEL"),
                    "blastit-*.whl",
                    "wheel",
                    "make wheel-manylinux",
                ),
                version,
            ),
            "deb": lambda: leg_package(
                workspace / "deb",
                "deb",
                discover(
                    arguments.deb or from_environment("BLASTIT_DEB"),
                    "*.deb",
                    ".deb",
                    "make packages",
                ),
                arguments.image or "debian:trixie",
                modes,
                version,
            ),
            "rpm": lambda: leg_package(
                workspace / "rpm",
                "rpm",
                discover(
                    arguments.rpm or from_environment("BLASTIT_RPM"),
                    "*.rpm",
                    ".rpm",
                    "make packages",
                ),
                arguments.image or "rockylinux:9",
                modes,
                version,
            ),
        }
        if "install" in wanted:
            try:
                binary = binary_argument(arguments.binary)
                lsp = lsp_argument(binary, arguments.lsp)
                printed = version_of(binary)
            except Skip as skipped:
                summary.add("install", "SKIP", str(skipped))
                wanted.discard("install")
            else:
                summary.add(
                    "binary",
                    "PASS" if printed == version else "FAIL",
                    f"{binary} ({binary.stat().st_size} bytes) says blastit {printed}"
                    + (
                        f"; lsp {lsp}"
                        if lsp is not None
                        else "; no blastit-lsp beside it"
                    ),
                )
                legs["install"] = lambda: leg_install_user(
                    workspace / "install", binary, lsp, version
                )
        for leg in LEGS:
            if leg not in wanted:
                continue
            try:
                summary.add(leg, "PASS", legs[leg]())
            except Skip as skipped:
                summary.add(leg, "SKIP", str(skipped))
            except LegFailure as failure:
                summary.add(leg, "FAIL", str(failure))
            except Exception as error:  # noqa: BLE001 - one broken leg must not end the run
                summary.add(leg, "FAIL", f"{type(error).__name__}: {error}")
    finally:
        if not arguments.keep:
            shutil.rmtree(workspace, ignore_errors=True)
    return summary.report(arguments.require_legs)


if __name__ == "__main__":
    sys.exit(main())
