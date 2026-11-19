# MB.42.2.4 — `make install` staging, `test-critical` staging, and one-binary four-runtime E2E

STATUS: done

## What changed

| File | Change |
|---|---|
| `Makefile` | `install` stages the §6.1 bundle (one universal `blastit` + `blastit-lsp` + every shim this host built + `LICENSE` + `python/blastit/*.py`, with `bin/{blastit,blastit-lsp}` as `../share/blastit/lib/...` symlinks; no `_bin/`, no separate `blastit-dbproxy`) and ends by running `scripts/check-bundle-layout.sh`; `rs-no-python` also builds the **release** binary `make install` ships and runs `check-portable-elf.sh` on it (passing `--max-glibc 2_28` only when the script supports the flag, `MB.42.3`); new `universal-smoke` (debug portable-ELF + `cargo test -p blastit-audit` + `critical_handoff`) is a `fast-check` prerequisite; new `e2e-universal` stages a bundle and runs the harness, and is a `check` prerequisite |
| `e2e/universal_binary.py` | **New.** The epic's acceptance as one harness: one sha256-identical binary serves a WSGI project under CPython 3.13 and 3.14 (marker module in the project's own `.venv` site-packages), the PHP 8.4 fixture, and a static mount; the `.venv/bin/blastit` handoff runs for `dev` (silently) and never for production/`--version`; and CPython 3.12 runs `python -m blastit check --json` + `python -m blastit.fold` through the bundle with no per-minor `.so`. One PASS/FAIL/SKIP line per leg + summary; exit 1 on any FAIL |
| `scripts/check-bundle-layout.sh` | **New.** Asserts the bundle tree exactly (two symlink entry points, `lib/{blastit,blastit-lsp,libblastit_php*.so,libblastit_py3*.so}`, `LICENSE`, `python/blastit/*.py`), refuses a retired `_bin/blastit-dbproxy`, refuses stray files under `share/blastit`, and proves `bin/blastit --version` runs |

No Rust source changed, so the workspace's own gates are unchanged in shape. `test-critical`
and `rs-test`'s `python-runtime` dependency were already correct (`MB.42.2.1`–`MB.42.2.3`) and
were not touched.

## Acceptance evidence

### `make install` → §6.1 layout, then `--version`

```
$ rm -rf /tmp/opencode/install-x && make install PREFIX=/tmp/opencode/install-x
… py-sapi: 2 shim(s) for x86_64-gnu in …/target/py-sapi; skipped: none
… php-sapi: cc -> …/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
… php-sapi: 1 shim(s) for x86_64-gnu in …/target/php-sapi; skipped: 8.5 8.3
ok   /tmp/opencode/install-x/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shim(s), 1 PHP shim(s), python/blastit/*.py
ok   /tmp/opencode/install-x/bin/blastit --version: blastit 0.2.0
install: python, php and static; libpython and libphp are never linked

$ /tmp/opencode/install-x/bin/blastit --version
blastit 0.2.0

$ find /tmp/opencode/install-x \( -type f -o -type l \) -printf '%y %P -> %l\n' | sort
f share/blastit/lib/blastit ->
f share/blastit/lib/blastit-lsp ->
f share/blastit/lib/libblastit_php84-x86_64-gnu.so ->
f share/blastit/lib/libblastit_py313-x86_64-gnu.so ->
f share/blastit/lib/libblastit_py314-x86_64-gnu.so ->
f share/blastit/LICENSE ->
f share/blastit/python/blastit/_blastit.py ->
f share/blastit/python/blastit/_check_pytest.py ->
f share/blastit/python/blastit/fold.py ->
f share/blastit/python/blastit/__init__.py ->
f share/blastit/python/blastit/logs.py ->
f share/blastit/python/blastit/__main__.py ->
f share/blastit/python/blastit/pytest_plugin.py ->
l bin/blastit-lsp -> ../share/blastit/lib/blastit-lsp
l bin/blastit -> ../share/blastit/lib/blastit
```

`python/blastit/_blastit.py` was **not** edited: its `_candidates()` already resolves
`<pkg>/../../lib/<name>`, which is `<prefix>/share/blastit/lib/blastit` in this layout. The
harness proves it instead of asserting it by inspection: the 3.12 leg runs
`python -c 'from blastit import _blastit; print(_blastit.bin_path())'` and requires
`os.path.samefile()` with the `--binary` path.

### `e2e/universal_binary.py` (against the bundle above)

