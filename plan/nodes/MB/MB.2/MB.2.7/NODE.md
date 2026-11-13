---
id: MB.2.7
title: "Prove denials, caching and documented commands end-to-end (Docker e2e + visual)"
kind: task
status: ready
owner: implementer
priority: 4
depends_on: [MB.1, MB.2.6]
paths: ["e2e/**", "docs/testing/VISUAL-MB.2.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-13T15:11"
---

# Goal

Prove on a real container that the page's commands run verbatim and that every promise holds: encoded traversal, NUL, symlink escape and dotfile leaks are refused, hashed assets are immutable, and a deep SPA URL returns `index.html`.

# Context

- New e2e leg under `e2e/`: `e2e/static_site/` (a `public/` tree, a complete `blastit.toml`, a symlink inside `public/` pointing outside it, `.env` and `.git/config` decoys), a stdlib driver `e2e/static_assets.py` (same shape as `scenario.py`, with `--self-test`), and the wiring in `e2e/run.py` + `e2e/compose.yaml` (a static service or a second port on the existing image).
- **Verbatim replay:** the driver reads `docs/frameworks/static-and-spa.md`, extracts the `$ ` commands from its console fences, runs them in the container, and asserts their documented output. A page edit that no longer matches is a failure at this node, not a silent drift. Serve the page's own fixture semantics (same manifest shape as `python/tests/runnable_docs/fixtures/static-site/`).
- **Assertions** (each one line in the driver's report): `/assets/app.<hash>.js` → `200` + `cache-control: public, max-age=31536000, immutable`; `/dashboard/settings` → `200` `text/html` = `index.html`; `/../etc/passwd`, `/%2e%2e/%2e%2e/etc/passwd`, `/assets/%2e%2e/%2e%2e/secret`, `/%00.js`, `/foo%5c..%5cbar` → `400`/`404`, never file bytes; `/.env`, `/.git/config`, `/.git/` → `404`; `public/link -> /etc/passwd` → `404`; `HEAD` returns headers and no body; `POST` → `405` with `Allow`.
- `visual-checker` inspects the built site (the nav gained a page in `MB.2.1`) and writes `docs/testing/VISUAL-MB.2.md` with `VERDICT: PASS`.
- Do not edit the page, the fixture, `inventory.toml` or any `crates/` file. If a documented command fails, reproduce it, record the exact failing fence and command in `RESULT.md`, and log a follow-up — the owning node fixes it.

# Acceptance

```console
$ uv run --no-project python e2e/static_assets.py --self-test
$ make e2e
$ make docs-test
$ make docs-check
$ grep -q 'VERDICT: PASS' docs/testing/VISUAL-MB.2.md
```

# Notes

- `make e2e` builds the wheel and runs the existing Django / Flask / rootless legs too; the new leg must not add more than a couple of minutes and must not need root.
- Keep the driver stdlib-only (`uv run --no-project`) and deterministic: fixed fixture, fixed port from `E2E_STATIC_PORT`, no network.
- Follow-up to log in `RESULT.md`: promoting the page's accounted fences to executed modes in `inventory.toml` (owned by `MB.2.1`) once this leg is green — do not edit that file here.
- If the rendered site is unchanged beyond the new nav entry, say so and attach the screenshots the checker used; still write the report file.
