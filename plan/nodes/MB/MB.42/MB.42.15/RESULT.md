# RESULT — MB.42.15

## Status
R12's wheel leg no longer asserts a subject `ADR-0017` deleted. It now certifies the
artifact the repack actually ships — the `py3-none` wheel's `blastit/_bin/` ELFs — and the
suite is green in **both** states the release gate can be in: with the built wheel in
`dist/` (six `_bin` ELFs checked) and with `dist/` empty (one-line skip). `BLASTIT_REQUIRE_WHEEL=1`
still turns "no wheel" into a failure. The leg was not deleted, not `#[ignore]`d, and no
portability assertion was weakened: `assert_portable` and its rule set are untouched, and
the same three rules now run over the universal binary, `blastit-lsp`, `blastit-dbproxy`
and every shim.

## 1. The leg, re-targeted (`crates/blastit/tests/critical_artifacts.rs`)
`the_wheel_extension_links_no_interpreter_and_no_php` → `the_universal_wheel_ships_no_interpreter_and_no_php`,
reading `certify_universal_wheel(&wheel)`:

1. **`py3-none` or refused, naming the tag.** The file name must be
   `<name>-<version>-<pytag>-<abitag>-<platform>.whl` with `py3-none`; a `cp314-cp314`
   wheel is refused with the tag it carries and why one universal artifact replaced it.
2. **`*.dist-info/WHEEL` must agree.** Exactly one `WHEEL` member and at least one `Tag:`,
   each equal to `py3-none-<the file name's platform>` — a `py3-none` name over a `cp314`
   `Tag:` is a retag, and a wheel whose promise nothing can read is an error, not a skip.
3. **No `_blastit*.so`.** The deleted extension is refused by name (`is_extension_member`
   is the old member rule; `_bin/libblastit_*.so` cannot match it, because the underscore
   is in `_bin/`).
4. **Every `blastit/_bin/` member is an ELF**, and `blastit/_bin/{blastit,
   blastit-dbproxy, blastit-lsp}` are all present — the three binaries
   `scripts/build-wheel.sh` greps for after the repack, since a wheel without them installs
   and cannot run.
5. **Every shim names the wheel's own `(arch, libc)`** (`libblastit_<kind>-<arch>-<libc>.so`
   against the platform tag's arch suffix and `gnu`/`musl`), because stage 1 `dlopen`s it.
   The architecture is matched as a *suffix*: `x86_64` carries an underscore of its own, so
   splitting the platform on `_` reads `64` (found by the first red run).
6. **`assert_portable` on each of them** — the unchanged rule set: no `DT_NEEDED` on
   `libpython*`, none on `libphp*`, no build-host `DT_RUNPATH`/`DT_RPATH`, and the
   anti-vacuity refusal of an ELF with no `DT_NEEDED` at all.

