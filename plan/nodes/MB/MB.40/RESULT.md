# Gate: MB.40 — GEPA G4 harness overhaul, P0s, dedup, god-file splits, CLI craft & visual docs

## Evidence — full `make check`, run once on this tree

```
$ make check
cargo fmt --all -- --check                      clean
cargo clippy --workspace --all-targets -D warnings
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)
cargo test --workspace --all-targets            all suites ok
make test-critical                              Ran 53 tests — OK
docs-unit                                       112 passed, 65 subtests
docs-check (zensical build --strict)            No issues found
docs-test                                       48 fences: 30 executed, 18 accounted, 0 failed
rs-no-python / rs-no-default                    0 pyo3 deps, static_feature_off ok
seam-check / platform-matrix-check / workflow-check / packaging-gates-test
zed-check                                       OK (59 properties), 44 tests
plan-validate                                   plan OK
CHECK=0
```

Also green: `guard.py`, `critical_guard.py` (every protected change authorized by its node), `make fast-check` on every slice.

## Slices

| Slice | Outcome |
|---|---|
| MB.40.1 | GEPA G4 `.opencode/` bundle, `make fast-check`, 7 regex guards retired (`14b4047`, pre-existing). |
| MB.40.2 | `88fd2cf` — the four live P0/High bugs: store-less `blastit check` no longer crashes (no listener, no control fd, `_send` is a no-op); `$?` observations are restored to `$1` before analysis so table grants pass and ungranted columns still fail, including `IN ($?, ...)`/`VALUES ($?), ...` tails; `check --learn` proposes for declared views and refuses scanner probes/static-covered paths; coverage flush gated on a real build decision (`cargo:rustc-cfg=coverage`), `LLVM_PROFILE_FILE` inherited only when set. |
| MB.40.3 | `e6b1291` + `8cdf457` + `84ff143` + `3d4d16f` + `715d6af`, gate `0490e41` — `escape_control` and the SQL literal rules come from their canonical owners; `Acceptor`/`Editor`/`static PORTS` are concrete (real loopback accept test replaces the fake); one `proxy_specs` wiring replaces three; `ObsVerdict` deleted (`LearnVerdict` behind one byte-identical wire adapter); one bounds-checked ELF reader in `blastit-sys::elf` (13 adversarial fixtures) for `host.rs` and PHP `discovery.rs`; `mock_pg.rs` retired for a real `TcpListener` PostgreSQL upstream with a parity test; `RequestIo` deleted for concrete `ChanIo` on real `socketpair` frames. |
| MB.40.4 | `fd9e28e` + `dfcf88f` + `834b2bc` + `0cc93c8`, gate `1fde3df` — four god-files split (scheduler 5,650 → 9 modules; config 4,463 → 13 with derived wire structs replacing 18 `json_*` accessors; `analysis.rs` 2,749 → 10 with an injected `AnalysisCache`; stage0/1 → 10 modules with fork/exec parameter structs). Every split proved behaviour-preserving (160/160 function bodies byte-identical for the scheduler; 477 operator literals identical for stage 0/1). |
| MB.40.5 | `7caf56a` + `4787d44` + `552bd57`, gate `41c6cb7` — `--version`/`help`/preset validation with exit `2`; one typed per-stream `Colour` resolved at the composition root (usage text no longer colours a piped stderr); banner with clickable `http://127.0.0.1:PORT`, `[LEARN MODE]`, view table; structured `reload` event with aligned 23-column output; phase lines before blocking waits; prose refusals on stderr; `check` names the dead-grant `sql`, `proxy`, observed MIME and exact `fix.to`; `fmt --check` prints a real unified diff (including a missing-final-newline and CRLF); `--learn --dry-run` prints the exact bytes `--learn` writes. |
| MB.40.6 | `4d6b41c` — README + `docs/**` visual hierarchy: command-first openings, Mermaid on every guide, caveats in admonitions after command 1, non-empty framework grants, diary headings replaced, roadmap language purged, and every documented console block aligned to what `blastit 0.2.0` prints. |

## Review outcomes (filtered)

Sixteen review passes (`review-arch` + `review-ux` per slice, `visual-checker` on the docs). Genuine defects found and fixed include: a `cfg(coverage)` flush that never compiled in; `...` tails making Django `__in`/`bulk_create` SQL unparseable; a learn fallback that donated a `/.env` probe's SQL to the real view; 3/5 new Python tests failing teardown (CI red); a store-less control-channel double close; a `privileged` member missing `serde(default)` (a pre-D6 config would have stopped booting); a panic-free ELF reader proven to bound every allocation; a protected scan silently narrowed by the stage1 split (restored by a new crate-wide test); `check`'s dbproxy drain swallowing every `pg.refused`/`sql.denied` warning on the success path; a vacuous `RequestIo` guard that would have certified a re-introduced trait; and every stale documented line.

Filtered as non-actionable: RESULT.md wording, `scripts/check_*.py` regex-guard requests, subjective naming, the `NAME_COLUMN` width trade, `frame()`'s bounded `unwrap_or`, and pre-existing shapes (`fork_dbproxy_dev`'s 7 args, `dev`'s own argv exit `1`).

## Follow-ups recorded (not blocking this gate)

- `python/tests/critical/test_check_contract.py` keeps its own `MockPgServer` copy (outside every slice's paths); the two ADR-0016 protected pins and `critical_loaded_files.rs`'s source scan should eventually become behavioural assertions (a `test-curator` job).
- `docs/{ci,security,local}.md` and the two deploy pages are still above the <130-line rule; `audit_v1.rs` is ~1,900 prod LOC and wants its own split node.
- `critical_dev_reload`/`critical_rewrite` contain timing-sensitive tests that flake under a loaded host (~8/8 cores from an unrelated concurrent job); each passes in isolation and as a file — no code defect observed.
