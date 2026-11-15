"""The PHP entry-swap check, driven from the host into a container of its own.

`e2e/rootless/run.py` calls `check_php_entry_swap_leaves_sentinel_untouched`,
which owns this module: a container with the PHP fixture baked in (no bind
mount, so the swap happens inside the container's own filesystem), a real
`blastit dev` running as uid 1000, and a privileged harness (`docker exec -u 0`)
that renames the route's `public/` aside and points it at a decoy tree.

The observed facts it reports, all measured (MB.3.7):

* the live route never runs the decoy — the decoy appends one line per open to a
  world-writable log, and that log stays empty; the response carries no decoy
  marker, and the entry refuses (a 5xx) because `open_basedir` is
  `<manifest_dir>:<view_tmp>` and the decoy is outside it;
* a worker recycled while the symlink is in place refuses the same way;
* restoring `public/` and recycling serves again, from an entry whose
  `(st_dev, st_ino)` is the application's own, inside the route's manifest dir;
* the route uid cannot `chmod`, create in or unlink the application tree
  (`EPERM`/`EACCES`), which is the permission half `ADR-0010` §2 relies on.

What it does **not** claim: that `[route.php] entry` is pinned by inode at boot.
It is a path the worker resolves per request (only `libphp` and the shim are
fd-pinned, `ADR-0010` §1), so a decoy *inside* the application root would run;
the check measures and prints that case instead of asserting it away.
"""

from __future__ import annotations

import json
import os
import subprocess
import time

CONTAINER = "blastit-d6-php-swap"
PROBE = "/opt/e2e/entry_probe.py"
DECOY = "/tmp/entry-swap-decoy"


def _docker(*args, check_rc=False, **kw):
    return subprocess.run(
        ["docker", *args], text=True, capture_output=True, check=False, **kw
    )


def _exec(*args, user="0", flags=()):
    """`docker exec`: the flags go *before* the container name, always.

    After it, docker treats them as the command (`exec -u 0 c -d …` tries to run
    `-d`), which is a 127 that looks like a missing binary.
    """
    completed = _docker("exec", "-u", user, *flags, CONTAINER, *args)
    if completed.returncode in (125, 126, 127):
        # A docker-level failure, not the command's own status: name it, or it
        # reads as the assertion under test failing for the wrong reason.
        print(
            f"  info docker exec -u {user} {' '.join(args)}: {completed.returncode} "
            f"{(completed.stderr or '').strip()[-300:]}",
            flush=True,
        )
    return completed


class _Server:
    """The `blastit dev` under test: started as uid 1000, stopped by pid."""

    def __init__(self) -> None:
        self.started = False

    def start(self) -> None:
        completed = _exec(
            "sh",
            "-c",
            "echo $$ > /tmp/dev.pid; exec blastit dev > /tmp/dev.log 2>&1",
            user="1000",
            flags=("-d", "-w", "/app"),
        )
        self.started = completed.returncode == 0
        time.sleep(1)
        if _exec("test", "-s", "/tmp/dev.log").returncode != 0:
            self.started = False

    def wait_ready(self, timeout: float = 120.0) -> bool:
        """The loopback answers `200` for `/` (a fresh, healthy boot)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if get("/").get("status") == 200:
                return True
            time.sleep(0.2)
        return False

    def wait_head(self, timeout: float = 120.0) -> bool:
        """Some response head comes back, whatever its status.

        The gate the review asked for: a restarted worker that is *dead* leaves
        `status` at `0` (`entry_probe.py`), so a check that only tests
        `status != 200` would pass on a server that never started. Waiting for
        a head first makes the later `500 <= status < 600` mean "it refused"
        rather than "nothing answered".
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if get("/__inspect").get("status", 0) != 0:
                return True
            time.sleep(0.2)
        return False

    def stop(self) -> None:
        if not self.started:
            return
        _exec("sh", "-c", "kill -TERM $(cat /tmp/dev.pid) 2>/dev/null; sleep 1")
        self.started = False

    def log(self) -> str:
        return _exec("sh", "-c", "cat /tmp/dev.log 2>/dev/null || true").stdout


def get(path: str) -> dict:
    """One GET inside the container's network namespace, as JSON."""
    completed = _exec("python3", PROBE, f"http://127.0.0.1:8080{path}", user="1000")
    for line in completed.stdout.splitlines():
        try:
            document = json.loads(line)
        except ValueError:
            continue
        if isinstance(document, dict):
            return document
    return {
        "error": (completed.stdout + completed.stderr).strip()[-400:] or "no answer"
    }


def _status(response: dict) -> int:
    return int(response.get("status", 0))


def _refused(response: dict) -> bool:
    """A 5xx answer, and never `0` ("no head") read as a pass."""
    return 500 <= _status(response) < 600


def _stat(path: str) -> tuple[int, int] | None:
    out = _exec("stat", "-c", "%d %i", path).stdout.split()
    if len(out) != 2:
        return None
    return int(out[0]), int(out[1])


def _opens() -> int:
    out = _exec(
        "sh", "-c", f"wc -l < {DECOY}/opens.log 2>/dev/null || echo 0"
    ).stdout.strip()
    return int(out or 0)


