---
id: MB.21.5.2
title: "Execute Blastfile doc fences live, make docs_fences strict and guard the retired manifest name"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.21.5.1]
paths:
  - "python/tests/runnable_docs/**"
  - python/tests/test_docs_assets.py
  - "crates/blastit-manifest/tests/docs_fences.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T01:49"
---

# Goal

Close the DX gate. Every `Blastfile` example on the site is executed by `make docs-test`,
`docs_fences` fails on any complete-manifest fence that is not a `Blastfile`, and a test named
`test_the_real_tree_carries_no_retired_toml_manifest` in `python/tests/test_docs_assets.py` makes the
retired file name impossible to reintroduce — mechanically, on `make check`, with no allowlist that a
lazy edit can widen.

# Context

- The precedent is the retired-PHP guard in the same module (`MB.3.30`): `Finding`, `guard_tree`,
  `live_briefs`, `RETIRED_STATUSES`, `BRIEF_DEFINING_THE_LIST`, the negative self-tests around line
  535, `test_the_real_tree_carries_no_retired_php_vocabulary` (line 802) and
  `test_the_guard_scans_a_real_tree` (line 808). Copy that shape — a new file or a new Makefile target
  is the wrong answer, because `docs-unit` already runs this module and `make check` already runs
  `docs-unit`.
- Compose the banned name so the guard module itself never carries it:
  `RETIRED_MANIFEST_NAME = "blastit" + ".toml"` and
  `RETIRED_MANIFEST = re.compile(r"blastit[.]toml", re.IGNORECASE)`. Then the module needs no
  self-exemption, and there is no "defining brief" exemption either: after `MB.21.5.1` every live
  brief is clean, and this brief is spelled the same way (the bracket form above).
- Scanned by the new `retired_manifest_findings(root) -> list[Finding]`; one hit fails with
  `file:line`, the matched text, and the fix: "read `./Blastfile` or `BLASTIT_MANIFEST`; the v1 format
  is retired and has no migration path".
  * `README.md`, `ARCHITECTURE.md`, `docs/VISION.md`;
  * every published page — reuse `python/tests/runnable_docs/fences.py:public_pages` so the set is
    "what zensical builds", not a hand-copied list;
  * `crates/**/*.rs` and `crates/**/Cargo.toml`;
  * `python/**/*.py` and `python/tests/runnable_docs/*.toml`;
  * `scripts/**`, `e2e/**` (Python, shell, YAML, Dockerfiles, Markdown) and the `Makefile`;
  * every live brief: `plan/nodes/**/NODE.md` whose `status` is not `done`/`superseded`.
- Records are never scanned: `docs/{decisions,design,research,knowledge,testing,archive}/**`,
  `docs/changelog.md`, `LOG.md`, `RESULT.md`, finished briefs. They are the reason the name can be
  recognised at all — do not "clean" one to get green.
- Negative self-tests in the same style as the PHP guard's: a temp product page that names it fails;
  the same text under `docs/decisions/` passes; a temp live brief fails; a `status: done` brief
  passes. Anti-vacuity test: the tree has product pages, Rust sources, scripts and briefs; at least
  one brief is live and at least one is finished; and at least one record (the changelog or
  `ADR-0012`) still names it, so the allowlist has a subject.
- Fences: `crates/blastit-manifest/tests/docs_fences.rs` drops the old opening constant (spelled from
  two fragments by `MB.21.4.4`) and makes any complete-manifest fence whose opening is not the
  `blastfile` one a **failure** naming the page and the expected opening; keep the per-file count
  rule and the `docs/frameworks/django.md` static fragment.
- `python/tests/runnable_docs/inventory.toml`: the two language pages' console fences move from
  `mode = "static"` (reason naming `MB.21.5.2`) to the live handler that proves them — `dev` against
  the `hello` fixture for the Local section, `check` against `shop` for CI, `presets` plus a `static`
  entry (cloud account, same reason style as `docs/deploy/digitalocean-paas.md`) for Deployments.
- Asymmetry in what this node may touch: the *readers* of the retired name live in `crates/` and
  `python/` and are already clean (`MB.21.4.4`); this node only adds the mechanism that keeps them
  clean.

# Acceptance

```console
$ make docs-test
$ make docs-unit
$ grep -n '"static"' -A 2 python/tests/runnable_docs/inventory.toml | grep -c 'MB.21.5.2'
$ cargo test -p blastit-manifest --locked --test docs_fences
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ make check
```

1. `make docs-test` reports **0 accounted-but-unexecuted** fences on the two language pages; the
   per-page executed totals are in `RESULT.md`.
2. The `MB.21.5.2` grep count is `0`: no reason says "the parser is missing" any more.
3. A complete-manifest fence opened with anything but the `blastfile` opening fails `docs_fences`
   naming the page (a fixture under `crates/blastit-manifest/tests/` proves it), and the per-file count
   rule still catches a drifted page; the `django.md` static fragment is still parsed.
4. `test_the_real_tree_carries_no_retired_toml_manifest` is green on the repository; the negative
   self-tests above fail/pass in both directions; the anti-vacuity test passes; and the test needs no
   allowlist entry for a product page, a crate, a script, a fixture or a live brief.
5. `make check` exit 0. `RESULT.md` states, with the file and test names, that the guard runs inside
   `docs-unit` and therefore inside `make check`, and pastes the mutation proof: add the retired name
   to a scanned page, `make docs-unit` fails naming `file:line`; restore and it passes.

# Notes

- Do not widen an allowlist to get green. If a *product* file legitimately needs the name, it is not
  legitimate: rewrite the file. If a *record* is flagged, the scan set is wrong, not the record.
- Do not add a `make` target, a new pytest file, or a `docs-test` special case: `docs-unit` already
  runs `python/tests/test_docs_assets.py`, so the guard is in `make check` by construction.
- If a documented command genuinely cannot run in the offline gate (a cloud account, a registry), keep
  `static` with the *real* reason and name the `e2e/` scenario that proves it instead.
- Keep the suite stdlib-only and fast: no network, no wheel, no subprocess beyond what the module
  already does.
