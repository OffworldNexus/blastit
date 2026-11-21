---
id: MB.42.13
title: "build-wheel.sh: parse manylinux_<major>_<minor> from the platform (maturin emits manylinux_2_39_x86_64, floor parse fails)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.3.3]
paths: ["scripts/build-wheel.sh", scripts/test_packaging_gates.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T05:25"
updated: "2026-11-20T05:37"
---

# Goal
`scripts/build-wheel.sh` derives the wheel's glibc floor from the maturin platform string with `${wheel_platform#manylinux_}`. For a `bindings = "bin"` wheel maturin 1.16 emits `manylinux_2_39_x86_64`, so the strip leaves **`2_39_x86_64`** — not a `<major>_<minor>` — and the floor parser refuses:

```
📦 Built wheel to …/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
error: --max-glibc '2_39_x86_64' is not <major>_<minor> (e.g. 2_28) or auto
error: libblastit_py313-x86_64-gnu.so outgrew the wheel's glibc floor (2_39_x86_64)
make: *** [Makefile:366: docs-test] Error 1
```

That kills `make docs-test`, `make check`, and every CI host wheel leg — found while running `MB.42.4`'s gate.

# Exact Deliverables
1. Parse the platform instead of stripping a prefix: take `manylinux_<major>_<minor>` (the first two numeric components) and ignore any trailing `_<arch>` / `_<arch>_<libc>` suffix. Keep `--manylinux`/`BLASTIT_WHEEL_MAX_GLIBC` still winning when given.
2. Make the derivation **fail loudly** on a platform it cannot parse, rather than passing a malformed string to `--max-glibc` (the current message blames the shim, not the parse).
3. Apply the same derivation at both call sites (`check_blob_floors` ~`:487` and the invocation ~`:568-569`) — one helper, no second copy.
4. A test in `scripts/test_packaging_gates.py` (or the wheel-tag cases) that feeds `manylinux_2_39_x86_64`, `manylinux_2_28_aarch64`, `musllinux_1_2_x86_64` and an unparseable string, asserting the derived floor and the loud failure.

# Notes
- Do not weaken the floor: the whole point is that a shim measured at 2.34 is *checked* against `manylinux_2_28` (2.28) — if it exceeds, the build must still refuse.
- Do not change `repack_universal_wheel.py`'s tag logic; it is correct (the tag stays `manylinux_2_39_x86_64` when the host demands it — the *floor* is what must be parsed).
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `scripts/build-wheel.sh --manylinux 2_28 --require-shims all` completes and the floor check passes
- the four platform strings above produce the right derived floor / a loud failure (paste)
- `make docs-test` and `make check` no longer die in `build-wheel.sh`
- `make fast-check`
