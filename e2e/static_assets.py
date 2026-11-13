"""Replay the static-assets page against a real `blastit dev` (MB.2.7).

    static_assets.py --base http://127.0.0.1:8080 --site /site \
        --nospa-base http://static_nospa:8080 --page /opt/e2e/static-and-spa.md \
        --banner "$(docker compose logs --no-log-prefix static)"
    static_assets.py --self-test

`docs/frameworks/static-and-spa.md` is the input of record. This driver reads the
page, extracts every `$ ` command from its ```` ```console ```` fences, and
replays the ones a running static site can answer against the container's own
`blastit dev`:

* the banner fence (`uv run blastit dev`) matches the process's first three lines;
* each `curl -i` fence is sent at the page's own path and must return the page's
  own status line and every header line it shows;
* the `spa = true` deep URL's body is `public/index.html`, not a rewritten file;
* the hashed asset's body is the file, under the year-long immutable cache;
* `uv run blastit check` exits `0` with no output, and `--json` is one document;
* the refusals the page's threat list names answer their documented status:
  `..`, percent-encoded `..`, NUL and backslash are `400`; a dotfile and a symlink
  that leaves `root` are `404`; none of them ever carries file bytes.

Commands that need a project or a network (`npx vite build`, `uv init`, `uv add`)
or a production host (`blastit --preset compose-traefik`) are listed as skipped
with the reason; the last one is owned by `e2e/rootless` (E1). An unknown `$ `
command is a failure: a page edit cannot silently stop being replayed.

Stdlib only, no network beyond loopback/compose DNS, no fixed sleep: readiness is
polled with a deadline.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import time
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs" / "frameworks" / "static-and-spa.md"
SITE = ROOT / "e2e" / "static_site"

ASSET = "/assets/app.4f1c2a9b7d3e.js"
DEEP = "/dashboard/settings"

#: The decoys `run-dev.sh` installs: none may ever appear in a response body.
LEAK_MARKERS = (
    b"root:x:0:0",
    b"E2E_DOTENV_MARKER",
    b"E2E_GIT_CONFIG_MARKER",
    b"E2E_STATIC_SECRET_MARKER",
)

#: A refusal the page names, and the status it promises. `docs/frameworks/
#: static-and-spa.md`: "`..` ... NUL ... absolute and backslash paths are refused
#: with `400`; a symlink that leaves `root` and a dotfile such as `.env` or
#: `.git` are `404`".
DENIALS = (
    ("/../etc/passwd", 400, "plain `..` is 400"),
    ("/%2e%2e/%2e%2e/etc/passwd", 400, "percent-encoded `..` is 400"),
    (
        "/assets/%2e%2e/%2e%2e/secret",
        400,
        "encoded `..` out of a deep directory is 400",
    ),
    ("/%00.js", 400, "a NUL byte is 400"),
    ("/foo%5c..%5cbar", 400, "a backslash path is 400"),
    ("/.env", 404, "a dotfile is 404"),
    ("/.git/config", 404, "a dotfile directory is 404"),
    ("/.git/", 404, "the dotfile directory itself is 404"),
    ("/link", 404, "a symlink that leaves root is 404"),
)

#: Commands that cannot run in this container, and why. Each is still a promise
#: of the page; the reason says which leg owns it instead.
SKIPS = {
    "npx vite build --outDir public": "needs Node and a Vite project; the fixture is a build output",
    "uv init --app": "creates a project on a developer's machine; needs a writable project and the network",
    "uv add blastit": "installs from PyPI; this leg is offline by design",
    "blastit --preset compose-traefik": "a production preset; e2e/rootless (E1) proves it",
}


class Checks:
    """One PASS/FAIL line per promise; any failure makes the driver exit 1."""

    def __init__(self) -> None:
        self.passed = 0
        self.failed: list[str] = []

    def that(self, condition: bool, promise: str) -> None:
        if condition:
            self.passed += 1
            print(f"PASS: {promise}", flush=True)
        else:
            print(f"FAIL: {promise}", flush=True)
            self.failed.append(promise)


class Response:
    def __init__(
        self, status: int, reason: str, headers: list[tuple[str, str]], body: bytes
    ) -> None:
        self.status = status
        self.reason = reason
        self.headers = headers
        self.body = body

    def header(self, name: str) -> str | None:
        for header, value in self.headers:
            if header.lower() == name.lower():
                return value
        return None


_CONSOLE = re.compile(
    r"^```console\n(?P<body>.*?)^```[ \t]*$", re.MULTILINE | re.DOTALL
)


def console_commands(text: str) -> list[tuple[str, list[str]]]:
    """Every `$ ` command in a ```` ```console ```` fence, with the lines under it."""
    found: list[tuple[str, list[str]]] = []
    for match in _CONSOLE.finditer(text):
        current: list[str] | None = None
        for line in match.group("body").splitlines():
            if line.startswith("$ "):
                current = []
                found.append((line[2:], current))
            elif current is not None:
                current.append(line)
    trimmed = []
    for command, output in found:
        while output and not output[-1].strip():
            output.pop()
        trimmed.append((command, output))
    return trimmed


def documented_head(output: list[str]) -> tuple[int, str, list[tuple[str, str]]]:
    """A curl fence's output: its status line and header lines, asserted as shown."""
    if not output:
        raise ValueError("a curl fence with no documented output")
    protocol, _, rest = output[0].strip().partition(" ")
    status, _, reason = rest.partition(" ")
    if protocol != "HTTP/1.1" or not status.isdigit():
        raise ValueError(f"not an HTTP/1.1 status line: {output[0]!r}")
    headers = []
    for line in output[1:]:
        if not line.strip():
            continue
        name, separator, value = line.partition(":")
        if not separator:
            raise ValueError(f"not a header line: {line!r}")
        headers.append((name.strip(), value.strip()))
    return int(status), reason, headers


def command_kind(command: str) -> str | None:
    if command.startswith("curl -i "):
        return "curl"
    if command == "uv run blastit dev":
        return "dev"
    if command == "uv run blastit check --json":
        return "check-json"
    if command in ("uv run blastit check", "blastit check"):
        return "check"
    if command in SKIPS:
        return "skip"
    return None


def _read_head(sock: socket.socket) -> tuple[bytes, bytes]:
    buffer = b""
    while b"\r\n\r\n" not in buffer:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buffer += chunk
    head, _, rest = buffer.partition(b"\r\n\r\n")
    return head, rest


def request(
    host: str,
    port: int,
    method: str,
    target: str,
    headers: list[tuple[str, str]] | None = None,
    body: bytes | None = None,
) -> Response:
    """One HTTP/1.1 exchange on a raw socket: the request target is sent exactly,
    so `/../etc/passwd` reaches the responder rather than a client's normaliser."""
    lines = [f"{method} {target} HTTP/1.1", f"Host: {host}:{port}", "Connection: close"]
    sent = body if body is not None else b""
    if body is not None or method not in ("GET", "HEAD"):
        lines.append(f"Content-Length: {len(sent)}")
    for name, value in headers or []:
        lines.append(f"{name}: {value}")
    payload = ("\r\n".join(lines) + "\r\n\r\n").encode("latin-1") + sent

    with socket.create_connection((host, port), timeout=15) as sock:
        sock.settimeout(15)
        sock.sendall(payload)
        head, rest = _read_head(sock)
        if not head:
            raise OSError(
                f"{method} {target}: the connection closed before a response head"
            )
        head_lines = head.decode("latin-1").split("\r\n")
        protocol, _, remainder = head_lines[0].partition(" ")
        status, _, reason = remainder.partition(" ")
        if protocol != "HTTP/1.1" or not status.isdigit():
            raise OSError(f"{method} {target}: bad status line {head_lines[0]!r}")
        parsed = []
        for line in head_lines[1:]:
            name, separator, value = line.partition(":")
            if not separator:
                raise OSError(f"{method} {target}: bad header line {line!r}")
            parsed.append((name.strip(), value.strip()))

        if method == "HEAD":
            return Response(int(status), reason, parsed, b"")

        length = next((v for n, v in parsed if n.lower() == "content-length"), None)
        if length is not None:
            wanted = int(length)
            data = rest
            while len(data) < wanted:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk
            return Response(int(status), reason, parsed, data[:wanted])

        data = rest
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
        return Response(int(status), reason, parsed, data)


def split_base(base: str) -> tuple[str, int]:
    parts = urllib.parse.urlsplit(base)
    if parts.scheme != "http" or not parts.hostname or parts.port is None:
        raise SystemExit(
            f"static_assets: --base must be http://host:port, not {base!r}"
        )
    return parts.hostname, parts.port


def wait_ready(host: str, port: int, timeout: int) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            if request(host, port, "GET", "/").status == 200:
                return
        except OSError:
            pass
        time.sleep(0.1)
    raise SystemExit(f"static_assets: {host}:{port} did not answer within {timeout}s")


def run_check(site: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ)
    env["BLASTIT_MANIFEST"] = str(site / "blastit.toml")
    return subprocess.run(
        ["blastit", "check", *arguments],
        cwd=site,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )


def replay(
    checks: Checks, page: str, base: str, nospa_base: str, site: Path, banner: str
) -> None:
    host, port = split_base(base)
    nospa_host, nospa_port = split_base(nospa_base)
    index = (site / "public" / "index.html").read_bytes()
    asset = (site / "public" / "assets" / ASSET.rsplit("/", 1)[-1]).read_bytes()

    commands = console_commands(page)
    checks.that(
        bool(commands), "the page still has ```console fences with `$ ` commands"
    )
    for command, _ in commands:
        checks.that(
            command_kind(command) is not None,
            f"the page's command is replayed or explained: {command}",
        )

    wait_ready(host, port, 60)

    for command, output in commands:
        kind = command_kind(command)
        if kind == "skip":
            print(f"SKIP: {command} ({SKIPS[command]})", flush=True)
        elif kind == "dev":
            # The page shows the default 127.0.0.1:8080; `E2E_STATIC_PORT` may
            # move the container's port, and the rest of the line is verbatim.
            shown = [
                line.rstrip().replace("127.0.0.1:8080", f"{host}:{port}")
                for line in output
            ]
            lines = [line.rstrip() for line in banner.splitlines()]
            window = next(
                (
                    i
                    for i in range(len(lines) - len(shown) + 1)
                    if lines[i : i + len(shown)] == shown
                ),
                None,
            )
            checks.that(
                window is not None, f"`{command}` prints the page's banner verbatim"
            )
        elif kind == "curl":
            url = urllib.parse.urlsplit(command.split()[-1])
            target = url.path + (f"?{url.query}" if url.query else "")
            status, reason, headers = documented_head(output)
            response = request(host, port, "GET", target)
            checks.that(
                (response.status, response.reason) == (status, reason),
                f"`curl -i {url.geturl()}` is {status} {reason}",
            )
            for name, value in headers:
                checks.that(
                    response.header(name) == value,
                    f"`{target}` answers `{name}: {value}`",
                )
            if target == ASSET:
                checks.that(
                    response.body == asset, f"`{target}` is the hashed file's bytes"
                )
            elif target == DEEP:
                checks.that(
                    response.body == index, f"`{target}` is `public/index.html`"
                )
        elif kind == "check":
            result = run_check(site)
            checks.that(
                result.returncode == 0, f"`{command}` exits 0 on an all-static manifest"
            )
            checks.that(result.stdout == "", f"`{command}` prints nothing on success")
        elif kind == "check-json":
            result = run_check(site, "--json")
            checks.that(result.returncode == 0, f"`{command}` exits 0")
            try:
                document = json.loads(result.stdout)
            except ValueError:
                document = None
            checks.that(
                isinstance(document, dict),
                f"`{command}` prints exactly one JSON document",
            )
            if isinstance(document, dict):
                checks.that(document.get("ok") is True, f"`{command}` says ok")
                checks.that(
                    (document.get("routes") or {}).get("total") == 1,
                    f"`{command}` counts the one static route",
                )
                checks.that(
                    document.get("requests") == 0,
                    f"`{command}` reports no app requests",
                )

    # The refusals the page's threat list names: their own status, and never bytes.
    for target, expected, promise in DENIALS:
        response = request(host, port, "GET", target)
        checks.that(response.status == expected, f"{promise} (`{target}`)")
        checks.that(
            response.body != index, f"`{target}` is not masked by the SPA fallback"
        )
        checks.that(
            not any(marker in response.body for marker in LEAK_MARKERS),
            f"`{target}` carries no file bytes",
        )

    # `.well-known/` is the one dotfile directory the page keeps public.
    response = request(host, port, "GET", "/.well-known/security.txt")
    checks.that(response.status == 200, "`/.well-known/` stays public (200)")
    checks.that(
        response.header("content-type") == "text/plain; charset=utf-8",
        "`/.well-known/security.txt` is text/plain",
    )

    # A directory answers its `index`; no redirect, no listing.
    for target in ("/docs", "/docs/"):
        response = request(host, port, "GET", target)
        checks.that(response.status == 200, f"`{target}` is the directory index (200)")
        checks.that(
            response.body == (site / "public" / "docs" / "index.html").read_bytes(),
            f"`{target}` is `public/docs/index.html`",
        )

    # HEAD is GET's head without a body; POST is 405 with exactly `Allow`.
    head = request(host, port, "HEAD", ASSET)
    checks.that((head.status, head.reason) == (200, "OK"), "`HEAD /assets/…` is 200 OK")
    checks.that(head.body == b"", "`HEAD /assets/…` has no body")
    checks.that(
        head.header("content-length") == str(len(asset)),
        "`HEAD /assets/…` carries the GET's Content-Length",
    )
    checks.that(
        head.header("cache-control") == "public, max-age=31536000, immutable",
        "`HEAD /assets/…` carries the immutable cache header",
    )

    post = request(host, port, "POST", ASSET)
    checks.that(
        (post.status, post.reason) == (405, "Method Not Allowed"),
        "`POST /assets/…` is 405",
    )
    checks.that(
        post.header("allow") == "GET, HEAD",
        "`POST /assets/…` carries `Allow: GET, HEAD`",
    )
    checks.that(
        post.header("cache-control") == "no-store", "`POST /assets/…` is `no-store`"
    )

    with_body = request(host, port, "GET", ASSET, body=b"x")
    checks.that(with_body.status == 413, "a `GET` that declares a body is 413")

    # `spa = false`: a path that is no file is a 404, not the app and not index.
    nospa_missing = request(nospa_host, nospa_port, "GET", DEEP)
    checks.that(
        nospa_missing.status == 404, "a `spa = false` route answers a deep URL with 404"
    )
    nospa_home = request(nospa_host, nospa_port, "GET", "/")
    checks.that(
        nospa_home.status == 200, "a `spa = false` route still answers its own index"
    )


