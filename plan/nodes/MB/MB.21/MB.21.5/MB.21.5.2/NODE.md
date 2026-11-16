---
id: MB.21.5.2
title: "Execute Blastfile doc fences live, make docs_fences strict and guard the retired file"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.21.5.1]
paths:
  - "python/tests/runnable_docs/**"
  - python/tests/test_retired_manifest.py
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Close the loop the DX gate needs: every `Blastfile` example on the site is **executed** by
`make docs-test`, `docs_fences` refuses a TOML manifest fence, and a guard with a negative self-test
keeps the retired `blastit.toml` from creeping back into a live page or fixture.

# Context

- Inventory shape (`python/tests/runnable_docs/inventory.toml`): every published page needs an entry, and
  every `console` fence needs a handler (`install`, `dev`, `dev-curl`, `check`, `summary`, `presets`,
  `static`, …). `run.py` checks the inventory **before** anything runs, and a `static`/`example` entry
  needs a `reason` of real words (≥ 15 characters) saying what the gate checks instead.
- What flips: the two language pages' console fences move from `mode = "static"` (their reasons named
  `MB.21.5.2`) to the live handler that proves them — `dev` against the `hello` fixture for Local,
  `check` against `shop` for CI, `presets` plus `static` (cloud account, same reason style as
  `docs/deploy/digitalocean-paas.md`) for Deployments. Every page `MB.21.5.1` rewrote keeps its existing
  modes; only a fence whose command changed gets a re-check.
- `crates/blastit-manifest/tests/docs_fences.rs`: drop the ```` ```toml title="blastit.toml" ````
  opening, make it a **failure** that names the page, and keep the per-file count rule (a page that adds
  or drops a complete document must say so).
- New guard `python/tests/test_retired_manifest.py` (stdlib, `unittest` or pytest): scans `README.md` and
  every published page plus `e2e/`, `scripts/`, `python/tests/**/*.py` and `Makefile` text for
  `blastit.toml`, with one allowlist — the two language pages (migration prose), `docs/changelog.md`,
  `docs/{decisions,design,knowledge,testing,research,archive}/`, plan briefs, and the tests that assert
  the migration refusal. Add it to `make docs-unit`'s pytest list.
- The guard needs its own negative self-test: a temporary tree containing a page that names the retired
  file must produce a finding (and an allowlisted page must not), following the style of
  `python/tests/test_docs_assets.py`'s unit tests.

# Acceptance

```console
$ make docs-test
$ make docs-unit
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_retired_manifest.py -q
$ grep -n '"static"' -A 2 python/tests/runnable_docs/inventory.toml | grep -c 'MB.21.5.2'
$ cargo test -p blastit-manifest --locked --test docs_fences
$ make check
```

1. `make docs-test` reports **0 accounted-but-unexecuted** fences on the two language pages, and the
   per-page totals in `RESULT.md` show the new executed count.
2. The `MB.21.5.2` grep count above is `0`: no reason says "the parser is missing" any more.
3. A ```` ```toml title="blastit.toml" ```` fence anywhere fails `docs_fences` naming the page (self-test
   with an inline fixture), and the count rule still catches a drifted page.
4. `test_retired_manifest.py` fails a temp page that names the retired file, passes the allowlisted ones,
   and is green on the repository; `make docs-unit` includes it.
5. `make check` exit 0.

# Notes

- Do not widen an allowlist to get green: every entry needs a sentence in `RESULT.md` saying why that
  file legitimately names the retired format.
- If a documented command genuinely cannot run in the offline gate (a cloud account, a registry), keep
  `static` with the *real* reason and name the `e2e/` scenario that proves it instead.
