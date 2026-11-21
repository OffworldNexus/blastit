# Result MB.2.4

## Summary

Resolver (`static_asset.rs`) and responder (`static_response.rs`) are landed, protected and
audited. The boundary is one `O_PATH|O_DIRECTORY|O_CLOEXEC` dirfd per root, `openat2` with
`RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS` on every probe, an
`O_RDONLY|O_CLOEXEC|O_NONBLOCK|O_NOCTTY` read open only for a regular file whose
`(dev, ino)` matches the probe, and a documented per-segment `O_NOFOLLOW` walk fallback —
never `canonicalize()` plus a string prefix. `respond` builds the whole head
(`Content-Type` closed table, `Content-Length`, quoted strong `ETag`, IMF-fixdate
`Last-Modified`, `nosniff`, `(answer kind, answering path)` cache rule, `304`) without ever
reading a byte, moving the one `OwnedFd` into the body and never `dup`ing it. This epic
adds no process, role, uid or supervisor: both modules are ordinary scheduler code, and
the no-view-spawn property of the wired path is MB.2.5's (asserted in `critical_rewrite.rs`).

The epic gate's test-curator audit (`docs/testing/AUDIT-MB.2.4.md`) returned **PASS on tier,
teeth and fixture hygiene, no BLOCKER**, and filed three gap leaves — all three are closed
here: MB.2.4.3 (G1–G4, resolver), MB.2.4.4 (G5–G6, responder), MB.2.4.5 (G7–G9, flow).
Two of the closed gaps were **real product bugs**, fixed in `scheduler.rs`: a buffered static
`Forward` admitted one half-body more than every other route, and a zero-byte read could park
a stream until `request_timeout`.

## Evidence

Tip `7f1d4d0` on `release/0.2.0`. Rows 1–3 and 5 were re-run by the doc-writer on this tree;
row 4 cannot pass in its literal single-node form and is recorded with the branch-wide
invocation the lead ruled on (`MB.2.4/LOG.md`, 2026-11-13T22:44).

| # | `# Acceptance` (NODE.md) | Status | Command | Evidence |
|---|---|---|---|---|
| 1 | `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked` | **met** | same, at tip | `test result: ok. 14 passed; 0 failed` (11 at MB.2.4.2, +3 by MB.2.4.4) — `MB.2.4.4/RESULT.md`; re-run here |
| 2 | `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked` | **met** | same, at tip | 15 targets, 0 failed: lib 46, `critical_rewrite` 32, `critical_static_assets` 14, `critical_static_resolve` 18, doctests 1, plus admission 11 / ingress 9 / ctl 9 / fairness 17 / flow 6 / head_allocs 2 / isolation 13 / lifecycle 18 / memory 14 / routing 14 — re-run here |
| 3 | `make rs-lint` | **met** | same, at tip | `cargo clippy --workspace --all-targets --locked -- -D warnings` → `Finished`, exit 0 — re-run here |
| 4 | `make critical-guard BASE=develop NODE=MB.2.4` | **not met as written; met by an equivalent** | `uv run --no-project tools/critical_guard.py --base origin/develop` + `--node` repeated for `MB.1.6 MB.1.8 MB.1.12 MB.1.13 MB.1.14 MB.1.15 MB.1.16 MB.2.4.1 MB.2.4.2 MB.2.4.3 MB.2.4.4 MB.2.4.5 MB.2.5.1 MB.2.5.2` | literal form: `FAILED: 4 protected critical test(s) ... in node(s) MB.2.4` — the four are `MB.1.6`/`MB.1.8` edits that predate this epic (`develop` has no such commits). Full set: `critical-guard: OK: all 11 protected change(s) are authorized by node(s) …` — re-run here; ruling in `MB.2.4/LOG.md` |
| 5 | `make rs-test` | **met** | same, at tip | `cargo test --workspace --all-targets --locked` + `--doc`: exit 0, 83 `test result: ok` lines, **1,966 tests passed**, 0 failed — re-run here |

Why row 4's replacement is legitimate: `--node` is `action="append"`
(`tools/critical_guard.py:262`), so the guard's contract is "*these* nodes authorize *these*
paths"; a leaf cannot authorize a sibling's protected edit, and no single epic can authorize
MB.1's branch-wide edits. The invocation above is the same tool, same base, every authorizing
node named — not weakened, not skipped.

### Gate evidence beyond the acceptance block