def self_test() -> int:
    checks = Checks()

    sample = (
        "intro\n```console\n$ first --flag\n  one\n  two\n$ second\n```\n"
        '```toml\n[route.static]\nroot = "public"\n```\n'
    )
    parsed = console_commands(sample)
    checks.that(
        parsed == [("first --flag", ["  one", "  two"]), ("second", [])],
        "a console fence's `$ ` commands and their output are parsed",
    )
    checks.that(
        console_commands(sample.replace("```console", "```toml")) == [],
        "a non-console fence is not a command",
    )

    head = documented_head(
        [
            "HTTP/1.1 200 OK",
            "Content-Type: text/javascript",
            "Cache-Control: public, max-age=31536000, immutable",
        ]
    )
    checks.that(
        head
        == (
            200,
            "OK",
            [
                ("Content-Type", "text/javascript"),
                ("Cache-Control", "public, max-age=31536000, immutable"),
            ],
        ),
        "a curl fence's status and headers parse as shown",
    )

    page = PAGE.read_text(encoding="utf-8")
    commands = console_commands(page)
    kinds = [command_kind(command) for command, _ in commands]
    checks.that(
        None not in kinds, "every `$ ` command on the page is replayed or explained"
    )
    checks.that(kinds.count("curl") == 2, "the page still shows two `curl -i` fences")
    checks.that(
        kinds.count("dev") == 1, "the page still shows the `blastit dev` banner fence"
    )
    checks.that(
        kinds.count("check") == 2, "the page still shows two `blastit check` fences"
    )
    checks.that(kinds.count("check-json") == 1, "the page still shows `check --json`")
    checks.that(
        kinds.count("skip") == 4, "the page still shows the four build/deploy commands"
    )

    urls = [
        command.split()[-1]
        for command, kind in zip((c for c, _ in commands), kinds)
        if kind == "curl"
    ]
    checks.that(
        [urllib.parse.urlsplit(url).path for url in urls] == [ASSET, DEEP],
        "the page's curl fences are the hashed asset and the deep SPA URL",
    )
    for command, output in commands:
        if command_kind(command) != "curl":
            continue
        status, reason, headers = documented_head(output)
        path = urllib.parse.urlsplit(command.split()[-1]).path
        checks.that(
            (status, reason) == (200, "OK"), f"`{path}` is documented as 200 OK"
        )
        if path == ASSET:
            checks.that(
                ("Cache-Control", "public, max-age=31536000, immutable") in headers,
                "the asset is documented immutable for a year",
            )
            checks.that(
                ("Content-Type", "text/javascript") in headers,
                "the asset is documented `text/javascript`",
            )
        else:
            checks.that(
                ("Content-Type", "text/html; charset=utf-8") in headers,
                "the deep URL is documented `text/html`",
            )
            checks.that(
                ("Cache-Control", "no-cache") in headers,
                "the deep URL is documented no-cache",
            )

    checks.that(
        len(DENIALS) == 9, "the driver still asserts every refusal the page names"
    )
    print(
        f"PASS: self-test, {checks.passed} checks"
        if not checks.failed
        else f"FAIL: self-test, {len(checks.failed)} of {checks.passed + len(checks.failed)} checks"
    )
    return 1 if checks.failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", help="http://host:port of the `spa = true` site")
    parser.add_argument(
        "--nospa-base", help="http://host:port of the `spa = false` site"
    )
    parser.add_argument(
        "--site", type=Path, default=SITE, help="the served fixture (its public/ tree)"
    )
    parser.add_argument(
        "--page", type=Path, default=PAGE, help="docs/frameworks/static-and-spa.md"
    )
    parser.add_argument(
        "--banner",
        default="",
        help="the static service's stdout, for the `blastit dev` fence",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="check the parser and the page's shape, then exit",
    )
    arguments = parser.parse_args()

    if arguments.self_test:
        return self_test()
    if not arguments.base or not arguments.nospa_base:
        parser.error("--base and --nospa-base are required unless --self-test")

    page = arguments.page.read_text(encoding="utf-8")
    site = arguments.site
    if not (site / "public" / "index.html").is_file():
        raise SystemExit(f"static_assets: {site} does not hold public/index.html")

    checks = Checks()
    replay(checks, page, arguments.base, arguments.nospa_base, site, arguments.banner)
    total = checks.passed + len(checks.failed)
    if checks.failed:
        print(f"FAIL: {len(checks.failed)} of {total} promises failed", flush=True)
        return 1
    print(
        f"PASS: docs/frameworks/static-and-spa.md replayed against a real blastit dev ({total} checks)",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
