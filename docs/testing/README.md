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
| `make docs-unit` | `python/tests/test_check_pytest_gate.py`, `python/tests/test_launcher.py` | The package's offline suites: the `check` exit-code contract and the project-binary handoff. No wheel build, no network, no root |
| `make e2e`, `make php-sapi-e2e` | Docker images and the real-load PHP probe | Multi-UID/rootless isolation and real PHP/PostgreSQL/static stacks. Gates only — never inside `fast-check` |

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
