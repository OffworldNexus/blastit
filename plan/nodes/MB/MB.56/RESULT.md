# Gate: MB.56 — TESTING_PLAN.md test & codebase cleanup

Base `51cc758` → `6acc1e7`. Five child epics (`MB.56.1`…`MB.56.5`), 20 slices. Full `make check` run **once** here (each slice verified with `make fast-check`).

## Final gates (all exit 0)

```
make check                        → 0   (rs-test + test-critical, docs-unit, docs-check,
                                        rs-no-python/rs-no-default, e2e-universal, zed-check, plan-validate)
make zed-check                    → 0   (11 grammar↔Zed tests)
make docs-check                   → 0   (zensical --strict)
uv run e2e/run_apps.py            → 10 passed, 0 failed, 0 skipped
uv run e2e/run_prod.py            →  7 passed, 0 failed, 0 skipped   (root + rootless presets)
uv run e2e/run_packaging.py       →  4 passed, 0 failed, 3 loud SKIP (rpm/deb images not built here)
```

## Before → after

| Surface | Before | After |
|---|---|---|
| Rust production LOC (19 crates) | 144,007 | **94,879** (−49,128, −34 %) |
| Rust test LOC | 83,472 | **19,238** (−77 %), in 4 E2E suites + `common/` + 4 §7 suites + 2 boundary suites |
| inline `#[cfg(test)]` modules | 150 | **3** (`blastit-sys` ELF 140 LOC, `blastit-frame` frame 193 LOC, scheduler §6 feature-off leg) |
| Python package + tests | 24,479 | **1,916** (−92 %) |
| `e2e/` harness | 10,044 | **8,241**, now three story runners (`run_apps`, `run_prod`, `run_packaging`) |
| `crates/*/tests/**` files | 29 | **4** `e2e_*.rs` + `common/` |
| meta-linter scripts | 13 | **0** |
| plan-node citations in `crates/*/src` | 295 | **0** |

**Net: −49,128 lines of production Rust and −64,234 lines of tests/python/harness**, with
*more* real coverage: every published `Blastfile` fence parsed + `fmt`-canonical, the
documented `check` exit table observed (0/1/2/3 incl. pytest `5`), the full HTTP refusal
family (431/408/417/503/100-continue), LSP hover/format-on-save/`didClose`/UTF-16/both
quick-fix spellings, PHP `php2shell` immunity + per-view grants + entry pinning over a real
SAPI, and both deployment presets in real containers.

## Real defects the new oracles found (all fixed, not papered over)

`--port` exit code documented as `1` (it is `2`); three pages claiming `check --learn` never
writes into an existing view (it does); `blastit fmt [PATH]` untested; four finding kinds
(`raw_ingress`, `ingress_mismatch`, `no_observations`, `proxy_unused`) and the `trait
connection` hint untested; `blastit-lsp` failing *open* into an `exec` loop; the two
deployment presets proving the **other** page's store shape; five packaging call sites able
to exit 0 having proved nothing; the R42 skip ban and the root-only assertion gone from CI;
`make packages-test` passing no artifact; and `docs/security.md` naming two deleted harnesses.
A symlinked-`Blastfile` refusal was **reverted**: it does not remove the swap hazard it
named and breaks Kubernetes ConfigMap mounts — stage 1's re-read is the barrier, now tested.

## Open human items

- `H-017` — `TESTING_PLAN.md` §2 promises "byte-range requests"; `blastit` answers the full
  `200` by design. The plan needs the owner's decision (drop the phrase or implement `206`).
- `H-018` — pre-existing: split the >800-LOC god-files (`blastit-events`, `blastit-lsp`,
  `blastit-php-sapi/loader`, `blastit-sys/fanotify`, `blastit/src/supervise.rs`, scheduler
  worker/discovery). Sliced L1–L6 in `MB.56.5.1/RESULT.md`; deliberately not in `MB.56`.
