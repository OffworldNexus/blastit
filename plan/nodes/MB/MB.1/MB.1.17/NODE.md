---
id: MB.1.17
title: "Regression-test the release hygiene trio in make check"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.1.11]
paths:
  - python/tests/test_docs_assets.py
  - python/tests/test_release_scripts.py
  - "scripts/build-wheel.sh"
  - "scripts/dev-version.sh"
  - Makefile
  - docs/brand/logo.svg
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:44"
updated: "2026-11-13T19:42"
---

# Goal

Turn the three one-shot manual proofs of the `MB.1` build/brand hygiene into permanent, offline regression tests, so a re-add of the old spelling, a 404 link in the wheel, or an invisible logo fails `make check`.

# Context

- `docs/testing/AUDIT-MB1.md` (`C6`/`C7`/`C8`/`C13`, `G6`). `MB.1.7` proved by hand: `build-wheel.sh` rewrites the README and restores it on every exit; `docs/brand/logo.svg` has a light-scheme cutout at 15.22:1; `dev-version.sh` now fails loudly on a lagging requirement. `MB.1.9` proved the `dev-version.sh` guard on throwaway copies. **None** of it is a test.
- The publish workflow rewrites the README *before* `build-wheel.sh`, so `check-long-description.py` would pass even if the script's own rewrite/trap regressed. `make docs-test` already builds a wheel, so a stdlib metadata assertion is nearly free.
- `make docs-check` reported "No issues found" for an *ill-formed* SVG in MB.1.7 (it rendered as alt text), so XML validity of `docs/brand/*.svg` is a real, currently unguarded failure mode. `prefers-color-scheme` only covers the OS light preference; the site's light **toggle** on a dark OS still shows the paper cutout (MB.1.7 follow-up) — assert whatever mechanism ships and state the residual in `RESULT.md` if it is not fixed here.
- `grep -rn CARGO_NET_OFFLINE=1` and the `blastit-events` `serde` removal are one `grep`/`tomllib` assertion away from being permanent.

# Acceptance

```console
# 1. Asset test (stdlib only, docs-unit): every docs/brand/*.svg parses as XML; logo.svg's
#    light-scheme fill has >= 3.0:1 contrast against #f4efe3 (compute from the hex values, do not
#    hard-code the ratio). Show it failing on a /tmp copy with the media rule removed and with a
#    literal "<img>" in the <style> comment.
$ UV_OFFLINE=1 uv run --no-project python -m pytest python/tests/test_docs_assets.py -q

# 2. Wheel description test: after `make docs-test` (which builds a wheel), assert every
#    dist/blastit-*.whl METADATA has no `](docs/`, `](ARCHITECTURE.md` or other relative link, and
#    has the github.com/OffworldNexus/blastit/blob/ form. Wire it so `make docs-test` fails when the
#    assertion fails (a stdlib script invoked after build-wheel is fine; no new dependency).
$ make docs-test
# on a /tmp copy with the rewrite removed from scripts/build-wheel.sh, `make docs-test` exits non-zero
# and names the relative link; paste that failing run and the green one.

# 3. build-wheel restore: a test that computes README.md's digest, runs
#    `MATURIN=/bin/false scripts/build-wheel.sh` (fails before any build), and asserts the digest is
#    unchanged and no `.blastit-README.*` temp file remains. Offline, no wheel built.
$ uv run --no-project python -m pytest python/tests/test_release_scripts.py -q

# 4. dev-version guard: add a read-only `--check` mode (no rewrite, no `cargo metadata`) and test it
#    on a synthetic tree: a lagging `version = "0.1.0", path` exits 1 naming `file:line`; a consistent
#    tree exits 0. The script is in `paths`; keep the rewrite path and the metadata proof unchanged.

# 5. Spelling/dependency invariants: assert no `CARGO_NET_OFFLINE=1` in Makefile/scripts/.github and
#    no `serde` dependency in crates/blastit-events/Cargo.toml (tomllib).

# 6. All of the above run offline in `make check` (docs-unit/docs-test); no network, no Docker, no sleep.
$ make docs-unit && make docs-check && make docs-test    # exit 0
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ uv run --no-project tools/plan.py validate
```

# Notes

- `depends_on: MB.1.11` — that leaf is rewriting `Makefile`/`scripts/**`; land this after it to avoid contention. Read the Makefile's mutants-scratch targets before editing it.
- Keep user docs lean: at most one sentence in `docs/knowledge/LESSONS.md` if you record a gotcha; no new public page.
- Do not touch protected `critical_*` tests or product Rust.
- `RESULT.md` must paste: the failing/green `docs-test` pair, the SVG probes, the dev-version synthetic-tree outputs, and the `make docs-unit` tail.
