# Result MA.5

## Summary

Authored the normative `0.1.1` technical design (`docs/design/ma-lean-dx.md`, requirements `R1..R42`) and six 1-page ADRs (`docs/decisions/ADR-0001..0006-*.md`, `Status: Proposed`) translating the accepted `MA.1` user documentation into exact Rust crate/trait contracts, wire protocols, security invariants, test oracles, and a 12-leaf (`<= 450` lines/leaf) implementation plan for `MA.2`.

Ran two rounds of parallel fresh-context goldfish critic reviews across `implementability`, `testability`, and `consistency` (`docs/design/ma-lean-dx.review-{implementability,testability,consistency}.md`) and resolved every `BLOCKER`, `MAJOR`, and `MINOR` finding in `## 9. Review Resolution` and the companion ADRs.

## Evidence

### 1. Design doc (`R1..R42`), 6 ADRs, and 3 critic artefacts exist

```console
$ ls -1 docs/design/ma-lean-dx* docs/decisions/ADR-*.md
docs/decisions/ADR-0001-unprivileged-dev-simulation.md
docs/decisions/ADR-0002-manifest-discovery-and-precedence.md
docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md
docs/decisions/ADR-0004-framework-agnostic-check-and-json-contract.md
docs/decisions/ADR-0005-deployment-presets-and-seccomp-gate.md
docs/decisions/ADR-0006-route-ingress-body-gate.md
docs/design/ma-lean-dx.md
docs/design/ma-lean-dx.review-consistency.md
docs/design/ma-lean-dx.review-implementability.md
docs/design/ma-lean-dx.review-testability.md
```

### 2. `make docs-check` (`uvx --offline zensical build --strict`)

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
```

### 3. `make check` (full workspace gate)

```console
$ make check
...
no Rust critical tests yet (crates/*/tests/critical_*.rs)
no Python critical tests yet (python/tests/critical/)
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
uv run --no-project tools/plan.py validate
plan OK
```

### 4. Zero production code or public user docs modified

```console
$ git status --short | grep -v '^.. plan/'
 M docs/decisions/README.md
 M docs/design/README.md
?? docs/decisions/ADR-0001-unprivileged-dev-simulation.md
?? docs/decisions/ADR-0002-manifest-discovery-and-precedence.md
?? docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md
?? docs/decisions/ADR-0004-framework-agnostic-check-and-json-contract.md
?? docs/decisions/ADR-0005-deployment-presets-and-seccomp-gate.md
?? docs/decisions/ADR-0006-route-ingress-body-gate.md
?? docs/design/ma-lean-dx.md
?? docs/design/ma-lean-dx.review-consistency.md
?? docs/design/ma-lean-dx.review-implementability.md
?? docs/design/ma-lean-dx.review-testability.md
```

## Files

- `docs/design/ma-lean-dx.md`
- `docs/design/README.md`
- `docs/design/ma-lean-dx.review-implementability.md`
- `docs/design/ma-lean-dx.review-testability.md`
- `docs/design/ma-lean-dx.review-consistency.md`
- `docs/decisions/ADR-0001-unprivileged-dev-simulation.md`
- `docs/decisions/ADR-0002-manifest-discovery-and-precedence.md`
- `docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md`
- `docs/decisions/ADR-0004-framework-agnostic-check-and-json-contract.md`
- `docs/decisions/ADR-0005-deployment-presets-and-seccomp-gate.md`
- `docs/decisions/ADR-0006-route-ingress-body-gate.md`
- `docs/decisions/README.md`

## Follow-ups

### Ordered `MA.2` Leaf Breakdown (for `planner` on `MA.2`, all `<= 450` lines; see `docs/design/ma-lean-dx.md` §8)
1. `workspace-seams-and-cargo-wiring` (sequential foundation owning `Cargo.toml` / `Cargo.lock` + `blastit-ingress` stub + `stage0`/`stage1` constructor shims)
2. `ingress-codecs-and-canonical-reencode` (`crates/blastit-ingress/**`, `critical_ingress.rs`)
3. `sqlpolicy-literal-redaction-and-learnable` (`crates/blastit-sqlpolicy/**`, `critical_literals.rs` — parallel with #2)
4. `manifest-011-schema-and-warnings` (`crates/blastit-manifest/{src,tests}/**`, `critical_manifest_011.rs` + standard `docs_fences.rs`)
5. `manifest-surgical-editor-and-atomic-write` (`crates/blastit-manifest/src/edit.rs`, `critical_manifest_edit.rs`)
6. `http-early-abort-and-scheduler-ingress` (`crates/blastit-{http,scheduler}/**`, `critical_early_abort.rs`, `critical_ingress_enforcement.rs`)
7. `dbproxy-learn-redaction-and-dynamic-check` (`crates/blastit-dbproxy/**`, `critical_learn_and_redaction.rs` — parallel with #6)
8. `python-ruff-clean-makefile-and-fold` (`Makefile`, `python/**`, `test_fold_literals_and_atomic.py` — parallel with #6 & #7)
9. `cli-config-discovery-and-presets` (`crates/blastit-events/**`, `crates/blastit/src/{cli,config}.rs`, `critical_config_and_presets.rs`)
10. `supervisor-unprivileged-dev-and-learn` (`crates/blastit/src/{stage0,stage1,child,zygote,supervise,dev_learn}.rs`, `critical_dev_and_fail_closed.rs`)
11. `check-audit-and-learn-engine` (`crates/blastit/src/{audit_v1,infer_routes}.rs`, `crates/blastit-py/**`, `critical_check_engine.rs`, `critical_check_schema.rs`)
12. `check-orchestrator-wsgi-gate-and-python-tests` (`crates/blastit/src/check.rs`, `python/blastit/{_check_pytest,pytest_plugin}.py`, `test_check_contract.py`, `test_check.py`)

### Out-of-Scope Doc & CI Sync Items (for `MA.3` & Post-`MA`)
- **`MA.3` (`docs/**`, `ARCHITECTURE.md`):** replace illustrative `SELECT …` in `docs/reference/manifest.md:48` with a concrete column list; clarify `docs/local.md:114` (`…` must be parameterised before granting) and `docs/reference/manifest.md:188` (`KNOWN_STATIC_SETUP_SQL` timezone strings); note exit `3` empty stdout and group/`observe` `fix` details in `docs/ci.md:83-99`; clarify `config` `boot` line in `docs/deploy/index.md:63`; sync `ARCHITECTURE.md` §§6, 8.1, 8.2 and document unprivileged `blastit_live_server`.
- **Post-`MA` (`.github/workflows/**`, `tools/**`):** add `blastit check --json` CI workflow step in `.github/workflows/ci.yml` and mechanical critical-skip check in `tools/guard.py`.
