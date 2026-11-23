# RESULT — MB.68: build-wheel action ran `build-wheel.sh` before `setup-uv`

## Change

`.github/actions/build-wheel/action.yml` only:

- Moved `astral-sh/setup-uv@v10.3.2` from just before `e2e/run_packaging.py` to
  immediately after `dtolnay/rust-toolchain@stable`, i.e. **before** the
  "Refuse a wheel with no manylinux container" and "Build the universal wheel"
  steps, and deleted the now-duplicate later step.
- Added a note to the action `description:` and a comment above the `setup-uv`
  step: `scripts/build-wheel.sh` needs `uv` (or `PYO3_PYTHON`) for its
  README-link rewrite and fails closed without one, so the action provides `uv`
  itself and no caller must know.
- `scripts/build-wheel.sh` untouched (still fails closed).

`.github/workflows/wheels.yml` needed no change: `wheels_gnu` calls this action,
which now supplies `uv`; the `run_packaging.py` step still finds it on `PATH`.

## Final `runs.steps` order (parsed)

```
$ uv run --no-project --offline --with pyyaml python - <<'PY'  # yaml.safe_load + enumerate runs.steps
1. uses: dtolnay/rust-toolchain@stable
2. uses: astral-sh/setup-uv@v10.3.2
3. Refuse a wheel with no manylinux container
4. Build the universal wheel (scripts/build-wheel.sh)
5. Install the wheel in the matrix's images and drive it (e2e/run_packaging.py)
6. uses: actions/upload-artifact@v4
```

The YAML parses cleanly (`yaml.safe_load`) and no `astral-sh/setup-uv` step
remains after the build. (`actionlint` is not installed and not in the offline
`uvx` cache; PyYAML was resolved from the offline cache via
`uv run --no-project --offline --with pyyaml`.)

## Local proof: with `uv` on `PATH` the gate is passed

```
$ sha256sum README.md
c56f884fab5ef4c3e26752d59276422dfe056cf195dc6f60627d243d2253284c  README.md
$ timeout 60 bash scripts/build-wheel.sh --manylinux 2_28 --require-shims py --out /tmp/mb68-with-uv
README.md: .github/brand/header-dark.svg → https://raw.githubusercontent.com/OffworldNexus/blastit/develop/.github/brand/header-dark.svg
README.md: .github/brand/header-light.svg → https://raw.githubusercontent.com/OffworldNexus/blastit/develop/.github/brand/header-light.svg
README.md: ARCHITECTURE.md → https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md
README.md: LICENSE → https://github.com/OffworldNexus/blastit/blob/develop/LICENSE
README.md: packaging/languages.tsv → https://github.com/OffworldNexus/blastit/blob/develop/packaging/languages.tsv
build-wheel: staged 6 shim(s) from this host for x86_64-gnu
py-sapi: 2 shim(s) for x86_64-gnu in /blobs; skipped: none
🍹 Building a mixed python/rust project
🐍 Using CPython 3.14 at /opt/python/cp314-cp314/bin/python to generate the bin bindings
...
exit=124            # timeout: the script is inside the manylinux container build
$ sha256sum README.md
c56f884fab5ef4c3e26752d59276422dfe056cf195dc6f60627d243d2253284c  README.md
```

It performed the README-link rewrite (the exact thing that needs `uv`), entered
the docker build, and was only stopped by `timeout 60` — never by the README
sentence. The `trap restore_readme EXIT` left `README.md` byte-identical.

## Local proof: still fails closed without `uv`/`PYO3_PYTHON`

```
$ env -u PYO3_PYTHON PATH=/usr/bin:/bin bash scripts/build-wheel.sh --manylinux 2_28 --out /tmp/mb68-no-uv
the README rewrite needs uv or PYO3_PYTHON: install uv, or point PYO3_PYTHON at an interpreter
exit=1
$ sha256sum README.md
c56f884fab5ef4c3e26752d59276422dfe056cf195dc6f60627d243d2253284c  README.md
```

This is the exact `wheels_gnu` failure on `develop`; running `setup-uv` first is
what removes it.

## Gate

No Rust/Python source changed; YAML only. `make fast-check` tail:

```
test the_two_switches_control_the_handoff ... ok
test an_explicit_blastit_bin_that_is_this_image_is_not_refused ... ok
test the_production_shapes_never_hand_off ... ok
test colour_is_decided_per_stream_and_the_two_variables_turn_it_off ... ok
test a_swapped_path_never_runs_a_program_that_verification_skipped ... ok

test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s

sh scripts/test_install.sh
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.1 (fixture))
install-test: ok (b) a wrong digest exits 1 and installs nothing
install-test: ok (c) an overridden origin without a digest fails closed with zero requests
install-test: ok (d) an archive with a symlink member is refused before unpacking
install-test: ok (e) --help exits 0 with env -i (no root, no network)
install-test: ok (f) usage errors exit 2 (relative --prefix, --nightly --user, path-shaped --version)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
install-test: ok (h) apt installs the core first, each shim alone, and warns instead of aborting on a shim it cannot install
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

Verdict: **green** (0 failures, 0 clippy/compiler warnings).

## Follow-ups

None.