def _install_fixture() -> bool:
    """`/fixture` → `/app` (root-owned, world-readable) plus the decoy tree."""
    script = (
        "set -eu; "
        "rm -rf /app /app.public.real " + DECOY + "; "
        "cp -a /fixture /app; "
        f"mkdir -p {DECOY}; "
        "mkdir -p /app/public/uploads; "
        "cp /app/canaries/prepend.php /app/public/prepend.php; "
        "cp /app/canaries/shell.jpg /app/public/uploads/shell.jpg; "
        # Root-owned and world-readable, *not* the route uid's: that is the
        # deployment shape `ADR-0010` §2 relies on (the route uid cannot write
        # its own application tree), and it is what makes the negative half of
        # this check meaningful. `blastit dev` only needs to read it.
        "chown -R 0:0 /app; chmod -R a+rX /app; "
        "cat > " + DECOY + "/index.php <<'PHP'\n"
        "<?php\n"
        f"file_put_contents('{DECOY}/opens.log', $_SERVER['REQUEST_URI'].\"\\n\", FILE_APPEND);\n"
        'echo "DECOY-RAN\\n";\n'
        "PHP\n"
        f"chmod 0777 {DECOY}; chmod 0644 {DECOY}/index.php"
    )
    completed = _exec("sh", "-c", script)
    return completed.returncode == 0


def check_entry_0000_refused_for_route_uid(report, server) -> bool:
    """NODE.md:72: an entry the worker's uid cannot read is refused, empty-handed.

    The root harness `chmod 000`s the manifest's `entry` and the live route must
    answer a `5xx` with **zero body bytes** — no PHP warning, no path, nothing —
    and the decoy must still show 0 opens. Restoring the mode serves again.
    `server.wait_head()` gates each step: a dead server answers `0`, which must
    not be read as "fails closed".
    """
    chmod = _exec("sh", "-c", "chmod 000 /app/public/index.php")
    if not report(chmod.returncode == 0, "a privileged harness makes the entry unreadable"):
        print(chmod.stderr[-400:])
        return False
    if not report(server.wait_head(), "the unreadable entry still answers a head"):
        print(server.log()[-1200:])
        return False
    refused = get("/users/42")
    ok = report(
        _refused(refused),
        f"`chmod 000` on the entry is refused for the route uid ({_status(refused)})",
    )
    ok &= report(
        refused.get("body", "") == "",
        f"and the answer has zero body bytes ({len(refused.get('body', ''))})",
    )
    ok &= report("DECOY-RAN" not in refused.get("body", ""), "and runs no decoy")
    ok &= report(_opens() == 0, f"the decoy recorded 0 opens ({_opens()})")
    restored = _exec("sh", "-c", "chmod 0644 /app/public/index.php")
    ok &= report(restored.returncode == 0, "restoring the entry's mode succeeds")
    if not report(server.wait_head(), "the restored entry answers a head"):
        return False
    back = get("/users/42")
    ok &= report(
        _status(back) == 200 and "front-controller" in back.get("body", ""),
        f"and serves again ({_status(back)})",
    )
    return bool(ok)


