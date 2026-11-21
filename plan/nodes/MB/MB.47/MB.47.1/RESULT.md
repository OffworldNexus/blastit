# RESULT — MB.47.1

## What changed
- `e2e/rootless/Dockerfile`, `Dockerfile.prebuilt`: `_bin` is resolved from the installed package (no site-packages literal); `_bin/blastit` is copied to `_bin/blastit-init`, then `chown` → `chmod 0500` → `setcap`; `CMD` `exec`s `_bin/blastit-init` with `BLASTIT_EXEC` naming `_bin/blastit` (no `-m blastit`, no interpreter copy).
- `e2e/rootless/compose.yml`: `app.command` starts `_bin/blastit-init` directly (compose `$$`-escaped) and exports `BLASTIT_EXEC`; comments distinguish the shared binary from the capable copy.
- `e2e/rootless/run.py`: new `BIN_DIR` / `CAPABILITIES_PROBE`; mode/uid, `getcap` and uid-refusal probes target `_bin/blastit-init`; asserts `getcap` shows no `cap_` on `_bin/blastit` or `python3`.
- `e2e/rootless/README.md`: capability prose/diagram describe the init binary in `_bin/`, and `BLASTIT_EXEC` for dbproxy.
- `e2e/venv-smoke.sh`: wheel glob is `blastit-*-py3-none-*.whl` with a `blastit-*.whl` fallback; header comment matches.
- `crates/blastit/tests/critical_handoff.rs`: copies the image to `fixture/global/blastit` first, hard-links that to `.venv/bin/blastit`, and runs the copy (no cross-filesystem `EXDEV`).
- `crates/blastit/tests/critical_python_shim.rs`: `usable_php_runtime()` is the boot's own `discover_php`; `200` is expected only with a shim **and** a usable host `libphp`, otherwise the PHP-terms refusal.

## Evidence
`make py-sapi`:
```
py-sapi: 2 shim(s) for x86_64-gnu in .../target/py-sapi; skipped: none
```
`scripts/build-wheel.sh --require-shims py` → `ls dist/` (exactly one):
```
blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
```
`e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt --port 28080 --tls-port 28443 --project blastit-mb471-rootless` → rc=0, 249 `ok`, 0 `FAIL`:
```
the dedicated init binary's file capabilities: .../_bin/blastit-init cap_chown,...,cap_setpcap=ep
ok   the capable copy is mode 0500 and owned by init's uid (mode=500 uid=10000)
ok   the worker's uid cannot run it (sh: .../_bin/blastit-init: Permission denied)
```
`docker run --user 0:0 ... debian:trixie-slim`, both binaries `--test-threads=1`:
```
0
critical_handoff:     test result: ok. 15 passed; 0 failed; 0 ignored
critical_python_shim: php: no usable host libphp beside the shim; refused in PHP's terms
critical_python_shim: test result: ok. 5 passed; 0 failed; 0 ignored
```
`make fast-check`: all legs green (fmt/clippy/tests, py suites, install test `all cases passed`).

## Review fixes
1. `e2e/rootless/README.md` "Standalone install": the copy and `setcap` now live in
   `$lib=/usr/local/share/blastit/lib`, started with `BLASTIT_EXEC="$lib/blastit"`,
   with a sentence on why the copy sits beside the package (`<exe_dir>/../python`)
   instead of `bin/`; the capability list/order is unchanged.
2. `e2e/rootless/README.md` "## In CI": the retired "base image is the interpreter
   the wheel targets" rule is replaced by the universal-wheel rule —
   `E2E_PYTHON_VERSION` (`PYTHON_VERSION` for compose) chooses the image, default
   `3.14` per `e2e/prebuilt_wheel.py`; only a `cp3NN` developer wheel pins it.
3. `e2e/rootless/run.py` `local()`: the same rationale replaced.
4. `e2e/rootless/README.md` (refusal bullet) and `run.py` `check_negative_boot()`:
   "shared interpreter" → "shared binary" (`python3 -m blastit` execs `_bin/blastit`).
5. `run.py` `caps_and_limits()`: the failure now names the expected capabilities on
   `$lib/blastit-init`, none on `$lib/blastit` and `python3`, the `setcap`/`setcap -r`
   fix, and prints what `getcap` actually saw.
6. `run.py` `check_container()`: the init-uid probe now requires the positive
   signal as well — `$lib/blastit-init --version` must begin with `blastit` — so an
   import error, an empty `$lib` or a missing file can no longer be green.
7. `e2e/venv-smoke.sh`: the no-wheel error names `$root/dist`, both globs and the fix.

Evidence (fresh, after the fixes):
```
$ bash -n e2e/venv-smoke.sh && uv run --no-project python -m compileall -q e2e/rootless/run.py
bash-n ok
compileall ok
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt \
    --port 28080 --tls-port 28443 --project blastit-mb471-rootless
  ok   init's own uid can run it — which is why nothing else may use that uid (blastit 0.2.0)
  ok   the worker's uid cannot run it (sh: …/_bin/blastit-init: Permission denied)
249 `ok`, 0 `FAIL`, rc=0
```

## Review fixes (round 2)
1. **R1 — formatting restored.** `e2e/rootless/run.py` is back on the file's own
   layout: only the semantic hunks changed (`git diff --stat` → `112 +++---`,
   80 insertions / 32 deletions), not the 859-line reflow. (The session's
   auto-formatter reflows any `.py` this agent edits, so the edits were landed
   through a `.txt` copy.)
