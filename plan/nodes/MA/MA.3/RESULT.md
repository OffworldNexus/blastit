# Result MA.3

## Summary

Every documented snippet now runs for real, the docs agree with the shipped `0.1.1` binary, the site renders cleanly, both e2e legs pass, and the release is prepared: `Cargo.toml`/`Cargo.lock` at `0.1.1`, a user-facing CHANGELOG entry, and a wheel whose `METADATA` says `0.1.1`. The audit-driven nodes also fixed three product bugs the docs exposed (watermark measurement, `queries = []` listener, one-pass `dev --learn`) and closed the docs-gate escapes.

## Evidence

Run by `lead` at `4338f88` unless noted:

```console
$ make check
exit 0 — guard, critical-guard, fmt/clippy/doc, workspace tests, 200 Rust critical across 12 suites,
         50 Python critical, docs-unit (46 passed / 57 subtests), 38 fences (26 executed, 12 accounted,
         0 failed), zensical --strict, plan OK
$ make e2e
EXIT=0 — 8x "PASS: 0 failure(s)" (app self-test, Django scenario + logs, SQLAlchemy app + logs,
         rootless self-test, rootless scenario + logs); rootless boot `privileges: capabilities`
         with exactly cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap,
         per-uid views 20001-20003 and 21000-21006 holding nothing + nnp=1, capable copy 0500 refused
         cross-uid; fail-closed `docker run -u 10000 --cap-drop ALL` exits 1 naming each cap
$ grep -n 'version = "0.1.1"' Cargo.toml
6:version = "0.1.1"
$ scripts/check-release-tag.sh v0.1.1 && unzip -p dist/blastit-0.1.1-*.whl '*/METADATA' | head -3
tag v0.1.1 names the workspace version 0.1.1 / Name: blastit / Version: 0.1.1
```

Acceptance mapping:

- **Runnable docs:** `make docs-test` executes the Getting Started, local `blastit dev`/`--learn` and `blastit check` fences against a freshly built wheel, unprivileged and Docker-free, with an inventory that fails on any unaccounted fence/page; `python/tests/runnable_docs/test_harness.py` + `python/tests/test_runnable_docs.py` pin the gate's own checks inside `make check` (`MA.3.9`, `MA.3.10`, `MA.3.13`).
- **Docs↔code agreement:** `MA.3.2` reconciled every claim with the binary after `MA.3.7` fixed the three product gaps the harness exposed.
- **Visual QA:** `visual-checker` PASS across 9 pages × light/dark/mobile, three Mermaid diagrams rendering (`docs/testing/VISUAL-MA.3.md`).
- **e2e + wheel:** `make e2e` green; wheel `METADATA` `Version: 0.1.1`.
- **Release prep:** `Cargo.toml`/`Cargo.lock` `0.1.1`, `CHANGELOG.md` opens with `## 0.1.1 (2026-11-12)`.

## Files

`Makefile`, `.github/workflows/{ci,docs}.yml`, `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `README.md`, `ARCHITECTURE.md`, `docs/**`, `docs/testing/{AUDIT-MA.3.md,VISUAL-MA.3.md,README.md}`, `python/tests/runnable_docs/**`, `python/tests/test_runnable_docs.py`, `crates/blastit/src/{mem_usage.rs,stage0.rs,stage1.rs,dev_learn.rs,config.rs}`, `crates/blastit-sqlpolicy/tests/critical_learn_policy.rs`, `crates/blastit/tests/critical_dev_and_fail_closed.rs`.

## Follow-ups

- **`cargo-mutants` is still absent (`H-002`)** — the milestone gate's literal `make mutants-rust PKG=blastit-manifest|blastit-sqlpolicy` evidence cannot be produced until it is installed. Anchored fallbacks: manifest 8/8, sqlpolicy 10/11 before `MA.3.11`, 12/12 after.
- `tools/critical_guard.py` has no enforce/union mode: a PR that adds or edits a task node in the same commit can self-authorise a protected-test change, one node cannot authorise two protected files, and an `exit 2` from one node is masked by a later `exit 0`. Needs a follow-up node (`tools/**` + `ci.yml`).
- `scripts/build-wheel.sh` does not run `scripts/readme-absolute-links.py`, so the wheel's long description check only passes after the publish workflow's rewrite; fold the rewrite into the script.
- `CARGO_NET_OFFLINE=1` is accepted by cargo but rejected by maturin ≥1.16 (it wants `true`); spell `=true` in design R42 and acceptance lines.
- `e2e/` has no fixture for the documented `queries = []` denying listener, and no permanent negative-boot job.
- One flake seen once: `test_check_contract.py::…learn_refuses_an_inline_literal…` (`server.statements []`), green on rerun.
- Brand logo `brand/logo.svg` uses `#fbfaf6`, low contrast on the light scheme.
