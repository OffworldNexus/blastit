---
id: MB.2.8
title: "Release prep 0.2.0: version bump, dated changelog, ARCHITECTURE sync"
kind: task
status: ready
owner: implementer
priority: 5
depends_on: [MB.1, MB.2.7]
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
created: "2026-11-13T15:11"
updated: "2026-11-13T15:11"
---

# Goal

Make `0.2.0` releasable: bump the workspace version and every intra-workspace requirement, write the dated changelog entry, sync `ARCHITECTURE.md` with the shipped static responder, and pass both release scripts.

# Context

- `Cargo.toml`: `[workspace.package] version = "0.2.0"`. Every intra-workspace `version = "<old>", path = "../…"` requirement in `crates/*/Cargo.toml` must name `0.2.0` (they are 28 today; the count is whatever `crates/*/Cargo.toml` contains — verify, do not assume). `Cargo.lock` is rewritten line by line, never with `cargo update --workspace`, so the tested dependency graph does not drift; `--locked` builds must pass.
- This is the only node allowed to touch `Cargo.toml`, `Cargo.lock` or `crates/*/Cargo.toml`. No new dependency may be introduced here; this is a version and lockfile change only.
- `CHANGELOG.md`: `## 0.2.0 (YYYY-MM-DD)` with user-visible bullets — built-in static serving, SPA fallback and front-controller rewriting, the denial classes (traversal/NUL/symlink/dotfiles), immutable hashed caching, and the `dev`/`check` experience for static-only sites. Same voice and length as `0.1.1`.
- `ARCHITECTURE.md`: one terse section for the static responder — it lives in the scheduler (no new role, no uid), the precedence order, and the path-safety boundary. Match the code, not the design's intentions; if they differ, say so in `RESULT.md`.
- `scripts/check-release-tag.sh v0.2.0` and `scripts/release-notes.sh --require-dated v0.2.0` are the release gates. Run them; never `git tag`, `git push`, or `gh pr`.

# Acceptance

```console
$ grep -q '^version = "0.2.0"$' Cargo.toml && echo "workspace version bumped"
$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep -v 'version = "0.2.0"' | grep 'version = ' && echo "every intra-workspace requirement names 0.2.0"
$ CARGO_NET_OFFLINE=true cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
$ scripts/check-release-tag.sh v0.2.0
$ scripts/release-notes.sh --require-dated v0.2.0 > /dev/null && echo "dated changelog entry"
$ make check
$ make e2e
```

# Notes

- Run this node **sequentially**, alone: it rewrites the workspace lockfile, and any parallel Rust leaf would contend on it.
- If a `cargo metadata --locked` failure shows an intra-workspace requirement still lagging, fix every occurrence and re-run — `MB.1.9` hardened `scripts/dev-version.sh` for the same class of bug (`version = "0.1.0"` left behind by a rewrite that matched nothing).
- Do not touch `python/**`, `e2e/**`, `crates/**/src/**` or the public docs here. `docs/changelog.md` mirrors `CHANGELOG.md` by build, so leave it alone.
- Record in `RESULT.md` the version, the date, the requirement count, and the exact script output tails; leave the tag and the Release PR to `conductor`.
