# Result MB.2.5

## Summary

`rewrite.rs` (new, 166 production lines) owns the whole precedence as one total function —
exact file → directory `index` → declared SPA / front-controller fallback → the route's own
app → `404`, with the method gate (`405` + exactly `Allow: GET, HEAD`) ahead of the body gate
(`413`). `route.rs` carries `static_policy`/`static_forward` into `RouteInfo`; `lib.rs` exports
the module. `scheduler.rs` wires it in: roots are opened by `run` before the first request, a
steps 3–4 refusal is answered in `begin_raw` *before* admission (no slot, no queue entry, no
descriptor), and `dispatch_static` resolves, decides and answers on the existing `http`
channel — no `api` frame, no view fork, no uid. The body pump is one `spawn_blocking` `pread`
per slot feeding the existing credit path; `slot_releasable(holds, pumping) = holds && !pumping`
is the only predicate and `release_static_slot` the only decrement, so a slot is released
exactly once even on timeout, cancel and short read.

The bug the node names cannot happen: a denial is final. A `Refused`/`Hidden`/`SpaLost`
outcome answers `400`/`404` even with `spa = true` and an app armed, and the flow suite proves
the refusal never becomes `200 index.html` or a forward (also replayed against a real boot in
`make e2e`). MB.2.4.5's audit leaf (G7–G9) added the flow oracles the audit found missing and
fixed two real pump/queue bugs in this file.

## Evidence

Tip `7f1d4d0` on `release/0.2.0`. Rows 1–3 and 5 were re-run by the doc-writer on this tree;
row 4 cannot pass in its literal single-node form and is recorded with the branch-wide
invocation the lead ruled on (`MB.2.4/LOG.md`, 2026-11-13T22:44).

| # | `# Acceptance` (NODE.md) | Status | Command | Evidence |
|---|---|---|---|---|
| 1 | `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked` | **met** | same, at tip | `test result: ok. 32 passed; 0 failed` in 0.17 s — 13 pure (MB.2.5.1) + 12 flow (MB.2.5.2) + 7 flow (MB.2.4.5); `MB.2.5.2/RESULT.md`, `MB.2.4.5/RESULT.md`; re-run here |
| 2 | `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked` | **met** | same, at tip | 15 targets, 0 failed: lib 46, `critical_rewrite` 32, `critical_static_assets` 14, `critical_static_resolve` 18, doctests 1, plus admission 11 / ingress 9 / ctl 9 / fairness 17 / flow 6 / head_allocs 2 / isolation 13 / lifecycle 18 / memory 14 / routing 14 — re-run here |
| 3 | `make rs-lint` | **met** | same, at tip | `cargo clippy --workspace --all-targets --locked -- -D warnings` → `Finished`, exit 0 — re-run here |
| 4 | `make critical-guard BASE=develop NODE=MB.2.5` | **not met as written; met by an equivalent** | `uv run --no-project tools/critical_guard.py --base origin/develop` + `--node` repeated for `MB.1.6 MB.1.8 MB.1.12 MB.1.13 MB.1.14 MB.1.15 MB.1.16 MB.2.4.1 MB.2.4.2 MB.2.4.3 MB.2.4.4 MB.2.4.5 MB.2.5.1 MB.2.5.2` | literal form: `FAILED: 4 protected critical test(s) ... in node(s) MB.2.5` — the four are `MB.1.6`/`MB.1.8` edits that predate this epic. Full set: `critical-guard: OK: all 11 protected change(s) are authorized by node(s) …` — re-run here; ruling in `MB.2.4/LOG.md` |
| 5 | `make rs-test` | **met** | same, at tip | `cargo test --workspace --all-targets --locked` + `--doc`: exit 0, 83 `test result: ok` lines, **1,966 tests passed**, 0 failed — re-run here |

Why row 4's replacement is legitimate: `--node` is `action="append"`
(`tools/critical_guard.py:262`), so the guard's contract is "*these* nodes authorize *these*
paths". `critical_rewrite.rs` is edited by MB.2.5.1, MB.2.5.2 and MB.2.4.5 (and created under
MB.2.5.1); a single epic cannot authorize MB.1's branch-wide protected edits. Same tool, same
base, every authorizing node named.

### Gate evidence beyond the acceptance block

