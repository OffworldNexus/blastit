# Log MB.3.61

## 2026-11-15T19:36 [lead]

created (task) Discovery must prefer a library that can serve, and the php_version sentence must not lie

## 2026-11-15T20:24 [lead]

edited fields

## 2026-11-15T20:24 [lead]

FOLDED IN from MB.3.60's review (same spirit — a guard that reports the wrong reason), both now in this node's paths:
(a) `scripts/check-exec-env.sh:127` — a non-ELF input aborts under `set -euo pipefail` with NO message and exit 1 ("too old") instead of exit 2 ("cannot decide"): `--binary README.md` → silent exit 1, a false verdict of the wrong kind. Wrap both `readelf` substitutions (`required`, `abi_nodes`) so a parse failure prints "cannot read symbol versions" and exits 2.
(b) `scripts/check_workflows.py:129` — rule 15 matches only the literal `scripts/build-standalone.sh`, so a shipping step running `make standalone` (the host build) would pass the audit. Treat `make standalone` (but not `standalone-glibc`) in `SHIPPING_WORKFLOWS` as a host build, with a self-test.
Also `docs/knowledge/LESSONS.md` is available to you if a genuine one-line gotcha comes out of this node (it is not in `paths`; the lead accepts the precedent set by MB.3.58/.60 — say in RESULT what you added and why).

## 2026-11-15T20:24 [lead]

status ready -> active

## 2026-11-15T20:24 [lead]

attempt 1/3 started

## 2026-11-15T20:24 [lead]

base 490bb9b

## 2026-11-15T20:50 [lead]

status active -> review

## 2026-11-15T20:55 [lead]

status review -> done
