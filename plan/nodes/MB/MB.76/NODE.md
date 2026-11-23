---
id: MB.76
title: "Bound every network step in the php-sapi image so a stall fails fast"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/php-sapi/Dockerfile", e2e/php_sapi_probe.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T12:37"
updated: "2026-11-23T13:23"
---

# Goal

Two consecutive CI attempts of the six `*-gnu` shim jobs sat inside `docker build` for the **full 30-minute `PHP_SAPI_TIMEOUT`** and died with `subprocess.TimeoutExpired`, while the same images build in ~2 minutes when the network is healthy and the `*-musl` legs were fine. The build has **no acquire timeout**, so a stalled fetch (Debian/Ubuntu mirror, or a rustup/uv download) consumes the entire job instead of failing fast into the retry ladder `MB.62`/`MB.74` already provide.

# Change

Make every network step inside `e2e/php-sapi/Dockerfile` bounded and retried, so a stall becomes a fast, *recoverable* failure:

1. APT: every `apt-get update`/`apt-get install` gets explicit `-o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=3 -o Acquire::ForceIPv4=true` (IPv4 avoids a blackholed v6 path, a common CI stall). Keep `MB.62`'s update escalation exactly as it is — the timeouts make each attempt bounded, they do not replace it.
2. `curl`: every invocation gets `--connect-timeout 15 --max-time 120 --retry 3` (the sury key/InRelease/payload fetches included).
3. Alpine path: `apk` gets `--timeout 60` (or the documented equivalent) on `apk update`/`apk add`.
4. rustup/uv installs in the image: bound them the same way (`RUSTUP_MAX_RETRIES`, curl flags), and **fail with a message naming the download** rather than hanging.

Also thread the same bounded-acquire settings through `e2e/php_sapi_probe.py`'s own `docker build` call so the probe cannot hang longer than the job: keep `PHP_SAPI_TIMEOUT` as the final backstop, but make the *inner* steps bounded so the retry machinery is reachable.

# Acceptance

- `docker build` of the php-sapi image for one gnu leg completes locally, and a deliberately unreachable mirror (`-o Acquire::https::Timeout=1` against a blackholed host, or a firewalled address) **fails within seconds**, not minutes.
- The `MB.62` escalation still triggers on a genuine index mismatch (re-run its proof or restate it).
- No behaviour change to the packages installed; `make fast-check`, `make docs-check` green.
- A real CI run of the six previously-hanging jobs completes well under the timeout.