```
$ uv run --no-project python e2e/universal_binary.py \
    --binary /tmp/opencode/install-x/bin/blastit \
    --python 3.13 --python 3.14 --php 8.4 --static --handoff --check

== one artifact, four runtimes ==
binary  /tmp/opencode/install-x/bin/blastit
file    /tmp/opencode/install-x/share/blastit/lib/blastit
sha256  29666504a3b8031e15514df3499375f57b0c18125ba29050bb69274ff55ad1f9
version blastit 0.2.0

PASS  python 3.13    200 from CPython 3.13 (python-3.13/.venv), same artifact (0.1s)
PASS  python 3.14    200 from CPython 3.14 (python-3.14/.venv), same artifact (0.1s)
PASS  php 8.4        200 from PHP 8.4 (libblastit_php84-x86_64-gnu.so) (0.1s)
PASS  static         200 with the file's own bytes (0.0s)
PASS  handoff        `dev` handed off silently; production and `--version` did not (0.0s)
PASS  check 3.12     check --json ok + fold on CPython 3.12, no per-minor .so (0.1s)

6 passed, 0 skipped, 0 failed

one artifact: 29666504a3b8… served every passing leg
```

The 3.12 leg's own evidence (`check --json` document, from the same run):

```
{ "schema_version": 1, "ok": true, "requests": 0, "statements": 0,
  "routes": { "total": 0, "exercised": 0 }, "proxies": {}, "findings": [] }
```

and `fold` prints the plan for the log's `SELECT id FROM notes WHERE id = $?` denial (the
`$?` placeholder is never re-parsed as SQL).

## Which gate runs the harness

**Not `fast-check`** — it stages a *release* bundle, builds a venv per interpreter and boots a
real `blastit dev` per leg. Two gates run it:

* `make check` (the epic/release gate) has `e2e-universal` as a prerequisite;
* `make e2e-universal` runs it on its own, against `target/e2e-universal/prefix` (override with
  `E2E_UNIVERSAL_PREFIX=…` and `E2E_UNIVERSAL_LEGS="…"`).

`fast-check` gains only the cheap half: `universal-smoke` (debug `blastit` portable-ELF,
`cargo test -p blastit-audit`, `cargo test -p blastit --test critical_handoff`).

## Notes / host facts

* The host has PHP 8.4 only (`php-config8.4`), so the bundle carries one PHP shim; 8.5/8.6 and
  the 3-minor matrix are `MB.42.5`/`make php-sapi`. The harness SKIPS the PHP leg, loudly with
  the exact command, when the shim or `libphp8.4` is absent.
* The handoff leg needs a tree design §4 accepts. This checkout's `/home/remy/offworld-nexus/blastit`
  is `0775`, so the harness relocates that fixture to a fresh `0700` directory under `$TMPDIR`
  (the root-owned sticky `/tmp` is the one exemption the rule allows) and says why; it SKIPs with
  `chmod 755` when nothing on the host qualifies.
* `sys.executable` inside an embedded WSGI view is the name the runtime was configured with, not
  the venv, so the legs prove "which venv served this" with a marker module that exists only in
  that venv's `site-packages` (the same trick `e2e/venv-smoke` uses) plus `sys.version_info`.
* The unnamed `postgres $DATABASE_URL` store is named `db` (§7.3) — the fold fixture's
  `sql.denied` event says `"proxy": "db"`.

## Follow-ups (out of this node's scope)

* `e2e/venv-smoke/Blastfile`'s header still says the wheel ships `blastit/_bin/blastit-dbproxy`;
  the binary plays `--role=dbproxy` itself now. Comment-only, its own slice.
* `scripts/check-portable-elf.sh` has no `--max-glibc` yet (`MB.42.3`); `rs-no-python` adds the
  flag only when the script advertises it, so the floor is enforced the moment `MB.42.3` lands.
* `e2e/universal_binary.py` is not ruff-linted (`py-lint` covers `python tools`). If the e2e
  drivers are ever added to that target, this file needs an `E501`/`S603`-free pass.
* `MB.42.5` should extend `LIBPHP_84`'s sibling list and the `--php` leg to 8.5/8.6; the harness
  already takes the minor as an argument and derives the shim name from it.

## Progress (run 2) — consolidated `review-arch` + `review-ux` fixes

STATUS: done. Paths unchanged (`Makefile`, `e2e/**`, `scripts/**`); no Rust source touched.

### BLOCKER — a SKIP can no longer leave the epic gate green

`e2e/universal_binary.py`'s summary takes `allow_skip`: any FAIL fails, and so does a
SKIP unless `--allow-skip` is passed. `make check` and `make e2e-universal` never pass it,
so a host without `libphp8.4-embed` (or a bundle that shipped no PHP shim) fails the gate
instead of printing a green line for a leg that never ran:

