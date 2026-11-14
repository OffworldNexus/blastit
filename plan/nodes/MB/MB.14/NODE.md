---
id: MB.14
title: "Dogfood: make docs-serve builds the site and serves it through blastit"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [Makefile, README.md, RELEASING.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T13:56"
updated: "2026-11-14T14:20"
---

# Goal

Dogfood the static backend: add a `make` target that builds the documentation site and serves it **through blastit itself** (a fresh `cargo build -p blastit`), so our own docs are a real, runnable static deployment.

# Context

Verified facts to build on (do not invent flags):
- `zensical.toml` sets `site_dir = "site"`; the Makefile already builds it with `$(UVX) zensical build --strict` (`docs-check`).
- The binary accepts `blastit dev [--port N] [--host ADDR] [--manifest PATH] [--learn]`; the manifest is discovered at `./blastit.toml` otherwise.
- An all-static manifest needs no `app`: one `[[route]]` with `path = ['.*']` and a `[route.static]` block (`root`, optional `index`, optional `immutable`). `root` is relative to the manifest and validated (no absolute path, no escaping `..`), so the manifest must sit beside the files it serves.
- `stack` = zensical → `site/`, blastit serves `site/`; nothing else.

# Acceptance

1. **`make docs-serve`** (port via `PORT`, default 8080):
   - builds the site (`zensical build`, non-strict is fine here — `docs-check` owns strictness),
   - builds the server from source: `cargo build -p blastit --locked` with the Makefile's existing `RUST_ENV`/`$(CARGO)`,
   - stages a self-contained serve root under `target/docs-serve/` (e.g. copy `site/` to `target/docs-serve/site/`, write `target/docs-serve/blastit.toml` with `root = "site"`), so nothing is written into the repo and no `..` escapes the root,
   - runs `target/debug/blastit dev --manifest target/docs-serve/blastit.toml --port "$PORT"` in the foreground, printing the URL and how to stop it.
2. **`make docs-serve-check`** — a bounded smoke test usable on its own (and optionally in a CI job, not in `make check`):
   - starts the same stack in the background, waits for the port to answer (bounded, e.g. 15 s, with the log dumped on timeout),
   - asserts `GET /` and a nested page (e.g. `/reference/cli/`) return `200` with `text/html`, that a missing path returns `404` (not the SPA index — docs are not an SPA), and that a hashed/static asset returns the documented `Cache-Control` (`no-cache` unless listed under `immutable`; the docs site has no `immutable` glob unless you add one deliberately — say which you chose and why),
   - always kills the server and leaves no process behind (trap), exits non-zero on any failure with the response printed.
3. Both targets appear in `make help` and in the README or `RELEASING.md` where the other dev commands are listed (one line each — this is a developer tool, not a user-facing feature).
4. No `sudo`, no Python project, no `blastit dev` requirement on the host beyond a built binary; works from a clean checkout after `cargo build`.
5. `make check` stays green (the new targets are not part of it unless `docs-serve-check` is fast and hermetic — your call, justified).

# Notes

- This is dogfooding, so it must use the **real static path**: `[route.static]`, `root`, the resolver, ETag/304, the refusal matrix — not a special-case file server.
- If serving the built site exposes a real defect (e.g. a status or header the docs site needs), stop and report it as its own node rather than papering over it in the Makefile.
- Keep the target dependency-light: `uvx zensical` is already the documented offline path; do not add new network requirements to `make check`.
