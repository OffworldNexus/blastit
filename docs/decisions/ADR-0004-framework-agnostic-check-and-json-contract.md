# ADR-0004: Framework-Agnostic `blastit check` Architecture and `--json` Machine Contract

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

Both human developers in CI and AI coding agents maintaining `blastit.toml` need one fast, unprivileged command (`blastit check`, aliased as `blastit test`) that runs `pytest -q` against any WSGI application (Django, Flask, SQLAlchemy, raw WSGI) and verifies 4-way manifest completeness without root, without failing on test-fixture/migration SQL, and with a deterministic machine-readable `--json` output contract.

## Decision

1. **Rust-orchestrated `blastit-dbproxy` + hermetic, idempotent `BlastitWsgiGate`:**
   - `crates/blastit/src/check.rs` orchestrates `blastit check` (receiving `Host` for `Host::dbproxy()`): it validates CLI flags, `Config`, `./blastit.toml` (`BLASTIT_MANIFEST`), and `[proxy.*]` inputs upfront (exiting **`2`** with `0` bytes on `stdout` before spawning `pytest`), launches `blastit-dbproxy` (`fork` + `execve` with empty `envp` and `Zeroizing` `secrets.pipe`) in `ListenerMode::DynamicCheck` on `<runtime_dir>/pg`, and spawns `[python, "-m", "pytest", "-q", "-p", "no:blastit", "-p", "blastit._check_pytest", *args]` (`-p no:blastit` prevents the `0.1.0` `pytest11` plugin from monkeypatching apps that still set `blastit_manifest` in `pyproject.toml`; subprocess fd 1 is redirected to fd 2 at OS `Stdio` level when `--json` is set; missing `pytest` exits **`2`**).
   - **Framework-agnostic credential barrier:** `swapped_env` renders proxy outputs with `socket_dir = <runtime_dir>/pg`. For non-Django suites, `pytest` spawns directly with `swapped_env` so real DB credentials never enter the Python process. When `pytest-django` is installed, real proxy inputs are passed in a one-shot `0600` file read and immediately unlinked for `django_db_setup`, then replaced by `swapped_env` + `{"op":"set_db"}` on `ctl.sock` before tests run.
   - In `DynamicCheck`, `dbproxy` **always returns `QueryDecision::Allow`**, emitting `WireSqlObs` over internal `obs_fd` while `active_route_stack` holds `ObsTarget::{Routed, Unrouted}` (and recording `SET`, `SHOW`, `KNOWN_STATIC_SETUP_SQL`, and read-only `pg_catalog` introspection `SELECT`s when empty for one-time replay on first routed reuse).
   - `BlastitWsgiGate` wraps the WSGI callable, `.wsgi_app`, and `django.core.handlers.base.BaseHandler.get_response` behind a `ContextVar` re-entrancy guard (guaranteeing **at most one** observation and `ctl.sock` push/pop per request), replays `wsgi.input` while capping `_blastit.probe_ingress` inspection at `64 KiB` (`truncated: bool`), and updates `blastit_live_server` to run `blastit dev` when non-root.
2. **Distinct exit codes (`0`, `1`, `2`, `3`):**
   - `0`: `pytest` exited `0` and zero `error`-severity findings (or clean `--learn` with ≥ 1 request observed).
   - `1`: `pytest` exited `0`, ≥ 1 `error`-severity finding (or `--learn` refused a statement or observed `0` requests, `"wrote": false`).
   - `2`: Blastit could not start (CLI usage, missing `pytest`, malformed `http_obs.jsonl`, config, env input, or manifest validation error).
   - `3`: `pytest` exited non-zero — explicitly **including `pytest` exit code `5` ("no tests collected")** — emitting no audit report (`0` bytes on `stdout` under `--json`) and writing nothing under `--learn`.
3. **Versioned `--json` contract and bootable `fix` guarantee:**
   - Audit mode emits `schema_version: 1` with `ok`, `requests`, `statements` (`== sum(proxies[*].statements)`), `routes: {total, exercised}`, `proxies: {<name>: {statements, denied}}`, and `findings` over the closed 9-kind enum, where `"action": "add_query"` is emitted **only** for bootable parameterized statements on matched routes (redacted `…` and transaction-control denials emit `"action": "observe"`).
   - `--learn --json` emits `{"proposed": {"routes": N, "queries": M}, "wrote": true|false}`.

## Consequences

- **Positive:** Hermetic against `0.1.0` `pytest11` entry points; zero real secrets in Flask/SQLAlchemy/raw-WSGI `pytest` env; bounded `64 KiB` body sampling in Python; OS-enforced `stdout` purity under `--json`.
- **Trade-offs:** Completeness proves only what the test suite exercised (`unexercised_route`, `proxy_unused`, and `no_observations` fail closed when routes or proxies go untested).
