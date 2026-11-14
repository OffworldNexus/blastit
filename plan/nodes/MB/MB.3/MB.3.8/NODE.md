---
id: MB.3.8
title: "Release prep 0.2.1: version bump, dated changelog, ARCHITECTURE sync"
kind: task
status: ready
owner: implementer
priority: 5
depends_on: [MB.3.7]
paths:
  - Cargo.toml
  - Cargo.lock
  - "crates/*/Cargo.toml"
  - CHANGELOG.md
  - ARCHITECTURE.md
  - "scripts/check-release-tag.sh"
  - "scripts/release-notes.sh"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:09"
---

# Goal

Make `0.2.1` releasable: bump the workspace version and every intra-workspace
requirement, write the dated changelog entry, and sync `ARCHITECTURE.md` with the
shipped FastCGI backend (session, pool ownership, param lockdown). Release gate only —
no feature work, no new dependency.

# Context

- `Cargo.toml`: `[workspace.package] version = "0.2.1"`. Every intra-workspace
  `version = "<old>", path = "../…"` requirement in `crates/*/Cargo.toml` must name
  `0.2.1` — including the new `crates/blastit-fastcgi/Cargo.toml`. Count them from
  the tree; do not assume the `0.2.0` count.
- `Cargo.lock` is rewritten line by line (the version fields of the workspace
  packages), never with `cargo update --workspace`, so the tested dependency graph
  does not drift; `--locked` builds must pass.
- This is the **only** node allowed to edit `Cargo.toml` / `Cargo.lock` version
  fields. `MB.3.4` added the new crate's lock entry; you only move versions. No new
  dependency may be introduced here.
- `CHANGELOG.md`: `## 0.2.1 (YYYY-MM-DD)` with the real date, same voice and length as
  `## 0.2.0` — PHP-FPM/FastCGI routes for Symfony/Laravel/PSR-15, one-manifest
  `public/` static + front controller, the refusal classes (`SCRIPT_FILENAME`,
  `PHP_VALUE`/`PHP_ADMIN_VALUE`, `-d` gadget probes, `httpoxy`), zero-`sudo` local
  pool, and the `dev --learn` / `check` experience for PHP routes.
- `ARCHITECTURE.md`: one terse section for the FastCGI backend — the pure codec crate,
  the scheduler session, who owns the pool child and its uid, the socket permission
  rule, the param allowlist/canonicalisation boundary, and `STDERR`/`END_REQUEST`
  accounting. Match the code, not the design's intentions; if they differ, say so in
  `RESULT.md`.
- `scripts/check-release-tag.sh v0.2.1` and
  `scripts/release-notes.sh --require-dated v0.2.1` are the release gates. Run them;
  never `git tag`, `git push`, or `gh pr`.

# Acceptance

```console
$ grep -q '^version = "0.2.1"$' Cargo.toml && echo "workspace version bumped"
$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep 'version = ' | grep -v 'version = "0.2.1"' && echo "every intra-workspace requirement names 0.2.1"
$ CARGO_NET_OFFLINE=true cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
$ grep -q '^## 0.2.1 (' CHANGELOG.md && echo "dated changelog entry"
$ grep -q 'FastCGI' ARCHITECTURE.md && grep -qi 'php' ARCHITECTURE.md
$ scripts/check-release-tag.sh v0.2.1
$ scripts/release-notes.sh --require-dated v0.2.1 > /dev/null && echo "release notes accept the entry"
$ make check
$ make e2e
```

# Notes

- Run this node **sequentially and alone**: it rewrites the workspace lockfile, and any
  parallel Rust leaf would contend on it. Every other `MB.3` child is `done` before it
  starts by construction (`depends_on`).
- If `cargo metadata --locked` still shows a lagging intra-workspace requirement, fix
  every occurrence and re-run — the `MB.1.9`/`MB.2.8` class of bug is a rewrite that
  matched nothing and left `version = "0.2.0"` behind.
- Do not touch `python/**`, `e2e/**`, `crates/**/src/**`, `docs/**` (the published
  `docs/changelog.md` mirrors `CHANGELOG.md` by build), or plan files.
- Record in `RESULT.md`: the version, the date, the requirement count, the exact script
  output tails, and the `make check` / `make e2e` tails. Leave the tag and the Release
  PR to `conductor`.
