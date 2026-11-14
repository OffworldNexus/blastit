---
id: MB.3.9.4
title: "blastit dev spawns/supervises/cleans up the php-fpm pool via the runtime provider (no listen, derived workers)"
kind: task
status: superseded
owner: implementer
priority: 1
depends_on: [MB.3.6.3, MB.3.9.3]
paths: ["crates/blastit/src/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:19"
updated: "2026-11-14T14:39"
---

# Goal

`blastit dev` spawns, configures, supervises and cleans up the php-fpm pool itself,
through the **same** provider code path production uses — one implementation, not a second
hand-rolled one. Also consumes the new grammar: no `listen` key, optional `workers`.

# Context

- Implement `docs/design/php-fastcgi.md` as amended by `MB.3.9.2` (grammar: derived socket,
  budgeted workers) and `MB.3.9.3` (dev lifecycle, discovery, cleanup, error contract).
  `MB.12` owns the runtime-provider contract this sits behind. Nothing in dispatch may
  `match` on backend kind.
- Shape: `dev` flips from `PoolSpawn::External` to managed for `[route.fastcgi]` routes.
  Production-only steps (privilege drop via `blastit_sys::drop_privileges`, cgroup setup,
  capability assertions) are skipped in `dev` — the *only* differences allowed, each named
  in the amendment. Everything else (`setpgid`, the S0–S7/U1 sequence, the rendered conf,
  `pm.max_requests` recycling, the stderr cap, the `PoolReady` gate, `/proc/self/fd/<parentfd>`
  connect) is shared source.
- If `MB.3.6.3` structured the production spawn as a one-off inside `init`, move it behind
  the provider seam **without** changing production behaviour; the production protected
  tests stay green unmodified and the RESULT says exactly what moved.
- `check` keeps its external-executor branch (it must not spawn a pool inside the child
  `dev` it is about to kill): `PoolSpawn::External` survives for `check`, not for `dev`.
- The derived socket path is blastit's (`<run>/<route>/fpm.sock`, parent `0700`); no code
  path may accept a caller-supplied socket path, and none may `canonicalize()`.
- **Runtime resolution is generic and resolved once at boot.** A provider declares its
  search names and its env override variable; for PHP that is `php-fpm`, then versioned
  names in `PATH`, overridable by `BLASTIT_PHP_FPM` (env only — never a manifest key).
  Resolve and validate it (file, executable, `--version` runs) in the stage-0 config
  composition root **before** the stage-1 `execve` swaps the environment, store the
  absolute path in the typed config, and use that one value in `dev`, `check` and
  production. The boot line and `--json` print it. Two routes must resolve **one** binary.
  Do not read `BLASTIT_PHP_FPM` after the barrier.

# Acceptance

```console
$ cargo test -p blastit --locked
$ cargo test -p blastit-scheduler --locked
$ make check
```
- Every existing protected `crates/blastit/tests/critical_*.rs` suite passes unmodified.
- Resolver unit tests (in `crates/blastit/src/**`) cover: `BLASTIT_PHP_FPM` wins over
  `PATH`; a non-executable override is refused naming the variable, the value and the
  reason; no binary anywhere refuses the boot naming the install package **and** the
  override; the versioned-name order is deterministic when two candidates exist; and two
  FastCGI routes resolve one absolute binary.
- A `blastit dev` boot on a fixture manifest with one `[route.fastcgi]` (one line, no
  `listen`) starts exactly one `php-fpm` child, and `blastit dev` returning leaves none
  (`MB.3.9.5`'s oracle proves it; this node makes it true).
- The boot line/log shows the derived worker value, so a developer can see what blastit
  chose without reading a config file.

# Notes

- Concurrency: one `make check` per checkout at a time; prefer no `Cargo.lock` change, and
  serialize it if one is unavoidable.
- Anything the amendment left open goes under `## Follow-ups` rather than being invented.
