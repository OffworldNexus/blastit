# RESULT — MB.42.13

**STATUS: done**

`scripts/build-wheel.sh` now *parses* the wheel's glibc floor out of the platform
tag instead of stripping the `manylinux_` prefix, so a maturin 1.16
`bindings = "bin"` wheel tagged `manylinux_2_39_x86_64` yields `2_39` at both
call sites. `make docs-test` and `make check` no longer die in `build-wheel.sh`.

## Changes

`scripts/build-wheel.sh`
- New `manylinux_floor <platform>` helper (one copy, above the argument loop):
  keeps the tag's first two numeric components and ignores a trailing `_<arch>`
  or `_<arch>_<libc>` (`manylinux_2_39_x86_64` → `2_39`,
  `manylinux_2_39_x86_64_glibc_2_39` → `2_39`). `--manylinux` wins, then
  `BLASTIT_WHEEL_MAX_GLIBC` (the header already promised it; the script now
  honours it).
- An unparseable platform is an error **naming the platform**
  (`cannot derive a glibc floor from platform 'manylinux_x86_64' (want
  manylinux_<major>_<minor>_<arch>); pass --manylinux 2_28`), and a
  `musllinux_*` tag says it names a musl version, not a glibc floor — instead of
  handing a malformed floor to `--max-glibc`, whose refusal then blamed the shim
  that had just been measured.
- The floor is derived once (`wheel_floor=$(manylinux_floor "$wheel_platform")`)
  and used by all three consumers: `check_blob_floors`, `check_wheel_tag.py
  --max-glibc/--platform`, and the per-member `check-portable-elf.sh
  --max-glibc`. No second copy of the derivation.
- `--print-glibc-floor <platform>` prints the derived floor and exits before any
  blob staging, README rewrite or build: the hook the self-test runs, so the
  test exercises the shipped parser rather than a paraphrase.
- Header docs: the stale `--max-glibc ${BLASTIT_WHEEL_MAX_GLIBC:-2_41}` claim for
  the host path (the host wheel runs `--allow-host-tag` and claims no floor) is
  replaced by what the script does, plus the parse rule.

`scripts/test_packaging_gates.py`
- `test_wheel_floor_cases()` (9 cases, registered in `main()` and the summary
  line as `wheel-floor=`), driving `build-wheel.sh --print-glibc-floor` with
  `BLASTIT_MANYLINUX`/`BLASTIT_WHEEL_MAX_GLIBC` stripped from the environment:
  `manylinux_2_39_x86_64`→`2_39`, `manylinux_2_28_aarch64`→`2_28`,
  `manylinux_2_39_x86_64_glibc_2_39`→`2_39`, `manylinux_2_17_aarch64_gnu`→`2_17`;
  `musllinux_1_2_x86_64`, `manylinux_x86_64`, `not-a-platform` → exit 1, naming
  the platform, and no `outgrew` (the parse is blamed, not a shim);
  `--manylinux 2_28` and `BLASTIT_WHEEL_MAX_GLIBC=2_17` still win.

The floor is not weakened: `test_portable_elf_glibc_cases` still refuses a strong
`GLIBC_2.39` need at `--max-glibc 2_28`, and the manylinux wheel below measured
every embedded ELF (2.16/2.17) against the `2_28` floor.

## Evidence

### The four platform strings (and an override)

```
$ for p in manylinux_2_39_x86_64 manylinux_2_28_aarch64 manylinux_2_17_aarch64_gnu musllinux_1_2_x86_64 manylinux_x86_64; do printf '%-34s -> ' "$p"; out=$(scripts/build-wheel.sh --print-glibc-floor "$p" 2>&1); echo "exit=$? ${out}"; done
manylinux_2_39_x86_64              -> exit=0 2_39
manylinux_2_28_aarch64             -> exit=0 2_28
manylinux_2_17_aarch64_gnu         -> exit=0 2_17
musllinux_1_2_x86_64               -> exit=1 error: platform 'musllinux_1_2_x86_64' is musllinux: it names a musl version, not a glibc floor
manylinux_x86_64                   -> exit=1 error: cannot derive a glibc floor from platform 'manylinux_x86_64' (want manylinux_<major>_<minor>_<arch>); pass --manylinux 2_28
$ scripts/build-wheel.sh --manylinux 2_28 --print-glibc-floor manylinux_2_39_x86_64
2_28
$ BLASTIT_WHEEL_MAX_GLIBC=2_17 scripts/build-wheel.sh --print-glibc-floor manylinux_2_39_x86_64
2_17
```

### The self-test

```
$ UV_OFFLINE=1 uv run --no-project python -c "import sys; sys.path.insert(0,'scripts'); import test_packaging_gates as t; print('wheel-floor cases:', t.test_wheel_floor_cases())"
wheel-floor cases: 9
```

### Release leg: `--manylinux 2_28` completes, floor check passes

