# Result — MB.56.5.2.2

## What changed

- `crates/blastit/src/**/*.rs`: **114** plan-node citations removed — 105 of the
  `` (`MB.x.y`) `` form and 9 single-component ones (`MB.49` ×2, `MB.53` ×2,
  `MB.17`, `MB.50`, …) that the two-component regex never matched. Nine
  historical blocks were hand-rewritten to state the invariant they described
  instead of telling the story of the leaf that found it:
  1. `check/pytest.rs` — the retired `../../..` bundle-root candidate,
  2. `lib.rs` — "`pub` for now because … the next leaf" (now: sibling consumer),
  3. `lib.rs` — "No role calls it yet" on the `view.chan` transport,
  4. `stage0/gates.rs` — "The gate used to be a bare `if let Err(..)`",
  5. `stage0/gates.rs` — the `MB.3.44` aside on a manifest with no PHP route,
  6. `stage1/spawn.rs` — "the routing-table disagreement the binding builder used to refuse",
  7. `supervise.rs` — "a same-wakeup `SPAWN` used to shift them",
  8. `stage0/project_python.rs` — the `// transitional (…)` marker,
  9. `scripts/build-wheel.sh` + `e2e/apps/php/probes.md` prose (below).
  Every `// SAFETY:`, trap note and 1–3 line Rustdoc stays.
- Last citations outside `crates/blastit`: `crates/blastit-view-static/src/lib.rs`
  (`design MB.36`), `crates/blastit-view/src/path.rs` (`design MB.36` D3),
  `crates/blastit-py-sapi/src/discovery/mod.rs` (`MB.50`) and
  `crates/blastit/build.rs` (`MB.17`) — the single-component spellings that
  MB.56.5.2.1's `MB\.[0-9]+\.[0-9]+` sweep could not see.
- `cargo fmt --all` was required: HEAD's `cli/run.rs`, `config/settings.rs`,
  `stage0/{dev,fork_exec}.rs`, `stage1/spawn.rs`, `visibility.rs` and
  `blastit-view-static/src/static_asset.rs` carried style-edition-2024 import
  ordering that `make rs-fmt` (`cargo fmt --all -- --check`) rejects. The churn
  is import order only (`LESSONS.md` §47).

## LOC: whole `MB.56.5` sweep (`2dc31d6` → worktree)

`prod` = `crates/<c>/src/**/*.rs`, `test` = `crates/<c>/tests/**/*.rs`.

| crate | prod before | prod after | test before | test after |
|---|---:|---:|---:|---:|
| blastit | 24249 | 24072 | 11513 | 11513 |
| blastit-dbproxy | 4115 | 4115 | 0 | 0 |
| blastit-env | 1694 | 1694 | 0 | 0 |
| blastit-events | 3759 | 3758 | 0 | 0 |
| blastit-frame | 2949 | 2949 | 0 | 0 |
| blastit-http | 3402 | 3402 | 889 | 889 |
| blastit-ingress | 1109 | 1109 | 818 | 818 |
| blastit-lsp | 1656 | 1656 | 977 | 977 |
| blastit-manifest | 11876 | 11871 | 1500 | 1500 |
| blastit-php-sapi | 3603 | 3601 | 0 | 0 |
| blastit-py-sapi | 1996 | 1995 | 134 | 134 |
| blastit-py-shim | 1228 | 1228 | 0 | 0 |
| blastit-scheduler | 7727 | 7723 | 1763 | 1763 |
| blastit-sqlpolicy | 7648 | 7634 | 1492 | 1492 |
| blastit-sys | 6738 | 6737 | 152 | 152 |
| blastit-view | 1371 | 1371 | 0 | 0 |
| blastit-view-php | 4626 | 4613 | 0 | 0 |
| blastit-view-python | 3860 | 3858 | 0 | 0 |
| blastit-view-static | 1286 | 1284 | 0 | 0 |
| **total** | **94892** | **94670** | **19238** | **19238** |

- **Citations:** 295 occurrences of `MB.<n>[.<n>…]` in `crates/*/src` → **0**;
  `crates/blastit/src` alone went 114 → 0.
- **Diary paragraphs removed:** 9 multi-line historical blocks rewritten (list
  above), and 348 comment lines removed against 280 added across `crates/`.
- **Test LOC unchanged in every crate** — this sweep touched comments and
  reference prose, never a test.

## Stale references and their real owners

| Where | Was | Now |
|---|---|---|
| `crates/blastit/src/stage0/gates.rs` | `scripts/check-no-test-seam.sh` measures the seam's absence | `scripts/build-standalone.sh` (its `for seam in BLASTIT_TEST_*` loop, lines 161–175) |
| `scripts/build-wheel.sh` | comment naming the retired `_bin/blastit-dbproxy` | the single `_bin/blastit`; the allow-list itself is unchanged (`{"blastit", "blastit-lsp"}` plus the shims) |
| `e2e/apps/php/probes.md` | provenance paragraph and `R8` note citing `MB.3.32` / `MB.3.17` / `MB.56` | the same history without node ids; the 25-row table is byte-identical (`php2shell.py --self-test` → 25 checks PASS) |
| `--internal-json` | mentioned as if live | no code/script mention remains; the surviving references are the historical `docs/design/universal-distribution.md`, `docs/decisions/ADR-0017…` and `CHANGELOG.md` records. Behaviour owners: `crates/blastit/src/cli/`, `python/blastit/_blastit.py` |
| `e2e/run.py` | named as an e2e driver | no live reference; the four suite owners are `e2e/run_apps.py`, `e2e/run_prod.py`, `e2e/run_packaging.py`, `e2e/universal_binary.py` |
| `critical_fastcgi_lockdown` | implied to exist | stated as deleted with the FastCGI param channel; each matrix row's owner is its `e2e/php2shell.py` probe |

## Gates

| Command | Exit |
|---|---:|
| `make fast-check` | 0 |
| `make docs-check` | 0 |
| `make zed-check` | 0 |
| `uv run --no-project e2e/run_apps.py` | 0 |

`rg 'MB\.[0-9]+\.[0-9]+' crates/*/src` → no matches (exit 1).

## Follow-ups

- `scripts/*.sh|py` still carry ~110 `MB.*` citations (`platform_matrix.py`
  alone has ~60, including `SHIM_REACHES` comments and footnotes). They were
  outside this node's reference list; a later sweep should strip them the same
  way, keeping every support statement.
- `crates/*/tests/**` and `e2e/*.py` comment headers still cite nodes
  (`e2e/php_sapi_probe.py`, `e2e/run_packaging.py`, `tests/common/mod.rs`); the
  same treatment applies, but test files were deliberately left untouched here.
