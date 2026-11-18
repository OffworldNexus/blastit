---
name: Testing
description: Blastit lab-first automated-testing policy. Covers test tiers (`critical`, `standard`, `docs`, `e2e`), `tools/critical_guard.py`, real `UnixStream::pair()`/`UnixListener` testing (zero `Mock*Server` / `StubPolicy`, zero regex guard scripts), and `cargo-mutants`.
---

# Testing policy

Tests exist to prove two things: **(1) the developer experience works exactly as documented with zero surprises**, and **(2) the security walls and CVE mitigations cannot silently regress**.

## 1. Test tiers & Gate split

| Tier | Location | Runs in | Protects |
|---|---|---|---|
| **critical** (protected) | Rust: `crates/<crate>/tests/critical_*.rs`<br/>Python: `python/tests/critical/test_*.py` | Rust in `make fast-check` (`rs-test`); Rust + Python staged extension in `make check` (`test-critical`) | Security boundaries, secret stripping, SQL/NoSQL/HTTP allowlists & `$1` parameterized table grants, 0-proxy store-less `check`, `blastit dev` vs prod fail-closed behaviour, `--learn` / `check` completeness, CVE payload rejection |
| **standard** | `crates/<crate>/src/**` unit tests, `crates/<crate>/tests/*.rs`, `python/tests/test_*.py` | `make fast-check` & `make check` | Component behaviour, edge cases, CLI terminal craft (`NO_COLOR`, `--version`, `fmt --check` diffs), wire codecs |
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
