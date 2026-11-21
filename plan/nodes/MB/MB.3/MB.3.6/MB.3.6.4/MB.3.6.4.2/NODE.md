---
id: MB.3.6.4.2
title: "check::serve_and_run, the 3-way branch order and the protected learn suite - design MB.3.6c part 2"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.4.1]
paths: [crates/blastit/src/cli.rs, crates/blastit/src/check.rs, crates/blastit/tests/critical_php_learn.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-18T20:36"
---

# Goal

Make `blastit check` honest for a route that has no Python: the 3-way branch order,
`check::serve_and_run` with the `<CheckRoot>/dev` handoff,
`blastit check -- <suite>` with exit `3` on a failing suite, and the protected learn suite
(design leaf `MB.3.6c`, part 2: the CLI half).

# Context

- Design of record: `ADR-0010` §1 (the embedded worker) and this brief's §4.10.3 (the
  three-way branch order, `serve_and_run`, the observation shapes): `<CheckRoot>/dev`
  handoff via `BLASTIT_CHECK_DEV_ROOT`, the child `dev` discovering `libphp` itself
  (ADR-0010 §3, no parent fd), `blastit check -- <suite>`, `executor_ran` exercise count.
- Files (the whole leaf, 3): `crates/blastit/src/{cli,check}.rs`,
  `crates/blastit/tests/critical_php_learn.rs`.
- What lands:
  - `check::{DEV_ROOT, SQL_OBS_FD}` (`pub(crate) const`s). There is no pool parent-fd
    const: the embedded worker needs none (ADR-0010 §1).
  - `check::serve_and_run`: create `<CheckRoot>/http_obs.jsonl` (`0600`) and a `socketpair`
    `(obs_read, obs_write)`; spawn
    `<current_exe> dev --port 8080` with `BLASTIT_CHECK_HTTP_OBS`, `BLASTIT_CHECK_SQL_OBS_FD`,
    `BLASTIT_CHECK_DEV_ROOT=<CheckRoot>/dev` (stdout →
    stderr under `--json`); wait for `127.0.0.1:8080`; run `<suite>` with
    `BLASTIT_URL=http://127.0.0.1:8080`, cwd = manifest dir, `env_clear` + prepared env;
    terminate the child `dev`, join the `ObsReader` and read `http_obs.jsonl` into
    `CheckObsV1::new_with_upstream`. **The parent starts no dbproxy** (`dev` does). A busy
    port or a spawn failure is `2`; a non-zero suite exit is **`3`** (`check::SUITE_FAILURE`).
  - The **three-way branch order**: (1) `manifest.is_all_static()` → exit `0`;
    (2) `config.app.is_none() && manifest.can_boot_without_app()` → external-executor mode
    (`-- <suite>` → `serve_and_run`; empty `--` exits `2` with
    `route "<first_php_route>" runs an embedded executor: blastit check -- <suite>`);
    (3) everything else → the existing pytest path, `--` forwarded **verbatim** to `pytest`
    (keeping `docs/ci.md`'s `-- -k products`, `-- tests/…`, and `critical_static_cli`'s
    app-less `MIXED` `"no interpreter to run pytest on"`). No `-k` flag heuristic.
  - `cli.rs`: `CheckOptions` for the new flags (the internal `--stage=1` arm is
    `MB.3.6.3.1`'s; do not re-add it).
  - Protected `crates/blastit/tests/critical_php_learn.rs` (new): fast unprivileged
    `dev` boot on the page's static+php manifest (exact 3-line banner, `1 route`, no
    `BLASTIT_APP`); `check` with no `--` on the page's manifest exits `2`; `app` +
    `[route.static]` and app-less `MIXED` manifests keep the pytest path; the page's JSON
    (3 assets → `requests:3, exercised:0, unexercised_route`); one `/users/42` →
    `requests:1, exercised:1`; one SQL statement via `<CheckRoot>/dev/views/0` child
    dbproxy → `statements:1` (`check_sql_obs_fd` closed outside dbproxy); head `413`
    (`executor_ran: false`) emits `missing_ingress` while keeping `exercised: 0`; learn
    proposes `[route.static] root = "public"`; `Param` carries no value; legacy
    `HttpObsLine` without `kind` parses; `check -- <suite>` maps a non-zero suite exit to
    `3`; `config_document_carries_no_check_fd`; `dev` with an unwired `[route.php]` exits
    `1` naming the route and never falls through to WSGI (`MB.3.28`);
    `embedded_php_never_spawns_an_external_php_process` with its positive control.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --test critical_php_learn --locked
$ cargo test -p blastit --locked
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
$ grep -q 'fn serve_and_run' crates/blastit/src/check.rs && echo "external-executor mode exists"
$ grep -q 'BLASTIT_CHECK_SQL_OBS_FD' crates/blastit/src/check.rs && echo "sql observation fd is pinned"
$ grep -q 'can_boot_without_app' crates/blastit/src/check.rs && echo "external-executor branch gated on the manifest"
$ make check
```

# Notes

- **Protected tests: add, never weaken.** `critical_php_learn.rs` is new.
  `critical_static_cli.rs` (MIXED `"no interpreter to run pytest on"`) and
  `critical_static_manifest.rs` must pass **unmodified** — the branch order exists to keep
  them green.
- Do not import `smoke.rs`: duplicate the `children()`/`fds()` helpers locally (precedent
  `critical_dev_and_fail_closed.rs:482`).
- Never write `sudo` into a documented command; `dev` must boot unprivileged.
- A half-wired `check` that reports PHP routes as unexercised-but-green is worse than a
  scoped gap: if the CLI half overruns the budget, land the branch order + `serve_and_run` +
  exit `3` first and record the residue in `RESULT.md`.
