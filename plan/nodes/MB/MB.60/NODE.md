---
id: MB.60
title: "Survive sury's stale arm64 Packages.xz in php-sapi Dockerfile and surface build errors"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/php-sapi/Dockerfile", e2e/php_sapi_probe.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T12:00"
updated: "2026-11-21T12:34"
---

# Goal

The last 2 red checks on PR #64 (`dffa4b5`) are both the **`aarch64-gnu` PHP 8.6** leg (`§2 PHP 8.6 (aarch64-gnu)` in `.github/workflows/php-sapi.yml`, and `shim (libblastit_php86-aarch64-gnu.so)` in `Publish`). Both die in ~10s inside `make php-sapi-e2e` with `docker build ... exit code: 100` and **no apt output at all**.

Root cause (measured, not guessed): `https://packages.sury.org/php/dists/bookworm/InRelease` (created `2026-11-21T10:18:11Z`) advertises `main/binary-arm64/Packages.xz` as 291892 bytes / SHA256 `9eda492b…d165`, but the mirror actually serves **290956** bytes. apt prefers `.xz`, so on arm64:

```
E: Failed to fetch https://packages.sury.org/php/dists/bookworm/main/binary-arm64/Packages.xz
   File has unexpected size (290956 != 291892). Mirror sync in progress?
```

The `.gz` index for the same suite hashes correctly. amd64's `.xz` is also correct, which is why every amd64 leg passes.

## 1. `e2e/php-sapi/Dockerfile`: survive a stale index variant (this is the fix)

In the `EXTRA_REPO=sury` path, the second `apt-get update` (the one that first sees the sury suite) must not fail the whole leg on an upstream publish race:

```sh
if ! apt-get update; then \
    echo "apt-get update failed; retrying with the gz indexes" >&2; \
    apt-get -o Acquire::CompressionTypes::Order::=gz update; \
fi; \
```

- Comment it: name the exact upstream defect (fresh `InRelease`, stale arm64 `Packages.xz`, correct `.gz`) and say the retry prefers `.gz` instead of letting a mirror sync race block a leg that installs fine.
- The retry must NOT weaken anything: if the `.gz` index is also bad the leg still fails, and `apt-get install` still has to resolve the very same package set.
- Do not add `Acquire::CompressionTypes::Order::=gz` globally (that would silence real mismatches for every index); only the failing update gets the fallback.
- Keep the first plain `apt-get update` (Debian-only, before sury is added) as it is.

## 2. `e2e/php_sapi_probe.py`: stop hiding the build error

`build()` runs `docker build … capture_output=True` and reports `tail(result.stdout + result.stderr)` at 40 lines — but BuildKit's failure summary is itself ~38 lines of the failing `RUN` echoed with `>>>`, so the real apt line (`E: Failed to fetch …`) never reaches the log. That is why this took two CI runs to diagnose.

- Pass `--progress=plain` to that `docker build` so the `RUN` output is captured.
- On a non-zero exit, report a longer tail for the build specifically (e.g. `tail(..., lines=120)`) so both the apt error and the Dockerfile echo survive.
- Change nothing else in the driver.

# Acceptance

- `docker build` of `e2e/php-sapi/Dockerfile` for `PHP_MINOR=8.6`/gnu on this host still succeeds (`make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1`), proving the normal path and the shell syntax are untouched.
- Container-local reproduction of the arm64 failure, pasted into `RESULT.md`: inside `debian:bookworm-slim` add sury with `dpkg --add-architecture arm64` and `[arch=arm64]`, then show (a) plain `apt-get update` failing with `File has unexpected size (290956 != 291892)` and (b) the Dockerfile's fallback form succeeding.
- `make fast-check` green (ruff/format for `e2e/`), `uv run --no-project tools/guard.py` clean.
