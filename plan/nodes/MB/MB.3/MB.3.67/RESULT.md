# MB.3.67 — no verdict without the evidence to support it

## Summary

Four "confident verdict from no evidence" shapes, all closed in `scripts/**`:

1. **Unreadable artifact read as "absent" (rc 0).** `scripts/check-no-test-seam.sh` folded
   `grep`'s exit 2 into `wc`'s `0` hits behind `|| true`, so `--absent` printed `ok … absent`
   and exited 0 about a file nothing had read. Now: the artifact must be `-r`, and a `grep`
   status above 1 is exit 2 ("unmeasured"), never the zero-hit answer. Both directions.
2. **The wheel is measured.** `scripts/build-wheel.sh` runs the same check on every
   `blastit/_blastit*.so` it extracts from the built wheel, and `scripts/wheel-smoke.sh` runs
   it on the **installed** extension — which is the leg the published wheels go through
   (maturin-action builds those, not `build-wheel.sh`), so the released wheel is no longer the
   one artifact family nothing reads. The seam itself (`crates/blastit/src/**`),
   `critical_php_embed.rs`, `MB.3.63`'s `INTERP`/`NEEDED` contradiction rules and `MB.3.66`'s
   glibc floor semantics are unchanged.
3. **Unclassifiable ELF read as "too old".** In `scripts/check-exec-env.sh` an ELF whose own
   headers name no C library (no `INTERP`, no `NEEDED libc.*` — the static-pie shape) still had
   its `GLIBC_ABI_DT_RELR` name read as a 2.36 floor and was refused by `alpine:3.21` with
   "a glibc ELF cannot start where there is no glibc" printed beside "interpreter none". The
   floor comparison is now gated on glibc evidence (`interp_glibc` or `needs_glibc`); neither is
   exit **2** ("cannot decide"), not exit 1.
4. **The 2.36 acceptance is pinned.** `scripts/test_packaging_gates.py` now also runs the
   `GLIBC_ABI_DT_RELR` glibc ELF against a 2.36 image (exit 0), so an "always exit 1" regression
   fails instead of passing.

## Evidence

