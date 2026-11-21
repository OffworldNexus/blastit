# MB.47.2 — CI py-sapi shims & root provenance, wheel `--require-shims`, tarball CPython shims

STATUS: done

## A. CI: no wheel or `cargo test` run without CPython shims
- `ci.yml` **`test`** and **`smoke (root)`**: `astral-sh/setup-uv@v10.3.2` +
  `uv python install 3.13 3.14` + `make python-runtime` before `cargo test`. Smoke also
  `chmod -R go-w "$HOME/.local"` after the install, passes `HOME` to every `sudo env`,
  job/timeout 30 min and root step 20 min (warning 1200 s).
- **`e2e`, `rootless`, `wheel-smoke`, `pytest-plugin`, `docs-gate`**: setup-uv + install +
  `make python-runtime` before `scripts/build-wheel.sh --require-shims py` /
  `make docs-test`. `Makefile`: `docs-test: docs-unit py-sapi`, `PY_SAPI_REQUIRE_ALL := 1`.
- `live.yml` (was the same hole): the three steps, gated on `steps.token.outputs.present`,
  before `MATURIN=maturin scripts/build-wheel.sh --require-shims py`.

## B. `blastit-<target>.tar.gz` ships the CPython shims
- `build-standalone.sh`: `--py-shim-dir` / `--require-py-shims` (default 2), stages
  `libblastit_py3*` beside `libblastit_php*`, fails closed; `assert_shim_target` reads the
  ELF (`readelf -h` machine, `NEEDED` libc) so a mislabeled shim is refused by name.
- `build-standalone-in-image.sh`: derives `shim_libc` from `--target`, builds both minors
  in-image with `PY_SAPI_REQUIRE_ALL=1 PY_SAPI_PROBE=0`; `binaries_musl` builds them in Alpine.
- `make standalone: py-sapi`; the stager's refusal names both remedies.

## Review fixes
- `live.yml` wheel now carries shims (`--require-shims py` + setup-uv/install/runtime).
- Shim name-vs-ELF: `assert_shim_target` (machine + `NEEDED` libc) before staging.
- All CI shim steps use `make python-runtime` (`PY_SAPI_REQUIRE_ALL=1`), not `make py-sapi`.
- In-image libc from `${TARGET##*-}` with the same `gnu|musl` guard; `PY_SAPI_REQUIRE_ALL=1`.
- `make standalone: py-sapi`; refusal text names `make py-sapi` and the in-image build.
- Archive README rewritten: real per-family basenames in `## Contents`, supported minors
  derived from staged files, the `UnsupportedMinor`/`NoLibpython` contract with per-family
  remedies, a WSGI `## Try it` block, a forward link, "Built on the release machine.",
  and the 0-PHP/2-CPython separator bug fixed.
- `make help` for `docs-test` names the shims; `PY_SAPI_REQUIRE_ALL=1` inherited.

## Evidence
- `make workflow-check` → `ok — 713 checks over 9 workflows and 1 actions, 0 problem(s)`
- `make packaging-gates-test` → `ok — 128 cases passed`
- `platform_matrix.py --check` → `ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared`
- `make py-sapi PY_MINORS="3.14 3.13" PY_SAPI_PROBE=0` → `2 shim(s) for x86_64-gnu … skipped: none`
- README render (block extracted verbatim, gnu, 0 PHP + 2 CPython): grounded PHP fallback
  line, real `libblastit_py3{13,14}-x86_64-gnu.so` bullets, `CPython \`3.13\` and \`3.14\``.
- Mislabeled shim (`-musl` name over a gnu ELF): exit 1, `is not linked against musl, but
  this archive claims x86_64-musl`; scratch dir removed by trap.
- `make fast-check` → green (`rs-test`, `workflow-check 0 problem(s)`, `install-test: all cases passed`)

## Follow-ups
- `make docs-test`/`make standalone` now build the shims themselves; a host with only one
  CPython minor fails naming that interpreter.
