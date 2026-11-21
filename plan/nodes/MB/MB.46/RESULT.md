# MB.46 — RESULT

## Files touched

| File | Why |
|---|---|
| `pyproject.toml` | Added one `format = "sdist"` include (`crates/**/*`) so the sdist carries **every** `[workspace] members = ["crates/*"]` path, not just the graph `crates/blastit` reaches, plus `format = "sdist"` `exclude` entries for `crates/**/target/**`, `crates/**/__pycache__/**`, `crates/**/.venv/**` and `crates/**/*.profraw` (the include glob is not `.gitignore`-filtered). No `format = "wheel"` entry was added, so `scripts/build-wheel.sh`'s direct `maturin build` is untouched. |
| `scripts/build-standalone.sh` | Dropped `--no-default-features --features php,static` from the echo, both `cargo build` lines and the `cargo tree` call: the archive is now the universal binary (`ADR-0017` decision 1). Added a `blastit-py-sapi present` assertion beside `0 pyo3` / `blastit-view-static present`, and rewrote the header, the archive `README.md` heredoc and the glibc/musl runtime requirements, which still claimed "interpreter-free / WSGI refused by design". |
| `scripts/build-standalone-in-image.sh` | Feature-spelling comment (~line 299) now says the archive is the default-feature universal binary. Kept it apostrophe-free: it is inside the single-quoted `bash -c` argument. |
| `.github/workflows/publish-pypi.yml` | The `binaries_gnu` step comment still described `--no-default-features --features php,static` as "the standalone artifact, no `python`, WSGI refused at exit 1"; replaced with the universal-artifact truth. |
| `scripts/standalone-runtime-check.sh` | WSGI half rewritten for the universal artifact: `BLASTIT_APP=app:application` so the boot reaches stage-0 discovery, then exit 1 + `no CPython 3.13 or 3.14 interpreter was found` / `run uv python install 3.14`; the run is backgrounded so a silently-serving artifact is a 20 s failure, not a hang. Header and `--require-no-python` comments no longer say the archive is interpreter-free / refuses WSGI by design. |

`blastit_build.py` was **not** changed: the `normalize` check failure was a stale assertion in the node's acceptance (since corrected), not a missing `_bin/blastit`, so no extra release build was added.

## Review round 2 — findings → fixes

