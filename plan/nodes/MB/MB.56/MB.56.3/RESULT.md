# Gate: MB.56.3 — four user-story E2E suites replace 29 fragmented test files

Base `d313856` → `853adbc`. Five sub-slices, each verified with `make fast-check`; full `make check` run **once** here.

## `make check` (exit 0, zero warnings)

```
e2e-universal: 6 passed, 0 skipped, 0 failed — one artifact served every passing leg
editors/zed: 11 passed (grammar↔Zed highlight/query/corpus agreement)
tools/plan.py validate → plan OK
```

## Consolidation

| Suite | `TESTING_PLAN.md` | Tests | LOC |
|---|---|---|---|
| `e2e_cli.rs` | §3 CLI surface & safe handoff | 21 | ~1,378 |
| `e2e_dev.rs` | §2 & §4 WSGI / PHP / static / reload / `--learn` | 25 | ~3,150 |
| `e2e_check_fmt.rs` | §5 check, exit table, learn, fmt | 24 | ~1,700 |
| `e2e_prod_refusals.rs` | §6 fail-closed startup & integrity | 29 | ~2,150 |
| `common/` | shared harness (spawn, pg wire stub, php host, static, views) | — | ~2,137 |

`crates/blastit/tests/`: **26,643 LOC / 29 files → 11,513 LOC / 4 suites + `common/`**.
The plan's 5,500-LOC target was not reachable without dropping §2–§6 coverage; the
arithmetic and every deliberate drop (with its successor) are in each slice's
`RESULT.md` and in `MB.56.3.5/RESULT.md`.

Real behaviour changes caught by the new oracles (all fixed here, not papered over):
`--port` docs said exit `1` (it is `2`), three pages claimed `check --learn` never
writes into an existing view (it does), `blastit fmt [PATH]` was untested,
`no_observations`/`proxy_unused`/`raw_ingress`/`ingress_mismatch` had no oracle,
the `trait connection` hint was untested, and a symlinked-`Blastfile` refusal was
**reverted** (it does not remove the swap hazard and breaks Kubernetes ConfigMap
mounts — stage 1's re-read is the real barrier, and its TOCTOU test stays).

New temporary human item: `H-017` (`TESTING_PLAN.md` §2 promises byte-range requests;
`blastit` answers full `200`).
