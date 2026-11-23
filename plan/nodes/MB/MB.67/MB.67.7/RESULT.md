# MB.67.7 — builders & shell helpers derive counts and minors from the loaders

STATUS: done

## What changed

Every packaging builder and CI shell helper now reads counts/minors from
`packaging/languages.tsv` through the existing loaders; no new parser, no
scanner.

- `scripts/build-deb.sh`, `scripts/build-rpm.sh`: added `--print-set` (prints
  `packaging/package-set.sh --rows`, exits before the bundle/root checks); the
  package count is `1 + blastit_package_shims`, the final shim check compares
  against the loader's shim count, and `BUNDLE_REQUIRE_*_SHIMS` come from
  `shim_count_of <family>` (family = the package-name segment after `blastit-`).
  Measured floors / `--max-glibc` untouched.
- `scripts/check-bundle-layout.sh`: same `shim_count_of` for the set-mode
  `BUNDLE_REQUIRE_PY_SHIMS`/`BUNDLE_REQUIRE_PHP_SHIMS`; the "-eq 7" check is the
  core-plus-shim invariant; failure messages name the table.
- `scripts/build-package-bundle.sh`: `require_py`/`require_php` from the loader;
  messages/usage no longer enumerate versions.
- `scripts/build-standalone.sh`: `--require-py-shims` defaults to the CPython
  row count; the README's `apt install libpython…` example uses the newest staged
  minor, not a literal.
- `scripts/build-standalone-in-image.sh`: `PY_MINORS` and
  `PY_SAPI_PYTHON_<digits>` derive inside the image from
  `bash /src/packaging/languages.sh --py-minors` (the repo is mounted at `/src`);
  `PY_SAPI_REQUIRE_ALL=1` still fails closed.
- `scripts/py_sapi_leg.sh`, `scripts/build-musl-wheel.sh`: accept only declared
  CPython minors and reject an undeclared one by name; `PY_SAPI_PYTHON_<digits>`
  names kept.
- `Makefile`: `PY_MINORS`/`PHP_MINORS` are `$(shell bash $(CURDIR)/packaging/languages.sh --…-minors | tr '\n' ' ')`
  (3.14 3.13 / 8.6 8.5 8.4 8.3 order preserved); `PY_SAPI_PYTHON_<digits>` and the
  `py-sapi` recipe semantics unchanged; `E2E_UNIVERSAL_LEGS` Python legs derive
  from `PY_MINORS`.

## Proof

```
$ bash packaging/package-set.sh --rows
blastit	core	(none)	(none)	(none)
blastit-python-3.13	shim	libblastit_py313-*-gnu.so	python3.13	python3.13
blastit-python-3.14	shim	libblastit_py314-*-gnu.so	python3.14	python3.14
blastit-php-8.3	shim	libblastit_php83-*-gnu.so	libphp8.3-embed	php-embedded
blastit-php-8.4	shim	libblastit_php84-*-gnu.so	libphp8.4-embed	php-embedded
blastit-php-8.5	shim	libblastit_php85-*-gnu.so	libphp8.5-embed	php-embedded
blastit-php-8.6	shim	libblastit_php86-*-gnu.so	libphp8.6-embed	php-embedded

$ diff <(bash scripts/build-deb.sh --print-set) <(bash packaging/package-set.sh --rows) && echo same-deb
same-deb
$ diff <(bash scripts/build-rpm.sh --print-set) <(bash packaging/package-set.sh --rows) && echo same-rpm
same-rpm

$ bash scripts/py_sapi_leg.sh --minor 3.15 --arch x86_64 --libc gnu
error: --minor '3.15' is not a CPython minor packaging/languages.tsv declares (declared: 3.14 3.13)
(exit 2)
$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu   # validation passes
py-sapi-leg: ok — libblastit_py313-x86_64-gnu.so (x86_64-gnu, CPython 3.13)   (exit 0)
$ bash scripts/py_sapi_leg.sh --minor 3.13 --arch x86_64 --libc gnu --out /proc/blastit-not-writable
mkdir: cannot create directory ‘/proc/blastit-not-writable’: No such file or directory   (exit 1: guard passed, no build)

$ bash scripts/build-musl-wheel.sh 3.15
error: '3.15' is not a CPython minor packaging/languages.tsv declares (declared: 3.14 3.13)

$ make -n py-sapi | grep -o 'for minor in [^;]*'
for minor in 3.14 3.13
$ make -p -n 2>/dev/null | grep '^PY_MINORS'
PY_MINORS = $(shell bash $(CURDIR)/packaging/languages.sh --py-minors | tr '\n' ' ')
$ make -p -n 2>/dev/null | grep '^PHP_MINORS'
PHP_MINORS = $(shell bash $(CURDIR)/packaging/languages.sh --php-minors | tr '\n' ' ')
$ bash packaging/languages.sh --py-minors | tr '\n' ' '
3.14 3.13

$ for s in scripts/build-deb.sh scripts/build-rpm.sh scripts/check-bundle-layout.sh \
           scripts/build-package-bundle.sh scripts/build-standalone.sh \
           scripts/build-standalone-in-image.sh scripts/py_sapi_leg.sh \
           scripts/build-musl-wheel.sh; do bash -n "$s"; done
bash -n ok (all eight)

$ bash -c 'source packaging/package-set.sh; source <(sed -n "/^shim_count_of()/,/^}/p" scripts/check-bundle-layout.sh); ...'
scripts/check-bundle-layout.sh: python=2 php=4
scripts/build-deb.sh: python=2 php=4
scripts/build-package-bundle.sh: python=2 php=4

$ grep -nE "8\.[3-7]|3\.1[3-9]" <the eight scripts> Makefile
scripts/build-standalone.sh:3:# a wheel (`ADR-0010` §4, `ADR-0017` decision 1, node MB.3.18).   <- node id
Makefile:45:# Resolve CPython >= 3.13 via uv ...                                          <- minimum-version floor
Makefile:46:PYO3_BIN := ... '>=3.13' ...                                                 <- same floor
Makefile:286:# ... would hand `newest` a `blastit-python-3.13_…` shim.                    <- one worked example
Makefile:550:E2E_UNIVERSAL_PHP_MINOR ?= 8.4                                              <- one worked example
Makefile:782:PHP_SAPI_IMAGE_MINOR ?= 8.4                                                 <- one worked example
(no bare list of supported minors remains)

$ make fast-check
...
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
sh scripts/test_install.sh
install-test: ok (h) apt installs the core first, each shim alone, and warns instead of aborting on a shim it cannot install
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
(exit 0)
```

