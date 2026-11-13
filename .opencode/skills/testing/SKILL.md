---
name: Testing
description: Blastit automated-testing policy. Covers test tiers (`critical`, `standard`, `e2e`, `cve`, `docs`), protected critical tests (`tools/critical_guard.py`), runnable documentation snippets, anti-fluff rules, and `cargo-mutants` audits.
---

# Testing policy

Tests exist to prove two things: **(1) the developer experience works exactly as documented with zero surprises**, and **(2) the security walls and CVE mitigations cannot silently regress**. Cheap models left unchecked write tautological fluff; reviewers and the `test-curator` reject fluff.

## 1. Test tiers

| Tier | Location | Runs when | Protects |
|---|---|---|---|
| **critical** (protected) | Rust: `crates/<crate>/tests/critical_*.rs`<br/>Python: `python/tests/critical/test_*.py` | Every `make check` (< 60 s total, unprivileged!) | Security boundaries, secret stripping, SQL/NoSQL/HTTP allowlist & parser-differential resistance, WASM fuel/memory budgets, CDR metadata/script stripping, `blastit dev` vs prod fail-closed behaviour, `--learn` / `check` completeness, CVE payload rejection |
| **standard** | `crates/<crate>/src/**` unit tests, `crates/<crate>/tests/*.rs`, `python/tests/test_*.py` | Every `make check` (unprivileged) | Component behaviour, edge cases, CLI formatting, wire codecs |
| **docs** | `make docs-check` (`zensical build --strict`) + runnable doc snippet tests | Every `make check` | Zero broken links/anchors, valid `blastit.toml` / CLI examples in `docs/` & `README.md` |
| **e2e & CVE** | `e2e/` (Docker, rootless capabilities, backend stacks, CVE exploit harnesses) | Epic & milestone gates (`make e2e`) and tasks touching `crates/blastit`, Dockerfiles, or new backends/proxies | Real multi-UID / rootless isolation, real PostgreSQL/MySQL/Mongo/Redis/HTTP brokers, real `php2shell` / `node2shell` / polyglot exploit neutralisation |

## 2. Protected critical tests (`tools/critical_guard.py`)

- Adding new files under `crates/*/tests/critical_*.rs` or `python/tests/critical/` is **always allowed**.
- Modifying or deleting an existing critical test file is **forbidden** unless the plan node's `paths` explicitly cover that file **and** `# Goal` explains why. `uv run --no-project tools/critical_guard.py --base <sha> --node <id>` enforces this mechanically; `reviewer` auto-FAILs violations.
- Never add `#[ignore]`, `pytest.mark.skip`, `xfail`, or weaken an assertion to get green.

## 3. What counts as fluff (reject in review, delete in curation)

- Asserting getters/constructors/serde derive defaults on trivial structs.
- Mocking our own functions instead of running the real parser/policy/broker over an in-memory socketpair or channel.
- Tests that would still pass if an allowlist check returned `Ok(())` unconditionally! Every security/policy test **must** assert both the **allowed** case and the **denied** case (including adversarial bypass attempts: multi-statement, comments, casing, duplicate JSON keys, oversized WASM fuel/memory, forged headers).

## 4. Mutation testing (`test-curator` at gates)

At epic and milestone gates touching security-critical crates (`blastit-manifest`, `blastit-sqlpolicy`, `blastit-env`, and future `blastit-ingress`, `blastit-wasm`, `blastit-fastcgi`, `blastit-proxy-*`), `test-curator` runs `make mutants-rust PKG=<crate>` (target **≥ 75 %** killed on policy/validation modules) and files follow-up plan tasks for any meaningful surviving mutant.
