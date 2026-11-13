---
id: MA.3.3
title: "Visual QA of the built Zensical site (desktop light/dark, mobile, Mermaid)"
kind: task
status: done
owner: "visual-checker"
priority: 3
depends_on: [MA.3.2]
paths: [zensical.toml, "docs/stylesheets/**", docs/brand/brand.css, "docs/testing/VISUAL-MA.3.md"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-13T01:50"
---

# Goal

Prove the built Zensical site is visually correct on desktop (light + dark) and mobile, that every Mermaid diagram renders, and that navigation is complete, recording `VERDICT: PASS` in `docs/testing/VISUAL-MA.3.md`.

# Context

- `make docs-check` builds `site/` with `--strict`; serve it (`uvx --offline zensical serve site` or `uv run --no-project python -m http.server`).
- Zensical renders Mermaid inside a closed shadow root (`docs/knowledge/LESSONS.md`): only screenshots prove a diagram drew. Diagrams: `docs/how-it-works.md` (x2) and `docs/deploy/fair-queuing.md`.
- Nav comes from `zensical.toml`: Home, Getting started, Local, CI/CD, 4 deploy pages, 2 framework pages, 4 reference pages, How it works, Security, Troubleshooting, Changelog.

# Acceptance

```console
$ make docs-check
$ ls site/index.html site/how-it-works/index.html site/deploy/fair-queuing/index.html site/reference/manifest/index.html
$ tail -3 docs/testing/VISUAL-MA.3.md    # VERDICT: PASS
```

- Screenshots at **1440x900 light**, **1440x900 dark** (`prefers-color-scheme: dark`) and **390x844** for `index`, `getting-started`, `how-it-works`, `deploy/fair-queuing`, `reference/manifest`.
- Both Mermaid diagrams render as diagrams (shapes + labels), not as code blocks; capture at desktop and mobile.
- Every nav entry loads; no clipped code block, horizontal scroll or overlapping table on 390x844; no broken image or favicon.
- Report written to `docs/testing/VISUAL-MA.3.md` ending in `VERDICT: PASS`, listing each page/size/diagram checked.

# Notes

- Fix only `zensical.toml`, `docs/stylesheets/**` and `docs/brand/brand.css`; page-content defects are `MA.3.2` follow-ups.
- If anything else fails, record `VERDICT: FAIL` with exact pages and open a follow-up; do not silently patch around it.
