"""The checks the venv smoke test runs against the server, in the venv. Stdlib only.

    check.py --base-url http://127.0.0.1:8080 --prefix /work/.venv --cwd /work \
        --marker blastit-venv-smoke

Waits for the server to answer (the boot takes a moment, and it fails fast if it
never does), then asserts what the packaging is supposed to guarantee: the app is
served by the interpreter of the virtualenv the wheel was installed into, from a
project that is not installed and has no `PYTHONPATH`.
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

FAILURES = []


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    if not cond:
        FAILURES.append(what)


def fetch(base_url, timeout):
    """The first successful response body, or the last error."""
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"{base_url}/", timeout=2) as response:
                return response.status, response.read(), dict(response.headers)
        except (urllib.error.URLError, ConnectionError, TimeoutError) as err:
            last = err
            time.sleep(0.25)
    raise SystemExit(f"{base_url}/ never answered: {last}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--prefix", required=True, help="the virtualenv's directory")
    parser.add_argument(
        "--cwd",
        help="the directory blastit was started in, which `sys.path[0]` must be "
        "(the project is not installed and no PYTHONPATH is set)",
    )
    parser.add_argument("--marker", default="blastit-venv-smoke")
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()

    status, body, headers = fetch(args.base_url, args.timeout)
    print(f"  GET {args.base_url}/ → {status} {headers.get('Content-Type')}")
    print(f"  body: {body.decode(errors='replace')}")

    check(status == 200, "the response is 200")
    try:
        document = json.loads(body)
    except ValueError as err:
        check(False, f"the body is JSON ({err})")
        return 1

    check(
        document.get("prefix") == args.prefix,
        f"sys.prefix is the virtualenv ({document.get('prefix')!r} == {args.prefix!r})",
    )
    check(
        document.get("executable", "").startswith(args.prefix),
        f"sys.executable is the venv's interpreter ({document.get('executable')!r})",
    )
    check(
        document.get("marker") == args.marker,
        "a module only in the venv's site-packages was importable",
    )
    check(
        document.get("path") == "/",
        f"PATH_INFO reached the app ({document.get('path')!r})",
    )
    check(document.get("route") == "root", f"the manifest routed it ({document.get('route')!r})")
    if args.cwd:
        first = document.get("path_first")
        check(
            first is not None and os.path.realpath(first) == os.path.realpath(args.cwd),
            f"the working directory blastit was started in is first on sys.path "
            f"({first!r}, expected {args.cwd!r})",
        )
        check(
            document.get("cwd") != args.cwd,
            "the app's own working directory is the view's scratch directory, so "
            "the entry above is the server's, not the view's",
        )

    for failure in FAILURES:
        print(f"FAILED: {failure}")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
