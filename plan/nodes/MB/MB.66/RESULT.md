# MB.66 — shim packages recommend (not depend on) the distro runtime

## What changed

- `scripts/build-deb.sh`: `write_control` takes a 7th `Recommends` argument and
  writes the field only when it is non-empty (core passes `""`, so the core
  control has no `Recommends:` line). A shim's `Depends:` is now
  `blastit (= <version>)` + its measured `libc6`/`libgcc-s1`; the interpreter
  moved to `Recommends:`. Header comment states the recommended, not required,
  relationship.
- `scripts/rpm/blastit.spec`: all four PHP stanzas `Suggests: php-embedded` →
  `Recommends: php-embedded` (CPython already `Recommends:`); header comment
  rewritten — both formats recommend, neither hard-depends.
- `scripts/build-rpm.sh`: header comment corrected; the container/host echo now
  prints `--recommends` beside `--requires`.
- `e2e/run_packaging.py`: new `shim_runtimes()` reads
  `packaging/package-set.sh --rows`; the deb/rpm leg now asserts on the package
  **header**: a shim's `Depends` is exactly `blastit` + `libc6`(+`libgcc-s1`),
  its `Recommends` names the runtime (deb) / `rpm -qpR` names no runtime and
  `rpm -qp --recommends` names it (rpm), and the core names no runtime. Stale
  "hard `Depends`" comments/messages rewritten.
- `docs/install.md`: table now has `Depends` / `Recommends` columns (core keeps
  `Depends`) + one sentence that shims *recommend* the distro runtime (so
  `apt-get install blastit-php-8.4` still pulls `libphp8.4-embed`, while a
  pyenv/source-built interpreter is fine). Page is 129 lines.
- `docs/install.sh`, `packaging/package-set.sh`, `scripts/platform_matrix.py`,
  `scripts/test_install.sh`: only the now-false comments/notes (a shim's
  `Depends:` naming the runtime) corrected. Installer behaviour is unchanged:
  a shim is still offered only when its distro runtime package is installed,
  and shims are still installed with `--no-install-recommends`.

## Field lines (header is the source of truth)

```
$ dpkg-deb --field blastit_0.2.1_amd64.deb Depends
libc6 (>= 2.34), libgcc-s1
$ dpkg-deb --field blastit_0.2.1_amd64.deb Recommends
                                  (no field)

$ dpkg-deb --field blastit-python-3.13_0.2.1_amd64.deb Depends
blastit (= 0.2.1), libc6 (>= 2.34), libgcc-s1
$ dpkg-deb --field blastit-python-3.13_0.2.1_amd64.deb Recommends
python3.13

$ dpkg-deb --field blastit-php-8.4_0.2.1_amd64.deb Depends
blastit (= 0.2.1), libc6 (>= 2.17)
$ dpkg-deb --field blastit-php-8.4_0.2.1_amd64.deb Recommends
libphp8.4-embed
```

```
$ docker run --rm -v /tmp/mb66/packages:/pkg:ro rockylinux:9 \
    sh -c 'for f in /pkg/blastit-python-3.1[34]-0.2.1-1.x86_64.rpm /pkg/blastit-php-8.4-0.2.1-1.x86_64.rpm /pkg/blastit-0.2.1-1.x86_64.rpm; do
             echo "--- $(basename $f)"; rpm -qpR "$f" | grep -v rpmlib; echo "  recommends:"; rpm -qp --recommends "$f"; done'
--- blastit-python-3.13-0.2.1-1.x86_64.rpm
blastit = 0.2.1-1
glibc >= 2.34
libgcc
  recommends:
python3.13
--- blastit-python-3.14-0.2.1-1.x86_64.rpm
blastit = 0.2.1-1
glibc >= 2.34
libgcc
  recommends:
python3.14
--- blastit-php-8.4-0.2.1-1.x86_64.rpm
blastit = 0.2.1-1
glibc >= 2.17
  recommends:
php-embedded
--- blastit-0.2.1-1.x86_64.rpm                    (core: measured floor only)
glibc >= 2.34
libgcc
  recommends:
                                                   (no weak dep, no runtime)
```

Both shim families are symmetric on both formats: every one of the six shims
names its runtime in `Recommends:` and no `Requires:`/`Depends:` names one, and
`rpm -qp --suggests` is empty for all seven RPMs (PHP promoted from `Suggests:`,
CPython already `Recommends:`).

## Install proofs

Built from the read-only MB.65 bundle copy (`cp -a /tmp/opencode/mb65-bundle
/tmp/mb66/bundle`), `--max-glibc 2_34`; 7 debs + 7 rpms.

`debian:12` (no `python3.13` anywhere), shim installed **without**
`--no-install-recommends`:

```
Setting up blastit-python-3.13 (0.2.1) ...
$ dpkg-query -W python3.13
python3.13	                      # empty version: not installed
$ dpkg-query -s python3.13
dpkg-query: package 'python3.13' is not installed and no information is available  (exit 1)
$ dpkg -l python3.13
un  python3.13  <none>  <none>
$ blastit --version
blastit 0.2.1
```

Same in a fresh `debian:12` with `apt-get install -y --no-install-recommends`:
`Setting up blastit-python-3.13 (0.2.1)` and `blastit 0.2.1`.

