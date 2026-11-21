---
id: MB.62
title: "Bounded spaced apt retries for sury's published-index race in php-sapi image"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/php-sapi/Dockerfile"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T14:27"
updated: "2026-11-21T14:48"
---

# Goal

`MB.60`'s `--progress=plain` + 120-line tail already paid off: the two `aarch64-gnu` PHP 8.6 legs now fail with a *readable* cause, and it is a **second, different** variant of the same upstream race. On run 44908280224 (2026-11-21T13:22Z), against `MB.60`'s shipped fallback:

```
E: Failed to fetch .../main/binary-arm64/Packages.xz  File has unexpected size (292780 != 292160). Mirror sync in progress?
+ echo apt-get update failed; retrying with the .gz indexes (a second failure means the repository is broken or unreachable)
+ apt-get -o Acquire::CompressionTypes::Order::=gz update
E: Failed to fetch .../main/binary-arm64/Packages.gz  File has unexpected size (520904 != 520306). Mirror sync in progress?
E: Some index files failed to download...
```

So the mirror is publishing a **new index revision**: the `.xz`, the `.gz` and the uncompressed `Packages` are all newer than the signed `InRelease`, and Fastly serves the mismatch from different edges (`140.248.147.52`, `151.101.163.52`). Preferring another compression cannot fix "every variant is newer than the signature"; only a **bounded, spaced retry** can, because the publisher rotates `InRelease` seconds-to-minutes later.

## Change (one site only: the post-`EXTRA_REPO` `apt-get update` in `e2e/php-sapi/Dockerfile`)

Replace `MB.60`'s single gz fallback with a bounded escalation that keeps every property `MB.60` established:

```sh
if ! apt-get update; then \
    echo "apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant" >&2; \
    sleep 5; \
    if ! apt-get -o Acquire::CompressionTypes::Order::=gz update; then \
        echo "apt-get update failed again: still publishing; retrying plainly after a longer pause" >&2; \
        sleep 15; \
        apt-get update; \
    fi; \
fi; \
```

- Attempt 1 plain, attempt 2 prefers `.gz` (`MB.60`'s case: `.xz` stale, `.gz` current), attempt 3 plainly after 15s (this incident's case: all variants ahead of `InRelease`).
- The final attempt is the last command of the `then` branch, so under `set -e` a third failure still fails the build with a non-zero exit. Total added worst-case wait: 20s.
- NO global `Acquire::CompressionTypes::Order`, NO change to the Debian-only first update, NO change to the Alpine branch, and `apt-get install` still resolves the very same package set.
- Update the layer's comment: keep the `MB.60` measurement and add this second one (the published-revision case with both variant sizes), and say a third failure means the repository is genuinely broken or unreachable rather than mid-publish.

## Do not touch

- `e2e/php_sapi_probe.py` — `--progress=plain` and the `keep = 120` tail are correct as shipped; this node must not change them.
- Nothing else in the repo.

# Acceptance

- `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1` green on this host (normal path + shell syntax).
- A shell-level proof in `RESULT.md` that the escalation is correct, e.g. in a `debian:bookworm-slim` container: a stub `apt-get` on `PATH` that fails on call 1 and 2 and succeeds on call 3, showing the retry order (plain → `-o Acquire::CompressionTypes::Order::=gz` → plain), the sleeps, and that a always-failing stub makes the layer exit non-zero.
- Container-local arm64-index reproduction of the CURRENT upstream state pasted in `RESULT.md` (`dpkg --add-architecture arm64` + `[arch=arm64]` sury source): show what `apt-get update` reports *today*, so the evidence is not a stale copy of `MB.60`'s numbers.
- `uvx --offline ruff check` / `format --check` on `e2e/` unchanged-green, `uv run --no-project tools/guard.py` clean, `uv run --no-project tools/plan.py validate` OK.