| Check | Result | Source |
|---|---|---|
| test-curator audit | **PASS**; `critical_rewrite.rs` **deserves** its protected tier — "the only oracle for slot, descriptor and queue accounting" through the real `run` loop; `R5`/`R6`/`R8`/`R10`/`R12` covered (pure + flow) | `docs/testing/AUDIT-MB.2.4.md` §Verdict, §Tier & teeth, §Requirement map |
| audit gaps G7–G9 closed | client cancel (parked and unparked), `REQ_END` while queued keeps one entry that still expires, static half-body bound — 7 new flow tests, 25 → 32 | `MB.2.4.5/RESULT.md` |
| `make check` | exit 0, 98 s over this tree (earlier 104 s at `69504ed`, and at `fafae8a`, `0796921`); tail `plan OK`, `47 fences: … 0 failed` | `/tmp/opencode/mb245-check.log` |
| `make e2e` | exit 0 at the `MB.2.7` state: static leg **72** assertions against a real containerised `blastit dev` (hashed asset immutable, deep SPA URL `200 text/html`, `400` for `..`/`%2e%2e`/NUL/backslash never masked by the fallback, `404` dotfiles/symlink, `HEAD`, `405` + `Allow`, `413`, `spa = false` `404`), and the deployment half **356** `^  ok` + `docs_deploy: 40/40`, `exit 0 wall 31s` | `MB.2.7/RESULT.md`; `/tmp/opencode/mb211-e2e.log` (that log predates `0be2b2f` and carries the deployment half only) |
| hand-mutation proofs | 9 `decide`/`declares_body` mutants each fail a named test (always-`Forward` → 10 failed, no method gate → 3 failed, no body gate → 2 failed, `Hidden`→`400` → 2 failed, …); `can_dispatch_static = slots < max_workers + 1` → three heads where two slots exist, fails in 5.9 s; slot-release-at-head, buffered-forward stall and zero-byte-read proofs as recorded | `MB.2.5.1/RESULT.md`, `MB.2.5.2/RESULT.md`, `MB.2.4.5/RESULT.md` |

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/rewrite.rs` | **new**: `Rewrite`, total `decide`, `STATIC_ALLOW`, `declares_body` (166 production lines, under its 250 cap) |
| `crates/blastit-scheduler/src/route.rs` | `RouteInfo::{static_policy, static_forward}`, `with_static(policy, has_app)`, `routes_from_manifest` passthrough, two unit tests (+88/−3) |
| `crates/blastit-scheduler/src/scheduler.rs` | boot (one `Arc<StaticRoot>` per static route), `begin_raw` head work, `admit_static`, `dispatch_static`, `serve_static`, `forward_static`, `refuse_static`, `pump_static`, `finish_static`/`abort_static`, `release_static_slot`, `request_end`'s buffered branch, `405` arm in `canned` (+875/−3, incl. MB.2.4.5's two fixes) |
| `crates/blastit-scheduler/src/lib.rs` | `pub mod rewrite;` |
| `crates/blastit-scheduler/tests/critical_rewrite.rs` | protected suite; 672 → **1,718** lines, 13 → **32** tests (pure precedence/status tables + `PumpTree` flow oracles) |

`tests/common/mod.rs` needed no change: `Http::send` already carries a client `Frame::Cancel`.
`src/path.rs` needed no change either: its one percent-decode already keeps `%2F` a separator
(`path.rs:7`, `percent_decode`), and `head_segments` reuses that production decoder instead of
re-decoding (MB.2.4.1 fixup 3). No dependency change.

## Follow-ups

1. **No `cargo-mutants` campaign for `rewrite.rs` or the static path of `scheduler.rs`.** Design
   §7.2 asks for `-f rewrite.rs` at ≥ 75 % killed; this epic has hand-mutations only (9 in
   MB.2.5.1, the slot-bound and pump mutants in MB.2.5.2/MB.2.4.5). MB.2.4.5's follow-up 2 asks
   for the same on `scheduler.rs` after the tree settles: `-f '**/src/rewrite.rs'` then
   `-f '**/src/scheduler.rs'`. The chunk-0 mutant is now `CAUGHT` (MB.2.4.5), so a `TIMEOUT`
   there in a future campaign is a regression, not a pass.
2. **The `want == 0` guard has no in-suite test.** `want = min(remaining, credit, max_payload,
   STATIC_CHUNK_BYTES)` can only be `0` when a bound is degenerate (`STATIC_CHUNK_BYTES = 0`),
   so MB.2.4.5's guard is proven by mutating the constant (13 failed in 0.13 s with it,
   `EXIT: 124` without), not by a test on shipped constants. An injectable chunk size, or an
   assertion on `want` itself, would close it.
3. **`on_static_read` trusts `read.want`.** A `min`→`max` mutant of `want` is constrained only
   by the chunk test's frame sizes; the same injectable-`want` assertion would pin it.
4. **Budgets are below what landed.** `scheduler.rs` +875 changed lines against the node's 600;
   `critical_rewrite.rs` 1,718 lines against design §8's recorded 1,600 ceiling (which still
   says the file reaches 1,365). Nothing was trimmed to fit — raise both recorded numbers to
   ~900 and ~1,750 so the next leaf is not asked to cut a §7.2 case.
5. **`BLASTIT_SCRIPT_NAME` + static routes is still open.** `begin_raw` runs
   `head_segments` on the **raw** request target (`scheduler.rs:1404`), so under a mount prefix
   `/mount/assets/x.js` would be looked up as `root/mount/assets/x.js`. `BLASTIT_SCRIPT_NAME` is
   documented as stripping the prefix first (`docs/reference/manifest.md:312`) and no test
   covers a prefix + static route; strip before `head_segments`, or document that a static root
   is served at `/` only.