| Finding | Fix |
|---|---|
| **[BLOCKER]** archive `## Contents` advertised `blastit serve`, which `crates/blastit/src/cli.rs` refuses (exit 2) | now `` `blastit [--preset NAME]` for a production boot, `blastit dev`, `blastit check` `` |
| **[MAJOR]** WSGI claim outran the artifact: the staging glob is `libblastit_php*` only, and a deployment resolves `libblastit_py3<minor>` beside the executable and nowhere else (`blastit-py-sapi/src/discovery/mod.rs`) | `:250` echo now says the **loader** is compiled in and the shim must sit beside the binary; the script header, the README headline, `## Contents` and **both** `What it needs at run time` branches say "PHP shims only", name `libblastit_py3<minor>-<arch>-<libc>.so`, the beside-`blastit` rule, `NoShim` when absent, and the two real ways to get it (wheel `blastit/_bin/` via `uv add --dev blastit`, or `make py-sapi` in a checkout); the `publish-pypi.yml` comment gained "(the archive carries PHP shims only)". No CPython shim is staged (that is `MB.42.3.3`'s matrix slice). |
| **[MINOR]** `crates/**/*` in `[tool.maturin] include` is not `.gitignore`-filtered | added `exclude` for `crates/**/target/**`, `crates/**/__pycache__/**`, `crates/**/.venv/**`, `crates/**/*.profraw` (all `format = "sdist"`) and said so in the comment. Measured on a deliberately dirty checkout: `target/junk.o`, `__pycache__/junk.pyc`, `.venv/pyvenv.cfg` and `junk.profraw` all reached the sdist with the bare patterns; `crates/**/target` / `crates/**/__pycache__` alone do **not** drop a directory's contents, hence `/**`. With the fix: **439 entries, zero dirty entries, no tracked file dropped**. |
| **[MINOR]** `php` capability was not asserted | `blastit-view-php present (an \`entrypoint php\` route is served)` added beside the `python`/`static` assertions, same shape — a build whose defaults lost `php` now fails here instead of shipping an archive that refuses every `[route.php]`. |
| **[MINOR]** `--require-no-python` was narrower than discovery | the guard also refuses `BLASTIT_PYTHON`, `VIRTUAL_ENV`, `BLASTIT_WHEEL_PYTHON` and `BLASTIT_PY_SHIM_DIR`; the `(no CPython on this host)` suffix and the closing "3/3 passed on a host with no CPython" print **only** with the flag. Verified: `env -i PATH=/usr/sbin VIRTUAL_ENV=/tmp/nope … --require-no-python` exits 1 with the `VIRTUAL_ENV` sentence. |
| **[MINOR]** DX polish | `1 shim` / `2 shims` in the echo, the error and `## Contents`; `DT_NEEDED` sentence is libc-aware (glibc: libc/libm/libgcc_s/loader, musl: `libc.musl`/`libgcc_s`); the no-PHP-shim text offers the wheel first and `make php-sapi` second; a runnable `## Try it` block (`index.html` + `static "/" { root "." }` + `./blastit dev --port 8080`, then `http://127.0.0.1:8080` — checked 200 locally); the `--require-no-python` rationale now says a present CPython yields `NoShim`, and the guard exists so the closing host claim is not a lie. |

## Final verification (review round 2)

sdist rehearsal, exact shape:

```
$ uvx --offline maturin sdist --out /tmp/opencode/sdist-check
📦 Including files matching "crates/**/*"
📦 Built source distribution to /tmp/opencode/sdist-check/blastit-0.2.0.tar.gz      (439 entries)
$ uv venv --python 3.13 … && VIRTUAL_ENV=… uv pip install --no-cache /tmp/opencode/sdist-check/blastit-*.tar.gz
Prepared 1 package in 1m 49s
 + blastit==0.2.0 (from file:///tmp/opencode/sdist-check/blastit-0.2.0.tar.gz)
HELP-OK        (blastit --help | grep -q "stage 0")
DBPROXY-OK     (test -x …/site-packages/blastit/_bin/blastit-dbproxy)
NORMALIZE-OK   (b.normalize('select 1') == 'select 1' and b.normalize('SELECT 1  ;') == 'SELECT 1')
```

archive + `debian:trixie-slim` rehearsal:

```
$ scripts/build-standalone.sh --out /tmp/opencode/standalone
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: blastit-py-sapi present (the dlopen loader: a WSGI view also needs the host's CPython 3.13/3.14 and that minor's shim beside the binary)
build-standalone: blastit-view-php present (an `entrypoint php` route is served)
build-standalone: blastit-view-static present (a static mount is served)
build-standalone: 1 shim for x86_64-gnu from …/target/php-sapi
build-standalone: 5779939 bytes -> /tmp/opencode/standalone/blastit-x86_64-unknown-linux-gnu.tar.gz
$ tar -C /tmp/opencode/artifact -xzf … && docker run --rm -v /tmp/opencode/artifact:/art:ro \
    -v "$PWD:/src:ro" debian:trixie-slim bash /src/scripts/standalone-runtime-check.sh \
    --require-no-python /art/blastit-x86_64-unknown-linux-gnu/blastit
host: no CPython on PATH, no BLASTIT_PYTHON/VIRTUAL_ENV/BLASTIT_WHEEL_PYTHON, no BLASTIT_PY_SHIM_DIR
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   2/2 static responses without touching an interpreter
standalone-runtime-check: /art/…/blastit dev --port 18460 (no CPython on this host)
ok   exit 1 before binding: no CPython 3.13/3.14 (discovery names 'uv python install 3.14')
standalone-runtime-check: 3/3 passed on a host with no CPython
```

Gates on the final tree:

```
$ make fast-check              install-test: all cases passed
$ make workflow-check          workflow-check: ok — 697 checks over 9 workflows and 1 actions, 0 problem(s)
$ make packaging-gates-test    packaging-gates-test: ok — 128 cases passed (… publish=19)
```

## Part A — `publish-pypi.yml` sdist rehearsal

Before (`uvx --offline maturin sdist`, 410 entries) the `crates/` set was missing exactly two workspace members:

```
crates/blastit-py-shim        (absent)
crates/blastit-view-python    (absent)
```

After `{ path = "crates/**/*", format = "sdist" }` (439 entries) the added files are exactly those two crates (29 files: `Cargo.toml`, `build.rs`, `src/**`, `tests/**`); nothing was removed:

```
crates/blastit-py-shim/build.rs
crates/blastit-py-shim/Cargo.toml
crates/blastit-py-shim/src/{boot,buffer,env,lib,request,serve,syntax}.rs
crates/blastit-py-shim/tests/abi_load.rs
crates/blastit-view-python/Cargo.toml
crates/blastit-view-python/src/{config,environ,immortal,input,lib,loaded,request,response,serve,zygote}.rs
crates/blastit-view-python/tests/{body,concurrency,critical_loaded_files,environ,flow,gil,serve}.rs
crates/blastit-view-python/tests/support/mod.rs
```

(`crates/**` alone matched nothing — maturin's glob needs `crates/**/*`; the log line `📦 Including files matching "crates/**/*"` and the entry count prove it.)

Exact rehearsal:

```
$ uvx --offline maturin sdist --out /tmp/opencode/sdist-check
📦 Including files matching "crates/**/*"
📦 Built source distribution to /tmp/opencode/sdist-check/blastit-0.2.0.tar.gz
$ uv venv --python 3.13 .../venv && VIRTUAL_ENV=... uv pip install --no-cache .../blastit-*.tar.gz
   Building blastit @ file:///tmp/opencode/sdist-check/blastit-0.2.0.tar.gz
Prepared 1 package in 1m 53s
 + blastit==0.2.0 (from file:///tmp/opencode/sdist-check/blastit-0.2.0.tar.gz)
$ .../venv/bin/blastit --help | grep -q "stage 0"        -> exit 0
$ test -x .../site-packages/blastit/_bin/blastit-dbproxy -> exit 0
```

`cargo build --release --locked -p blastit-dbproxy` inside the unpacked sdist (the `--locked` failure this node exists for) now succeeds — the install above *is* that build.

### One acceptance literal is wrong (proved, not worked around)

```
$ .../venv/bin/python -c "import blastit._blastit as b; print(repr(b.normalize('select 1')))"
'select 1'
$ .../venv/bin/python -c "... print(b.normalize('SELECT 1  ;'), b.normalize('WHERE id = \$1'), b._PROGRAM)"
SELECT 1 WHERE id = $? /tmp/opencode/sdist-check/venv/bin/blastit
```

The bridge works — it spawns the sdist's own `blastit` (`_program`) and returns its answers (`$1` → `$?`, `;` dropped). `normalize` is **case-preserving** by design (`crates/blastit-sqlpolicy/src/normalize.rs` doctest: `normalize("SELECT 1 -- …") == "SELECT 1"`), so the original `b.normalize('select 1') == 'SELECT 1'` literal could never hold. The node's acceptance now reads `b.normalize('select 1') == 'select 1' and b.normalize('SELECT 1  ;') == 'SELECT 1'` (lead-edited), which the run above satisfies; no `_bin/blastit` staging was added to `blastit_build.py`.

## Part B — universal `scripts/build-standalone.sh`

```
$ scripts/build-standalone.sh --out /tmp/opencode/standalone
build-standalone: cargo build --release --locked -p blastit --bin blastit
ok   .../target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-standalone: glibc floor 2.39 (readelf --version-info)
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: blastit-py-sapi present (a WSGI app is served on CPython 3.13/3.14)
build-standalone: blastit-view-static present (a static mount is served)
build-standalone: 1 shim(s) for x86_64-gnu from .../target/php-sapi
build-standalone: 5779706 bytes -> /tmp/opencode/standalone/blastit-x86_64-unknown-linux-gnu.tar.gz
```

`scripts/check-portable-elf.sh` (called by the script) and `scripts/check-no-test-seam.sh` both pass on the produced binary, as quoted above. Archive removed afterwards.

### `scripts/standalone-runtime-check.sh` — WSGI half, rewritten

The old assertion (`this build has no Python support` / `uv add blastit`) belongs to
`no_python_support`, which is `#[cfg(any(not(feature = "python"), test))]`
(`crates/blastit/src/stage0/gates.rs`) and therefore cannot fire in the universal
artifact. The half now proves the real promise: with `python` on and no CPython on
the host, a WSGI boot fails closed at exit 1 through stage-0 discovery with
`DiscoveryError::NoInterpreter`'s sentence. `BLASTIT_APP=app:application` is exported
so the boot clears `app_rule` and actually reaches discovery, and the process is
backgrounded (20 s ceiling) so an artifact that *did* serve is reported as a leak
instead of hanging the check.

Exact CI shape (`binaries_gnu`): archive extracted, bare `debian:trixie-slim`, no CPython.

```
$ scripts/build-standalone.sh --out /tmp/opencode/standalone
build-standalone: 5779705 bytes -> /tmp/opencode/standalone/blastit-x86_64-unknown-linux-gnu.tar.gz
$ tar -C /tmp/opencode/artifact -xzf ... && docker run --rm -v /tmp/opencode/artifact:/art:ro \
    -v "$PWD:/src:ro" debian:trixie-slim \
    bash /src/scripts/standalone-runtime-check.sh --require-no-python \
    /art/blastit-x86_64-unknown-linux-gnu/blastit
host: no CPython on PATH
standalone-runtime-check: /art/.../blastit dev --port 18400
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   2/2 static responses without touching an interpreter
standalone-runtime-check: /art/.../blastit dev --port 18460 (no CPython on this host)
ok   exit 1 before binding: no CPython 3.13/3.14 (discovery names 'uv python install 3.14')
standalone-runtime-check: 3/3 passed on a host with no CPython
```

## Gates

```
$ make fast-check
workflow-check: ok — 697 checks over 9 workflows and 1 actions, 0 problem(s)
plan OK
test result: ok. 15 passed; 0 failed   (blastit critical_handoff)
install-test: all cases passed
$ make workflow-check
workflow-check: ok — 697 checks over 9 workflows and 1 actions, 0 problem(s)
$ make packaging-gates-test
packaging-gates-test: ok — 128 cases passed (workflow=22, matrix=15, …, publish=19)
```

## Follow-ups

1. **(a)** the `.tar.gz` must ship `libblastit_py3{13,14}-<arch>-<libc>.so` beside the binary (`build-standalone.sh` staging + `--require-py-shims` + the `binaries_*` legs' `make py-sapi`), else `install.sh --user`/Alpine WSGI stays `NoShim`.
2. **(b)** normative tables now contradicting the script: `docs/design/backend-runtimes.md:253` and `:318`, `docs/design/view-crates-architecture.md:604`.
3. **(c)** `scripts/check-portable-elf.sh:5-7` still describes the pre-`ADR-0017` binary + `_blastit` extension.
4. **(d)** `ci.yml:534` step name is narrow.
5. **(e)** `docs/` pages calling the archive "interpreter-free".
6. Also carried over: `scripts/build-standalone.sh`'s musl `--max-glibc` comment block explains `dlopen` only in terms of `libphp` (not false, just narrow).