```
$ scripts/build-wheel.sh --manylinux 2_28 --require-shims py
wheel: blastit/_bin/blastit
ok   …/_bin0: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
max-glibc: 2.17 (…/_bin0)
needed: ld-linux-x86-64.so.2 libc.so.6 libdl.so.2 libgcc_s.so.1 libm.so.6 libpthread.so.0
ok   …/_bin0: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
…
wheel: blastit/_bin/libblastit_py314-x86_64-gnu.so
max-glibc: 2.17 (…/_bin4)
ok   …/_bin4: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
…
$ ls -1 dist
blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl
```

Every embedded ELF (built inside the `manylinux_2_28` image) was measured at
2.16/2.17 against the `2_28` floor and the repack completed with that tag.

`--require-shims all` cannot complete **in this worktree** for a reason that
predates this node: the four PHP shims come from a distro leg and no
`target/php-sapi/*.so` exists here (only `target/proven-shims/libblastit_php86-…`).
With `all` the run reaches the count assert *after* the floor check has passed:

```
$ scripts/build-wheel.sh --manylinux 2_28 --require-shims all
📦 Built wheel to /wheel-out/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl
error: --require-shims all needs 2 CPython + 4 PHP shims, …/target/wheel-blobs holds 2 + 0
  PHP shims come from a distro leg (`make php-sapi`, or --shims-from a php-sapi artifact); a manylinux/CI build cannot compile them
```

### `make docs-test` (the leg the bug killed)

```
📦 Built wheel to …/target/docs-wheel/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
wheel-tag: blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: Tag: py3-none-manylinux_2_39_x86_64
PASS  docs/getting-started.md  fence 2  install   0.5s  installed blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl and verified it against the built wheel
…
60 fences: 32 executed, 28 accounted, 0 failed · 32.1s
```

### `make check`

`build-wheel.sh` is no longer a failure point: inside `make check` the docs wheel
builds, the floor is checked, and all 60 fences pass before the gate moves on —

```
$ grep -n "Built wheel to\|Tag: \|outgrew\|fences: " /tmp/opencode/check3.log
4133:📦 Built wheel to …/target/docs-wheel/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
4138:wheel-tag: blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl: Tag: py3-none-manylinux_2_39_x86_64
4244:60 fences: 32 executed, 28 accounted, 0 failed · 29.9s
```

`make check` **still exits 2**, on the next leg after docs-test, and it is
unrelated to this node (`build-wheel.sh` is not involved):

```
$ make check
…
error: /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/release/blastit-lsp needs glibc 2.34, above the 2.28 this artifact must run on
make: *** [Makefile:182: rs-no-python] Error 1
check exit=2
```

`rs-no-python` (`Makefile:175-190`) builds `blastit`/`blastit-lsp` **on this
host** and asserts them at `--max-glibc 2_28`; this host is glibc 2.41
(`ldd (Debian GLIBC 2.41-12+deb13u4) 2.41`), so the host-built LSP needs 2.34.
That leg needs a `manylinux_2_28` builder (or an older sysroot), not this node.

Two earlier `make check` runs stopped before that, both pre-existing and
unrelated: one in `crates/blastit/tests/critical_artifacts.rs` because a
`--manylinux` wheel I had left in `dist/` carries no `_blastit*.so` (dist
removed, as it was absent before my runs), and one in
`crates/blastit-scheduler/tests/critical_rewrite.rs` (5 timing-dependent cases,
`condition never became true; heads in flight: [1], body frames: 0`) which
passes standalone (`32 passed; 0 failed`) and passed on the next full run.

### `make fast-check`

```
$ cargo fmt --all && cargo fmt --all -- --check && make fast-check
…
fast-check exit=0
```

`scripts/test_packaging_gates.py` is the `packaging-gates-test` target (a `make check`
leg, not a `fast-check` one), run here so the new cases are in the log:

```
$ make packaging-gates-test
packaging-gates-test: ok — 117 cases passed (workflow=19, matrix=15, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=13, matrix-producer=2, package-workflow=11, wheel-floor=9, publish=19)
```

`make workflow-check` (inside `fast-check`) is green too:

```
workflow-check: ok — 674 checks over 9 workflows and 1 actions, 0 problem(s)
```

## Follow-ups

- `rs-no-python` asserts a `2_28` floor on **host-built** binaries; on a glibc
  2.41 host it can only fail. It belongs in the manylinux builder image (or
  needs `--build-target`/sysroot), otherwise `make check` is red on any modern
  developer host.
- `scripts/build-wheel.sh` stages `--shims-from` files **after**
  `check_blob_floors`, so a foreign PHP shim handed in that way is never measured
  against the wheel's floor. Ordering only; the floor logic itself is correct.
- `crates/blastit/tests/critical_artifacts.rs` reads any `dist/*.whl` it finds: a
  leftover `--manylinux` (bin-only) wheel there makes `make check` fail with
  "carries no blastit/_blastit*.so". Guard it to the host wheel, or clean
  `dist/` in `rs-test`.
- `crates/blastit-scheduler/tests/critical_rewrite.rs` has 5 timing-dependent
  cases that flake under the full workspace run's parallel load.