| # | Acceptance item | Command | Result (tail) |
|---|---|---|---|
| 1a | `chmod 000` **before** (`f1f933a`'s script) | `printf 'x\0BLASTIT_TEST_SWAP_POINT=y\0' > sealed-release.so; chmod 000 sealed-release.so; git show f1f933a:scripts/check-no-test-seam.sh > old.sh; old.sh --absent sealed-release.so` | `grep: sealed-release.so: Permission denied` / `ok   sealed-release.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` / **`rc=0`** — a pass about a file nothing read |
| 1b | `chmod 000` **after** | `scripts/check-no-test-seam.sh --absent sealed-release.so` | `error: cannot read sealed-release.so, so whether it carries BLASTIT_TEST_SWAP_POINT is unknown:` / `  'absent, as shipped' is a verdict about a file this check read, and this one it` / `  could not. Make it readable (chmod +r sealed-release.so) and re-run this check.` / **`rc=2`** |
| 1c | same file, debug direction | `scripts/check-no-test-seam.sh --present sealed-release.so` | same `cannot read …` sentence naming the path, **`rc=2`** (not 1 "the seam is gone") |
| 2 | the four real directions | `scripts/check-no-test-seam.sh [--absent\|--present] <artifact>` | `ok … /tmp/…/_blastit.cpython-314-x86_64-linux-gnu.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` rc 0 (wheel extension); `ok … target/release/blastit: no …` rc 0; `ok … target/debug/blastit: BLASTIT_TEST_SWAP_POINT present (2 occurrence(s)), as a debug build must` rc 0; wrong ways: `--present target/release/blastit` → `… does not contain BLASTIT_TEST_SWAP_POINT …` rc 1, `--absent target/debug/blastit` → `… contains BLASTIT_TEST_SWAP_POINT (2 occurrence(s)) … denial-of-service` rc 1 |
| 3a | static-pie shape **before** (`f1f933a`'s script) on real `alpine:3.21` | `gcc -shared -fPIC -nostdlib -o static-pie-shape.so x.c -Wl,--version-script=map.map` (`map.map`: `GLIBC_ABI_DT_RELR { global: foo; local: *; };`) then `old.sh --image alpine:3.21 --binary static-pie-shape.so` | `check-exec-env: FAIL — static-pie-shape.so needs glibc 2.36, and alpine:3.21 provides musl` / `  a glibc ELF cannot start where there is no glibc: the loader is missing, not older` / `  the binary's own ELF: machine Advanced Micro Devices X86-64, interpreter none, NEEDED:` / **`rc=1`** — the contradiction, from a name nothing attributed to glibc |
| 3b | same file **after** | `scripts/check-exec-env.sh --image alpine:3.21 --binary static-pie-shape.so` | `check-exec-env: FAIL — cannot decide which C library static-pie-shape.so needs, so no image can be judged:` / `  its own headers name none: machine Advanced Micro Devices X86-64, interpreter none, NEEDED:` / `  a GLIBC_* or GLIBC_ABI_* name in .gnu.version_r is only a floor for a file whose` / `  INTERP or NEEDED says glibc; …` / **`rc=2`** |
| 3c | a genuine `gcc -static-pie` binary (same shape, no names at all) | `gcc -static-pie -o real-static-pie m.c` (`file`: `static-pie linked`, `(no INTERP)`, `(no NEEDED)`, `No version information found`) then before/after on `alpine:3.21` | before: `ok — real-static-pie requires no glibc symbol version (musl or static) …` rc 0; after: `cannot decide which C library real-static-pie needs …` **rc=2** (see Follow-ups) |
| 4 | real glibc ELF still refused below its floor | `gcc -Wl,-z,pack-relative-relocs -o glibc-dtrelr m.c` (needs `GLIBC_ABI_DT_RELR` + `GLIBC_2.34`, floor 2.36) | `--image debian:bullseye-slim` → `FAIL — glibc-dtrelr needs glibc 2.36, debian:bullseye-slim has 2.31` / `version 'GLIBC_ABI_DT_RELR' not found` **rc=1**; `--image debian:bookworm-slim` → `ok — glibc-dtrelr needs glibc 2.36, debian:bookworm-slim provides 2.36` **rc=0** |
| 5 | suite green + both new cases **red against `f1f933a`** | `uv run --no-project scripts/test_packaging_gates.py` | `packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)` (was 46: exec-env 10→12, test-seam 6→7) |
| 5a | new seam case vs `f1f933a`'s script | same test file, `REPO/scripts` pointed at `f1f933a`'s `check-no-test-seam.sh` | `AssertionError: an unreadable artifact must be exit 2 (no verdict), never 'absent' with 0; got exit 0` + the old `ok … absent` line — red |
| 5b | new exec-env case vs `f1f933a`'s script | same, `check-exec-env.sh` from `f1f933a` | `AssertionError: an ELF with no INTERP and no NEEDED libc.* cannot be classified, so it must be exit 2 (cannot decide), never 1 (too old); got 1` + `a glibc ELF cannot start where there is no glibc` — red |
| 5c | item 4 is not vacuous | script mutated to `if true; then` (always "too old" when a floor exists), rest unchanged | `AssertionError: the same glibc ELF needing 2.36 must pass in a 2.36 image, or the refusal above is a constant; got 1` — the new accept case is the one that fires |
| 6a | the wheel edge, local build | `scripts/build-wheel.sh -o /tmp/.../wheel2` | `📦 Built wheel for CPython 3.14 to …manylinux_2_39_x86_64.whl` / `wheel-tag: … Tag: cp314-cp314-manylinux_2_39_x86_64` / `wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so` / `ok …: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` rc 0 |
| 6b | the released-wheel leg (the CI path) | `docker run … python:3.14-slim bash /src/scripts/wheel-smoke.sh /dist/blastit-…whl` | `wheel-smoke: blastit --help ok` / `ok … site-packages/blastit/_blastit.cpython-314-x86_64-linux-gnu.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` / `wheel-smoke: 3.14 wheel ok` rc 0 |
| 6c | …and its denied path | same wheel with `target/debug/lib_blastit.so` substituted for the extension (a `--release` wheel carrying the seam) | `error: …site-packages/blastit/_blastit.cpython-314-x86_64-linux-gnu.so contains BLASTIT_TEST_SWAP_POINT (2 occurrence(s)), so a shipped boot could be` / `  paused by an environment variable before it binds a listener — a denial-of-service` **rc=1** |
| 6d | the script runs in the musl legs' toolchain | `docker run … alpine:3.21 sh -c 'apk add bash; bash /src/scripts/check-no-test-seam.sh /w/_blastit*.so'` | `ok … no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` rc 0 (busybox `grep -a -o -F`, `wc`, `tr`) |
| 7 | full gate (re-run on the final tree) | `make check` | `packaging-gates-test: ok — 49 cases passed (…)` / `doc-tone: ok — 28 user-facing page(s), no banned phrase` / `plan OK` / **`make-check rc=0`**; inside it `docs-test`'s real wheel: `wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so` → `ok … no BLASTIT_TEST_SWAP_POINT (absent, as shipped)`, and `seam-check`: `ok … target/debug/blastit: … present (2 occurrence(s))` + `ok … target/release/blastit: no …` |
| 8 | scope | `git status --short` | only `scripts/…` from me: `M scripts/build-wheel.sh`, `M scripts/check-exec-env.sh`, `M scripts/check-no-test-seam.sh`, `M scripts/test_packaging_gates.py`, `M scripts/wheel-smoke.sh` (+ this untracked node folder). The `plan/*`, `docs/blastfile*`, `python/tests/runnable_docs/inventory.toml` and `zensical.toml` entries are **pre-existing / another agent's concurrent work in this worktree** — untouched by this node. |

## Files

- `scripts/check-no-test-seam.sh` — `-r` guard, `grep` status > 1 → exit 2, header updated.
- `scripts/check-exec-env.sh` — no-libc-evidence → exit 2 before any floor is compared; header/exit codes updated.
- `scripts/build-wheel.sh` — seam check on every extracted `blastit/_blastit*.so`.
- `scripts/wheel-smoke.sh` — seam check on the installed `_blastit*.so` (the published-wheel leg).
- `scripts/test_packaging_gates.py` — unreadable-artifact case (red against `f1f933a`), no-C-library case (red against `f1f933a`), 2.36 acceptance case (red against an "always too old" mutant).

## Follow-ups

- **`blastit-dbproxy` is still not seam-measured.** It is a release ELF the wheel ships, and the
  one-line call belongs beside the extension's — but `python/tests/test_release_scripts.py`
  (outside `scripts/**`) copies only `build-wheel.sh` and `readme-absolute-links.py` into its
  stub tree, so *any* unconditional sibling-script call there is `No such file`. Copying
  `scripts/check-no-test-seam.sh` into that fixture is the whole fix; then the call can move out
  of the wheel-extraction block and cover dbproxy too.
- **A static ELF now exits 2** ("cannot decide"), where it used to exit 0 ("requires no glibc
  symbol version — musl or static"): measured on a real `gcc -static-pie` binary. That is the
  owner's rule (neither `INTERP` nor `NEEDED` ⇒ no verdict) and no shipped artifact is static
  (both standalone legs are dynamic by construction), but if a static artifact is ever shipped
  it needs an explicit "names no libc ⇒ needs nothing" rule rather than this conservative 2.
- The `grep` status > 1 branch is unreachable without a race once the `-r` guard is in place
  (a file `access(2)` calls readable is one `grep` can open). Kept as defence in depth rather
  than deleted; a future reader should not assume it is covered by a test.
- **LESSONS candidate (out of scope for `scripts/**`):** `hits=$(grep … | wc -l)` under
  `set -euo pipefail` turns `grep`'s "cannot read" (2) into a clean `0`; only the *status*, kept
  with `|| status=$?`, distinguishes "no match" from "no reading".
- Both **aarch64 legs are CI-only**: nothing here proves one. `alpine:3.21`/`debian` images used
  above are x86_64; the wheel-smoke change was only exercised on the gnu wheel, plus a
  busybox/bashed `check-no-test-seam.sh` run in `alpine:3.21` for the toolchain.
