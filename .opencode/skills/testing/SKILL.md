---
name: Testing
description: Blastit lab-first automated-testing policy. Covers test tiers (`critical`, `standard`, `docs`, `e2e`), `tools/critical_guard.py`, real `UnixStream::pair()`/`UnixListener` testing (zero `Mock*Server` / `StubPolicy`, zero regex guard scripts), and `cargo-mutants`.
---

# Testing policy

Tests exist to prove two things: **(1) the developer experience works exactly as documented with zero surprises**, and **(2) the security walls and CVE mitigations cannot silently regress**.

## 0. `TESTING_PLAN.md` is the source of truth (human-gated)

- Every test maps to exactly one user story there and runs **end-to-end against the real binary** by default; non-E2E tests exist only for `TESTING_PLAN.md` §7's focused surfaces (`Blastfile` syntax/formatting, SQL grant matching, HTTP framing, LSP, Zed). Zero pointless unit tests, meta-tests, YAML/bash/prose linters, or micro-tests.
- New user-facing feature/runtime/proxy/workflow: propose the pure user-story update (zero file/crate/code references), `human add` + `set <id> blocked`, and proceed only after validation; no slice silently edits `TESTING_PLAN.md`.
- **Reviewers reject** tests with no story, unit tests duplicating E2E coverage, and broken/vacuous oracles (pass when skipped, assert exit codes/log substrings instead of real HTTP/SQL/sandbox state, test helper artefacts). `review-arch` runs `make coverage TESTS="<story tests>" FILES="<changed crates/ files>"` on changed `crates/` files only (Python: judged by story E2E test): dead/speculative uncovered code → demand **deletion**; needed-but-untested story/boundary → demand the **E2E test be extended**. A `0.0%` row is not dead code — an `env_clear()` spawn hides `.profraw`, so confirm the bin object is included first.

## 1. Test tiers & Gate split

| Tier | Location | Runs in | Protects |
|---|---|---|---|
| **critical** (protected) | Rust: `crates/<crate>/tests/critical_*.rs`<br/>Python: `python/tests/critical/test_*.py` | Rust in `make fast-check` (`rs-test`); Rust + Python staged extension in `make check` (`test-critical`) | Security boundaries, secret stripping, SQL/NoSQL/HTTP allowlists & `$1` parameterized table grants, 0-proxy store-less `check`, `blastit dev` vs prod fail-closed behaviour, `--learn` / `check` completeness, CVE payload rejection |
| **standard** | §7-surface component/edge-case tests (§0 governs: no new `src/**` micro-tests) + `crates/<crate>/tests/*.rs` story tests, `python/tests/test_*.py` | `make fast-check` & `make check` | Component behaviour, edge cases, CLI terminal craft (`NO_COLOR`, `--version`, `fmt --check` diffs), wire codecs |
| **docs** | `make docs-check` (`zensical build --strict`) + `make docs-test` runnable fences | `make docs-check` on doc slices; full `make docs-test` once in `make check` at gates | Zero broken links/anchors, valid `Blastfile` & CLI examples in `docs/` & `README.md` |
| **e2e & CVE** | `e2e/` (Docker, rootless capabilities, PHP SAPIshimming, CVE harnesses) | Epic & milestone gates (`make e2e`, `make php-sapi-e2e`) | Real multi-UID / rootless isolation, real PostgreSQL/PHP/static stacks, `php2shell` / `node2shell` / polyglot neutralization |

## 2. Protected critical tests (`tools/critical_guard.py`)

- Adding new files under `crates/*/tests/critical_*.rs` or `python/tests/critical/` is **always allowed**.
- Modifying or deleting an existing critical test file is **forbidden** unless the plan node's `paths` explicitly cover that file **and** `# Goal` explains why. `uv run --no-project tools/critical_guard.py --base <sha> --node <id>` enforces this mechanically.
- Never add `#[ignore]`, `pytest.mark.skip`, `xfail`, `unittest.skip`, or weaken an assertion (`make critical-skip-check` enforces zero skips).

## 3. Lab-First Rules & Anti-Fluff / Anti-Guard-Bloat (`review-arch` enforces)

- **Real sockets & wire frames, ZERO fake test doubles:** Drive real components (`blastit-dbproxy`, `blastit-sqlpolicy`, `blastit-scheduler`, `blastit-view-*`) over `UnixStream::pair()`, `UnixListener`, or in-memory channels — **never** write `MockPgServer`, `StubPolicy`, `AlwaysAllowPolicy`, or single-implementation mock traits (`RequestIo`, `Acceptor`, `Clock`).
- **Both Allowed AND Denied/Adversarial cases:** Every security/policy/CLI check test **must** exercise both the **allowed** path (including `$1` parameterized table/column grants and 0-proxy store-less manifests) and the **denied/adversarial** path (SQLi, ungranted columns, multi-statement, `php2shell` `%ADd`, `..`/`.env` path traversal), verifying zero secret leaks in error output.
- **Zero `scripts/check_*.py` source-grep regex guards:** Enforce architectural invariants with Rust crate dependency graphs, module visibility, and `#![forbid(unsafe_code)]` — **never** add Python/shell regex scanners over `.rs` or `.md` source text to `Makefile`.
- **Consolidate CLI assertions:** Test `--help`, `--version`, bad flags, and unknown `--preset` exits coherently rather than spawning 10 redundant subprocesses for one-line substring checks.

## 4. Mutation testing (`make mutants-rust` at gates)

At epic and milestone gates touching security-critical policy crates (`blastit-manifest`, `blastit-sqlpolicy`, `blastit-env`, `blastit-php-sapi`, and upcoming `blastit-ingress`, `blastit-wasm`, `blastit-proxy-*`), run `make mutants-rust PKG=<crate>` (disk scratch + 12 GiB cgroup cap, target **>= 75 %** killed on policy/validation modules) and close meaningful non-equivalent survivors in coarse grouped slices (never 1-node-per-mutant sprawl).
