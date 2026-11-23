# MB.71 — RPM install legs must not see the other architecture's packages

## What changed

1. `.github/workflows/packages.yml` (`install` job, the only download in it):

   ```yaml
             pattern: package-${{ matrix.arch }}-*
   ```

   Artifacts are named `package-<arch>-<format>`, so an `x86_64` leg now downloads
   only `package-x86_64-deb` + `package-x86_64-rpm` instead of all 28 files. The
   `stage` job's `pattern: package-*` (line 346) is untouched: it must keep seeing
   the whole set.

2. `e2e/run_packaging.py` — new `rpm_set_arch()` beside the other package helpers,
   used by the RPM install leg:

   ```python
               rpm_glob = f"/pkg/blastit-*.{rpm_set_arch(package)}.rpm"
   ```

   The architecture comes from this leg's own core filename
   (`blastit-0.2.1-1.x86_64.rpm` → `x86_64`), exactly as the deb leg scopes by
   `suffix`; no literal arch and no second matrix read. `rpm_set_arch()` first
   asserts that `package.parent` holds the core plus every `shim_packages()` name
   with that same suffix and nothing else matching `blastit-*.<arch>.rpm`, then
   returns the arch. Six of seven now fails by name instead of installing six.

   No existing assertion was weakened: `--version`, `blastit-lsp --version`,
   `fmt`, `fmt --check`, `check`, the served page, payload/ownership,
   `Depends`/`Requires`/`Recommends` and the WSGI legs are unchanged.

## Root cause

Two independent over-broad selections put both architectures in one container:
the install job's `package-*` download, and the arch-blind `/pkg/blastit-*.rpm`
`dnf install`. `dnf` then refused
`blastit-0.2.1-1.aarch64 ... does not have a compatible architecture` on the
`x86_64` leg (run 45145992227).

`scripts/install_matrix.sh`'s `core_package()` (~line 112) was **already**
arch-scoped (`blastit-[0-9]*."$package_arch".rpm`) and was **not** the cause — it
only picks the one core package to pass to `e2e/run_packaging.py`. Do not "fix" it.

## Evidence

Real artifacts from run 45145992227 (`/tmp/blastit-mb71/x86_64/` and
`/tmp/blastit-mb71/aarch64/`, 7 RPMs each) merged into
`/tmp/blastit-mb71/merged` (14 files), then the real `rpm_set_arch()` called on
the `x86_64` core:

```text
$ uv run --no-project /tmp/blastit-mb71/demo.py
merged dir holds 14 files
OLD /pkg/blastit-*.rpm        matches 14 files
NEW /pkg/blastit-*.x86_64.rpm matches 7 files (leg arch x86_64)
  kept: blastit-0.2.1-1.x86_64.rpm
  kept: blastit-php-8.3-0.2.1-1.x86_64.rpm
  kept: blastit-php-8.4-0.2.1-1.x86_64.rpm
  kept: blastit-php-8.5-0.2.1-1.x86_64.rpm
  kept: blastit-php-8.6-0.2.1-1.x86_64.rpm
  kept: blastit-python-3.13-0.2.1-1.x86_64.rpm
  kept: blastit-python-3.14-0.2.1-1.x86_64.rpm
  OLD GLOB ALSO PULLED: blastit-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-php-8.3-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-php-8.4-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-php-8.5-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-php-8.6-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-python-3.13-0.2.1-1.aarch64.rpm
  OLD GLOB ALSO PULLED: blastit-python-3.14-0.2.1-1.aarch64.rpm
SIX OF SEVEN -> LegFailure: this leg saw 6 of 7 rpm packages for x86_64: missing blastit-php-8.6-0.2.1-1.x86_64.rpm
```

The `aarch64` leg mirrors it (7 kept, the 7 `x86_64` files excluded). The
six-of-seven case ran against a copy of the same real set with
`blastit-php-8.6-…x86_64.rpm` moved aside, through the shipped assertion (not a
re-implementation), and failed before any install.

Workflow selection after the change (install scoped, stage still whole):

```text
$ grep -n "pattern: package" .github/workflows/packages.yml
283:          pattern: package-${{ matrix.arch }}-*
346:          pattern: package-*
```

## Follow-ups

- `.github/workflows/nightly.yml:323` has the identical over-broad
  `pattern: package-*` in its install job (its `stage`-equivalent download around
  line 346 is correct). Same failure mode, outside this node's `paths` —
  `e2e/run_packaging.py` is already fixed for it by the arch-scoped glob, but the
  nightly install job still ships both arches to every leg.

## Verification

`make fast-check` (includes the workflow-syntax gate via
`scripts/workflow_script_syntax.py`): **exit 0**, tail:

```text
test colour_is_decided_per_stream_and_the_two_variables_turn_it_off ... ok
test a_swapped_path_never_runs_a_program_that_verification_skipped ... ok

test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s

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
