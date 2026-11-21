# AUDIT MB.2.4 + MB.2.5 — resolve, respond, rewrite and dispatch in `blastit-scheduler`

- **Date:** 2026-11-13
- **Scope (MB.2.4):** `crates/blastit-scheduler/src/{static_asset.rs,static_response.rs,config.rs,lib.rs}`,
  `tests/{critical_static_resolve.rs,critical_static_assets.rs,common/static_fixture.rs}` at `b8e8806`.
- **Scope (MB.2.5, added after `cb7a1e4`):** `src/rewrite.rs`, `src/scheduler.rs`'s static path
  (`admit_static`, `dispatch_static`, `serve_static`, `forward_static`, `refuse_static`,
  `pump_static`, `finish_static`, `abort_static`, `release_static_slot`, `request_end`'s buffered
  branch, `forward_static`'s admission) and `tests/critical_rewrite.rs` (pure + flow halves).
- **Base:** `b8e8806` (campaign; MB.2.4.1 `7a79a95`, MB.2.4.2 `b8e8806`). **Tip read:** `cb7a1e4`
  (MB.2.5.1 `d37f765`, MB.2.5.2 `cb7a1e4`, design sync `9e3b4d6`).
- **Requirements:** `docs/design/static-and-spa.md` §3 `R1`–`R15`, §4.2–§4.6, §5.1–§5.2, §7.1–§7.2;
  `ADR-0007`; skill `testing` §1–§4, skill `security-invariants` §3, §5.
- **Not audited:** `MB.2.3` (`critical_static_manifest.rs`, 15 tests) beyond the requirement map;
  `MB.2.6`/`MB.2.7` suites (their nodes are `ready`, the files do not exist yet); `docs/deploy`,
  `e2e/` (MB.2.11).

## Verdict

**PASS on tier, teeth and fixture hygiene. The protected tier owns every requirement it has to, and
the three suites have real teeth. The raw mutation ratio is below the 75 % target only because 19 of
the 27 survivors are equivalent or unobservable-in-scope flag mutants; after that equivalence check the
killed ratio is 88.1 %, and the 8 meaningful survivors are filed as three tasks. No BLOCKER.**

- **Tier:** correct. All three static suites are new protected files under
  `crates/*/tests/critical_*.rs`; none skips, none is `#[ignore]`d, none weakens an assertion. The two
  MB.2.4 suites are the only unprivileged oracle for a parser-differential / root-escape / DoS surface,
  which is exactly what the critical tier exists for. `critical_rewrite.rs` **deserves** its protected
  tier (see §"Tier & teeth").
- **Teeth:** every suite asserts the denied path *and* its accepted twin; the permissive-answer probe
  below finds no suite that would pass as a whole.
- **Mutation:** 96 mutants in `static_asset.rs`: **57 caught + 2 timeout = 59 killed / 86 viable =
  68.6 %** raw, **59/67 = 88.1 %** after removing the 19 equivalences. 8 meaningful survivors, all
  mapped to `MB.2.4.3`–`MB.2.4.5`.
- **The two review-known gaps are real and still unpinned**, and a third (client cancel) has no oracle
  anywhere in the crate. All three are `MB.2.4.5`.

## Method

1. Read both children's `NODE.md`/`RESULT.md`, the epic node, `MB.2.5.1`/`MB.2.5.2`'s commits and
   `docs/design/static-and-spa.md` §4.2/§4.5/§5.1/§5.2/§7.2 (the oracle map as synced by `MB.2.10`),
   `ADR-0007`, and every line of the three suites, `static_fixture.rs`, `rewrite.rs` and the static
   half of `scheduler.rs`.
2. Ran the two MB.2.4 suites at the tip: **14 passed** (`critical_static_resolve`), **11 passed**
   (`critical_static_assets`), 0 ignored, 0 failed.
3. `cargo mutants` campaign (see §"Mutation evidence"), file-scoped to `**/src/static_asset.rs`, in a
   **detached worktree at `b8e8806`** so the MB.2.5.2 code could not act as an oracle for MB.2.4.
4. Re-checked every survivor against its `mutants.out/diff` and classified equivalence by hand.
5. Judged `rewrite.rs`/`scheduler.rs` from the suites' teeth plus the two mutations the reviewer
   already reproduced — **no second campaign** (per the scope extension), and said so.
6. Probed two claims with a shell check rather than a test run: the sparse `big.bin` really costs 0 B
   on the tmpfs `/tmp`; `grep` shows `client_cancel`/`client_gone` appear in **no** test file.

## Inventory & classification

| Path | Tests | Tier | Class | Verdict |
|---|---|---|---|---|
| `crates/blastit-scheduler/tests/critical_static_resolve.rs` (`MB.2.4.1`) | 14 | critical (protected) | **critical** | right tier: rows 1–35/37–40 over S/F/N/D × Openat2/Walk, every refusal variant, TOCTOU substitution, positive twins, flags/limits |
| `crates/blastit-scheduler/tests/critical_static_assets.rs` (`MB.2.4.2`) | 11 | critical (protected) | **critical** | right tier: 24 `200` cases, header table both ways, `304`, `HEAD`, cache kinds, `Content-Type`, `http_date`, `decide` statuses |
| `crates/blastit-scheduler/tests/critical_rewrite.rs` (`MB.2.5.1` + `MB.2.5.2`) | 25 (13 sync + 12 tokio) | critical (protected) | **critical** | right tier: the pure precedence/status tables and the only oracle for slot/descriptor accounting |
| `crates/blastit-scheduler/tests/common/static_fixture.rs` | 0 (fixture) | support | **useful** | deterministic, cleaned, root-safe; two latent CI risks noted below |
| `crates/blastit-scheduler/tests/common/mod.rs` (`World`, 1,281 ln) | 0 (harness) | support | **useful** | the real `run` loop against a fake init/client; deterministic under `tokio::time::pause()` |
| `crates/blastit-scheduler/src/{static_asset,static_response,rewrite}.rs` | 0 inline | — | — | deliberately no inline unit tests; the protected suites are the oracle |
| `crates/blastit-scheduler/src/config.rs` | 8 inline (0 new) | standard | useful | the two new fields are exercised indirectly by the flow suite (`..SchedulerConfig::default()`), not by a named test |

No test in scope is `fluff`, `misplaced`, or `broken-oracle`.

## Tier & teeth (anti-fluff, `testing` §3)

For each suite: the assertion that would **still pass if the code returned the permissive answer**,
then the assertion that gives it teeth.

| Suite | Passes under the permissive answer | Teeth |
|---|---|---|
| `critical_static_resolve.rs` | `the_documented_limits_and_flags_are_the_ones_asserted` (it restates four constants; the `RESOLVE_*` half is the only unprivileged oracle for the flag set and is kept on purpose) and `the_cafe_file_is_the_nfc_bytes_and_nothing_else` (fixture-only) | Row 1 (`..` → `Refused(Traversal)`) **next to** row 14 (`/%252e…` → `SpaFallback` with `index.html`'s bytes): a resolver that normalises a second time, or refuses everything, or answers the near-miss file, fails. Row 23 (`/link2/x` → `Hidden`) is the walk-escape oracle; the `resolve_with` swap is the TOCTOU oracle |
| `critical_static_assets.rs` | `the_suites_sha256_oracle_is_sha256` (the test oracle's own NIST vectors — deliberate) and `the_content_type_table_is_closed` as a pure table test | `the_cache_header_is_keyed_on_the_answering_file` (fixture D: `/index.html` immutable, `/` and `/dashboard` **the same bytes and ETag** `no-cache`; plus the front-controller policy that earns the year for the asset only) fails an always-immutable responder; `if_none_match…` fails never-304 and always-304; `head_answers…` fails HEAD-with-body; every row's ETag is recomputed from `stat(2)` |
| `critical_rewrite.rs`, pure half | **`a_miss_forwards_only_with_an_app_and_keeps_every_method_and_body` passes in full if `decide` always forwards**, and `declares_body_reads_the_head…` is product-independent by nature | `an_exact_file_is_served_for_every_alias_and_never_forwarded` (a `File` is never forwarded, with `has_app = true`), `the_status_mapping_is_total_over_every_outcome_and_every_request` (5 outcomes × 9 methods × 2 bodies × 2 apps) and `the_method_gate_is_405_with_exactly_allow_get_head` (9 methods on 5 servable shapes) fail always-Forward **and** always-Serve. The design's claim that neither can pass is true of the file, not of one test — recorded honestly |
| `critical_rewrite.rs`, flow half | `a_serve_reaches_the_wire_with_the_responders_own_bytes_and_headers` (one stream, no contention) | `more_than_max_workers_static_streams_complete_in_waves_inside_the_slot_bound` fails an early slot release (three heads in flight where two slots exist, and three descriptors where `max_workers = 2`); `a_short_read…` fails a double release (the fourth request would answer while two are parked); `a_forward_hands_the_slot_back…` fails a `Forward` that never returns its slot (`max_workers = 1`); `a_full_static_queue…` fails an unbounded queue |

**`critical_rewrite.rs` at 1,718 lines (672 after MB.2.5.1 + 1,046 after MB.2.5.2) is justified.**
It is 25 tests, not padding: 200 lines of table helpers plus the `PumpTree`/`until_pumped` harness,
which is the *only* way to observe slot, descriptor and queue state through the real `run` loop. The
two halves are two oracle families in one file, which `MB.2.5.2`'s node explicitly asked for and
design §7.2 sanctions ("the matrix is one table in one protected file"). Nothing in it is a getter or
a serde default; the pure half alone kills an always-Serve and an always-Forward `decide`.

## Fixture & harness review

`tests/common/static_fixture.rs` (231 lines):

- **Deterministic:** `blastit-static-<pid>-<nanos>-<atomic counter>-<S|F|N|D>` under
  `std::env::temp_dir()`; no wall-clock, no host name, no shared path between tests.
- **Cleaned up:** `Drop` removes the whole base; the four policies share one tree by construction
  (`Fixture` only changes the policy), so file rows are comparable across S/F/N/D.
- **Root-safe:** writes only under its own temp base; it never touches `/etc`, `/proc` or a manifest
  directory; `link -> /etc/passwd` and `proclink -> /proc` are *symlink targets*, never opened.
- **No large artifact in `/tmp`:** `big.bin` is `set_len(64 MiB + 1)` past one written byte, i.e.
  sparse. Verified on this host's tmpfs: `truncate -s 67108865` → `du` reports **0**. A 64 MiB
  non-sparse fixture would have been a real tmpfs cost; this is not one.
- **Preconditions are hard failures, never skips:** `geteuid() != 0`, `/DOCS` absent (case-sensitive
  FS), `café.js` found under NFC and not NFD. That is design §7.1's rule, and it is why the mode-000
  and `/DOCS` rows cannot silently invert.
- **Latent risks (not filed):** (a) a **root** CI container fails the whole suite by design — correct
  per §7.1, worth knowing when a Docker job runs `make check`; (b) the UNIX socket name must fit
  `sun_path` (108 B): `TMPDIR` + `/blastit-static-<pid>-<19-digit nanos>-<n>-<S>/root/sock` leaves
  ≈52 B of head-room, so a deep `TMPDIR` panics at `UnixListener::bind` instead of failing clearly;
  (c) `Drop` ignores `remove_dir_all`'s error and a panic *before* `Self` is constructed leaks the
  tree; (d) the precondition checks `euid`, not `CAP_DAC_OVERRIDE`, so a non-root user holding that
  capability would fail row 25 rather than skip.

`critical_rewrite.rs`'s `PumpTree` reads `/proc/self/fd` for the descriptor oracle and returns `0`
when it cannot. That is the right direction: the fd assertions then **fail** (`1 != 0`), they do not
pass vacuously. `until_pumped` is bounded by 4,000 yields under a paused clock, not by wall time, so a
loaded box cannot make it flake.

## Mutation evidence

```console
$ git worktree add --detach /var/tmp/mb24-clean b8e8806
$ cd <repo> && TMPDIR=/var/tmp/mb24-audit2 MUTANTS_JOBS=2 MUTANTS_TIMEOUT=90 \
    scripts/mutants-run.sh -d /var/tmp/mb24-clean -p blastit-scheduler \
      -f '**/src/static_asset.rs' -o /var/tmp/mb24-audit2/out
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 96 mutants to test
96 mutants tested in 9m: 27 missed, 57 caught, 10 unviable, 2 timeouts
```

`/var/tmp` had 647 G free (`df`) — far above the 20 G floor — and `TMPDIR` was on disk, not the tmpfs
`/tmp`. The oracle was the **whole** `blastit-scheduler` test package at `b8e8806` (all 15 targets),
not a test filter.

| File | Mutants | Caught | Timeout | Missed | Unviable | Killed / viable |
|---|---|---|---|---|---|---|
| `src/static_asset.rs` | 96 | 57 | 2 | 27 | 10 | **59 / 86 = 68.6 %** |

After the equivalence check below: **59 / 67 = 88.1 %**. The 75 % target is met once equivalent
mutants are excluded, and it is *not* met on the raw number. Both figures are reported because the
raw number alone would misrepresent this module: it is a flag-composition module, and cargo-mutants
generates `|`→`^` and `|`→`&` variants of every `OFlags`/`ResolveFlags` composition, most of which
cannot change behaviour.

A first, throw-away run was made on the working tree while MB.2.5.2 was uncommitted
(56 caught / 3 timeout / 27 missed / 10 unviable). Its **missed set was byte-identical** to the clean
run's, which is the useful property: a survivor against the *larger* oracle is certainly a survivor
against MB.2.4's. The clean run is the one reported.

### The 27 survivors, classified

**Equivalent (12)** — disjoint bitflag XOR, or a delete whose arm falls through to the same result:

| Mutant | Why it cannot be observed |
|---|---|
| `293:26`, `293:46`, `409:41`, `409:59`, `409:78`, `447:68`, `506:34`, `506:53`, `507:58`, `532:24`, `544:22` — `\|` → `^` | both operands are distinct bits of `OFlags`/`ResolveFlags`, so XOR is OR |
| `653:9` delete `FileType::RegularFile => Err(Hidden)` in `classify` | over-cap regular files then hit the `_` arm: same `Hidden` |

**No unprivileged oracle (7)** — `|` → `&` degenerations to `0` on `O_PATH`/`O_CLOEXEC`/
`O_NONBLOCK`/`O_NOCTTY`: the root dirfd opened `O_RDONLY` is still a valid directory descriptor, a
regular file opened without `NONBLOCK` still reads, and `NOCTTY` is only observable on a tty/device —
row 36, `MB.2.7`'s e2e. Defence in depth, not a hole: the boundary is the resolve flags, not these.

**Meaningful (8)**, each mapped to a filed task:

| Mutant | Consequence if it shipped | Gap |
|---|---|---|
| `302:34` (`==`→`!=`), `302:51` (`\|\|`→`&&`), `302:54` (delete `!`), `541:5` (`supports_openat2`→`true`/`false`) | a root silently takes `Walk` (or `Openat2`) forever; the kernel-enforced `RESOLVE_BENEATH\|NO_SYMLINKS\|NO_MAGICLINKS` boundary is lost and no test notices | **G1** → `MB.2.4.3` |
| `429:16` (`>`→`>=`, `>`→`==`) | a file **exactly at** `static_max_bytes` answers `Hidden`/`404` | **G3** → `MB.2.4.3` |
| `575:40` (`<`→`<=`) | a decoded **space** is `StaticRefusal::Control`: `/my%20file.css` answers `400` | **G4** → `MB.2.4.3` |

**Timeouts (2), counted as kills:** `211:36` (`end += 1` → `*= 1`) hangs `glob` — detected by the
timer; `447:68` (`PATH\|CLOEXEC` → `&` = 0) makes the probe read-open `/fifo`, which blocks — the
timeout *is* the proof that FIFOs are never read-opened on the correct path.

### The two review-known gaps — verdict

1. **`request_end` "must not re-enqueue" half: CONFIRMED unpinned.** `scheduler.rs::request_end`'s
   buffered branch keeps one queue entry (`if queued { return Ok(()) }`) and keeps the request's
   `queued_since` (`if !queued { request.queued_since = None }`).
   `a_buffered_forward_that_ends_while_queued_is_still_answered` proves the request is *answered*
   after the slot frees. It cannot see a duplicate FIFO id: the walk's `if !self.queue.contains(id)`
   after the first dispatch removes both, so deleting the early return is invisible. Deleting the
   `!` is worse and equally invisible — the request's queue deadline is cleared, so it can never be
   answered by `expire_queue`. → **G8**, `MB.2.4.5`.
2. **Buffered-forward admission bound: CONFIRMED off by one vs the app path.** `begin_raw`'s app
   path sets `body_ready = false` **before** `buffering_count() >= queue_size`, so the request counts
   itself (queue_size `N` ⇒ at most `N−1` half-bodies). `forward_static` checks while the request
   still has `body_ready = true` and only then clears it, so a static front controller admits `N` —
   one more than any other route, contradicting its own comment ("the app path's own bound"). →
   **G9**, `MB.2.4.5`.

### New gap found by this audit

3. **Client cancel has no oracle at all.** `client_cancel`/`client_gone` appear in **no** test file
   (only in `scheduler.rs`). A client that disappears mid-static-response must report `client_gone`,
   must not release a slot a `pread` still owns (`static_read_parked`), and must release it exactly
   once when the read result returns. Timeout, double release, waves, forward handback and the queue
   bound are pinned; this transition is not. → **G7**, `MB.2.4.5`.

A static request that times out **while a read is outstanding**, as
opposed to parked on credit, has no oracle either (`expire_requests`' `static_read_parked` guard).
Making that race deterministic needs a blocking read, so it is recorded as accepted-with-reason
rather than filed; the parked-credit timeout is covered by
`a_client_that_stops_granting_credit_is_aborted_at_request_timeout`.

## Requirement → protected-suite map

| Reqs | Protected suite | State |
|---|---|---|
| `R1`–`R4` grammar/root/index/spa | `blastit-manifest/tests/critical_static_manifest.rs` (15) | out of scope, present |
| `R5` (immutable), `R9` (headers) | `critical_static_assets.rs` | **covered**, incl. both cache directions |
| `R7` (refusals), `R11` (boundary) | `critical_static_resolve.rs` | **covered** on both backends |
| `R5`/`R6`/`R8`/`R10`/`R12` (precedence, gate, slot, no new role) | `critical_rewrite.rs` | **covered** (pure + flow) |
| `R13` (boot), `R14` (`check`) | — | **unprotected**: `MB.2.6` (`ready`) owns `critical_static_cli.rs` |
| `R15` (docs) | `blastit-manifest/tests/docs_fences.rs` (standard) + `MB.2.7` e2e | docs half present; e2e half `MB.2.7` (`ready`) |

No requirement owned by MB.2.4/MB.2.5 is unprotected.

## GAPS

| # | Gap | Evidence | Filed |
|---|---|---|---|
| G1 | The effective backend is never observed; `effective` in the suite is a **copy** of the production predicate and only `println!`d | 5 surviving mutants (`302:34/51/54`, `541:5` ×2) | `MB.2.4.3` |
| G2 | A `root` declared through a symlink (`current/public`) is documented in §5.1 and contradicted by ADR-0007's consequences; no test either way | §5.1 "the only place a symlink is followed" vs ADR-0007 "needs the real path" | `MB.2.4.3` |
| G3 | A file **exactly at** `static_max_bytes` is refused by `finish` under `>=`; the matrix has only the `+1` row | 2 surviving mutants (`429:16`) | `MB.2.4.3` |
| G4 | A decoded space is refused as `Control` under `<=`; no case feeds `0x20` to `segments` | 1 surviving mutant (`575:40`) | `MB.2.4.3` |
| G5 | `respond` "moves the `OwnedFd` … never `dup`ed" has no assertion; a `dup` or a leaked fd on `HEAD`/`304` is invisible | §4.2; no raw-fd comparison in the suite | `MB.2.4.4` |
| G6 | `respond`'s second ceiling disjunct (`file.meta.size > max_bytes`) is untested — `forged()` sets `len == meta.size` | `an_oversize_file_is_a_404…` | `MB.2.4.4` |
| G7 | Client cancel on a static stream: `client_gone`, slot retained while the read is outstanding, one release | `grep` finds `client_cancel` in no test | `MB.2.4.5` |
| G8 | Buffered forward: duplicate queue entry possible, and `queued_since` can be cleared so `expire_queue` never fires | §"two review-known gaps" (1) | `MB.2.4.5` |
| G9 | Buffered-forward admission bound is one more permissive than the app path, contradicting its comment | §"two review-known gaps" (2) | `MB.2.4.5` |

Not filed (recorded only): `http_date` has no leap-day/century vector (`2000-02-29`, `1900-03-01`), so
`civil_from_days`' century terms are unexercised; `trim_ows`' tab handling and the lowercase `w/`
prefix are accepted but untested; the fixture's `sun_path` head-room and `Drop` error swallowing; the
default `manifest_dir` (empty) is never exercised, but every static route must set it in stage 1 and
`MB.2.6` owns that wiring. None is a security boundary.

## Files

| File | Change |
|---|---|
| `docs/testing/AUDIT-MB.2.4.md` | this report (new) |
| `plan/nodes/MB/MB.2/MB.2.4/MB.2.4.3/` | task: resolver oracles (G1–G4) |
| `plan/nodes/MB/MB.2/MB.2.4/MB.2.4.4/` | task: responder oracles (G5–G6) |
| `plan/nodes/MB/MB.2/MB.2.4/MB.2.4.5/` | task: flow-half oracles (G7–G9) |

No product or test file was edited. No commit, no push. Campaign evidence:
`/var/tmp/mb24-audit2/out/mutants.out/{caught,missed,timeout,unviable}.txt`, `diff/`, `log/`.
