---
id: MB.2.1
title: "Write the lean static assets & SPA rewriting page (Local/CI/Deploy) and register it"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.1]
paths:
  - "docs/frameworks/static-and-spa.md"
  - docs/frameworks/index.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "python/tests/runnable_docs/fixtures/static-site/**"
  - "docs/design/static-and-spa.review-dx.md"
  - "docs/design/static-and-spa.review-ai-ergonomics.md"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-13T20:22"
---

# Goal

Publish the **one** page that defines `0.2.0` — `docs/frameworks/static-and-spa.md`, safe built-in static asset serving plus SPA / front-controller URL rewriting — as the accepted source of truth that `MB.2.2` designs and `MB.2.3`–`MB.2.6` implement. User-facing documentation only: no design doc, no ADR, no Rust.

# Context

- Iron law: skill `dx-first`. This node is the first actionable child of `MB.2`; nothing else may start before it is accepted.
- The page must be **under 150 lines**, open with a working 10-line `blastit.toml` + one command, and cover exactly three environments:
  1. **Local machine, zero `sudo`:** `uv add blastit`, a `public/` directory, `uv run blastit dev`, then `curl -i` for (a) a hashed asset (`/assets/app.<hash>.js` → `200` with an immutable cache header), (b) a deep SPA URL (`/dashboard/settings` → `200` `text/html`, i.e. `index.html`).
  2. **CI/CD:** `blastit check` (and `blastit check --json`) proving every declared static root exists, every route exercised, no dead grants; one command, one exit code.
  3. **Deployments:** the same block in a 12-factor container under an existing preset, production still failing closed without capabilities — link `deploy/`, do not re-teach isolation.
- Show the front-controller case in the same rewrite section (`public/index.php`). `0.2.0` ships the *rewrite*; the PHP worker is `0.2.1` (`docs/frameworks/php.md`), so that one fence is registered accounted (`static`) with a written reason, never faked.
- New runnable-docs fixture `python/tests/runnable_docs/fixtures/static-site/`: a complete `blastit.toml`, `public/index.html`, `public/assets/app.<12 hex>.js`, `public/docs/index.html`. No PostgreSQL, no Python app — a static site is the point.
- Registration is part of this node and the same commit as the page: a `[[page]]` entry plus one `[[page.fence]]` per console fence in `python/tests/runnable_docs/inventory.toml`, a nav entry in `zensical.toml`, and a link from `docs/frameworks/index.md`.
- Fresh-context `critic` reviews (`dx` and `ai-ergonomics` lenses) land in `docs/design/static-and-spa.review-dx.md` and `docs/design/static-and-spa.review-ai-ergonomics.md`. Every `BLOCKER`/`MAJOR` is resolved, and `RESULT.md` carries a `## Review Resolution` section naming each finding and its fix.

# Acceptance

```console
$ test "$(wc -l < docs/frameworks/static-and-spa.md)" -le 150 && echo "page is lean"
$ test "$(grep -c '^\$ ' docs/frameworks/static-and-spa.md)" -ge 6 && echo "local + CI + deploy commands shown"
$ grep -q 'public/index.php' docs/frameworks/static-and-spa.md
$ make docs-unit      # published page set vs inventory: page entry and every fence present
$ make docs-check     # zensical build --strict: nav, links, anchors
$ make docs-test      # harness runs or accounts every fence; nothing unaccounted
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked   # untouched, still green
$ for f in docs/design/static-and-spa.review-dx.md docs/design/static-and-spa.review-ai-ergonomics.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "both lenses pass"
```

# Notes

- **Fence honesty.** The served-output fences (asset 200/immutable, SPA fallback 200/text-html) cannot execute before the code exists: register them as `static` accounted with a reason that names the exact replacement proof — `e2e/static_assets.py` replays this page's own `$ ` commands in a container in `MB.2.7`. `make docs-test` is green now; `MB.2.7` owns the verbatim execution proof.
- **`docs_fences.rs` is deliberately untouched here.** Its `FILES` list holds only manifests the *current* parser validates; a `[static]` fence cannot pass until the grammar exists. `MB.2.3` adds this page to `FILES` and bumps `[&str; 6]` → `[&str; 7]` in the same change as the parser.
- **Zero kernel lectures.** No `openat2`, `seccomp`, `CAP_*`, inode or symlink mechanics on this page: those belong to `docs/how-it-works.md` / `docs/security.md` and are the design's (`MB.2.2`). The page states the *promise* (traversal, NUL, symlink escape, dotfile leaks are refused with `400`/`404`) and links out.
- **One canonical way per task.** No nginx/regex-soup alternative, no second way to point at a directory. If a flag or key is not in the accepted page, implementers must not invent it.
- Do not edit `docs/reference/manifest.md` (that reference entry belongs to `MB.2.3`), `docs/reference/cli.md`, or any `docs/design/*.md` content page other than the two review artefacts.
