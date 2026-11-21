---
id: MB.3.8
title: "Release prep 0.2.0: dated changelog covering static + presets + PHP, ARCHITECTURE sync, release-tag guard"
kind: task
status: done
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
  - docs/decisions/README.md
  - "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md"
  - "docs/design/static-and-spa.md"
  - "scripts/check-release-tag.sh"
  - "scripts/release-notes.sh"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-15T09:35"
---

# Goal

Make `0.2.0` releasable: confirm the workspace version (already `0.2.0` — the release was
folded, so there is no `0.2.1` bump) and every intra-workspace requirement, write the dated
changelog entry, and sync `ARCHITECTURE.md`, `docs/security.md`,
`docs/design/static-and-spa.md` and the one-line `ADR-0007` amendment banner with the shipped
embedded PHP backend. Release gate only — no feature work, no new dependency.

# Context

- `Cargo.toml`: `[workspace.package] version = "0.2.0"` and it must **not** bump (release
  fold, `MB.3/LOG.md` 2026-11-14T16:36). Every intra-workspace `version = "<old>",
  path = "../…"` requirement in `crates/*/Cargo.toml` must name `0.2.0` — including
  `crates/blastit-php-sapi/Cargo.toml` and `crates/blastit-view/Cargo.toml`. Count them from
  the tree; do not assume a stale count.
- `Cargo.lock` is rewritten line by line (the version fields of the workspace packages),
  never with `cargo update --workspace`, so the tested dependency graph does not drift;
  `--locked` builds must pass. This is the **last** link of the serialized lock chain
  (`MB.3.4.1` → `MB.9.1.1` → `MB.9.2.1.1` → `MB.3.8`; the retired `MB.3.6.1.2` link went with
  the external PHP path, `MB.3.17`): every crate and dependency edge already has its lock
  entry; you only move versions.
- `CHANGELOG.md`: the dated `## 0.2.0 (YYYY-MM-DD)` entry (finalize the date; it already
  covers static + pooler presets and gains PHP), same voice and length as the rest of the
  file — embedded PHP routes (host NTS `libphp`, one `[route.php]` front controller with
  `entry` and an optional `workers` cap) for Symfony/Laravel/PSR-15, one-manifest `public/`
  static + front controller, the refusal classes (`SCRIPT_FILENAME`, `PHP_VALUE`/
  `PHP_ADMIN_VALUE`, `-d` gadget probes, `httpoxy`), zero-`sudo` local dev, and the
  `dev --learn` / `check` experience for PHP routes.
- `ARCHITECTURE.md` §7.3: one terse section for the embedded PHP backend — the `view.chan`
  `RawFrame` exchange, the per-minor SAPI shim, who owns the `EmbeddedView` child and its uid,
  the `libphp` fd-verification rule, the C-memory `$_SERVER` boundary, and the
  `app.output`/CPU-ns accounting. Match the code, not the design's intentions; if they
  differ, say so in `RESULT.md`.
- `docs/security.md` and `docs/design/static-and-spa.md` §5.1: record only what `ADR-0010`
  states — `SeccompRole::Scheduler` keeps zero `socket()` (§2, Consequences) and the PHP route
  runs under `SeccompRole::EmbeddedView { egress }` (§1), citing `ADR-0010` §1 and §2 plus
  `ARCHITECTURE.md` §1. This node writes no new security decision: anything not in those
  places stays out.
- `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md`: **no banner.** `ADR-0010`
  does not amend it — §1–§2 restore decision 1's zero-`socket()` scheduler (the ADR-0008
  decision 7 `AF_UNIX` allowance went with the pool, `MB.3.17`). Never write an amendment
  note into a decision record that did not happen.
- `scripts/check-release-tag.sh v0.2.0` and `scripts/release-notes.sh --require-dated v0.2.0`
  are the release gates. Run them; never `git tag`, `git push`, or `gh pr`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo check --workspace
$ grep -q '^version = "0.2.0"$' Cargo.toml && echo "workspace version is 0.2.0 (no bump)"
$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep 'version = ' | grep -v 'version = "0.2.0"' && echo "every intra-workspace requirement names 0.2.0"
$ cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
$ grep -q '^## 0.2.0 (' CHANGELOG.md && echo "dated changelog entry"
$ grep -q 'libphp' ARCHITECTURE.md && grep -qi 'php' ARCHITECTURE.md
$ grep -q 'may not call' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md && grep -q 'socket()' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md && ! grep -q 'Amended by ADR-0010' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md && echo "ADR-0007 decision 1 stands unamended"
$ scripts/check-release-tag.sh v0.2.0
$ scripts/release-notes.sh --require-dated v0.2.0 > /dev/null && echo "release notes accept the entry"
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

## Release checklist item (owner, 2026-11-14) — certification gate

> no `.php`/`.phar` under a static root is servable (404, protected `critical_static_entry`), proven before the 0.2.0 tag.

Fail certification if this is not proven. Implemented by `MB.3.33`;
`MB.9.2.2` must preserve the gate when it ports static serving.

> `body = "raw"` without `allow_raw` refuses at boot in production — stage 1 by protected test (`critical_raw_ingress`), stage 0 by the `ServeGates` type (`clear` is its only constructor and *is* `raw_ingress_gate`; `bind_view_sockets` demands the token, so deleting or reordering the gate is a compile error rather than a runtime test; stage 0's call site is unwitnessed at runtime).

Fail certification if this is not proven. Implemented by MB.3.53; the decision is documented in docs/design/learn-single-pass.md (ruling 4) and docs/decisions/ADR-0011-*.md.