`debian:trixie` (positive pair, recommends left on):

```
Setting up python3.13 ...
$ dpkg-query -W python3.13
python3.13	3.13.5-2+deb13u5
$ blastit --version
blastit 0.2.1
```

Six-distribution matrix (the six advertised amd64 legs; `--matrix` additionally
enumerates the six aarch64 legs, and only amd64 sets were built here — the
aarch64 set is CI's, exactly as MB.65 recorded):

```
PASS  deb   deb (debian:bookworm-slim)    1 passed, 0 failed, 0 skipped
PASS  deb   deb (debian:trixie)           1 passed, 0 failed, 0 skipped
PASS  deb   deb (ubuntu:22.04)            1 passed, 0 failed, 0 skipped
PASS  deb   deb (ubuntu:24.04)            1 passed, 0 failed, 0 skipped
PASS  rpm   rpm (rockylinux:9)            1 passed, 0 failed, 0 skipped
PASS  rpm   rpm (fedora:latest)           1 passed, 0 failed, 0 skipped
```

Each leg ran the new header assertions (they read `/pkg` for all seven
packages), so the deb and rpm shapes above are asserted in-container too.

```
$ packaging/package-set.sh --check                  → ok (7 packages; …)
$ uv run --no-project scripts/platform_matrix.py --check → ok, 0 leg(s) dropped
$ sh scripts/test_install.sh                        → all cases passed
```

## Gates

`make fast-check`'s own targets all pass, plus `make docs-check`:

```
$ make guard rs-fmt rs-lint rs-doc py-lint plan-validate install-test \
       universal-smoke docs-check
guard: OK (3 staged/untracked file(s) checked)
    Finished `dev` profile … (fmt/clippy/rustdoc, warnings denied)
install-test: all cases passed (fixture on http://127.0.0.1:8745, …)
    Finished `test` profile … (universal-smoke handoff suite)
No issues found

$ cargo test --workspace --all-targets --locked --features python \
      -- --skip learn_writes_one_grant_without_widening_a_view
… 39 test binaries: test result: ok (0 failed; 1 filtered out)
$ cargo test --workspace --doc --locked --features python
test result: ok. 13 passed; 0 failed …
test result: ok. 9 passed; 0 failed …
$ uv run --no-project tools/guard.py                → guard: OK
$ uv run --no-project tools/plan.py validate       → plan OK
$ uvx --offline ruff check e2e / format --check e2e  → 17 files formatted;
    the 4 `FURB167` notes are `e2e/docs_deploy.py`'s pre-existing `re.M`/`re.S`,
    none in `e2e/run_packaging.py`
```

One test is host-bound, not slice-related: `e2e_check_fmt`'s
`learn_writes_one_grant_without_widening_a_view` builds a UNIX socket under
`/tmp/opencode/blastit-e2e-dev-…/tmp/blastit-check-…/pg/.s.PGSQL.5432`, longer
than `SUN_LEN` (the crate's `scratch_root()` prefers `/tmp/opencode`; MB.65 saw
the same). With `/tmp/opencode` masked and `TMPDIR=/tmp`, it passes:

```
$ unshare -rm bash -c 'mount -t tmpfs tmpfs /tmp; : >/tmp/opencode; …'
test learn_writes_one_grant_without_widening_a_view ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 23 filtered out
```

## Follow-ups (out of this slice's scope)

- `packaging/README.md` (package table `Depends` column, `Suggests:` rows) and
  `CHANGELOG.md`’s MB.65 note still describe the old hard-dependency shape; both
  are outside this node's `paths` and should be updated with the next packaging
  doc change.
- `docs/install.md`’s "PHP 8.3/8.4 beyond Debian 13 and Ubuntu 24.04" note still
  says `add-apt-repository ppa:ondrej/php`, which is Ubuntu-only — Debian 12 needs
  a `packages.sury.org` apt line. Pre-existing, deliberately left untouched here;
  it needs its own docs fix (and a check of the 8.5/8.6 repository advice).
- `scripts/install_matrix.sh --matrix --dir` requires aarch64 sets as well; run
  locally it stops at `debian-12-aarch64` with
  `error: no single arm64 core .deb under …`. The six amd64 legs pass (above);
  the aarch64 sets are built by CI.

## Review fixes (review-ux)

- `docs/install.sh`: the `hinted_python` skip note now names the shim package as
  the way forward — the host already has the runtime, so only `detect_shims`'
  distro-package check is skipping it — and the comment above `python_lib_hint`
  says the same. `detect_shims`/`runtime_installed` selection is unchanged.
- `docs/install.md`: the recommends sentence now adds that the shim can be added
  by name, because the page's one-line installer only offers a shim whose distro
  runtime package it can see. Page is 129 lines.

```
$ sh -n docs/install.sh && (command -v dash >/dev/null && dash -n docs/install.sh || true)
syntax: ok
$ sh scripts/test_install.sh
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
$ make docs-check
No issues found
```

- Restored the `!!! note "PHP 8.3/8.4 beyond Debian 13 and Ubuntu 24.04"` body
  byte-for-byte to its pre-node text (the pre-existing sury/PPA advice is now a
  separate Follow-up, not a reworded claim in this diff); `docs/install.md` is
  130 lines.

