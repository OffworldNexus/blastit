# RESULT MB.3.64 — a deterministic seam for the shim-swap boot test

## Summary

`critical_php_embed::a_shim_replaced_between_the_stages_refuses_the_boot` no longer races the
scheduler. Stage 1 now **pauses** between stage 0's inode record and its own re-open until the test
says go, so the rename is inside the window because both sides say so. The 4 × 20 s retry loop, the
"there is no deterministic seam without a `src/` change" comment and the "a removed pin produces the
same served attempts" panic are all gone; every assertion the leg made is still made, and the leg is
now **red when the shim's inode comparison is removed** — proved below by mutation, not asserted.

**How the seam is gated (the mechanism, not a promise).** `BLASTIT_TEST_SWAP_POINT` is read in
`stage0::config_document` under `#[cfg(debug_assertions)]` and written into the configuration
document as `test_swap_point` — the same barrier-crossing route `php_probe_timeout_ms` takes, because
the stage 0→1 `execve` is handed an environment from which `blastit-env` has stripped every
`BLASTIT_*` variable. `stage1::read_config` reads the member back, and `stage1::test_swap_pause`
calls `php_view::await_test_swap` immediately before `boot_bindings` opens the pair. The two
constants and the reader are `#[cfg(debug_assertions)]`; the wait itself is
`#[cfg(all(debug_assertions, feature = "php"))]` — every `cargo test` build — so none of them exists
in a release build, and `test_swap_pause` has a second arm (`#[cfg(not(all(...)))]`) whose whole body
is empty: a release build has no dead parameter and no branch either.

**Why it is inert in production.** Three independent things, one of them measured:

1. With the variable unset — every boot outside the two tests — `config_document` writes no member,
   `read_config` yields `None`, and `test_swap_pause` returns on its first `if let`. No new pause, no
   new timing dependency, no new failure mode (all 24 other legs of the suite and the whole
   workspace boot through that path).
2. The value never enters an environment at all: stage 0 `execve`s stage 1 with the filtered app
   environment (`blastit-env` has stripped every `BLASTIT_*` variable) and hands the point over in
   the configuration pipe, which stage 1 closes after reading. The roles stage 1 forks inherit an
   environment that never held the name, so no route process can read it.
3. `#[cfg(debug_assertions)]` is now **measured on the artifact**: `scripts/check-no-test-seam.sh`
   byte-searches a real file for the name in both directions, and the shipped direction is
   unconditional in `scripts/build-standalone.sh` (after `check-portable-elf.sh`, before packaging).
   So the claim is *"the shipped artifact contains no such name, and this check fails if one
   appears"* — not *"the seam cannot run in production"*.

## Evidence

| # | Item | Verdict | Command and output tail |
|---|---|---|---|
| 1 | Whole suite green | PASS | `cargo test -p blastit --locked --test critical_php_embed` → `test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.91s` |
| 2 | 20/20 consecutive passes of the one leg | PASS | `for i in $(seq 1 20); do cargo test -p blastit --locked --test critical_php_embed -- --exact a_shim_replaced_between_the_stages_refuses_the_boot …` → tail:<br>`run 18: pass=18 fail=0`<br>`run 19: pass=19 fail=0`<br>`run 20: pass=20 fail=0` |
| 3a | Red with the shim's pin removed | PASS (red as required) | mutated `php_view.rs:393` to `if what != "shim" && (stat.st_dev != dev \|\| stat.st_ino != ino) {` → `cargo test … --exact a_shim_replaced_between_the_stages_refuses_the_boot` → `FAILED` with<br>`the boot served the route even though the seam paused it between stage 0's record and stage 1's re-open: the rename landed inside the window by construction, so this is **not** a lost race. A boot that serves here means stage 1 no longer compares the re-opened **shim**'s (st_dev, st_ino) against stage 0's record …` and, in the same output, `"event":"ready"` (the route **served**) |
| 3b | The library leg cannot cover it (the shim leg owns its mutation) | PASS | same mutation, `--exact a_libphp_replaced_between_the_stages_refuses_the_boot` → `1 passed; 0 failed` |
| 3c | File restored byte-identical, green again | PASS | `sha256sum -c`: `crates/blastit/src/php_view.rs: OK` (before = after = `80efbf01462e1bdd1f08b468bec69ce95eb3c4cb23c065b2b752fed310770ad8`), then the 25-test suite green again (item 1) |
| 4 | `make rs-test-php-image` | PASS | image was **stale** (built before this change), so I rebuilt it first: `make php-sapi-e2e PHP_SAPI_KEEP=1` → `EXIT:0` (new image `blastit-php-sapi-e2e:local` = `d72f8044d7e0`, probe legs ok). Then `make rs-test-php-image` → root run `test result: ok. 25 passed; 0 failed`, unprivileged witness `rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000`. Both new legs ran inside the image: `refused (stage 1 inode pin): …` and `refused (stage 1 shim inode pin): …` |
| 5 | `make check` | PASS | `make check > /tmp/opencode/make-check-final.log 2>&1; rc=$?` → `make check rc=0`; tail `packaging-gates-test: ok — 43 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=7, test-seam=6)` / `plan OK`. Inside it: `cargo test -p blastit --features python --test critical_php_embed` → `25 passed`, and `scripts/check-no-test-seam.sh --present …/target/debug/blastit` → `ok … present (2 occurrence(s))`, `ok …/target/release/blastit: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)` |
| 6 | Protected edit authorized | PASS | `uv run --no-project tools/critical_guard.py --base fedd78e --node MB.3.64` → `critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.64` |
| 7 | Absence measurable, not asserted (owner addition) | PASS | `scripts/check-no-test-seam.sh target/release/blastit` → `ok … no BLASTIT_TEST_SWAP_POINT (absent, as shipped)`; `--present target/debug/blastit` → `ok … present (2 occurrence(s))`; `--present target/release/blastit` → exit 1 (`… does not contain BLASTIT_TEST_SWAP_POINT, so the seam is not in a debug build either`); `target/debug/blastit` with `--absent` → exit 1 (`denial-of-service`); missing file & no argument → exit 2. Fixture: `test_test_seam_cases()` (6 cases) in `scripts/test_packaging_gates.py` builds four synthetic binaries and proves both directions **go red**; wired into `make check` via `packaging-gates-test` and `seam-check`. |

