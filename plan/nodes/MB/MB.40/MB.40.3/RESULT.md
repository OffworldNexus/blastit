# Gate: MB.40.3 — Deduplicate forks & seams

## Evidence

| Check | Command | Result |
|---|---|---|
| Rust + Python leaf gate | `make fast-check` | exit 0 (148 `test result: ok`, ruff clean, `plan OK`) |
| Feature-matrix (no Python / no default features) | `make rs-no-python rs-no-default` | exit 0 (0 pyo3 deps; `static_feature_off` 2 passed) |
| Protected critical tests | `critical_guard.py --base e6b1291 --node MB.40.x` per slice | OK on every slice, each protected change authorized by its node |
| Invariants | `uv run --no-project tools/guard.py` / `plan.py validate` | OK / `plan OK` |
| `mock_pg` retirement | `grep -rni "mock_pg\|MockPg" crates/` | no matches |

Full `make check` (wheel, standalone ELF matrix, runnable docs, Zensical) runs **once** at the `MB.40` gate, not here — MB.40.3 is an inner container, so running it twice would be pure gate tax.

## Slices

| Slice | Outcome |
|---|---|
| MB.40.3, MB.40.3.1 | Commit `e6b1291` + `8cdf457`: `escape_control` and `fold.py`'s SQL rules now come from their canonical owners (`blastit_manifest`, `_blastit` FFI), `Acceptor`/`Editor`/`static PORTS` are concrete (real loopback accept test replaces the fake, stack-formatted ports), `dbproxy_wiring::proxy_specs` replaces three `ProxySpec` boot loops, and the `ObsVerdict` fork is gone — `LearnVerdict` crosses the wire through one serde adapter with a byte-identical `--json` record. |
| MB.40.3.2 | Commit `84ff143`: one panic-free, bounds-checked ELF reader in `blastit-sys::elf` (499 prod LOC) now serves both `host.rs` (`SHT_DYNAMIC`) and PHP `discovery.rs` (`.dynsym`); −464/+148 lines across the two callers, 13 adversarial fixtures (truncated header, wrapping counts, table past EOF, oversized section) each asserting a typed error and never a panic. |
| MB.40.3.3 | Commit `3d4d16f`: `tests/common/mock_pg.rs` (282 LOC) and its inline duplicate are deleted, replaced by one real `TcpListener` PostgreSQL-v3 upstream (`PgUpstream`) shared verbatim by both critical suites, with a parity test that fails the build if the two copies ever drift. Test counts unchanged (20 and 9), all 231 assertion sites preserved, `crates/` free of `mock_pg`/`MockPg`. |
| MB.40.3.4 | Commit `715d6af`: the `RequestIo` trait (zero implementers, six in-file fakes) is deleted for the concrete `ChanIo` transport; the six cases now run on real `UnixStream::pair()` frames with every assertion intact; `serve`/`ChanIo` are no longer published-but-uncallable; and the ADR-0016 scan now refuses `RequestIo` in **every** file — a narrowing, proved by a new in-test plant of the old home. |

## Review outcomes (noise filtered)

`review-arch` and `review-ux` ran on every slice. Rejected as non-actionable: RESULT.md wording, `frame()`'s bounded `unwrap_or(u32::MAX)` (rebutted with the ≤1 MiB body bound), and doc-only `docs/design/*` staleness (recorded as a follow-up). Fixed because genuine: the vacuous `RequestIo` rule (MAJOR — a re-introduced trait would have scanned clean), the unguarded 346-LOC duplicate harness (MAJOR), two diagnostic regressions in ELF error text, a dead `logs.sql_regions` wrapper, and a lost `HttpError::Accept` coverage point.

## Accepted deviations

- `crates/blastit-dbproxy/tests/common/pg_upstream.rs` exists as a byte-identical copy (a test target can only `mod` its own crate's `tests/` tree), enforced by `crates/blastit/tests/common_harness_parity.rs`.
- `crates/blastit/tests/critical_artifacts.rs` and the two `blastit` critical suites carry the authorized, assertion-preserving fixture/exemption edits above; `python/tests/critical/test_check_contract.py` keeps its own independent `MockPgServer` (outside every slice's paths) — follow-up.