def check(image: str) -> int:
    """The whole check; returns 0 only when every assertion held."""
    print("PHP: the entry is not swappable under a live route")
    ok = True

    def report(condition: bool, what: str) -> bool:
        nonlocal ok
        print(f"  {'ok  ' if condition else 'FAIL'} {what}", flush=True)
        ok = ok and bool(condition)
        return bool(condition)

    _docker("rm", "-f", CONTAINER)
    started = _docker(
        "run",
        "-d",
        "--name",
        CONTAINER,
        "--entrypoint",
        "sleep",
        image,
        "infinity",
    )
    if started.returncode != 0:
        report(
            False, f"the PHP fixture container starts ({started.stderr.strip()[-300:]})"
        )
        return 1
    server = _Server()
    try:
        if not report(
            _install_fixture(), "the fixture is installed inside the container"
        ):
            return 1
        before = _stat("/app/public/index.php")
        server.start()
        if not report(server.wait_ready(), "`blastit dev` serves the PHP route"):
            print(server.log()[-1200:])
            return 1
        live = get("/users/42")
        report(
            live.get("status") == 200 and "front-controller" in live.get("body", ""),
            f"`GET /users/42` is the front controller ({live.get('status')})",
        )
        report(before is not None, f"the entry's inode is known ({before})")

        # The swap: `public/` aside, a symlink to a decoy tree outside the app.
        swapped = _exec(
            "sh",
            "-c",
            f"mv /app/public /app/public.real && ln -s {DECOY} /app/public",
        )
        if not report(
            swapped.returncode == 0, "a privileged harness swaps `public/` aside"
        ):
            print(swapped.stderr[-400:])
            return 1
        if not report(
            server.wait_head(), "the swapped route still answers a head (not a dead server)"
        ):
            print(server.log()[-1200:])
            return 1
        after = get("/users/42")
        report(_refused(after), f"the live route fails closed while swapped ({_status(after)})")
        report(
            "DECOY-RAN" not in after.get("body", ""),
            "and the decoy's marker never reaches the client",
        )
        report(_opens() == 0, f"the decoy recorded 0 opens ({_opens()})")

        # A worker recycled while the symlink is in place: same refusal, and the
        # same gate — a restart that never came up must not read as "fails
        # closed".
        server.stop()
        server.start()
        if not report(
            server.wait_head(), "a recycled worker answers a head (not a dead server)"
        ):
            print(server.log()[-1200:])
            return 1
        recycled = get("/users/42")
        report(
            _refused(recycled),
            f"a worker recycled while symlinked fails closed ({_status(recycled)})",
        )
        report("DECOY-RAN" not in recycled.get("body", ""), "and still runs no decoy")
        report(
            _opens() == 0, f"the decoy recorded 0 opens after the recycle ({_opens()})"
        )

        # Restore: the entry is the application's own again, inside the root.
        restored = _exec(
            "sh", "-c", "rm -f /app/public && mv /app/public.real /app/public"
        )
        report(restored.returncode == 0, "restoring `public/` succeeds")
        server.stop()
        server.start()
        if not report(
            server.wait_ready(), "`blastit dev` serves again after the restore"
        ):
            print(server.log()[-1200:])
            return 1
        clean = get("/__inspect")
        document = {}
        try:
            document = json.loads(clean.get("body") or "{}")
        except ValueError:
            pass
        report(
            clean.get("status") == 200,
            f"`GET /__inspect` answers again ({clean.get('status')})",
        )
        report(
            document.get("script_filename") == "/app/public/index.php",
            f"and the entry is the manifest's, inside its root "
            f"({document.get('script_filename')!r})",
        )
        print(
            f"  info the restored entry's (st_dev, st_ino) is {_stat('/app/public/index.php')} "
            f"(it was {before} before the swap); the `mv` round-trip restores the "
            "same directory, so this is a property of the swap, not of the worker",
            flush=True,
        )

        if not check_entry_0000_refused_for_route_uid(report, server):
            return 1

        # The route uid's own reach: none of it. This is the permission half
        # `ADR-0010` §2 relies on (a deployment's manifest dir is not route-uid
        # writable), and it is what makes the swap an operator-level act.
        tenant = _exec("sh", "-c", "chmod 0644 /app/public/index.php", user="1000")
        report(
            tenant.returncode != 0,
            "the route uid cannot `chmod` the entry "
            f"({(tenant.stderr or tenant.stdout).strip()[-120:]})",
        )
        create = _exec("sh", "-c", "echo x > /app/public/planted.php", user="1000")
        report(
            create.returncode != 0,
            "or create a file in the static root "
            f"({(create.stderr or create.stdout).strip()[-120:]})",
        )
        unlink = _exec("sh", "-c", "rm -f /app/blastit.toml", user="1000")
        report(
            unlink.returncode != 0,
            "or unlink the manifest "
            f"({(unlink.stderr or unlink.stdout).strip()[-120:]})",
        )

        # MB.3.56's witness, kept on purpose. The entry is a *path*, so a decoy
        # inside the application root is executed (`ADR-0010` §1 pins `libphp`
        # and the shim by descriptor, not the script). This is `info`, not an
        # accepted behaviour: MB.3.56 pins the entry by `(st_dev, st_ino)` and
        # this line is what that node must flip to a refusal.
        inside = _exec(
            "sh",
            "-c",
            "mkdir -p /app/decoy && cp " + DECOY + "/index.php /app/decoy/index.php && "
            "mv /app/public /app/public.real && ln -s /app/decoy /app/public",
        )
        if inside.returncode == 0:
            measured = get("/users/42")
            print(
                f"  info a decoy *inside* the manifest dir answers {measured.get('status')} "
                f"({'DECOY-RAN' if 'DECOY-RAN' in measured.get('body', '') else 'no decoy marker'}): "
                "MB.3.56's witness, not an accepted behaviour — the entry is a path "
                "the worker re-resolves per request, so only `libphp`/the shim are "
                "fd-pinned (ADR-0010 §1); MB.3.56 pins the entry by (st_dev, st_ino) "
                "and must flip this line to a refusal",
                flush=True,
            )
            _exec("sh", "-c", "rm -f /app/public && mv /app/public.real /app/public")
    finally:
        server.stop()
        if os.environ.get("E2E_KEEP_PHP_SWAP"):
            print(f"  info kept the container {CONTAINER} (E2E_KEEP_PHP_SWAP)")
        else:
            _docker("rm", "-f", CONTAINER)
    return 0 if ok else 1


def build(image: str, context: str, python_version: str | None) -> int:
    """Build the PHP image the check needs (cached after the `php` leg's build)."""
    args = ["build", "-f", os.path.join(context, "e2e", "php", "Dockerfile")]
    if python_version:
        args += ["--build-arg", f"PYTHON_VERSION={python_version}"]
    args += ["-t", image, context]
    print(f"  building {image} ({python_version})")
    return _docker(*args).returncode


if __name__ == "__main__":  # pragma: no cover - the driver imports `check`
    raise SystemExit(check(os.environ.get("E2E_PHP_IMAGE", "blastit-e2e-php")))
