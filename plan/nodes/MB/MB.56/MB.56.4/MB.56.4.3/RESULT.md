# MB.56.4.3 — `e2e/run_packaging.py`: deb/rpm/install.sh/wheel/composer

## What changed

`TESTING_PLAN.md` §3 now has one runner, `e2e/run_packaging.py`, with one leg per
install story: `install` (`docs/install.sh --user`, zero `sudo`), `alpine`
(`install.sh --user` unprivileged on `alpine:3.21` + musl + a tampered archive),
`deb` and `rpm` (`install.sh` with `BLASTIT_DEB`/`BLASTIT_RPM` inside
`debian:trixie` / `rockylinux:9`), `wheel` (the universal `py3-none` wheel with
`pip` in `python:3.12/3.13/3.14-slim`) and `composer` (`packages/composer-blastit`
in `composer:2`). Every leg installs the artifact and then runs the story's own
commands *inside the target*: `blastit --version`, `blastit fmt`,
`blastit fmt --check`, `blastit check`, `blastit-lsp --version` where the artifact
ships it, plus a served page (`--modes "install static wsgi"` also imports and
serves the WSGI fixture through the package's own `python/blastit`).

It reuses the `Skip`/`LegFailure`/`http` helpers `run_apps.py` and `run_prod.py`
already share from `universal_binary.py` (same summary, same port pool from
`28080`, same scratch root style), so this is one harness style, not a third.

Fixtures moved to `e2e/packaging/{fixture,fixture-wsgi}`; the static fixture's
page now names the tests that serve it, and the WSGI fixture is unchanged.

## Old → new map

| Was | Now |
|---|---|
| `scripts/test_packages.sh` (Docker legs; `.deb` payload/ownership, static + WSGI) | `e2e/run_packaging.py --leg deb/--leg rpm` (`--image`, `--modes`); the payload (§6.1: 2 entry points, 13 paths, `root:root`), the four commands and the served page all come from inside the image |
| `scripts/composer_smoke.sh` (4 proofs in `composer:2`) | `e2e/run_packaging.py --leg composer` (installed, default origin + warm run, tampered archive, origin without a digest, `pending` table) |
| `e2e/venv-smoke.sh` + `e2e/venv-smoke/*` (wheel in `modelw/base`, venv layout, `/proc` refusals) | `e2e/run_packaging.py --leg wheel` (the wheel installed with `pip` into the three supported CPythons: CLI/`check`/`fmt` on 3.12, WSGI through the shipped shim on 3.13/3.14). The `modelw/base` venv-ownership and dbproxy-refusal stories were `MB.49`-era boot refusals, now owned by `crates/blastit`'s production suites |
| `e2e/prebuilt_wheel.py` (which CPython a prebuilt image installs for) | gone: the wheel leg installs the one wheel into each minor explicitly, so nothing has to read a `cp3NN` tag |
| `e2e/packages/fixture{,-wsgi}` | `e2e/packaging/fixture{,-wsgi}` |
| `scripts/install_matrix.sh` → `scripts/test_packages.sh` | `scripts/install_matrix.sh` → `uv run --no-project e2e/run_packaging.py --leg deb|rpm --image <matrix image> --modes "<matrix modes>"` |
| `make packages-test`, `make composer-smoke` | both call the new runner; `make e2e-packaging` runs every leg |
| `e2e/php-sapi/run-two-package.sh`, `run-unsupported-minor.sh` (+ their `Dockerfile.*`) | deleted: the two-package discovery order is pinned by `stage0.rs`'s unit test and the probe order by `crates/blastit/tests/common/libphp_probes.rs`/`e2e_dev`, and the 8.2 refusal is a `Production` boot refusal in `crates/blastit`'s suites. Nothing in the tree built or referenced these images |

`e2e/universal_binary.py` **stays**: `make check` runs it as `e2e-universal` (one
staged bundle across CPython 3.13/3.14, PHP, static, handoff), and it is also
where the three e2e drivers import `Skip`/`LegFailure`/`http` from. It earns its
place; it was not a packaging runner.

## Drops (files)

- `scripts/test_packages.sh`, `scripts/composer_smoke.sh`
- `e2e/prebuilt_wheel.py`, `e2e/venv-smoke.sh`, `e2e/venv-smoke/` (4 files)
- `e2e/packages/` (5 fixture files, relocated)
- `e2e/php-sapi/{Dockerfile.two-package,Dockerfile.unsupported-minor}{,.dockerignore}`, `run-two-package.sh`, `run-unsupported-minor.sh`

`e2e/php-sapi/Dockerfile` and `run-probe.sh` stay: `make php-sapi-e2e` and
`php_sapi_probe.py` drive them.

## Boundaries this suite asserts

- **Fail closed with a sentence, never a traceback:** `install.sh --user` against
  a tampered tarball prints `sha256 mismatch for … — refusing to install it` and
  leaves no `bin/blastit`; an overridden origin with no digest prints
  `an overridden release origin is unsigned` and installs nothing. Alpine proves
  the same tamper refusal as a non-root uid.
- **The artifact, not the host:** every `--version`/`fmt`/`check` assertion runs
  inside the target (`docker exec`), and the wheel/package legs serve a page from
  the installed artifact.
- **Hygiene:** containers are `blastit-pkg-*`, ports come from `28080`, scratch is
  `${TMPDIR:-/tmp/opencode}/blastit-pkg`, and each `Container`/`Serving` removes
  what it created (`docker rm -f`, server shutdown) even on failure. No image is
  built, so nothing is left to `rmi`.

## Evidence

```
$ uv run --no-project e2e/run_packaging.py
PASS  binary                …/target/release/blastit (16903664 bytes) says blastit 0.2.0; lsp …/target/release/blastit-lsp
PASS  composer              composer wrapper (vendor/bin/blastit)
PASS  install               install.sh --user
  note alpine: the archive holds a stand-in (no musl build on this host); build one with `scripts/build-standalone.sh --target x86_64-unknown-linux-musl` to run the real fmt/check there
PASS  alpine                alpine:3.21 non-root (fixture archive)
SKIP  wheel                 no wheel in target/packages/ or dist/: build it with `make wheel-manylinux`
SKIP  deb                   no .deb in target/packages/ or dist/: build it with `make packages`
SKIP  rpm                   no .rpm in target/packages/ or dist/: build it with `make packages`

4 passed, 0 failed, 3 skipped (a SKIP names the artifact it needs)
```

The `deb` leg was run for real locally against a throwaway `.deb` staged from
`target/release/{blastit,blastit-lsp}`, `python/blastit/*.py` and the built
shims (`dpkg-deb --root-owner-group`, the §6.1 layout), in both modes:

```
$ uv run --no-project e2e/run_packaging.py --leg deb --deb /tmp/opencode/blastit_0.2.0_amd64.deb --modes "install static wsgi"
PASS  deb                   deb (debian:trixie)
1 passed, 0 failed, 0 skipped (a SKIP names the artifact it needs)
```

That run proved `install.sh` → `dpkg`, the 13-path payload and `root:root`
ownership, `blastit --version`/`blastit-lsp --version`, `fmt`, `fmt --check`,
`check`, the served page, and (in `wsgi` mode) `apt-get install libpython3.13`
plus the packaged-gate `check` and the WSGI view through the shipped shim. The
throwaway `.deb` was not committed; `dist/` is empty here and the leg SKIPs.

`make fast-check`:

```
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s

sh scripts/test_install.sh
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.0 (fixture))
install-test: ok (b) a wrong digest exits 1 and installs nothing
install-test: ok (c) an overridden origin without a digest fails closed with zero requests
install-test: ok (d) an archive with a symlink member is refused before unpacking
install-test: ok (e) --help exits 0 with env -i (no root, no network)
install-test: ok (f) usage errors exit 2 (bad mirror, --nightly --user, path-shaped --version)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
install-test: all cases passed
```

Notes for the reviewer:

- The wheel and rpm legs were not exercised locally (no wheel/`.rpm` built here);
  they SKIP with the exact build command. The `.deb` leg was proven for real in
  both modes (above). `wheel-smoke` in `ci.yml` now runs the wheel leg, and
  `packages.yml`'s install matrix now runs the deb/rpm legs — both build their
  artifact first, so the code paths run there.
- The Alpine leg runs the *installer* for real against the musl target; without a
  musl build on the host the archive is a stand-in whose `--version` says so. Pass
  `--musl-binary`/`--musl-archive` to make `fmt`/`check` real there too.

## Follow-ups

- `docs/design/universal-distribution.md` (the `wheels` row) and
  `docs/knowledge/LESSONS.md` (the `COPY dist/` bullet) still name the deleted
  `e2e/prebuilt_wheel.py`; the wheel is now installed into each CPython by
  `e2e/run_packaging.py --leg wheel`. Out of this node's paths (`docs/**`).
- `scripts/test_install.sh` (in `make fast-check`) still owns the installer's
  refusal matrix (wrong digest, symlink member, usage errors) with a fixture
  binary; the new E2E leg proves the same refusals with a real release archive,
  so the two should eventually fold the fast cases into the one suite.
- `platform_matrix.py`'s install `modes` still says which images may run the WSGI
  gate; the package leg honours it (`--modes`), and the wheel leg covers WSGI on
  3.13/3.14, so the field can shrink to `install static` if the WSGI-through-package
  proof there ever stops earning its network `apt-get`.