Scan method: `grep -a -o -F` over the raw file, not `strings` — the name is one contiguous ASCII
literal in `.rodata`, so a byte search cannot miss it, and `strings` would add a minimum-run-length
rule and a binutils dependency inside the `manylinux` builder images. Release binary measured:
`cargo build --release --locked -p blastit --bin blastit --no-default-features --features php` (the
`build-standalone.sh` shape) → 0 occurrences; debug → 2.

**aarch64 legs: CI-only, not proved here.** Nothing above runs on, builds for, or exercises an
`aarch64` target; both legs remain declared-and-unproved in `scripts/platform_matrix.py` until a
release PR runs them on `ubuntu-24.04-arm`.

## Files

- `crates/blastit/src/stage0.rs` — `TEST_SWAP_POINT_VAR`/`TEST_SWAP_POINT_MEMBER` (debug builds
  only) and the member folded into `config_document` beside `php_probe_timeout_ms`.
- `crates/blastit/src/stage1.rs` — `read_config` returns the swap point; `test_swap_pause` (real arm
  / empty arm) is called once, immediately before `php_view::boot_bindings`.
- `crates/blastit/src/php_view.rs` — `await_test_swap` + `swap_sidecar` + `TEST_SWAP_TIMEOUT`
  (`debug_assertions` + `php` only): create `<point>.ready`, block until `<point>.go`, refuse with a
  sentence naming the variable after 30 s.
- `crates/blastit/tests/critical_php_embed.rs` — both inode-pin legs (library and shim) rewritten to
  the rendezvous; retry loops and the non-determinism comments deleted; every refusal assertion kept
  (shim named, `restore the package and restart blastit`, exit 1, no `ready`/`spawned`/`request`, no
  listener, no stub loaded).
- `scripts/check-no-test-seam.sh` (new) — the two-direction byte search on a real file.
- `scripts/build-standalone.sh` — runs it on the binary each archive ships.
- `scripts/test_packaging_gates.py` — `test_test_seam_cases()`: 6 self-tests, both directions
  proved to go red.
- `Makefile` — `seam-check` (debug `--present`, plus the release direction when this checkout has a
  release artifact) added to `check`, right after `rs-no-python` builds `target/debug/blastit`.

## Follow-ups

1. `make check`'s `critical-guard` step runs without `--node`, so it reports
   `[VIOLATION: not covered by any node path]` for the working-tree edit to
   `critical_php_embed.rs` (exit 0 by design — CI passes the changed task nodes). The authored gate
   for this node is item 6 above.
2. The wheel (`_blastit*.so`, maturin release build) has the same `debug_assertions` gate but no
   artifact check yet: `scripts/build-wheel.sh` could call `scripts/check-no-test-seam.sh` on the
   built extension the way `build-standalone.sh` does. Out of this node's paths.
3. Pre-existing, untouched: `cargo check -p blastit --no-default-features --features python` warns
   `unused variable: index` at `crates/blastit/src/supervise.rs:582`. That configuration is not in
   `make check` (`rs-lint` enables `python` on top of the default `php`), so it is not a regression
   here, but it will fail a future `-D warnings` run on that combination.
4. `BLASTIT_TEST_SWAP_POINT` pauses any debug boot with a `[route.php]`-less manifest for 30 s before
   refusing (the wait is not conditional on a PHP route). It is a test-only variable with a sentence
   that names it and says to unset it; narrowing it to boots that actually open a pair would be a
   small improvement if the variable ever grows a second user.
