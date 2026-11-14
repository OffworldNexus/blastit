---
id: MB.3.8
title: "Release prep 0.2.0: dated changelog covering static + presets + PHP, ARCHITECTURE sync, release-tag guard"
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
  - docs/security.md
  - "docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md"
  - "docs/design/static-and-spa.md"
  - "scripts/check-release-tag.sh"
  - "scripts/release-notes.sh"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T16:36"
---

# Goal

Make `0.2.1` releasable: bump the workspace version and every intra-workspace requirement,
write the dated changelog entry, and sync `ARCHITECTURE.md`, `docs/security.md`,
`docs/design/static-and-spa.md` and the one-line `ADR-0007` amendment banner with the shipped
FastCGI backend. Release gate only — no feature work, no new dependency.

# Context

- `Cargo.toml`: `[workspace.package] version = "0.2.1"`. Every intra-workspace
  `version = "<old>", path = "../…"` requirement in `crates/*/Cargo.toml` must name `0.2.1`
  — including `crates/blastit-fastcgi/Cargo.toml` and `crates/blastit-backend/Cargo.toml`.
  Count them from the tree; do not assume the `0.2.0` count.
- `Cargo.lock` is rewritten line by line (the version fields of the workspace packages),
  never with `cargo update --workspace`, so the tested dependency graph does not drift;
  `--locked` builds must pass. This is the **last** link of the serialized lock chain
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.6.1.2` → `MB.3.8`): every crate and
  dependency edge already has its lock entry; you only move versions.
- `CHANGELOG.md`: `## 0.2.1 (YYYY-MM-DD)` with the real date, same voice and length as
  `## 0.2.0` — PHP-FPM/FastCGI routes for Symfony/Laravel/PSR-15, one-manifest `public/`
  static + front controller, the refusal classes (`SCRIPT_FILENAME`, `PHP_VALUE`/
  `PHP_ADMIN_VALUE`, `-d` gadget probes, `httpoxy`), zero-`sudo` local pool, and the
  `dev --learn` / `check` experience for PHP routes.
- `ARCHITECTURE.md` §7.3: one terse section for the FastCGI backend — the pure codec crate,
  the scheduler session, who owns the pool child and its uid, the socket permission rule, the
  param allowlist/canonicalisation boundary, and `STDERR`/`END_REQUEST` accounting. Match the
  code, not the design's intentions; if they differ, say so in `RESULT.md`.
- `docs/security.md` and `docs/design/static-and-spa.md` §5.1: record the scheduler's
  `AF_UNIX` seccomp allowance together with `SYS_bind`/`SYS_listen` `EPERM` (ADR-0008
  decision 7, which amends ADR-0007 decision 1). State that the BPF wall is
  `args[0] == AF_UNIX` only and that the path wall is the pinned `parentfd` + the `0600`
  scheduler-owned socket.
- `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md`: add the **1-line**
  `Amended by ADR-0008` banner — nothing else in that ADR changes.
- `scripts/check-release-tag.sh v0.2.1` and `scripts/release-notes.sh --require-dated v0.2.1`
  are the release gates. Run them; never `git tag`, `git push`, or `gh pr`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo check --workspace
$ grep -q '^version = "0.2.1"$' Cargo.toml && echo "workspace version bumped"
$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep 'version = ' | grep -v 'version = "0.2.1"' && echo "every intra-workspace requirement names 0.2.1"
$ cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
$ grep -q '^## 0.2.1 (' CHANGELOG.md && echo "dated changelog entry"
$ grep -q 'FastCGI' ARCHITECTURE.md && grep -qi 'php' ARCHITECTURE.md
$ grep -q 'Amended by ADR-0008' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md
$ scripts/check-release-tag.sh v0.2.1
$ scripts/release-notes.sh --require-dated v0.2.1 > /dev/null && echo "release notes accept the entry"
$ make check
$ make e2e
```

# Notes

- Run this node **sequentially and alone**: it rewrites the workspace lockfile, and any
  parallel Rust leaf would contend on it. The first cargo command runs offline **without**
  `--locked` (the version bump rewrites the lock), then everything else is `--locked`.
- If `cargo metadata --locked` still shows a lagging intra-workspace requirement, fix every
  occurrence and re-run — the `MB.1.9`/`MB.2.8` class of bug is a rewrite that matched nothing
  and left `version = "0.2.0"` behind.
- Do not touch `python/**`, `e2e/**`, `crates/**/src/**`, the published `docs/` pages (the
  site mirrors `CHANGELOG.md` by build), or plan files.
- Record in `RESULT.md`: the version, the date, the requirement count, the exact script
  output tails, and the `make check` / `make e2e` tails. Leave the tag and the Release PR to
  `conductor`.

## Release target: 0.2.0, not 0.2.1 (owner, 2026-11-14)

"There's no point opening a PR for 0.2.1, just put everything into 0.2.0."

- `Cargo.toml`'s `[workspace.package] version` stays **0.2.0**; nothing bumps to 0.2.1.
- The `CHANGELOG.md` `## 0.2.0 (YYYY-MM-DD)` entry must cover **static serving + the pooler
  presets + PHP (`[route.php]`, embedded `libphp`)**, so it **cannot be finalized until
  `MB.3.13` lands** — do not write a dated entry for a feature that is still being built.
- `scripts/check-release-tag.sh v0.2.0` and
  `scripts/release-notes.sh --require-dated v0.2.0` are the guards, unchanged in shape.
- Any doc text that says "`0.2.1` adds …" becomes 0.2.0: that is `MB.3.10`/`MB.3.20`'s
  edit, and their acceptance now names it so it is verified, not remembered.
