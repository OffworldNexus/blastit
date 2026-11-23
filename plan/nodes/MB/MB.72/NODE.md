---
id: MB.72
title: Bump GitHub Actions off Node 20 so no job emits the deprecation warning
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths: [".github/workflows/**", ".github/actions/**", docs/ci.md, RELEASING.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T11:04"
updated: "2026-11-23T11:09"
---

# Goal

Every `Packages` job shows the same annotation (17 warnings on one run):

```
Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4
```

Inventory on this tree:

| uses | count |
|---|---|
| `actions/checkout@v4` | 49 |
| `astral-sh/setup-uv@v10.3.2` | 39 |
| `actions/download-artifact@v4` | 22 |
| `actions/upload-artifact@v4` | 19 |
| `dtolnay/rust-toolchain@stable` | 17 |
| `Swatinem/rust-cache@v2` | 16 |
| `actions/setup-python@v5` | 9 |
| `actions/upload-pages-artifact@v3`, `actions/deploy-pages@v4`, `actions/configure-pages@v5` | 1 each |

Bump **only** what is needed so no job warns, and no more:

1. For each action, determine from GitHub (tags/releases, or the action's `action.yml` `runs.using`) which **current major** runs on Node 24. Bump to that major **as a major tag** (`@v5`), not a SHA, matching the repo's existing style.
2. Known candidates: `actions/checkout` (v4 → v5), `actions/download-artifact` (v4 → v5), `actions/upload-artifact` (v4 → v5), `actions/setup-python` (v5 → v6). Verify each rather than assuming.
3. Do **not** bump an action that already runs on Node 24 (`astral-sh/setup-uv@v10`, `dtolnay/rust-toolchain` composite) just for symmetry.
4. Check the behavioural deltas before bumping, and adapt call sites if required: `upload-artifact@v5`/`download-artifact@v5` keep `pattern`/`merge-multiple`/`name` but changed defaults; `setup-python@v6` drops some inputs. Any needed adjustment must be stated in `RESULT.md`.
5. Keep the pins uniform across `.github/workflows/*.yml` and `.github/actions/**/action.yml` — one version per action, no stragglers. A `grep` in `RESULT.md` proves it.

# Acceptance

- `grep -rhoE "uses: [^ ]+" .github/ | sort | uniq -c` shows one major per action and **no** `@v4` for checkout/artifact actions, and no action known to target Node 20.
- A real run proves the annotations are gone: after merge the conductor will point at the `Packages` run; locally, at minimum `actionlint` (if available) or a YAML parse of every workflow, plus `make fast-check`.
- Nothing else changes: same triggers, same permissions, same artifact names/patterns, same matrix wiring.
