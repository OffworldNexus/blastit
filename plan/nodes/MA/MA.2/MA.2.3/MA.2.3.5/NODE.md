---
id: MA.2.3.5
title: "Leaf 12: check orchestrator, WSGI gate and python check tests"
kind: task
status: done
owner: expert
priority: 2
depends_on: [MA.2.3.1, MA.2.3.4]
paths:
  - crates/blastit/src/lib.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/check.rs
  - python/blastit/_check_pytest.py
  - python/blastit/pytest_plugin.py
  - python/tests/test_check.py
  - python/tests/test_live_server.py
  - python/tests/test_plugin_helpers.py
  - "python/tests/critical/**"
attempts: 1
max_attempts: 4
escalation: expert
created: "2026-11-12T13:12"
updated: "2026-11-12T20:37"
---

# Goal

Close `MA.2` with the `blastit check` orchestrator: spawn `pytest -q -p no:blastit -p blastit._check_pytest` unprivileged, redirect the child's fd 1 onto stderr for JSON purity, run a hermetic `dbproxy` with secret-free `swapped_env`, map exit codes `0/1/2/3` including pytest `5`, and install the `ContextVar` `BlastitWsgiGate` plus the real-suite and contract tests.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §1.1/§1.3, §4.2 D.2/E.1, §7.2 row `R4..R5, R30, R39..R42`, §8 Leaf 12; `ADR-0004`; requirements `R3`, `R4`, `R5`, `R30`, `R31` (replay wiring), `R39`.
- `crates/blastit/src/check.rs` owns the transaction: build config + manifest, resolve `Host::dbproxy()` (fallback `current_exe().with_file_name("blastit-dbproxy")`), stage the runtime dir, spawn pytest with fd 1 -> fd 2, wait, read `CheckObsV1`, evaluate, print human or one JSON document, and write the manifest only when pytest exited `0`, `obs.http.len() > 0`, zero refusals, `!dry_run`, and `(proposed.routes > 0 || proposed.queries > 0)`.
- `python/blastit/_check_pytest.py` pushes exactly one `ObsTarget` per request over `ctl.sock` (`ContextVar` guard), caps samples at 64 KiB, and raises so a control failure maps to exit `3`. `blastit_live_server` spawns `blastit dev` when `geteuid() != 0`.
- Load skills `rust-standards`, `python-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo build --locked -p blastit-dbproxy -p blastit-py
$ cargo test -p blastit --locked
$ uv run --no-project pytest python/tests/critical -q
$ uv run --no-project pytest python/tests/test_check.py -q
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
```

- `python/tests/critical/test_check_contract.py` is **new**: a 25-line mini-runner loads `-p blastit._check_pytest` while `-p no:blastit` blocks the `pyproject.toml` `0.1.0` plugin, drives a raw-WSGI + Flask-like `.wsgi_app` through a real `blastit-dbproxy` + `MockPgServer` -> `exit 0`, `requests == 1`, `findings == []`, exactly one JSON document despite `print()` / `os.write(1, b"noise")`.
- Negative twins: bad manifest or corrupt `http_obs.jsonl` -> `exit 2` with `stdout == b""`; pytest `1` and `5` -> `exit 3` with `stdout == b""`; the real DB password is absent from the non-Django `os.environ`.
- `python/tests/test_check.py` is the real pytest + PostgreSQL suite (runs in PG CI); it must not be silently skipped in `make check`.

# Notes

- ~420 changed lines. Depends on `MA.2.3.1` (pytest_plugin/`Makefile`) and `MA.2.3.4` (audit engine); keep `python/blastit/_check_pytest.py` new rather than editing `fold.py`.
- Do not edit `docs/**`; `docs/ci.md` truthfulness sync is `MA.3`.