## Follow-ups

- `.github/workflows/publish-pypi.yml:394` still spells `make py-sapi PY_MINORS="3.14 3.13"`;
  the four workflows are MB.67.2's uncommitted slice, so it was reported, not edited.
- `ci.yml`/`publish-pypi.yml`/`wheels.yml` still `uv python install 3.13 3.14` and pin
  `PYTHON_VERSION: "3.13"`; those are runner pins, for MB.67.2/MB.67.4 to derive if wanted.
- `Makefile`'s `PYO3_BIN '>=3.13'` is a minimum-version floor for the PyO3 test interpreter,
  not a declared-minor list.

## Workflow follow-up

Derived the supported CPython minor list in the three workflows that still carried it,
inline from `packaging/languages.sh --py-minors` (no second copy). The four workflows
MB.67.2 edited (`packages.yml`, `nightly.yml`, `php-sapi.yml`, `py-sapi.yml`) were not
touched.

- `publish-pypi.yml`: `uv python install 3.13 3.14` →
  `$(bash packaging/languages.sh --py-minors | tr '\n' ' ')`; `make py-sapi` no longer
  passes `PY_MINORS`, so the Makefile's derived default applies.
- `ci.yml`: the three `uv python install …` sites, the `/usr/local/bin` link loop and the
  "image really has no Python" probe order all derive. The probe order is handed to the
  container as `PY_CANDIDATES` (`python python3 python3.14 python3.13`) because that block
  is one single-quoted `sh -c` word and cannot expand runner-side. `PYTHON_VERSION: "3.13"`
  and `python-version: "3.14"` are the PYO3 build interpreter (a tool floor), not the shim
  list; they stay, with a comment saying so.
- `wheels.yml`: `PY_VER=3.13` is the **wheel-build interpreter** — a tool pin, since
  `scripts/build-musl-wheel.sh` now refuses a minor the table does not declare — not a
  runtime smoke list. It stays, with a comment saying exactly that.

Proof (no cargo/make run for this follow-up; another session owns the Rust/Python tree):

```
$ uv run --no-project --offline --with pyyaml python -c '... yaml.safe_load ...'
.github/workflows/ci.yml yaml ok; jobs: 10
.github/workflows/publish-pypi.yml yaml ok; jobs: 15
.github/workflows/wheels.yml yaml ok; jobs: 3

$ grep -nE '3\.1[34]'  .github/workflows/{ci,publish-pypi,wheels}.yml
ci.yml:34             PYTHON_VERSION: "3.13"        tool floor: the PYO3 build interpreter
ci.yml:87             # ... pins 3.13 for PYO3 only, a tool floor   comment on that pin
ci.yml:95             # /usr/local/bin/python3.13 finds nothing      one worked path example
ci.yml:220            name: '§2 apps: ... on 3.13/3.14 ...'        job display name (renaming renames the required check)
ci.yml:281            # libpython3.14.so.1.0 *and* a RUNPATH ...     one worked example of the RUNPATH defect
ci.yml:432            python-version: "3.14"        tool floor: that job's build interpreter
publish-pypi.yml:597  -e PY_VER=3.13                tool pin: the musllinux wheel-build interpreter
publish-pypi.yml:685  uv venv --python 3.13 ...     tool pin: the interpreter the sdist is installed into
wheels.yml:23         # ... python:3.12/3.13/3.14-slim ...         matrix prose (platform_matrix.py owns runtime_images)
wheels.yml:68         name: '§3 pip install in python:3.12/3.13/3.14-slim ...'   matrix display name
wheels.yml:87         name: '§3 pip install in python:3.12/3.13/3.14-alpine ...' same, alpine leg
wheels.yml:112        -e PY_VER=3.13                tool pin: the musllinux wheel-build interpreter (now commented)
(no bare supported-minor list remains: every install loop and probe order derives)

$ bash packaging/languages.sh --py-minors | tr '\n' ' '        -> 3.14 3.13
$ bash packaging/languages.sh --py-minors | sed 's/^/python/' -> python3.14 python3.13
$ make -n py-sapi   (unchanged from the proof above, not re-run)
for minor in 3.14 3.13
```