| Check | Result | Source |
|---|---|---|
| `make check` | exit 0, 98 s over this tree (earlier 104 s at `69504ed`, and at `fafae8a`, `0796921`); tail `plan OK`, `47 fences: 28 executed, 19 accounted, 0 failed` | `/tmp/opencode/mb245-check.log` (its `critical_guard --base HEAD` line is *report-only* by Makefile design and names the two then-unstaged protected files) |
| `make e2e` | exit 0 at the `MB.2.7` state: deployment half **356** `^  ok` lines + `docs_deploy: 40/40`, `exit 0 wall 31s`; static leg **72** checks against a real containerised `blastit dev` | `/tmp/opencode/mb211-e2e.log` (`grep -c '^  ok'` → 356; the log predates `0be2b2f`, so the static leg is in `MB.2.7/RESULT.md`), `MB.2.7/RESULT.md` |
| test-curator audit | **PASS** (tier, teeth, fixture hygiene); `critical_rewrite.rs` verdict: **deserves protected**; the two review-known gaps **confirmed** plus one new client-cancel gap, all filed | `docs/testing/AUDIT-MB.2.4.md` §Verdict, §GAPS |
| G1–G4 closed (resolver) | 8 filed survivors → 7 caught; backend recorded not copied, symlink-declared root, both file-ceiling sides, `0x20` legal | `MB.2.4.3/RESULT.md` |
| G5–G6 closed (responder) | descriptor moves (`as_raw_fd` equal, in-tree fd count `1→2→2→1`), `HEAD`/`304` leave only the root dirfd, each ceiling disjunct alone | `MB.2.4.4/RESULT.md` |
| G7–G9 closed (flow) | client cancel (parked and unparked), one queue entry that still expires, static half-body bound; **two product fixes** | `MB.2.4.5/RESULT.md` |
| Mutation — resolver | `static_asset.rs` **66/86 = 76.7 %** raw (was 59/86 = 68.6 %), **66/66** after the audit's equivalence classes; 20 survivors = 12 disjoint-bit `\|`→`^` + 7 `\|`→`&` flag degenerations + `541:5 → true` | `MB.2.4.3/RESULT.md` §Campaign |
| Mutation — responder | `static_response.rs` **152/159 = 95.6 %** viable (was 147/159 = 92.5 %), no mutant caught→missed; 7 survivors = `218:41 trim_ows` + six `civil_from_days` | `MB.2.4.4/RESULT.md` §Campaign |
| Hand-mutation proofs | Walk root escape (`caller_supplied_segments…` with the guard removed: `Walk` answers `file:Exact:/../../../etc/passwd:1440`); buffered-forward stall (delete `!` → `no response for stream 2`); zero-byte read (`STATIC_CHUNK_BYTES = 0`: 13 failed in 0.13 s with the guard, `EXIT: 124` without) | `MB.2.4.1` fixup 1, `MB.2.4.5` gap table |

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/static_asset.rs` | new resolver — 685 lines, 400 production: `segments`, `is_dotfile`, `classify`, `ImmutableGlob`, `same_inode`, `StaticRoot::{open, open_with, resolve, resolve_with}`, `STATIC_RESOLVE_FLAGS`, `Backend::Walk`, total errno table (MB.2.4.1) |
| `crates/blastit-scheduler/src/static_response.rs` | new responder — 317 lines, 200 non-comment: `respond`, `StaticResponse`/`StaticBody`, `http_date` (MB.2.4.2) |
| `crates/blastit-scheduler/src/config.rs` | `manifest_dir`, `static_max_bytes` with defaults (MB.2.4.1) |
| `crates/blastit-scheduler/src/lib.rs` | `pub mod static_asset;` / `pub mod static_response;` |
| `crates/blastit-scheduler/src/scheduler.rs` | MB.2.4.5's two fixes: static buffered `Forward` counts itself against the half-body bound; `pump_static` aborts on `want == 0` (file shared with MB.2.5.2) |
| `crates/blastit-scheduler/tests/critical_static_resolve.rs` | protected suite, 14 → **18** tests; 40-row matrix × S/F/N/D × Openat2/Walk |
| `crates/blastit-scheduler/tests/critical_static_assets.rs` | protected suite, 11 → **14** tests; response table + descriptor ownership + ceiling |
| `crates/blastit-scheduler/tests/critical_rewrite.rs` | +7 flow tests for G7–G9 (25 → 32; file owned by MB.2.5) |
| `crates/blastit-scheduler/tests/common/static_fixture.rs` | S/F/N/D fixture; adds `cap.bin` (sparse, at ceiling), `grow.bin`, `my file.css`, `current` symlink root |
| `docs/knowledge/LESSONS.md` | one bullet: holding a `spawn_blocking` read deterministically |
| `docs/testing/AUDIT-MB.2.4.md` | gate audit (test-curator, new) |

No `Cargo.toml`/`Cargo.lock` edit, no new dependency, `lib.rs` still `#![forbid(unsafe_code)]`.

## Follow-ups

1. **Resolver survivors left as equivalences (20/86).** Twelve disjoint-bit `|`→`^`
   compositions, seven `|`→`&` degenerations of `O_PATH`/`O_CLOEXEC`/`O_NONBLOCK`/`O_NOCTTY`
   with no unprivileged oracle, and `541:5 supports_openat2 → true` (indistinguishable on a
   kernel that has `openat2`). Only an e2e job on a seccomp-filtered runtime that returns
   `ENOSYS` can kill the last one; nothing in this crate can.
2. **`trim_ows` and `civil_from_days` (7 response survivors) — still unfiled.** `218:41`
   (`trim_ows` `&&`→`||`: tab-only OWS) and six `http_date` arithmetic mutants
   (`301:17`, `305:42`, `305:64`, `313:21`) survive because no vector exists for
   `2000-02-29`/`1900-03-01` or a one-sided tab. Cheap to add to
   `critical_static_assets.rs`; MB.2.4.4's review asked for it, no node was created, and the
   audit records both as accepted-with-reason.
3. **Depth-override observation seam.** Design §8's accepted depth-overlap override
   (`static_asset.rs` resolves / `rewrite.rs` decides / `static_response.rs` responds, not to
   be re-litigated) is honoured. What it leaves open is the test seam: `open_with`,
   `resolve_with` and `head_segments` are `#[doc(hidden)] pub` (sanctioned by §7.2), and
   MB.2.4.3's backend oracle reads `StaticRoot`'s derived `Debug` because §4.2 forbids a
   `backend()` accessor. No node filed; settle it if the frozen surface is ever revisited.