```
$ uv run --no-project python e2e/universal_binary.py --binary /tmp/opencode/install-y/bin/blastit --php 8.9 --static
1 passed, 1 skipped, 0 failed
error: 1 requested leg(s) could not run: this host does not prove the bundle's claim; fix the SKIP above or pass --allow-skip
one artifact: f7d548a1610b… did not pass every leg
exit(no allow-skip)=1
$ … --allow-skip
1 passed, 1 skipped, 0 failed
one artifact: f7d548a1610b… served every passing leg
exit(--allow-skip)=0
```

`--allow-skip` exists for a developer on a host that cannot run one runtime; the two gates
never use it.

### MAJOR — the PHP leg proves the *bundle's* shim

`leg_php` now requires `libblastit_php<mm>-*.so` **beside the resolved binary**
(`ctx.binary.resolve().parent`) like `leg_python` already did, and the `dev` boot gets no
`BLASTIT_PHP_SHIM_DIR`: `exe_dir = …/share/blastit/lib` already finds the shim the bundle
ships, so a bundle that shipped none SKIPs (and, above, fails the gate). `BLASTIT_LIBPHP`
stays: it is a host fact, not the artifact under test. `make e2e-universal` stages with
`BUNDLE_REQUIRE_PHP_SHIMS=1`, and the counter refuses a shortfall — proved by asking for
two on a one-shim host:

```
$ BUNDLE_REQUIRE_PHP_SHIMS=2 make install PREFIX=/tmp/opencode/install-neg
error: 1 PHP shim shipped, and BUNDLE_REQUIRE_PHP_SHIMS=2; run make php-sapi on a host with php<m.m>-dev and libphp<m.m>-embed
error: 1 problem in the bundle under /tmp/opencode/install-neg
make: *** [Makefile:808: install] Error 1
```

### MAJOR — `blastit-lsp` is ELF-checked, installed or not

`rs-no-python` now builds and checks **both** release binaries, and `check-bundle-layout.sh`
runs `check-portable-elf.sh` over every installed ELF (`$lib/blastit`, `$lib/blastit-lsp`,
`$lib/libblastit_*.so`) before it reports the tree:

```
ok   …/target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   …/target/release/blastit-lsp: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
```

### MAJOR — the glibc-floor probe reads the parser, not prose

`rs-no-python` distinguishes *advertised* (`grep -- '--max-glibc'`) from *parsed*
(`grep -e '--max-glibc)'`) and fails loudly when only the first is true, instead of passing
`--max-glibc 2_28` as two artifact paths:

```
if [ "$advertised" -eq 1 ] && [ "$parsed" -eq 0 ]; then
  echo "error: scripts/check-portable-elf.sh advertises --max-glibc without parsing it: a floor that is never applied reads as enforced; …" >&2; exit 1
fi
```

Today the script mentions neither, so `check-portable-elf.sh <release binaries>` runs with no
extra flag; when `MB.42.3` adds a `--max-glibc)` arm the floor is applied automatically.

### MAJOR — `install` output: measured capabilities, a next step, and why Python lives there

```
ok   /tmp/opencode/install-y/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shims, 1 PHP shim, python/blastit/*.py
ok   /tmp/opencode/install-y/bin/blastit --version: blastit 0.2.0
install: capabilities: static always; python 2 shim(s); php 1 shim(s)
install: python package: /tmp/opencode/install-y/share/blastit/python/blastit (it belongs to this binary, not to your interpreter)
install: no pip needed:  PYTHONPATH=/tmp/opencode/install-y/share/blastit/python python -m blastit check
install: next: in a project with a Blastfile, run  /tmp/opencode/install-y/bin/blastit dev
install: add it to PATH:  export PATH="/tmp/opencode/install-y/bin:$PATH"
```

The counts are the ones the bundle actually carries (0 prints its own `note:` with the
command that would build a shim), never a static "python, php and static".

### MINORs

1. `check-bundle-layout.sh` grew `plural()`; the summary and both counters say
   `1 Python shim` / `2 Python shims` and `1 problem`. ✔
2. The harness's last line says `did not pass every leg` when `status != 0`. ✔
3. A missing host/artifact prerequisite is a **SKIP** in `leg_python`, `leg_php` and
   `leg_check` alike; the new `--allow-skip` rule decides the exit code. ✔
4. The `_bin` refusal names the fix (`remove -r $package/_bin, or re-run make install`) and the
   stray-file arm lists §6.1's members. ✔