2. **R2 — `getcap` on a symlink.** `CAPABILITIES_PROBE` resolves each shared path
   with `readlink -f` and prints `label: <inode> <resolved path> <getcap>`; the
   driver FAILs unless every line names a real inode and an absolute path with no
   `cap_`, for `shared` (`_bin/blastit`), `dbproxy` (`_bin/blastit-dbproxy`),
   each `_bin/libblastit_*.so` shim, and `python3`. An empty/`MISSING` line is now
   a FAIL, never a pass.
3. **R3 — remediation text** uses `$(readlink -f "$(command -v python3)")` and
   names `$lib/blastit`, `$lib/blastit-dbproxy` too.
4. **R4 — `usable_php_runtime()`** goes through one `php_search()` with
   `PhpSearch::new(None, Some(php_shim_dir()), Development)`: exactly the cleared
   child's environment, no `BLASTIT_LIBPHP` forwarded.
5. **R5 — `answer` matched first:** `Ok(a)` asserts `200` + `php served this`
   (a 500/404 can no longer pass); `Err(_)` with a shim **and** a usable libphp
   still panics.
6. **R6 — refusal assertions** use `php_refusal_sentence()`, the boot's own
   `php_refusal_message("app", "public/index.php", …)` from `discover_php`'s
   refusal; both refusal arms assert that whole sentence.
7. **R7 — `python3 -I -c`** in both Dockerfiles (RUN + CMD) and the compose
   `command`, so an app-tree `blastit.py`/`blastit/` cannot choose `$lib` /
   `BLASTIT_EXEC`.
8. **R8 — the runnable probe** now `echo "rc=$?"` and requires `rc=0` **and** a
   `blastit …` version line; the refused probe also requires a non-zero status.
9. **R9 — README** notes that `pip install --force-reinstall blastit` leaves the
   old capable copy behind, so the copy and `setcap` must be recreated.
10. **R10 — wording:** `check_negative_boot` says "shared binary"; the README has
    no `/usr/local/bin` layout left (the copy lives in `$lib`).

Evidence (fresh):
```
$ git diff --stat -- e2e/rootless/run.py
 e2e/rootless/run.py | 112 +++++++++++++++++++++++++++++++++++++---------------
 1 file changed, 80 insertions(+), 32 deletions(-)
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt …   (rc=0, 249 ok, 0 FAIL)
  the dedicated init binary's file capabilities: …/_bin/blastit-init cap_chown,…,cap_setpcap=ep
    shared: 2728273294 /usr/local/lib/python3.14/site-packages/blastit/_bin/blastit
    dbproxy: 2728273307 /usr/local/lib/python3.14/site-packages/blastit/_bin/blastit-dbproxy
    shim: 2730143371 /usr/local/lib/python3.14/site-packages/blastit/_bin/libblastit_php84-x86_64-gnu.so
    shim: 2730143378 …/libblastit_py313-x86_64-gnu.so   shim: 2730143380 …/libblastit_py314-x86_64-gnu.so
    python: 2970784101 /usr/local/bin/python3.14
  ok   init's own uid can run it — which is why nothing else may use that uid (blastit 0.2.0
rc=0)
  ok   the worker's uid cannot run it (…/blastit-init: Permission denied
rc=126)
$ cargo test -p blastit --test critical_python_shim -- --test-threads=1     → 5 passed
$ docker run --rm --security-opt seccomp=unconfined -v "$PWD:$PWD:ro" -w "$PWD" debian:trixie-slim …
  critical_handoff: 15 passed;  critical_python_shim: 5 passed   (as uid 0)
$ (no libphp) critical_python_shim php leg → "php: no usable host libphp beside the shim;
  refused in PHP's terms"; the boot's own message was asserted whole:
  route "app": entrypoint php "public/index.php": no usable NTS libphp was found …
```

## Round 3
`a_candidate_that_is_this_image_does_not_hand_off` linked a *fresh copy* of the
image under the parallel harness, so `Command::output()`'s fork met a live write
descriptor on the new inode and `execve` returned `ETXTBSY` (serial passed,
parallel failed). The direct `fs::hard_link(real, link)` is now the primary path
(exec `real`, same inode as `/proc/self/exe`); the copy-then-link is the
`ErrorKind::CrossesDevices` (`EXDEV`) fallback for the `sudo`/container fixtures,
which run serial. No assertion changed, no retry added.

```
parallel ×5:  test result: ok. 15 passed; 0 failed; 0 ignored   (every run)
serial:       test result: ok. 15 passed; 0 failed; 0 ignored
uid 0 (debian:trixie-slim, --test-threads=1, EXDEV fallback):
  critical_handoff:     test result: ok. 15 passed; 0 failed
  critical_python_shim: test result: ok. 5 passed; 0 failed
```

## Follow-ups
- `docs/deploy/compose-traefik.md` §5–6 starts `blastit-init` with no `BLASTIT_EXEC`; `Host::binary()` then refuses a capable copy, so that recipe needs the variable (docs owner).
- `crates/blastit/src/host.rs` (outside this node): refuse a stale `_bin/blastit-init` (older inode/mtime than `_bin/blastit`), and do not resolve a sibling `blastit` as the binary — the production half of R9's reinstall hazard and of R7's app-tree resolution.
- `e2e/run.py:168`, `e2e/Dockerfile.prebuilt`, `e2e/Dockerfile` and `e2e/compose.yaml` still state the retired "not abi3, so the image is the interpreter the wheel targets" rule for the *other* prebuilt image (not in this review's list).
