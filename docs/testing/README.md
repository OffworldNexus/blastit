# Testing in this repository

The automated tiers are `cargo test` / `pytest` for code, and the docs gates for
everything a page promises.

## Test curation audits

Fresh-context test-suite audits (`AUDIT-<scope>-<YYYY-MM-DD>.md`) written by `test-curator` at
epic and milestone gates (skill `testing`).

## Gates

| Command | What it runs | What it proves |
|---|---|---|
| `make fast-check` | `guard`, `rs-fmt`, `rs-lint`, `rs-doc`, `rs-test`, `py-lint`, `plan-validate`, `universal-smoke` | Formatting, `clippy -D warnings`, rustdoc, every Rust unit/integration/doc test (including `crates/*/tests/critical_*.rs`), Python lint, plan consistency, and the debug binary's portable-ELF shape |
| `make check` | `fast-check` + `test-critical`, `docs-unit`, `docs-check`, `rs-no-python`, `rs-no-default`, `e2e-universal`, `zed-check` | The epic/release gate: staged Python critical suites, the offline package suites, the strict docs build, the standalone artifact's ELF and dependency graph, one bundle across four runtimes, and the Zed extension |
| `make docs-check` | `uvx --offline zensical build --strict` | Zero broken links/anchors, and every Markdown page `exclude_docs` does not keep out is named by the nav (an unpublished orphan is a page no review read) |
| `make docs-unit` | `python/tests/test_check_pytest_gate.py`, `python/tests/test_launcher.py`, plus its `install-sh-syntax` prerequisite (`sh -n`/`dash -n` on `docs/install.sh`) | The package's offline suites: the `check` exit-code contract and the project-binary handoff, and that the installer still parses as POSIX `sh`. No wheel build, no network, no root |
| `make e2e` (`e2e/run_apps.py`, `--legs wsgi`/`php`/`static`) | `§2`: real PHP 8.4 + `libphp`, Django and Flask on CPython 3.13/3.14, a static site in a container with neither interpreter | Per-view SQL grants, the five refusals, `exit()`/fatal recovery, `php2shell` neutrality and the static mount's boundary. **PR** (`ci.yml`), and the WSGI family in `py-sapi.yml` |
| `make e2e-prod` (`e2e/run_prod.py`) | `§6`: both deployment presets as container root and as a capable service user beside `postgres:18`/Traefik/PgBouncer | Per-view uids ≥ 20000, zero capabilities, the `/proc` credential scan, one grant per view, fair queuing and `SIGTERM` drain. **Nightly** (`nightly.yml`) — an image build per leg, never on a PR |
| `make e2e-packaging` (`e2e/run_packaging.py`) | `§3`: `.deb`/`.rpm` installs, `install.sh --user`, the Alpine musl install, the `py3-none` wheel and the Composer wrapper | Each artifact installs where the story says and answers `--version`/`fmt`/`check` (plus a served WSGI view through the shipped shim). **Tags**: `packages.yml`, `wheels.yml`, `publish-pypi.yml`; the wheel also in `ci.yml` |
| `make php-sapi-e2e`, `make rs-test-php-image` | The real-load PHP probe and the boot-level `e2e_dev` view legs, in the leg's own distro image | `§2`'s PHP 8.3–8.6 × `x86_64`/`aarch64` × `gnu`/`musl` matrix. **PR** (`php-sapi.yml`), a hard failure when a leg skips |

## Where the story suites run

| Workflow | What it proves | When |
|---|---|---|
| `ci.yml` | `§2` applications (PHP 8.4, Django/Flask on 3.13/3.14, static), plus lint, the Rust suites, the R42 guard, one root-only assertion, `blastit-lsp` and the wheel | every PR; `develop` |
| `php-sapi.yml` | `§2` PHP 8.3, 8.4, 8.5 and 8.6 on `x86_64`+`aarch64`, `gnu`+`musl`, with the boot-level view legs | every PR; `develop` |
| `py-sapi.yml` | `§2` the per-minor CPython shims and the Django/Flask WSGI legs they serve | every PR; `develop` |
| `wheels.yml`, `packages.yml` | `§3` the wheel legs, the `.deb`/`.rpm` installs, `install.sh --user` and the Composer wrapper | `wheels.yml`: `develop`; `packages.yml`: tags + dispatch |
| `publish-pypi.yml` | `§3` the user-space install stories against the release archives; PyPI + the release assets | release PR + release |
| `nightly.yml` | `§3` the nightly packages and `§6` both production presets | nightly + dispatch |
| `docs.yml` | the published pages, the nav/orphan check and the served `install.sh` | every PR; `develop`; tags |

## Writing a test

- **Story-bound:** every test names the `TESTING_PLAN.md` story it proves and runs
  end-to-end against the real binary; non-E2E tests exist only for that plan's §7
  surfaces (`Blastfile` syntax/formatting, SQL grant matching, HTTP framing, LSP, Zed).
- **Real collaborators:** `UnixStream::pair()` / `UnixListener` / real wire frames — no
  `Mock*Server`, `StubPolicy` or `AlwaysAllowPolicy`, and no single-implementation mock
  traits.
- **Both directions:** every security test exercises the allowed path (including `$1`
  parameterized grants and 0-proxy store-less manifests) *and* the denied one (SQLi,
  ungranted columns, `php2shell` `%ADd`, `..`/`.env` traversal), with zero secret leaks
  in the error text.
- **Invariants in types:** crate boundaries, module visibility and the type system
  enforce architecture. No source-grep regex scripts over `.rs`/`.md` files.

## Critical suites

`crates/*/tests/critical_*.rs` and `python/tests/critical/test_*.py` hold the security
walls and the CVE regressions. Adding new ones is always welcome; modifying, deleting or
weakening an existing one needs the plan node's `paths` and a `# Goal` that says why.
`make critical-skip-check` refuses `#[ignore]`, `pytest.mark.skip`, `xfail` and
`unittest.skip` in them.

## Mutation testing

At gates touching security-critical policy crates, run
`TMPDIR=$HOME/.cache/blastit-mutants cargo mutants -p <crate>` (disk scratch, never
tmpfs `/tmp`) and target **>= 75 %** killed on the policy/validation modules.
