"""One GET against a loopback URL, printed as one JSON line.

`e2e/rootless/run.py` drives the PHP entry-swap check from the *host*, while the
server binds `127.0.0.1` inside the container: this is the probe it runs there
(`docker exec ... python3 /opt/e2e/entry_probe.py http://127.0.0.1:8080/…`).

Stdlib only, and deliberately tiny: a check that fails closed must be able to
tell a status from a refused connection without a client library's retries.
"""

from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: entry_probe.py URL", file=sys.stderr)
        return 2
    url = sys.argv[1]
    try:
        with urllib.request.urlopen(url, timeout=20) as response:  # noqa: S310 - loopback
            body = response.read(65536).decode("utf-8", errors="replace")
            print(json.dumps({"status": response.status, "body": body}))
    except urllib.error.HTTPError as error:
        body = error.read(65536).decode("utf-8", errors="replace")
        print(json.dumps({"status": error.code, "body": body}))
    except OSError as error:
        # `status: 0`, never a missing key: a caller that tests `status != 200`
        # must not read a refused connection as a successful assertion. Zero is
        # "no response head", which is the only honest answer here.
        print(json.dumps({"status": 0, "error": str(error)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
