# Gate: MB.56.2 — enforce `TESTING_PLAN.md` §1/§7 across the workspace

Base `e6c8a89` → `48cf668`. Each of the five sub-slices verified with `make fast-check`; full `make check` run **once** here.

## `make check` (exit 0, zero warnings)

```
e2e-universal: 6 passed, 0 skipped, 0 failed — one artifact served every passing leg
cd editors/zed && cargo test --locked → 11 passed (grammar↔Zed highlight/query/corpus agreement)
uv run --no-project tools/plan.py validate → plan OK
```

## Test-LOC outcome

| Crate | Before | After | Cap |
|---|---|---|---|
| `blastit-scheduler` | 13,611 | 2 (one §6 feature-off leg) | — |
| `blastit-dbproxy` | 5,957 | 0 | — |
| `blastit-view-python` | 5,044 | 0 | — |
| `blastit-view-static` | 3,388 | 0 | — |
| `blastit-view-php` | 2,318 | 0 | — |
| `crates/blastit/src/**` | 11,591 | 0 (E2E suites own it from `MB.56.3`) | — |
| remaining nine light crates | ~15,000 | 333 (`blastit-sys` ELF 140 + `blastit-frame` 193) | ≤ 250 / ≤ 200 |
| `blastit-ingress` | 818 | 818 (kept whole — it **is** the §7 body-policy owner) | — |
| `blastit-manifest` | 15,017 | 1,500 (5 files) | ≤ 1,500 |
| `blastit-sqlpolicy` | 5,080 | 1,492 (3 files) | ≤ 1,500 |
| `blastit-http` | 3,854 | 889 (1 file) | ≤ 800 (documented overage: 300-line harness) |
| `blastit-lsp` | 413 | 600 (1 file) | ≤ 600 |

**Total test LOC: 83,472 → 5,633** while the four §7 suites gained genuinely documented
coverage (every published `Blastfile` fence parsed + `fmt`-canonical; statement-kind
refusals; CTE/`UNION` extents; `431`/`408`/`417`/`503`/`100-continue`; LSP hover,
format-on-save `null`, `didClose` clearing, UTF-16 spans, both quick-fix spellings).

`tools/coverage_report.py` also gained a fix (binaries-first object order, cross-crate
`FILES`) so `make coverage` works per suite.

Every deleted user-visible case is inventoried in each slice's `RESULT.md` and mirrored
onto `MB.56.3`/`MB.56.4`/`MB.56.5` logs for end-to-end re-proof.
