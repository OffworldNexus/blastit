# Implementability review — docs/design/static-and-spa.md

Lens: implementability
Pass 11 of the goldfish review. Re-read: the current design (1007 lines), ADR-0007 (86 lines), the normative page, the `MB.2.4`/`MB.2.5` epics and their four child briefs, `scheduler.rs`/`route.rs`/`config.rs`/`error.rs`, `stage0.rs`/`stage1.rs`/`config.rs`/`check.rs`/`visibility.rs`, and `rustix-1.1.5` (`fs::openat2`, `FileType`, `Stat`, `io::pread`). Status per finding is in [Pass 11](#pass-11).

### I1. No view-less admission path, and the design names none [BLOCKER — closed]
Where: design:446–455.
What: `can_dispatch_static`, `dispatch_static`, `admit_ready`'s branch, spawn suppression in both queue walks and `start`, and the slot's release are all pinned; `NextFlow.stream_id` (`fair.rs:374–383`) lets the fair walk inspect the head before popping, so both walks are writable. `Rustix 1.1.5` has everything §4.2 needs. Residuals folded into I15.
Fix: none.

### I2. `decide`'s signature cannot express the precedence table it owns [BLOCKER — closed]
Where: design:264–304.
What: `StaticOutcome::SpaLost`, `decide(outcome, method, has_body, has_app)` and `Reject { status, allow }` cover steps 7–10 and the 405 `Allow`; `no-store` is assigned to a single writer. Residual: that writer is unnamed ("the one scheduler helper", design:303–304) — name it (`reject_static`) or reuse `respond` with an extra header argument.
Fix: none required beyond the naming.

### I3. `blastit dev` never gets a static policy on `RouteInfo` [BLOCKER — closed]
Where: design:498–505, §8:765; `stage1.rs:285–300`.
What: §4.7 now names `stage1.rs` (all three constructions, `.with_static`), §8 puts it in MB.2.6's Files, and Follow-up 1 (`MB.2.2/RESULT.md:87`) records the pending path extension. Closed once the lead applies that extension; the learn branch must chain on the *manifest* route, which it does.
Fix: none.

### I4. `SchedulerConfig.manifest_dir` has no owner [BLOCKER — closed]
Where: design:478–482; `stage1.rs:686–716`.
What: `Config.manifest_dir` is absolutised once at load and copied by `stage1::scheduler_config`; the setter now exists inside a named node. Residual (MINOR): `parent()` of `BLASTIT_MANIFEST=blastit.toml` is `Some("")`, and `std::path::absolute("")` errors — use `.filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))`.
Fix: fold the `filter` into the pinned expression.

### I5. All-static boot dies in stage 1: the app import is unconditional [BLOCKER — closed]
Where: design:498–505; `stage1.rs:551, 878–880`; `supervise.rs:126, 484`.
What: `load` returns `py.None()` and skips import/`check_view_paths`/preload/warmup/immortalise; `Parts.app` is only consumed by `spawn_now` (`supervise.rs:484`), which static routes never reach, so `supervise.rs` needs no change.
Fix: none.

### I6. Static bodies read inline on the single scheduler task [MAJOR — closed]
Where: design:426–474, §6.6.
What: one `spawn_blocking` pread per response, results over a `static_in` select arm, one read in flight, credit-bounded. `rustix::io::pread` is available without a feature change; `spawn_blocking` works on the runtime `run` is driven on. Residual: "at most `max_workers` blocking reads" is not true after a cancel (a released request's read still holds a thread while its slot is reused) — say "≤ `max_workers` live + stragglers".
Fix: one sentence.

### I7. Ingress planning runs before the static decision [MAJOR — closed]
Where/What: design:342–357 — §4.4 orders resolve/`decide` before `plan_request_body`, forbids buffering/transcoding a selected target and states the 405/413 precedence. Fix: none.

### I8. `SchedulerError::StaticRoot` has no owner [MAJOR — closed]
Where: design:298–304, 524–526.
What: reuses `SchedulerError::Protocol("static root …")` (no `error.rs` edge) and `static_asset.rs` formats its own boot error. Residual (MINOR): `StaticError` and `StaticMeta`'s field types are still unspelled.
Fix: give `StaticMeta` types (`size: u64, mtime_secs: i64, mtime_nsec: u32, mode: u32`) or say "implementer's choice".

### I9. Over-long limit and "no normalization" [MAJOR — closed]
Where/What: design:118–139, 239–248 — the two `MAX_*` constants are pinned, `segments()` owns the empty/`.` drops in MB.2.4, R11 is reworded, and rows 13/33 pin both directions. Fix: none.

### I10. `groups` ambiguity in `StaticDeadDeclaration` [MAJOR — closed]
Where/What: design:104–108 — R4 checks only what is **declared on the route**; inherited manifest-wide groups are explicitly not re-checked. Fix: none.

### I11. MB.2.4/MB.2.5 cannot finish in their stated budget, and their briefs contradict the design [MAJOR — closed]
Where: design:763–769; `MB.2.4/NODE.md:31` ("≤ 600 changed lines … production ~380"); `MB.2.5/NODE.md:28`; `MB.2.6/NODE.md:31, 49`.
What: MB.2.4 now also owns `tests/common/static_fixture.rs` (the S/F/N/D builder) and runs the 36-row matrix over **two** backends (§7.2), plus `classify`, the 25 `Content-Type` rows, ETag/304/HEAD, on top of a ~380-line module: ~2× the budget. MB.2.5 gained `static_in`, `pump_static`, `dispatch_static`, `can_dispatch_static`, `decide` and a protected suite; §8:768 says "if it exceeds, split" — a contingency the implementer cannot execute without `plan.py` and a lead. The briefs still carry the pre-pass-1 API: MB.2.4's `ETag` from `(dev, ino, size, mtime)` vs design:365; MB.2.5's `Fallback(target)`/`Reject(status)` vs design:292–296; MB.2.6's banner change (Follow-up 6) and "manifest line" (Follow-up 7). A cheap implementer follows the brief and fails the design's tests.
Fix: pre-split now (e.g. MB.2.4a resolver+classify, MB.2.4b response+fixture; MB.2.5a rewrite/decide, MB.2.5b admission/pump) or raise both budgets in `NODE.md`, and refresh the four `Context` bullets, not only `paths`.

### I12. `openat2` enumeration and FIFO wording [MINOR — closed]
Where/What: design:568–584, 539–549 — the errno table is total (`ENXIO`/`EISDIR`/`ENODEV` included), `O_PATH` probes and read-opens only regular files; `FileType::{RegularFile,Directory,Fifo,Socket,CharacterDevice,BlockDevice}` exists in rustix 1.1.5. Fix: none.

### I13. The responder-uid probe cannot run in `dev` or `check` as written [MAJOR — closed]
Where: design:493–496, 510–511; `stage0.rs:625–700`; `check.rs:126–153`; `visibility.rs:248–277, 847–860, 1020–1062, 124–167`.
What: (a) `layout.scheduler()` is evaluated in `run_dev` and `check::execute`, but neither has a `UidLayout` (`grep` finds none), and in `dev` the layout uid is `base+2` ≠ euid, so the probe child's `drop_privileges` fails and `by_probe_or_mode` falls back to mode bits — where a stock Ubuntu `0750` `$HOME` blocks, so the page's `uv run blastit dev` refuses to boot. The design's parenthetical ("dev: the probe is the caller") is right but the pinned expression is not it. (b) `roots` (`&[PathBuf]`) is produced by no named function. (c) `Blocker::message()` is the `sys.path`/`UV_PYTHON_INSTALL_DIR`/lazy-import sentence, wrong prose for a static root, and correcting it means editing `visibility.rs`, in no node's `paths`.
Fix: production only (`probe: layout.scheduler()`); `dev`/`check` use `DroppedTo { uids: BTreeSet::from([geteuid()]), probe: geteuid() }` (or skip); add `Manifest::static_roots(&self, manifest_dir: &Path) -> Vec<PathBuf>`; build stage0's own one-line message from `Blocker::{path, mode, probed_uid}` — or add `visibility.rs` to MB.2.6.

### I14. `decide`/`respond`/`static_target` cannot share one `OwnedFd` as pinned [MAJOR — closed]
Where: design:291–299, 273–285, 446–447.
What: `decide(outcome: &StaticOutcome, …) -> Rewrite` with `Rewrite::Serve(StaticFile)` cannot move an `OwnedFd` out of a shared reference (E0507); `respond(&self, file: &StaticFile, …) -> StaticResponse` must then `dup` the descriptor for `StaticBody`, and `Request.static_target: Option<StaticFile>` stores the same file a third time, so "the slot is released when `RESP_END` is pushed" leaves two descriptors whose closure is unspecified. A cheap implementer hits a compile error at the first line and must redesign the API.
Fix: `decide(outcome: StaticOutcome, …)` (by value) and `respond(self, file: StaticFile, …) -> StaticResponse`; make `Request.static_target` the single owner (`Serve` carries the file into it) and say which copy `release` drops.

### I15. Residual under-specifications in the pump and the timeout path [MINOR]
Where: design:449–466; `scheduler.rs:1791–1830, 1957–1999, 3223–3290`.
What: (a) `dispatch_static` must set `dispatched_at`/`deadline` exactly as `dispatch` does (scheduler.rs:1801–1803), or `next_deadline` and `expire_requests` never fire for a static stream. (b) `expire_requests` sets `complete` but never calls `release` for a view-less request (unlike `client_cancel`), so a timed-out static answer leaks its slot; §4.6 says the timeout path frees it. (c) `Scheduler` needs a named home for `Vec<Option<Arc<StaticRoot>>>` and the `static_in` sender; `run` creates only the receiver. (d) `StaticResponse` has no `reason` for `RespHead` (map 200/304). (e) `SchedulerConfig.static_max_bytes` "set by stage1" has no `Config` source — it can only be `STATIC_MAX_FILE_BYTES`; say so.
Fix: pin (a)–(b) in §4.6 and (c)–(e) in §4.2/§4.7.

### I16. `same_inode`'s protected oracle has no seam, and the node's swap test cannot be written [MAJOR — closed]
Where: design:266–272 (`pub(crate) fn same_inode`, "unit-tested … either field differing"), design:809, 836 (§7.2 puts the table in `critical_static_resolve.rs`), design:840–842 ("the `same_inode` call site, which no unprivileged test can drive"), `plan/nodes/MB/MB.2/MB.2.4/MB.2.4.1/NODE.md:47` ("a swap attempt … proving the `(st_dev, st_ino)` identity check"), `rustix-1.1.5/src/backend/linux_raw/fs/types.rs:636–640` (`Stat` is `#[non_exhaustive]`, no `Default` or constructor).
What: `tests/*.rs` are separate crates, so a `pub(crate)` function is invisible to `critical_static_resolve.rs`; the only oracle for the TOCTOU identity check would be an unprotected in-module unit test. A `Stat` cannot be built by a test, so "either field differing" needs a real second filesystem (`/proc`), not the pure table the text implies, and the node's swap test has no interleave hook — which the design itself concedes. A cheap implementer either changes visibility unasked or writes a racy protected test.
Fix: `#[doc(hidden)] pub fn same_inode(probe: (u64, u64), read: (u64, u64)) -> bool` (call site `same_inode(dev_ino(&probe), dev_ino(&read))`) and table it in `critical_static_resolve.rs`; delete the swap attempt from MB.2.4.1's Notes (pure table + the `resolve` rows are the oracle) or add a doc-hidden probe/read hook; state that the call-site mutant is a Follow-up rather than a kill.

Pass 5: closed by design:270 (tuple `same_inode`, `#[doc(hidden)] pub`), design:296–308 (`resolve_with(segments, Option<&dyn Fn()>)`, `resolve` = `…, None`), design:850 (tuple table + substitution through the hook) and design:858 (the substitution is what kills the call-site mutant).

### I17. A buffered `Forward` still takes its slot twice [MAJOR — closed]
Where: design:556–561 (`admit_ready` order), design:550–555 (`dispatch_reserved`), `scheduler.rs:1572`, `scheduler.rs:1816`.
What: the earlier text let a buffered `Forward` re-enter the ordinary path with `request.slots == true` and call `dispatch`, which increments again. Pass 9 fixes it: `admit_ready` is `if request.slots { dispatch_reserved } else if request.static_pending { dispatch_static } else { unchanged }`, so the re-entry reuses the counted slot (`dispatch_reserved` has no increment). Residual: I19 shows that branch cannot serve a view that is not ready yet.
Fix: none for the buffered re-entry.

### I18. §4.4 contradicts itself on whether `plan_request_body` runs before the static verdict [MAJOR — closed]
Where: design:427–439; `scheduler.rs:1067`, `1179–1206`.
What: Pass 9 replaces the contradiction with one order: `begin_raw` computes the plan, `body_ready` is true so `dispatch_static` runs at the head; `Serve` answers `405`/`413` from the head and drops the buffer; only `Forward` honours the plan (`body_ready = false`, transcode at `REQ_END`, re-enter `admit_ready`). §7.2:912 pins the observable difference (Serve `413`, never a codec `400`; Forward `400` at `REQ_END`). Residual MINOR: how the deferred codec error is carried from `begin_raw` to the `Forward` branch is unnamed (`body_plan` is a `BodyPlan`, not a `Result`).
Fix: none for the ordering; name the carrier if the plan can be `Err`.

### I19. A `Forward` whose route has no ready view has nowhere to wait [BLOCKER — closed]
Where: design:550–555 (`dispatch_reserved` is "`dispatch`'s body without `slots += 1`" and "the same buffered/unbuffered split `admit_ready` uses"), design:556–561 (`admit_ready` order), design:562–566 (the forward-capable route "keeps the ordinary lazy spawn"), `scheduler.rs:1678–1712` (`dispatch_queued` returns early when `slots >= max_workers`, then `try_spawn`s and calls `dispatch`), `scheduler.rs:1656–1662` (`can_dispatch` false with no ready view), `scheduler.rs:1816` (`dispatch` increments).
What: a static route is admitted before `can_dispatch`, so on the front controller's **first** request the route's view does not exist. `dispatch_static`'s `Forward` calls `dispatch_reserved`, which the text defines as `dispatch`'s body: `reserve` returns `None` and nothing is spawned or queued — the request holds its slot and times out. If instead `dispatch_reserved` is read as the view half and enqueues the slot-holder, `dispatch_queued` cannot serve it: the walk returns early while `slots >= max_workers` (which is true because the request holds the slot), and when it does run it calls `dispatch`, adding a second slot. The design's two descriptions of `dispatch_reserved` cannot both be implemented, and neither reaches "the ordinary lazy spawn".
Fix: simplest — `Forward` calls `release_static_slot` (pumping = false), clears `static_pending`, and re-enters the **ordinary** view path (queue/`can_dispatch`/`try_spawn`/`dispatch`), so a Forward holds no static slot; delete "`Serve`/`Forward` keep the one slot" and the `slots` branch of `admit_ready` (a buffered Forward then buffers as any view request). Or keep the slot and add a dedicated holder map plus a `dispatch_queued` branch that uses `dispatch_reserved` for slot-holders **before** its `slots >= max_workers` return; say which. Also spell out the `can_dispatch_static` gate around `dispatch_static` in the `admit_ready` order, which the sentence omits.

Pass 10: closed by the first option — design:566–574 releases the static slot, clears `static_pending` and calls `admit_ready`, `dispatch_reserved` is gone from §4.6/§7.2/§8, and §7.2:936 keeps the `max_workers = 1` observable. (I20 is the same `admit_ready` order's other half.)

### I20. A static request that waits for a slot has no way out of the queue [MAJOR — closed]
Where: design:544 (`can_dispatch_static`), design:552–558 (`admit_ready` order), design:575–577 (spawn bullet), design:578–579 ("the same bounded queue, `queue_timeout` and `503` apply"), `scheduler.rs:1678–1712` (`dispatch_queued` calls `can_dispatch`/`dispatch`, spawns only when the view is absent), `scheduler.rs:1816`.
What: the order sentence has no `can_dispatch_static` gate and no enqueue/503 branch, so a literal `if static_pending { dispatch_static }` runs past `max_workers`. And when a static request *is* queued (the stated bounded queue), nothing pops it into `dispatch_static`: `dispatch_queued` calls `dispatch` (a view slot) and, for a pure static route, `try_spawn` is suppressed, so the queued request is skipped until `queue_timeout`. The §7.2 wave test ("more than `max_workers` static streams all complete in waves", design:915) cannot pass.
Fix: pin both halves — `admit_ready`'s static branch is `if can_dispatch_static() { dispatch_static } else if queue full { 503 } else { enqueue }`; and each `dispatch_queued` walk starts its per-request body with `if request.static_pending { if can_dispatch_static() { dispatch_static } ; continue }` (never `try_spawn`).

Pass 11: closed at design:552–558 — the gate, the queue-full `503`, the enqueue, and both walks taking a queued `static_pending` request through `dispatch_static` while `slots < max_workers`.

### I21. The buffered `Forward` conflicts with `admit_ready`, and its body has no queue timeout [MINOR — closed]
Where: design:444–449 (§4.4: `Buffer` → `body_ready = false` and return), design:566–574 (§4.6: "release … then `admit_ready(stream_id)`"), `scheduler.rs:1179–1206` (the `begin_raw` Buffer branch sets `queued_since`), `scheduler.rs:743–777` (`next_deadline` needs `queued_since`).
What: for a `Buffer` plan the §4.6 bullet's unconditional `admit_ready` dispatches the request before its body is transcoded (and `REQ_END` then re-dispatches); §4.4's "returns" is the right reading. Neither says the request takes `queued_since = now`, so a body that never ends is never timed out by `next_deadline`/`expire_queue`.
Fix: state the Forward branch as `release_static_slot; static_pending = false; if plan == Buffer { body_ready = false; queued_since = now; return } else { admit_ready }`, and scope the §4.6 "then `admit_ready`" to the non-Buffer case.

Pass 11: closed at design:566–574 — release, clear `static_pending`/`dispatched_at`/`deadline`, no `release`, `queued_since = now` for `Buffer`, then `REQ_END`. Residual MINOR: design:574 says the buffered body is "timed out by `expire_requests`", but that function only sees `slots`/`deadline`; the buffered-body `503` is `expire_queue` (`scheduler.rs:1880–1948`, `!body_ready && queued_since`).

## Pass 2

| Id | Status | Closing / blocking reference |
|---|---|---|
| I1 | closed | §4.6 names `can_dispatch_static`/`dispatch_static`/spawn suppression |
| I2 | closed | §4.2 `SpaLost`, `decide(…, has_app)`, `Reject { status, allow }` |
| I3 | closed | §4.7 + §8 MB.2.6 Files; path extension Follow-up 1 |
| I4 | closed | §4.7 `Config.manifest_dir` → `stage1::scheduler_config` |
| I5 | closed | §4.7 `load` returns `py.None()`; `Parts.app` unused without spawns |
| I6 | closed | §4.6 `spawn_blocking` + `static_in`; §6.6 |
| I7 | closed | §4.4 resolve/`decide` before `plan_request_body` |
| I8 | closed | §4.7 `SchedulerError::Protocol`; `error.rs` untouched |
| I9 | closed | R11, §4.2 constants, `segments()` in MB.2.4 |
| I10 | closed | R4 "declared on the route"; manifest-wide not re-checked |
| I11 | **open** | MB.2.4 ~2× budget; §8:768 is a contingency; briefs stale |
| I12 | closed | §5.1 total errno table + `O_PATH` probe |
| I13 | **open** | probe uid/`layout` absent in `dev`/`check`; `Blocker::message()` |
| I14 | **open** | `&StaticOutcome` → `Serve(StaticFile)`; triple `OwnedFd` |
| I15 | open (MINOR) | deadline/release, roots/sender, `reason`, `StaticError` |

## Pass 3

Pass 3 supersedes the `open` cells above; I1–I12 are unchanged.

| Id | Status | Evidence |
|---|---|---|
| I11 | closed | §8 splits `MB.2.4a/b` and `MB.2.5a/b`, ≤ 500 production lines, brief re-sync is a precondition (`†`), `MB.2.9` docs leaf |
| I13 | closed | §4.7 step 5: `Manifest::static_roots`, `DroppedTo` per mode (production `layout`, `dev`+`check` euid), fixed `"static root <root>: permission denied"`, `Blocker` prose dropped |
| I14 | closed | §4.2: `decide(StaticOutcome)` by value, `respond(_, StaticFile, _)` consumes, `Request.static_target` single owner, `release` drops body then target, no `dup` |
| I15 | closed | §4.6 `dispatched_at`/`deadline`, timeout/cancel release, `static_roots`/`static_reads`/sender; §4.2 `reason`; §4.7 `STATIC_MAX_FILE_BYTES` |
| MINOR (new) | open | `respond(self, …)` must be `&self` (`Arc<StaticRoot>` cannot be moved out); `open_with(…) -> Self` cannot report a failed root open; `DroppedTo { uids: {…} }` is not Rust and "answers without a fork" is wrong (the euid probe forks and succeeds); design:502–506 ("slot … never by the timeout") contradicts design:783 ("slot released exactly once (including the timeout path)"); `StaticMeta` fields still untyped |

## Pass 4

The four open pass-2 items stay closed; the split, chain and module homes check out. One new MAJOR (I16).

| Id | Status | Note |
|---|---|---|
| I1 | closed | §4.6 admission/pump unchanged |
| I2 | closed | §4.2 `decide`/`Reject` unchanged |
| I3 | closed | §4.7 `stage1.rs` owned by MB.2.6 |
| I4 | closed | §4.7 `Config.manifest_dir` |
| I5 | closed | §4.7 app-less `load` |
| I6 | closed | §4.6 `spawn_blocking` + re-drive |
| I7 | closed | §4.4 ordering |
| I8 | closed | `SchedulerError::Protocol` |
| I9 | closed | `segments` leading-`//` rule now explicit (§4.2) and rows 5–6 agree |
| I10 | closed | R4 route-declared only |
| I11 | closed | §8 chain `2.3→2.4.1→2.5.1→2.4.2→2.5.2→2.6`; nodes on disk match (2.4.2 needs 2.4.1+2.5.1, 2.5.2 needs 2.4.2+2.5.1); `static_response.rs` override + `pub(crate)` fields keep 400/260-line modules |
| I12 | closed | §5.1 errno table |
| I13 | closed | §4.7 per-mode probe |
| I14 | closed | §4.2 by-value `decide`/`respond` |
| I15 | closed | §4.6 deadline/release/roots/reads; rows 37/37b/38 consistent |
| I16 | **open** | `same_inode` `pub(crate)` vs protected integration suite; `Stat` unconstructible; node's swap test has no hook |

MINOR residuals (do not block): MB.2.4.2's brief still says "36-row" and "both backends" at the `decide` layer (§5.2 has 39 entries incl. 37b; §7.2 puts the backend dimension in MB.2.4.1); MB.2.4.1's fixture list omits `café.js`/`proclink` even though it says "§5.2 exactly"; §8's MB.2.5.1 row omits the `lib.rs` its node owns; ADR §1 still names `static_asset.rs` as the answer builder and "that identity" for `dev`/`check`; `SchedulerConfig.static_max_bytes → StaticRoot.max_bytes` has no named assignment; `can_spawn_read` is named in §7.1 but homed nowhere.

## Pass 5

I16 and five of the six pass-4 minors are closed; the slot-until-result simplification reads as a clean bound. One pass-4 minor survives, plus five new MINORs (none blocks).

| Id | Status | Evidence |
|---|---|---|
| I16 | closed | design:270 tuple `same_inode`; :296–308 `resolve_with`; :850 the substitution oracle; :858 the call-site mutant |
| M1 36-row | closed | §8:913–916 names the 36-row brief as a re-sync precondition (design:820/849/907 say 38) |
| M2 fixture list | closed | §8:915 "child briefs are re-synced"; §5.2:700–705 and §7.1:833–837 carry `café.js`/`proclink` |
| M3 lib.rs | closed | §8:906 `{rewrite,route,lib}`; `MB.2.5.1/NODE.md` has `lib.rs` |
| M4 ADR §1 | closed | ADR:20–22 names `static_asset.rs` + `static_response.rs` |
| M5 `max_bytes` | **open (MINOR)** | design:278–280 says `StaticRoot::open` copies `SchedulerConfig.static_max_bytes`, but `open` takes `(manifest_dir, policy)`; only `run` can set the `pub(crate)` field |
| M6 `can_spawn_read` | closed | removed; no reference in §4.6/§7.1/§7.2 |

New MINORs (do not block): `resolve_with` is missing from §8:905's "Lands" list and from `MB.2.4.1`'s symbol list (the file/lane is right); `MB.2.5.2/NODE.md:23` still says `static_reads` and "release a view-less request like a dispatched one", which §4.6:551–563 replaced; §4.6:551's "at most `max_workers` … descriptors" ignores the one `StaticFile` each queued request holds (bounded by `queue_size`); §4.6:558–563 does not say which call removes a non-pumping timed-out/cancelled static request (`release_static_slot` frees the slot, `release` at §4.2:360 drops the body/fd); the matrix is 39 entries (1–38 incl. 37b) wherever "38-row" appears.

## Pass 6

Five of the six pass-5 items are closed; the queued-descriptor fix is real but has opened a new BLOCKER on the `Forward` path.

| Id | Status | Evidence |
|---|---|---|
| max_bytes | closed | design:278–281 — `max_bytes` is a `pub(crate)` field `run` sets after `open`; no false "copies" claim |
| resolve_with Lands | closed | design:932 — `StaticRoot::{open,open_with,resolve,resolve_with}` |
| MB.2.5.2 brief | **open (MINOR)** | `MB.2.5.2/NODE.md:23` still says `static_reads`, "release … exactly like a dispatched one", and `dispatch_static` setting the slot before `resolve`; the re-sync note (design:940–944) still does not name it |
| queued fds | closed | design:538–541 — `segments` at the head, `resolve` inside `dispatch_static`, so a queued request holds no `StaticFile` |
| release non-pumping | **open (MINOR)** | design:574–590 — `release_static_slot` frees the slot, but no call removes a non-pumping timed-out/cancelled `Request`; `release` (design:360) is the only dropper of `static_body`/fd and §4.6 never invokes it |
| 38-vs-39 | closed | matrix renumbered to 40 rows (design:772–774, 847, 876, 912, 934) |
| I17 | **open (BLOCKER)** | `Forward` double-counts `slots`, the no-`try_spawn` rule contradicts step 9, and the buffered re-entry has no marker |

New MINORs (do not block on their own): design:397 still says "steps 5–12" for an 11-step table; §7.1:846 still says "rows … 37–38's refusal half" after the renumbering (37/38 are the NFC/NFD positives); the dotfile rule of step 4 is said to run "when the head is admitted" but no function owns it.

## Pass 7

The pass-6 blocker's core is fixed; two Forward-path clauses and one stale brief remain (I17 stays open at MAJOR).

| Id | Status | Evidence |
|---|---|---|
| I17 slot hand-over | closed | design:542–545 `dispatch_reserved` (dispatch without `slots += 1`); one `release` decrements (design:545) |
| I17 spawn scoping | closed in production | design:552–557 `RouteInfo.static_forward` scopes both walks and `start` |
| release non-pumping | closed | design:595–602 `release_static_slot -> bool`, then `release`; parked read waits for its result |
| is_dotfile | closed | design:274, 400–403 — `static_asset.rs` (`MB.2.4.1`), called from the scheduler head path |
| §8 Lands | closed | design:948 (`is_dotfile`), 951 (`dispatch_static`/`dispatch_reserved`/`static_forward`) |
| §4.3 steps | closed | design:400–404 "steps 5–11" |
| §7.1 refusal half | closed | design:862 "…39's refusal half" |
| miss-row sha256 | closed | design:791–793 — rows 14/19/38 assert the index sha256 and never the near-miss bytes |
| I17 dev `static_forward` | **open (MAJOR)** | design:553 vs design:647 — dev/learn `RouteInfo` gets only `.with_static(policy)`; no `has_app`, so `static_forward == false` and `Forward` never spawns |
| I17 buffered `Forward` | **open (MAJOR)** | design:544 points at a buffered split in `admit_ready` (it lives in `begin_raw`); `REQ_END` re-enters the static branch (`scheduler.rs:1507–1581`) with no `forwarded` marker, so `dispatch_static` takes the slot twice |
| MB.2.5.2 brief | **open (MINOR)** | `MB.2.5.2/NODE.md:23` still says `static_reads` and an unconditional "never `try_spawn`"; no `dispatch_reserved`/`static_forward`/`slot_releasable` |

## Pass 8

The dev/`static_forward` arm and the brief drift are closed; the buffered arm survives, and it exposed a §4.4 ordering contradiction (I18).

| Id | Status | Evidence |
|---|---|---|
| I17 dev arm | closed | design:560–566 — `with_static(policy, has_app)`, all four constructions incl. the three stage-1 builders |
| I17 buffered arm | **open (MAJOR)** | `static_pending` skips the static branch only; `REQ_END` → `admit_ready` → `dispatch` still does `slots += 1` (`scheduler.rs:1572`, `:1816`) with `request.slots == true` |
| I18 plan_body order | **open (MAJOR)** | design:428–433 — "never reaches `plan_request_body`" vs "may have had a body buffered by `begin_raw`"; `scheduler.rs:1067` is the only call |
| MB.2.5.2 brief | closed | design:966–968 names "`MB.2.5.2` the removed `static_reads` rule" |
| §4.7 call site | **open (MINOR)** | design:657 still shows `.with_static(route.static_policy().cloned())`, one argument, against `with_static(policy, has_app)` at design:560 |
| ADR identity | **open (MINOR)** | ADR:30–31 "`check`, `dev` and the production boot probe it in that identity" — for `dev`/`check` the probe is the caller (§4.7 step 5) |
| release / `is_dotfile` / steps / §7.1 / sha256 | closed | see the Pass 7 table; unchanged |

## Pass 9

I17's buffered arm and I18 are closed, and both minors are fixed; the slot-holder approach they introduced leaves one BLOCKER on a `Forward` whose view is not ready.

| Id | Status | Evidence |
|---|---|---|
| I17 buffered arm | closed | design:556–561 — `admit_ready` sends a slot-holder to `dispatch_reserved`, so the buffered `Forward` reuses its one slot |
| I18 plan order | closed | design:427–439 — `Serve` answers from the head; only `Forward` honours the plan; §7.2:912 pins 413-vs-400 |
| §4.7 `with_static` | closed | design:657 — `.with_static(policy, manifest.app().is_some())` |
| ADR probe | closed | ADR:30–31 — `dev`/`check` as the invoking uid, production as the responder's uid |
| I18 residual | **open (MINOR)** | the codec error that `begin_raw` computes but `Serve` must not act on has no named carrier (`body_plan` is a `BodyPlan`) |
| I19 Forward/view | **open (BLOCKER)** | `dispatch_reserved` is `dispatch`'s body with no `can_dispatch`/queue/spawn; a slot-holder in `dispatch_queued` stalls or double-counts |

## Pass 10

I19 and the `body_plan_error` minor are closed; the release-and-re-enter half of the static admission exposes a missing queue half.

| Id | Status | Evidence |
|---|---|---|
| I19 Forward/view | closed | design:566–574 — release the slot, clear `static_pending`, `admit_ready` (ordinary queue/`try_spawn`/`dispatch`); `dispatch_reserved` removed from design:542–585, :936, :993 |
| `body_plan_error` | closed | design:437, 445 — stored and answered only by `Forward`; §7.2:936 tests `413` on `Serve` vs `400` on `Forward` |
| I20 queue gate/drain | **open (MAJOR)** | design:552–558 has no `can_dispatch_static`/enqueue/503; `dispatch_queued` never calls `dispatch_static`, so the wave test cannot pass |
| I21 buffered Forward | **open (MINOR)** | design:444–449 ("returns") vs design:566–574 ("then `admit_ready`"); no `queued_since` for the buffered body |

## Pass 11

I20 and I21 are closed; one MINOR wording survives.

| Id | Status | Evidence |
|---|---|---|
| I20 gate/drain | closed | design:552–558 — `can_dispatch_static` → dispatch / queue-full `503` / enqueue, and both `dispatch_queued` walks route a queued `static_pending` request to `dispatch_static` |
| I21 buffered Forward | closed | design:566–574 — release the slot, clear `static_pending`/`dispatched_at`/`deadline`, no `release`, `queued_since = now`, wait for `REQ_END` |
| I21 residual | **open (MINOR)** | design:574 says "timed out by `expire_requests`"; the buffered-body `503` is `expire_queue` (`scheduler.rs:1880–1948`), which reads `!body_ready && queued_since` |
| I1–I19 | closed | see the Passes 1–10 tables; unchanged |

## Verdict
Zero BLOCKER/MAJOR. The static admission, queue gate, drain, `Forward` hand-back and buffered `Forward` body all have named homes and a pinned order; the matrix, the per-mode probe and the split nodes have been re-checked through pass 10. One MINOR: the buffered-body timeout is attributed to `expire_requests` instead of `expire_queue` — the behaviour (and the existing code) is right, the sentence names the wrong function.

Attempt-3 re-anchor: `STATIC_RESOLVE_FLAGS` and the `resolve_with` hook were added at §4.2, so the numbers in the earlier pass tables shift after §4.2; the re-quoted numbers in the pass-10/11 rows are current.

VERDICT: PASS
