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
