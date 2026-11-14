---
id: MB.3.7
title: "Prove embedded PHP, the documented commands and the php2shell suite end-to-end (wheel + Python-free binary)"
kind: task
status: ready
owner: implementer
priority: 4
depends_on: [MB.3.6]
paths: ["e2e/php/**", e2e/php2shell.py, e2e/compose.yaml, e2e/run.py, "e2e/rootless/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T14:39"
---

# Goal

Prove on a live, **unpatched** `php-fpm` that the epic's promises hold: a Symfony/Laravel
style `public/index.php` front controller runs under `blastit dev` and `blastit check` with
zero `sudo`, and every `php2shell` probe in the design's refusal matrix fails harmlessly.
Replay `docs/frameworks/php.md`'s own commands verbatim, in a container, against the shipped
wheel.

# Context

- Design of record: `docs/design/php-fastcgi.md` §7.3 (the e2e replay and the fixture
  contract), §4.7 (the `E1`–`E25` probe table), §5.2 (the CVE defence table) and §8's
  `MB.3.7` row. Everything is proven here, not taken on trust: the page (`MB.3.2`), the
  design (`MB.3.3`), the codec (`MB.3.4`), the grammar (`MB.3.5`) and the session/CLI
  (`MB.3.6`).
- Files (the whole leaf, 5 path entries): `e2e/php/**`, `e2e/php2shell.py`,
  `e2e/compose.yaml`, `e2e/run.py`, `e2e/rootless/**`.
- Fixture (`e2e/php/**`): a pinned **unpatched** `php:7.4-fpm`-style image (record the
  digest and why that tag; the point is that the runtime is not what saves us), a non-root
  pool listening on a Unix socket in a writable dir, `public/index.php` answering
  `GET`/`POST /__inspect` with exactly `{"script_filename","script_name","path_info",
  "document_root","request_uri","query_string","request_method","content_length",
  "allow_url_include","http_proxy","user_ini","server_keys"}` (`server_keys` is
  `array_keys($_SERVER)` — names only, never values), a `public/assets/app.<12 hex>.css` for
  the static responder, and the canary files (`prepend.php`, `evil.php`, `shell.jpg`).
- Driver `e2e/php2shell.py` (stdlib only, same shape as `e2e/static_assets.py`):
  - `--self-test` parses §4.7's table, asserts the **24** non-`—` rows (`E1`–`E17`,
    `E19`–`E25`) match `PROBES` 1-to-1, parses the `#n` column to assert the union is exactly
    `{1..20}`, and requires every `200` probe to carry a **non-empty** discriminating
    key-value assertion.
  - replays every `$ ` command of `docs/frameworks/php.md` verbatim (an unknown `$ `
    command is a failure). **Re-read after 2026-11-14 (MB.3.9.1):** the page's Local is now
    `uv tool install blastit` then `blastit dev`, and CI is `blastit check -- <suite>`; the
    hand-written `php-fpm.conf`, the `mkdir -p …/run` and the absolute `listen` fence are
    **gone**, so the old "`uv init`/`uv add` exempt" clause and the `php-fpm -tt -y` run on
    *the page's* INI no longer apply. The drifted-`clear_env`/`listen.mode`/`pm.max_children`
    assertions move to the **generated** pool file — and only if the submitted mechanism
    keeps a pool; if `MB.12` lands the embed model, the equivalent assertion is that
    `blastit dev` starts the PHP worker with the derived configuration and that a second
    route gets its own.
  - runs the two PHPUnit suites behind the page's CI fences: `uv run blastit check --
    vendor/bin/phpunit` against `e2e/php/suite/` (`24` HTTP requests →
    `blastit check: 24 requests, 0 statements, 1/1 routes, clean`) and
    `uv run blastit check --json -- vendor/bin/phpunit` against `e2e/php/assets/`
    (3 asset requests → `requests:3, exercised:0, unexercised_route`), parsing `N` in
    `check: N requests, …`.
  - `P1`–`P12` serve the 24 client probes; the canaries must be absent from any response
    **except `P4b`**, whose canary must be **present** (the positive control of §5.2,
    excluded from the absent assertion). `P12` includes a raw-socket malformed/proxy-control
    CGI-head responder for row 24.
- `e2e/rootless/**`: `check_fastcgi_parent_pivot_leaves_sentinel_untouched` (after
  `PoolReady`, a privileged harness binds a decoy listener at `/sentinel/fpm.sock`, renames
  `run` aside and symlinks `run -> /sentinel` → the live `GET /users/42` answers `200` from
  the pinned `entry` while `/sentinel/fpm.sock` records **0** `accept`s; `RETIRE`/respawn
  while symlinked fails closed with `503` + Unwind `U1`; restoring `run` and respawning binds
  a fresh `(sock_dev, sock_ino)` inside `parentfd`), `check_fastcgi_entry_0000_refused_for_route_uid`,
  the route-uid `chmod`/`O_CREAT`/`unlink` `EPERM`/`EACCES` negatives, and cross-route
  isolation (route A's uid cannot `connect` to pool B → `EACCES`).
- Registration: one `php` service in `e2e/compose.yaml` and one leg in `e2e/run.py`,
  additive to the existing static/sqlalchemy/rootless legs, so bare `make e2e` covers it.
  Keep the leg's wall time small (reuse the prebuilt wheel image pattern).

# Acceptance

```console
$ uv run --no-project e2e/php2shell.py --self-test
$ grep -q 'docs/frameworks/php.md' e2e/php2shell.py && echo "driver replays the page"
$ grep -q 'php' e2e/compose.yaml && grep -q 'php2shell' e2e/run.py
$ grep -q 'check_fastcgi_parent_pivot_leaves_sentinel_untouched' e2e/rootless/run.py
$ make e2e
$ make check
```

# Notes

- **Coordination:** `MB.8` also lands an `e2e/**` leg (Django `collectstatic`). Both edits to
  `e2e/compose.yaml` and `e2e/run.py` are additive — rebase, keep their service and leg,
  never reorder or delete theirs.
- Docker only, no network installs beyond what other legs already cache; the PHP image pull
  is the one new artifact and must be pinned.
- Do not weaken a probe to make it pass: if a payload succeeds, that is the finding —
  `plan.py --agent implementer human add "…" --node MB.3.7`, and record the reproduction in
  `RESULT.md`. Never edit a `critical_*.rs` or the design matrix to match observed behaviour.
- No product code in this node: `crates/**` is out of scope. A real defect found here
  becomes its own node.