5. Stray **directories** are caught (`find … -print0`, a `case` allowing exactly `lib`,
   `python`, `python/blastit`), and `python/blastit/*.py` must be one of the seven names, so an
   eighth module is a failure too. ✔
6. The fold assertion is the grant shape, not the echoed denial: `query 'SELECT id FROM notes
   WHERE id = $?'` plus the `# view notes (store db)` heading `fold` prints. ✔
7. `main()` rejects a `--binary` whose `--version` does not start with `blastit ` (and whose
   `--version` exits non-zero) with a usage error naming the bundle path. ✔
8. `E2E_UNIVERSAL_PREFIX` moved out of the checkout to
   `$(if $(TMPDIR),$(TMPDIR),/tmp)/opencode/blastit-e2e/prefix`, so `stage0`'s `cargo_target()`
   cannot hand a `dev` leg `<checkout>/target/py-sapi` as a shim candidate. The bundle's own
   shim is what `leg_php`/`leg_python` require. ✔
9. `universal-smoke` keeps the debug portable-ELF check (the release floor runs in `check`) and
   `cargo test -p blastit --test critical_handoff` (so a handoff regression names itself);
   `cargo test -p blastit-audit` was dropped because `rs-test` already runs it. ✔

### Discovered while producing this run's evidence — `make test-critical` was red for a host reason

The protected `test_lsp_stdio.py` drives `python -m blastit lsp`, which execs
`<stage>/blastit/_bin/blastit lsp`. That binary's §4 handoff saw `$VIRTUAL_ENV` (exported by
`uv run`, pointing at the checkout's `.venv`) and **refused** it — the checkout is `0775`, so
the explicit candidate fails the tree rule and handoff exits `1`, closing stdout before the
handshake (`22` failures). The tier proves the *staged* artifacts, so the recipe now exports
the documented escape hatch `BLASTIT_NO_LOCAL_HANDOFF=1`; nothing in `python/tests/critical/`
changed:

```
$ make test-critical
protected critical tests hold no skip construct
cargo build --locked -p blastit
python -m unittest discover -s python/tests/critical
.....................................................
----------------------------------------------------------------------
Ran 53 tests in 10.860s
OK
```

`make check` was red on any `0775` checkout for the same reason; this makes the gate depend on
the staged bundle rather than on the developer's shell.

### This run's harness summary (`make e2e-universal`, bundle outside the checkout)

```
binary  /tmp/opencode/blastit-e2e/prefix/bin/blastit
file    /tmp/opencode/blastit-e2e/prefix/share/blastit/lib/blastit
sha256  f7d548a1610b8505e50699de1513c0e4f7a88ab784a76b7d09bde1fbb694cd6a
PASS  python 3.13    200 from CPython 3.13 (python-3.13/.venv), same artifact (0.1s)
PASS  python 3.14    200 from CPython 3.14 (python-3.14/.venv), same artifact (0.1s)
PASS  php 8.4        200 from PHP 8.4 (libblastit_php84-x86_64-gnu.so) (0.1s)
PASS  static         200 with the file's own bytes (0.0s)
PASS  handoff        `dev` handed off silently; production and `--version` did not (0.0s)
PASS  check 3.12     check --json ok + fold on CPython 3.12, no per-minor .so (0.2s)

6 passed, 0 skipped, 0 failed

one artifact: f7d548a1610b… served every passing leg
```

`make rs-no-python` ends green on both release binaries and the default-build assertions
(`default build: 0 pyo3 dependencies` / `blastit-py-sapi present, 0 pyo3, no libpython
linked`), and `/tmp/opencode/install-y/bin/blastit --version` prints `blastit 0.2.0`.

Gate wiring is unchanged: `e2e-universal` is a `check` prerequisite, never `fast-check`
(which carries `universal-smoke` only).

### Follow-ups (run 2)

* `/home/remy/offworld-nexus/blastit` is `0775`, which makes every *explicit* handoff
  (`$VIRTUAL_ENV`, and therefore any `uv run` shell) fail closed on this host. `test-critical`
  now pins `BLASTIT_NO_LOCAL_HANDOFF=1`, but a `chmod 755` on the checkout would let the
  handoff suite exercise the explicit path for real.
* `MB.42.3`'s `--max-glibc` must land as a `case`/`if` arm matching `--max-glibc)`, or
  `rs-no-python` refuses to run it (by design: the error names the parser).
* `BUNDLE_REQUIRE_PY_SHIMS`/`BUNDLE_REQUIRE_PHP_SHIMS` mean *at least* N; a release pipeline
  that wants the full 2 Python + 4 PHP matrix should pass those exact numbers after
  `MB.42.5`.
