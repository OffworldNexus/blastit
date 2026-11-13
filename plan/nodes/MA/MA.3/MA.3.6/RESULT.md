# RESULT MA.3.6 — bump 0.1.1, changelog entry, wheel metadata proof

**Verdict: DONE** — version, lockfile and changelog are in place and every acceptance item
was run here. One acceptance line does not pass as written for a tooling reason
(`CARGO_NET_OFFLINE=1` is rejected by maturin, not by cargo); the equivalent command is green
and both runs are pasted below. Nothing was tagged, pushed or committed.

## What changed

- `Cargo.toml`: `[workspace.package] version` `0.1.0` → `0.1.1` (line 6).
- `Cargo.lock`: the 13 workspace crates' `version` only — 13 insertions, 13 deletions, no
  dependency churn (the `pregresql` git dependency, which is also `0.1.0`, is untouched).
- `CHANGELOG.md`: a new `## 0.1.1 (2026-11-12)` entry opening the file — four user-visible
  bullets (zero-`sudo` `blastit dev`, one-pass `--learn`, `blastit check` in CI, the leaner
  Local/CI/Deploy pages) linking `local/`, `local/#fill-the-manifest-in-one-pass`, `ci/`,
  `deploy/` and `getting-started/`; all four URLs and the anchor exist in `site/`.
- `docs/knowledge/LESSONS.md`: two appended gotchas (below).

## Acceptance

```console
$ grep -n 'version = "0.1.1"' Cargo.toml
6:version = "0.1.1"

$ CARGO_NET_OFFLINE=1 cargo metadata --locked --format-version 1 > /dev/null
metadata EXIT=0

$ scripts/check-release-tag.sh v0.1.1
tag v0.1.1 names the workspace version 0.1.1

$ git diff --stat Cargo.lock
 Cargo.lock | 26 +++++++++++++-------------
 1 file changed, 13 insertions(+), 13 deletions(-)
$ git diff Cargo.lock | grep '^[+-]' | grep -v '^[+-][+-]' | grep -v 'version = "0\.1\.[01]"'
(no output: every changed line is a workspace crate version)

$ sed -n '1,12p' CHANGELOG.md
# Changelog

What each release changes for whoever runs an app under blastit. The version
lives in `Cargo.toml` (`[workspace.package] version`), and a release is the tag
`v` plus that version — `scripts/check-release-tag.sh` refuses a tag that says
anything else, and [`RELEASING.md`](RELEASING.md) has the steps.

## 0.1.1 (2026-11-12)

The first hour with blastit now happens on your own machine, as you, in one pass.

- **Run it without `sudo`.** `blastit dev` runs your app the way production does

$ scripts/build-wheel.sh
    Finished `release` profile [optimized] target(s) in 25.60s
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl

$ unzip -p dist/blastit-0.1.1-*.whl '*/METADATA' | head -4
Metadata-Version: 2.4
Name: blastit
Version: 0.1.1
Classifier: Development Status :: 3 - Alpha

$ uv run --no-project scripts/check-long-description.py dist/*
blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl: ok

1 file(s), rendered the way PyPI renders them
```

`make check`:

```console
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
🍹 Building a mixed python/rust project
💥 maturin failed
  Caused by: Failed to build a native library through cargo
  Caused by: failed to parse environment variable `CARGO_NET_OFFLINE`
  Caused by: provided string was not `true` or `false`
make: *** [Makefile:125: docs-test] Error 1        EXIT=2

$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
test result: ok. 19 passed; 0 failed ... (critical_dev_and_fail_closed)
Ran 50 tests in 3.174s / OK              (python/tests/critical)
All checks passed!                       (ruff check, ruff format --check)
No issues found                           (zensical build --strict)
38 fences: 26 executed, 12 accounted, 0 failed · 24.7s   (docs-test, runnable docs)
plan OK                                   (plan.py validate)          EXIT=0
```

## Two deviations, with their evidence

1. **`CARGO_NET_OFFLINE=1` cannot reach `docs-test`.** Cargo accepts `1`; maturin 1.16.0
   parses the variable as a strict bool before it shells cargo, so `make check` (and
   `make docs-test`, `scripts/build-wheel.sh` through it) dies there while every plain-cargo
   target is already green. `CARGO_NET_OFFLINE=true` runs the identical offline gate and is
   green (above). Appended to `LESSONS.md`; `docs/design/ma-lean-dx.md` R42 still spells `=1`.
2. **The description check needs the release's README rewrite.** `scripts/build-wheel.sh` does
   not run `scripts/readme-absolute-links.py` (the `Publish` workflow does, before building), so
   a wheel built straight from the checked-in README fails with
   `` `src` in the rendered description is relative: '.github/brand/header-light.svg' ``. The
   evidence above is the release-faithful sequence — rewrite, `scripts/build-wheel.sh`,
   `scripts/check-long-description.py` (ok), then `git restore README.md` — so the tree carries
   no README change. Stale `dist/blastit-0.1.0-*.whl` artifacts were removed so `dist/*` is the
   0.1.1 set.

## Flake seen once (not touched)

`python/tests/critical/test_check_contract.py::…test_learn_refuses_an_inline_literal_with_exit_one_and_writes_nothing`
failed in one gate run with `AssertionError: Lists differ: [] != ["SELECT 1 WHERE 's3cr3t' = 's3cr3t'"]`
(the fake server saw no statement), then passed on rerun (`Ran 50 tests … OK`) and in the full
green `make check`. It is a protected test, so it was left exactly as it is.

## Follow-ups

- Fold `scripts/readme-absolute-links.py` into `scripts/build-wheel.sh` (or add a
  `make wheel-description`) so a locally built wheel passes the description check without a
  manual rewrite; `scripts/build-wheel.sh`, `Makefile`, `docs/design/` are outside this node.
- Spell `CARGO_NET_OFFLINE=true` in `make check`'s documented recipe (R42) and in future node
  acceptance lines.
- `crates/*/Cargo.toml` intra-workspace requirements still say `version = "0.1.0"`: they are
  caret ranges (`^0.1.0`) that `0.1.1` satisfies, and the crates are `publish = false`;
  `scripts/dev-version.sh` is what rewrites them for dev wheels.
- `RELEASING.md` still narrates the 0.1.0 release (steps, `git tag` examples) — the owner's
  step, out of this node's paths.