`REQUIRE_WHEEL` behaviour, the `select_wheel` version rule (and its "two wheels for one
version is a failure") and the one-line skip are byte-identical to before.

## 2. The negative half: `the_universal_wheel_shape_is_refused`
Ten fixtures, all built from the existing `synthetic` ELF and stored-entry zip writers:
the allowed twin (`py3-none`, three binaries, one own-target shim → certified), then
`cp314-cp314` tag, a `cp314` `Tag:` under a `py3-none` name, a stray
`_blastit.cpython-314-…so`, a missing `blastit/_bin/blastit`, an `aarch64` shim in an
`x86_64` wheel, a 64-byte non-ELF `_bin` member, `libpython` on the universal binary,
`libphp` on a shim, and a build-host `RUNPATH` on the universal binary. `expect_shape_refusal`
prints each refusal's first line, so the evidence is in the test output, not in a comment.

`the_wheel_without_an_extension_is_refused` is untouched: `extension_objects` keeps its
exact behaviour (errors with "carries no blastit/_blastit*.so …") and is documented as the
reader's own fixture half, with `is_extension_member` carrying the member rule it now shares
with the real leg.

## 3. Documentation in the file kept true
The module doc's R12 bullet, the `## Both directions` list and the section heading now
describe the universal payload; three stale "the extension" phrases inside
`assert_portable`/the zip reader say "an ELF"/"the payload" instead. No assertion string the
untouched leg checks was changed (`links an interpreter`, `links libphp`, `library search
path`, `ELF magic`, `DT_STRSZ`, `names no DT_NEEDED`, `not little-endian`, `names no library`
all still appear verbatim).

## Evidence

### `cargo test -p blastit --locked --test critical_artifacts` with the wheel in `dist/`
```
running 7 tests
test the_wheel_without_an_extension_is_refused ... ok
test the_trait_scan_catches_a_planted_name ... ok
test the_universal_wheel_shape_is_refused ... ok
test the_deleted_transport_trait_stays_deleted ... ok
test the_universal_wheel_ships_no_interpreter_and_no_php ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.31s
```
(`--nocapture`, the real artifact — the line the release gate now prints instead of an error)
```
ok   …/dist/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: py3-none wheel, 6 ELF(s) with no
libpython/libphp NEEDED and no build-host search path: blastit/_bin/blastit [libgcc_s.so.1
libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2], blastit/_bin/blastit-dbproxy
[…], blastit/_bin/blastit-lsp [libgcc_s.so.1 libpthread.so.0 libc.so.6 ld-linux-x86-64.so.2],
blastit/_bin/libblastit_php84-x86_64-gnu.so [libc.so.6],
blastit/_bin/libblastit_py313-x86_64-gnu.so […], blastit/_bin/libblastit_py314-x86_64-gnu.so […]
```

### Synthetic negatives (`--nocapture`, `the_universal_wheel_shape_is_refused`)
```
ok   blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: refused — …/cp/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl is tagged `cp314-cp314`, not `py3-none`: a `cp3NN` wheel installs only on the CPython minor that built it, and this artifact is one universal binary that `exec`s its own per-minor shims (`ADR-0017`, design §6.1). Rebuild it with `scripts/build-wheel.sh`, whose repack retags it
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — …/stray/… carries blastit/_blastit.cpython-314-x86_64-linux-gnu.so: `ADR-0017` retired the per-minor `blastit/_blastit*.so` extension for one `py3-none` universal wheel, so an extension here is an artifact from the old layout — rebuild it with `scripts/build-wheel.sh`, whose repack deletes it
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — …/retag/…: blastit-0.2.0.dist-info/WHEEL declares `Tag: cp314-cp314-manylinux_2_28_x86_64`, not `Tag: py3-none-manylinux_2_28_x86_64`: …
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — …/binary/… carries no blastit/_bin/blastit: every wheel `scripts/build-wheel.sh` builds carries the universal binary, `blastit-lsp` and the `blastit-dbproxy` stage 0 `execve`s — a wheel without them installs and cannot run
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — …/foreign/…: blastit/_bin/libblastit_py314-aarch64-gnu.so is built for aarch64-gnu and the wheel is x86_64-gnu: …
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — …/notelf/…:blastit/_bin/notes.txt: no ELF magic at the start of the member, so it is not an ELF this check can certify
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — error: …/python/…:blastit/_bin/blastit links an interpreter, so it only runs where CPython is installed:
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — error: …/php/…:blastit/_bin/libblastit_php84-x86_64-gnu.so links libphp, so it carries its own PHP instead of loading the host's:
ok   blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: refused — error: …/host/…:blastit/_bin/blastit carries a library search path from its build host:
```

### `dist/` empty (wheel moved aside, then restored)
```
running 7 tests
no blastit-0.2.0 wheel in …/dist: the R12 check needs a `scripts/build-wheel.sh` build (set BLASTIT_REQUIRE_WHEEL=1 to require the artifact)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.35s
```

### `BLASTIT_REQUIRE_WHEEL=1` with `dist/` empty
```
---- the_universal_wheel_ships_no_interpreter_and_no_php stdout ----
no blastit-0.2.0 wheel in …/dist: the R12 check needs a `scripts/build-wheel.sh` build (set BLASTIT_REQUIRE_WHEEL=1 to require the artifact)
Error: "BLASTIT_REQUIRE_WHEEL=1 is set but …/dist holds no blastit-0.2.0 wheel: build it with `scripts/build-wheel.sh` (`make docs-test` and the CI wheel legs do), then re-run `cargo test -p blastit --test critical_artifacts --locked`"

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out
```

### Lint, guard, gates
```
cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.22s

uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.15
critical-guard: HEAD..working tree, 6 changed file(s)
node MB.42.15: paths ['crates/blastit/tests/critical_artifacts.rs']
  M  crates/blastit/tests/critical_artifacts.rs  [authorized by crates/blastit/tests/critical_artifacts.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.42.15

make fast-check
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

### `make check` (full gate, `exit=0`)
```
running tests/critical_artifacts.rs
running 7 tests
test the_wheel_without_an_extension_is_refused ... ok
test the_universal_wheel_shape_is_refused ... ok
test the_trait_scan_catches_a_planted_name ... ok
test the_deleted_transport_trait_stays_deleted ... ok
test the_universal_wheel_ships_no_interpreter_and_no_php ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.56s

uv run --no-project e2e/universal_binary.py --binary /tmp/opencode/blastit-e2e/prefix/bin/blastit \
    --python 3.13 --python 3.14 --php 8.4 --static --handoff --check
PASS  python 3.13    200 from CPython 3.13 (python-3.13/.venv), same artifact (0.1s)
PASS  python 3.14    200 from CPython 3.14 (python-3.14/.venv), same artifact (0.1s)
PASS  php 8.4        200 from PHP 8.4 (libblastit_php84-x86_64-gnu.so) (0.1s)
PASS  static         200 with the file's own bytes (0.0s)
PASS  handoff        `dev` handed off silently; production and `--version` did not (0.0s)
PASS  check 3.12     check --json ok + fold on CPython 3.12, no per-minor .so (0.2s)
6 passed, 0 skipped, 0 failed

zed-check: OK (59 properties checked)
Ran 44 tests in 8.303s
OK
uv run --no-project tools/plan.py validate
plan OK
```
(`make check` exited 0; no failure to report. `docs-test` also built and exercised its own
wheel — `target/docs-wheel/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl`, `_bin/`
carrying the three binaries and the three host shims — so the shape checks are proved against
a freshly built artifact as well as the release one in `dist/`.)

## Follow-ups
- `make fast-check`'s `critical-guard` step runs without `--node`, so it can only *report*
  "not covered by any node path" for this authorized edit (exit 0 by design). The enforced
  run with `--node MB.42.15` is in the evidence above.
- Nothing in this node's `paths` was touched besides
  `crates/blastit/tests/critical_artifacts.rs` (the wheel in `dist/` was moved aside for the
  empty-`dist/` run and restored byte-identically).
- `docs/design/universal-distribution.md` still says `critical_artifacts.rs` is untouched
  "unless authorized in node `# Goal`" — this node is that authorization; no doc edit was
  needed inside this node's single-file `paths`.

