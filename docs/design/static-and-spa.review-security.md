# Security review — docs/design/static-and-spa.md

Lens: security · pass 2 — re-reviewed the rewritten design (778 lines) and ADR (78 lines) against `docs/frameworks/static-and-spa.md`; pass-1 ids kept.

### S1. Responder privilege premise and root readability [BLOCKER] — closed
`Where:` §1.4 (l.63–69), R2 (l.97–100), §4.7 step 5 (l.493–495), §5.1 "Identity" (l.534–538), ADR §1 (l.20–29).
`Status:` closed. `UID_BASE+2` is stated, the root must be readable by that uid, and the boot/`check` probe is `visibility::unreadable(&roots, &DroppedTo { uids: {layout.scheduler()}, probe: layout.scheduler() }, false)` — a real helper (`crates/blastit/src/visibility.rs:847`) that forks a `Probe::Read` child at the given uid. `StaticRoot::open` remains the authority. No residual.

### S2. `index` could name a dotfile and `spa` served it to every miss [MAJOR] — closed
`Where:` R3 (l.101–103), §4.1 table (l.174) and rule phrase list (l.199), §7.2 negative twins (l.726).
`Status:` closed: a dotfile segment in `index` is fatal with its own fixed phrase and a `.env` negative twin.

### S3. `immutable` could match the index/SPA answer [MAJOR] — closed
`Where:` R5 (l.109–114), `AnswerKind` + `decide` (l.272–298), the cache rule keyed on `(answer kind, answering path)` (l.371–382), glob row (l.424), rows 34–35 (l.638–639), §7.2 (l.728).
`Status:` closed: only `AnswerKind::Exact` is glob-matched and every `index` is `no-cache`; the two contradictory rules are gone.

### S4. errno map incomplete/ambiguous, and an existence oracle [MAJOR] — closed
`Where:` §5.1 (l.561–584), constants (l.239–240), matrix rows 13, 25, 36.
`Status:` closed: a total table (`ENOENT`/`ENOTDIR` → `Missing`; everything else, including `EAGAIN`, `EMFILE`, `EIO`, "any other" → `Hidden`), the length constants make `ENAMETOOLONG` unreachable, and the 404-vs-fallthrough disclosure is stated rather than denied, bounded to the status code with a canned body.

### S5. Blocking reads on the scheduler loop defeat the request timeout [MAJOR] — closed
`Where:` §4.6 pump (l.446–474), R10 (l.129–132), §6 alt 6 (l.691–692), §7.2 timeout test (l.729).
`Status:` closed: one `pread` per response on `spawn_blocking`, results returned over `static_in`, the loop never performs I/O, and the mount assumption is stated. See S11 for what the pump does not yet bound.

### S6. `root = "/"` was both fatal and legal [MAJOR] — closed
`Where:` R2 (l.97–99, "a leading `/` is a boot error, `/` included"), §4.1 (l.173).
`Status:` closed; §5.1's "legal but publishes the container" sentence is gone.

### S7. Mounts beneath `root`, and the unreachable EXDEV row [MAJOR] — closed
`Where:` §5.1 "Mounts are followed" (l.563–567), errno row `EXDEV` (l.575), ADR §3 (l.45–46), R11.
`Status:` closed: `RESOLVE_NO_XDEV` is deliberately not used, the boundary is named ("beneath `root` in the path namespace"), and `EXDEV` is documented as defence with no tested row. No contradictory test remains.

### S8. The page's security-model link is still owned by no one [MAJOR] — closed (pass 3)
`Where:` page l.97–98 ("Mechanism in How it works…, threat list in the security model"); design §8 (l.760–766), §9 (l.775–778); ADR Consequences (l.71–74).
`What:` nothing changed. `docs/security.md`'s compromise table still says the scheduler "can do anything a view could not already be made to do by an HTTP request" and never says it now reads client-named files; `docs/how-it-works.md` still shows the scheduler holding only the routing table. The accepted page sends readers to exactly those two files for the threat list, and the ADR's own trade-off ("The scheduler now reads files named by a client") is absent from the public contract. The recorded resolution defers this to `MB.2.8`, but `plan/nodes/MB/MB.2/MB.2.8/NODE.md` neither lists `docs/security.md`/`docs/how-it-works.md` in `paths` nor may touch them ("Do not touch … the public docs here"); it is a release-prep node, not the docs leaf the resolution claims.
`Fix:` add the two docs to a leaf that may write them (a docs leaf, or `MB.2.6`/`MB.2.7` paths) with the rows named in §5.1 and the mechanism paragraph, and make the design or `RESULT.md` name that node, not `MB.2.8`.

### S9. Special files were opened before classification [MINOR] — closed
`Where:` R11 (l.133–139), §5.1 (l.539–549), §6 alt 5 (l.688–690), rows 24/36.
`Status:` closed: `O_PATH` probe + `fstat`, read-open with `O_RDONLY|O_NOCTTY|O_NONBLOCK` only for a regular file, identity-checked. See S15 for the one check missing.

### S10. Hardening and wording [MINOR] — closed
`Where:` R9 (l.126–128), §4.5 (l.361–369), `has_body` (l.301–303), row 19 (l.623).
`Status:` closed: `nosniff` on every `200`/`304`, `ETag` is `"<size>-<mtime_ns>"` (no `st_dev`/`st_ino`), `has_body` reads "declares a body", and the `.well-known`-miss row is a deliberate page-consistent choice.

### S11. A timed-out static read leaks a blocking thread and its descriptor, without bound [MAJOR] — NEW
`Where:` §4.6 (l.454–455 slot "released … by the timeout/cancel path"; l.457–466 "One read in flight per response, at most `max_workers` blocking reads"; l.472–474 mount assumption).
`What:` the `spawn_blocking` closure owns the `StaticBody` (and its `OwnedFd`) and a parked `pread` cannot be cancelled. When `request_timeout`/`CANCEL` drops the request and releases the slot, the thread and the fd stay behind; the next request spawns another read. Repeated requests on a stalled mount therefore accumulate parked threads (tokio stops at 512) and fds *per request*, not `max_workers`: static serving queues behind the stuck reads, `openat2` then fails `EMFILE` → `Hidden` (`404` per §5.1), and the scheduler loses descriptors it needs for the rest of its work. "a stalled read then holds one blocking thread" is true per read and false in aggregate, and the "at most `max_workers` blocking reads" invariant is unenforced.
`Fix:` count reads globally and gate `spawn_blocking` on `reads_in_flight < max_workers`; keep the admission slot charged until the read result returns (release on the result, not on the timeout), so a parked read can never be duplicated behind a freed slot; treat a full reader pool as `503` + `Retry-After`, not as a new read; test that N timed-out reads leave ≤ `max_workers` threads/fds and a healthy stream still completes.

### S12. The `remaining == 0` arm of the pump is unstated [MINOR] — NEW
`Where:` §4.6 (l.457–463); sequence loop (l.437–443) says "until `len` bytes pushed".
`What:` for a zero-length file `min(remaining, credit, max_payload, STATIC_CHUNK_BYTES)` is `0`, so the pump spawns a zero-byte `pread` whose `Ok(vec![])` has no terminating arm in the prose — "pushes one queued chunk" can spin or emit an empty `RESP_BODY` instead of `RESP_END`. A 0-byte file is ordinary (an empty chunk, `__init__.py`).
`Fix:` state `remaining == 0 ⇒ RESP_END` before scheduling any read, and test a zero-length file on `Backend::Openat2` and `Backend::Walk`, `GET` and `HEAD` (`200`, `Content-Length: 0`, no body frame).

### S13. The new `ETag` no longer changes on every rewrite [MINOR] — NEW
`Where:` §4.5 (l.365 "it changes on every rewrite").
`What:` `(size, mtime_ns)` is unchanged by a rewrite that preserves both — an app writing under `root` can set its own mtime, and a volume/ConfigMap symlink swap keeps the target's — so a `no-cache` revalidation can answer `304` with stale bytes. No leak, but the sentence overclaims what the identity check guarantees.
`Fix:` say "changes when a rewrite changes size or mtime", or add a cheap content witness (first/last 4 KiB digest) if the guarantee is wanted.

### S14. "Before any fork" contradicts the step-5 probe [MINOR] — NEW
`Where:` §4.7 (l.484–485 "staying **before any fork**" vs step 5, l.493–495).
`What:` `visibility::unreadable` answers by forking a probe and dropping it to `DroppedTo.probe`, so step 5 does fork. An implementer who reads the ordering literally must conclude the probe cannot run as the scheduler and either moves it after the listeners or runs it as init — reopening S1.
`Fix:` state the invariant actually needed: before any listener is bound or any role child exists; the only pre-boot fork is the read-only probe.

### S15. The identity check does not re-assert the file type [MINOR] — NEW
`Where:` §5.1 (l.543–547): `O_PATH` probe, then `O_RDONLY` re-open, `(st_dev, st_ino)` "must agree".
`What:` `(dev, ino)` equality proves the inode was not substituted, not that the read-open fd is still a regular file (inode reuse after an unlink in the probe→open window would pass only by number collision, so impact is nil). The section's claim "a special file is **never** read-opened" is then one `S_ISREG` short of literal.
`Fix:` `fstat` the read-open fd and require `is_file()` (and `st_dev`/`st_ino` equality) there; one syscall, and it makes the sentence true.

### S16. The `same_inode` call site is the defence and has no test that can kill it [MAJOR] — closed (pass 5)
`Where:` §4.2 `same_inode` doc (l.267–271), §7.1 mutation audit (l.841–844), §7.2 `R7`/`R11` row (l.836).
`What:` the predicate is unit-tested, but the *call site* — the one thing that turns a probe→read substitution into `Hidden` — has no test and no hook, and §7.1 says so: "no unprivileged test can drive" it. `cargo-mutants` kills a mutant only when a test fails, so `!same_inode(..)` → `same_inode(..)` at the call site survives every suite and can hide inside the ≥75 % target. "its call site is what a mutation audit must kill" is therefore the one thing the audit cannot do, and §5.1 (l.682–688) explicitly does not police what the app writes under `root`, so the substitution race is live wherever uploads sit inside the root.
`Fix:` give `resolve` a seam a test can drive — a `Backend` trait (or crate-private `#[cfg(test)] open_hook`) whose test backend returns a *different* inode for probe and read — and assert `Hidden`; then the call-site mutant is killed by a real test. If no seam is wanted, mark the call site `#[mutants::skip]` and record the accepted residue in §5.1 rather than claiming the audit covers it.

### S19. One admission slot, two instructions about who releases it [MAJOR] — closed (pass 6)
`Where:` §4.6 (l.521–523, "released … by that read's result (never by the timeout alone)"), l.548–556 ("`expire_requests` and `client_cancel` must call `release` … or the slot leaks"), §7.2 (l.837, "slot released exactly once").
`What:` a timed-out or cancelled static stream has its slot owned by a parked read, and both `release` and the read result are told they free it: "never by the timeout alone" says the timeout must not, and "must call `release` … or the slot leaks" says it must. An implementer who frees `slots`/`static_reads` in `release` gets a double release (timeout, then result), `slots` under-counts, and the next wave admits more than `max_workers` readers — S11's invariant fails in exactly the scenario it was written for, silently.
`Fix:` name the single owner: `release` is idempotent and frees everything **except** a slot whose `pumping` flag is set; `finish_static_read` is the only place that decrements `slots`/`static_reads` for that slot, guarded by one `released` bit. Test double release (expire → result, cancel → result): `slots == 0`, `static_reads == 0`, one `RESP_END`, no second write.

`Pass 5:` double release is closed (§4.6 l.555–563: `release_static_slot` is idempotent; the counter is gone, so `slots ≤ max_workers` is the reader bound, l.547–553). What is left is the *guard*: the function "decrements only while `request.slots` is still true", while the same paragraph prohibits the timeout from freeing "the slot of a request whose read is still parked" and still says "the read result and the timeout/cancel path can both call it". With that guard, a timeout on a parked read frees the slot, the next request admits and spawns another read, and parked reads grow past `max_workers` — S11 again. The §7.1 fd oracle only parks streams on *credit* (`pumping == false`), so it does not cover this arm.

`Pass 6:` closed. §4.6 (l.574–589) makes the guard explicit and testable: `slot_releasable(holds_slot, pumping) = holds_slot && !pumping`, `release_static_slot` decrements only when it is true and clears `request.slots`, the read result clears `pumping` and then calls it, and `expire_requests`/`client_cancel` calling it with `pumping == true` is a documented no-op. The three-row unit table (`(true,false)→true`, `(true,true)→false`, `(false,_)→false`) is the oracle for the timeout-while-parked arm no unprivileged test can drive, and the second call after the result decrements nothing. The slot bound is further strengthened by change (b): `resolve`+`decide` moved into `dispatch_static`, so a queued request holds no `StaticFile` and the slot bound is also the descriptor bound (l.538–541).

### S17. `url_path` must be the resolved path, not the request's [MINOR] — closed (pass 6)
`Where:` §4.2 (l.313 `url_path`, "the immutable key"), R5 (l.109–111), §5.3 (l.767–769, "cannot disagree"), row 33 (l.734) vs rows 31/37b (l.732, l.739).
`What:` the cache key is `(answer kind, answering path)`, so `url_path` has to be the *collapsed* path; if it is echoed from the request target, `/assets//app.<hash>.js` (legal: interior `//` collapses) answers the same file but fails `/assets/*` and is `no-cache`, splitting one file across two cache policies. No matrix row covers the interior-`//` immutable case. Separately, rows 31 (`/DOCS`) and 37b (NFD) assume a byte-exact local filesystem; on a casefold or normalizing `root` those paths resolve to existing files and the rows invert silently rather than failing.
`Fix:` state that `url_path` is built from the resolved segments and never echoed; add `/assets//app.<hash>.js` → immutable; say rows 31/37b require a byte-exact filesystem and make the fixture assert it (hard failure, not a silent inversion).

`Pass 5:` the filesystem half is closed — §7.1 (l.833–837) makes the fixture assert non-root, case-sensitivity (`/DOCS` absent) and normalization preservation (NFC found, NFD not) as hard failures, so rows 31/37/37b can no longer invert silently, and row 38/§5.3 (l.747, l.765–768) now narrows the magic-link claim to `ELOOP` at the symlink with `RESOLVE_NO_MAGICLINKS` asserted as a flag rather than a row. The `url_path` half is not stated anywhere: §4.2 (l.321), R5 (l.109–111), the cache rule (l.447) and §5.3 (l.777–779) all say "the answering file's path" without saying it is built from the resolved segments, and no row covers `/assets//app.<hash>.js`.

`Pass 6:` closed. §4.2 (l.321–325) says `url_path` is "Built from the **resolved segments** (after `//` collapse), so every alias of one file shares one key: `/assets//app.js` and `/assets/app.js` have the same `url_path`, `ETag` and cache header", and row 40 (§5.2 l.774) pins the interior-`//` case as `200 immutable` on all three fixtures, with §7.2 (l.875, l.877) carrying row 40 in the matrix and the positive twins.

### S20. The ordering prose still says "steps 5–12"; the table has eleven [MINOR] — closed (pass 11)
`Where:` §4.3 (l.396–398, "steps 5–12 are the filesystem half"), §4.6 (l.539, "`resolve` (steps 5–12)"), against the §4.3 table (l.379–393, rows 1–11).
`What:` there is no step 12. The sentence is the one that tells an implementer *which* steps must be deferred until a slot exists — the change that closes the descriptor bound — so a reader counting rows finds the range off by one and has to decide whether a step is missing. Cosmetic, but in the paragraph that carries the invariant.
`Fix:` "steps 5–11" in both places, or drop the numbers and say "the filesystem half (`resolve` + `decide` …)".

`Pass 7:` one of the two is fixed (§4.3 l.402–403 now reads "steps 5–11"); §4.6 l.549 still reads "`resolve` (steps 5–12) runs inside `dispatch_static`". One-word fix, in the bullet that carries the descriptor-bound argument.

### S21. The `Forward` arm and the pre-dispatch body window are under-specified [MINOR] — closed (pass 9)
`Where:` §4.6 (l.535–543: `Forward` "follows the ordinary view path", then "both `dispatch_queued` walks and `Scheduler::start` … **never** call `try_spawn` for its route"), §4.4 (l.423–426: "Only a `Forward` verdict computes the ingress plan").
`What:` two seams left by moving `resolve`+`decide` behind admission. (i) The no-`try_spawn` bullet is unconditional, but a `Forward` must reach the manifest's `app`, which is a view: scoped to the pre-`decide` decision it is right, read literally it starves the very request it forwards. (ii) Between head admission and `dispatch_static` a static-route request has no `body_plan`; the existing fail-closed default (`AllowEmptyOnly`) turns body bytes that arrive in that window into `413`, so a front-controller upload that a `Forward` would proxy is refused under queue pressure, while planning from the head buffers a body for a request that then answers `405`/`413`. Neither direction is a leak — both are wrong statuses — but the design does not say which.
`Fix:` scope the bullet ("before `decide`, a static route is never dispatched by spawning; a `Forward` verdict then follows the ordinary view path, spawn included"), and state the pre-dispatch rule: no plan until `decide`, pre-dispatch bytes bounded by the existing response/upload credit and never buffered, `Forward` planning from the head as today.

`Pass 7:` (i) is closed — `RouteInfo.static_forward` scopes the no-spawn rule ("a pure static route (`static_forward == false`) is never spawned… a forward-capable one keeps the ordinary lazy spawn", §4.6 l.552–557) and `dispatch_reserved` is `dispatch`'s body without `slots += 1` (l.542–545). Still open, three small things: (ii) §4.4 (l.426–433) still says only "Only a `Forward` verdict computes the ingress plan" — nothing says what a static-route request's `body_plan` is between head admission and `dispatch_static`, so the fail-closed default (`AllowEmptyOnly`) turns body bytes in that window into `413` for a request that would have forwarded; (iii) ADR §1 (l.25) still says a static answer "keeps no view" without the `Forward` handover that does reach one; (iv) `static_forward` is set by `routes_from_manifest` only (l.553), while stage 1's three direct constructions chain `.with_static(...)` alone (§4.7 l.647–649) — the field defaults `false`, so dev/learn is fail-closed but a forward-capable route there would never spawn.

`Pass 8:` (ii) and (iii) are closed — §4.4 l.430–433 now names the buffered case ("A forward-capable static route may have had a body buffered by `begin_raw` before the verdict; a `Serve` then answers `413` and drops the buffer"), and a pure static route can declare no ingress (R4), so `AllowEmptyOnly` is the whole rule there; ADR §1 l.25–26 now reads "when the verdict is `Forward`, hands that same counted slot to the route's ordinary view path; it keeps no view of its own". (iv) remains as prose only: §4.6 l.560 states `with_static(policy, has_app)`, but §4.7 l.657 still shows `.with_static(route.static_policy().cloned())` and the `MB.2.6` leaf (l.962) still says "`.with_static` in all three constructions" with no `has_app` — a cheap implementer copying the leaf leaves `static_forward == false` in dev, which is fail-closed but silently over-closed.

`Pass 9:` all four closed. (iv): §4.7 (l.665) now shows `.with_static(route.static_policy().cloned(), manifest.app().is_some())`, so dev and `--learn` set `static_forward` from the same two inputs as `routes_from_manifest`; the `MB.2.6` leaf keeps the shorthand, which no longer contradicts the snippet. (ii) is strengthened by the head-admission rule (`body_ready = true` for a policy route, `Serve` answers from the head and drops the buffer, `Forward` clears it and transcodes at `REQ_END`), and (i)/(iii) stand from pass 8.

### S22. The stored `body_plan_error` answer is a fourth exit from `dispatch_static`, and the slot invariant names only three [MAJOR] — closed (pass 11)
`Where:` §4.4 (l.431–441, "`Forward` is the only path that answers the stored `body_plan_error` (as today)"), §4.6 (l.549–557: `Serve` keeps its slot, `Forward` hands it back, `Reject` "releases it before writing the status"), §7.2 (l.915, the `413`-on-`Serve` / `400`-on-`Forward` test).
`What:` storing `plan_body`'s `Err(status)` introduces an exit with no slot rule. A `POST` to a forward-capable static route with a mismatched `Content-Type` (e.g. `text/plain` on `body = "json"`) has already taken a slot in `dispatch_static`, then `decide` says `Forward`, then the Forward arm writes the stored `400` and returns — it is neither `Serve` (nothing streams), nor the `Forward` that "gives the slot back", nor the `decide` `Reject` the third bullet covers. Written the obvious way (`respond(400); return`), each such request permanently loses one slot, so `max_workers` anonymous bad-content-type POSTs take static serving down to `503 queue_timeout` — remotely, unprivileged, and the invariant sentence ("a slot is never taken twice and never leaked") reads as satisfied.
`Fix:` name the branch and give it the `Reject` treatment: the stored-error answer is `Reject { status }` for slot purposes — `release_static_slot` (pumping is false) then `release`, before the status is written — and add it to the §7.2 assertion so the test checks `slots` returns to its starting value after the `400`, not only the status.

`Pass 11:` closed. §4.6 l.568–572 now says a slot "is never taken twice and never leaked: `Serve` keeps its one slot until `RESP_END`/the read result, `Forward` hands it back and the view path takes its own, and **both** `Reject` and the stored-`body_plan_error` answer release it before writing the status". The test also pins the status split per method (§7.2 l.928: `POST` + `text/plain` → `405` + `Allow` on `Serve`, `400` on `Forward`; `GET` + any body → `413` on both), so the branch is observable as well as specified.

### S23. `Forward`'s handback should clear the static dispatch clock [MINOR] — closed (pass 11)
`Where:` §4.6 (l.540–545 "sets … `request.dispatched_at = now`, `request.deadline = now + cfg.request_timeout` … or `next_deadline` and `expire_requests` would never fire for it"), l.552–558 (the handback: "`release_static_slot`, clear `static_pending`, then `admit_ready(stream_id)`").
`What:` after the handback the request re-queues as an ordinary one, but it still carries the `dispatched_at`/`deadline` `dispatch_static` set. If `admit_ready`'s queue path does not reset them, `expire_requests` can fire at 30 s measured from the *static* dispatch while the request is still waiting behind the route's own queue — a `504` for a request that had `queue_timeout` left. No leak and no view exposure (a release here decrements nothing, because the slot was already handed back), but the two clocks should not be shared. The handback also must not call `release`: the sentence names three steps, and a copy of the read-result arm (which does call it) would drop the request mid-flight.
`Fix:` say the handback clears `dispatched_at`/`deadline` (and `queued_since` is what the queue path sets), and add "not `release`" to the three-step list.

`Pass 11:` closed. §4.6 l.566–574 spells the handback out: "`Forward` **gives the slot back** … `release_static_slot`, clear `static_pending`, `dispatched_at = None`, `deadline = None` … and **do not** `release`; then `admit_ready(stream_id)`", and a `Buffer` plan "records `queued_since = now`, sets `body_ready = false` and returns to wait for `REQ_END` **after** that handback, so a client that never ends its body is timed out by `expire_requests` instead of holding a slot". The buffered wait therefore holds no slot at all — stronger than the bound I asked for.

### S24. ADR §1 still describes the old slot handover [MINOR] — NEW (pass 11)
`Where:` ADR §1 (l.24–26: "takes one existing admission slot (`BLASTIT_MAX_WORKERS`) and, when the verdict is `Forward`, hands that same counted slot to the route's ordinary view path; it keeps no view of its own") against §4.6 (l.552–561: the handback releases the slot and the view path "takes its own").
`What:` the ADR sentence, which exists to describe exactly this handover, now contradicts the design: there is no handover, there is a release followed by the ordinary path's own admission. A reader of the decision record alone would implement the double-counting version. No behaviour follows from the ADR (the design is unambiguous), so it is prose only — but it is the one sentence an implementer would quote back when the two disagree.
`Fix:` "…takes one existing admission slot, and a `Forward` verdict releases it before the request rejoins the ordinary view path (which takes its own); it keeps no view of its own."

### S18. ADR §1 still names the wrong module for the answer [MINOR] — closed (pass 6)
`Where:` ADR §1 (l.20–21) says `static_asset.rs` "builds the answer"; design §4.2 (l.241–247, l.322–360) and §8 (l.874–879) put the head/body in `static_response.rs`.
`What:` the split is declared "not to be re-litigated", and the ADR — the artefact an implementer reads for the decision — still assigns the answer to the resolve module. No security impact; a stale anchor in the decision record.
`Fix:` name both modules in ADR §1 (`static_asset.rs` resolves, `static_response.rs` responds, `scheduler.rs` dispatches).

## Pass 2
| id | pass-1 grade | status | closed by / owner |
|---|---|---|---|
| S1 | BLOCKER | closed | §1.4, R2, §4.7 step 5 (`visibility::unreadable`), §5.1, ADR §1 |
| S2 | MAJOR | closed | R3, §4.1, §7.2 negative twin |
| S3 | MAJOR | closed | R5, `AnswerKind` cache rule, rows 34–35 |
| S4 | MAJOR | closed | §5.1 total errno table + constants (oracle stated) |
| S5 | MAJOR | closed | §4.6 `spawn_blocking` pump, R10, §6 alt 6 |
| S6 | MAJOR | closed | R2 ("`/` included") |
| S7 | MAJOR | closed | §5.1 "Mounts are followed", ADR §3 |
| S8 | MAJOR | **open** | deferred to `MB.2.8`, whose `paths` exclude the public docs (§8, page l.97) |
| S9 | MINOR | closed | R11, §5.1 `O_PATH` probe + read-open |
| S10 | MINOR | closed | §4.5 `nosniff`/`ETag`, §4.2 `has_body`, row 19 |
| S11 | — | new MAJOR | §4.6 reader pool / admission slot |
| S12 | — | new MINOR | §4.6 pump, zero-length file |
| S13 | — | new MINOR | §4.5 `ETag` wording |
| S14 | — | new MINOR | §4.7 boot-order wording |
| S15 | — | new MINOR | §5.1 read-open classification |

## Pass 3
| id | pass-2 | status | closed by |
|---|---|---|---|
| S8 | MAJOR | closed | §8 `MB.2.9` (docs, proposed) owns `docs/security.md`, `docs/how-it-works.md`, `docs/ci.md`, `docs/reference/cli.md`, `inventory.toml`, acceptance `make docs-check && make docs-unit`; §9 names it. Residual: the leaf is not yet in `plan/` — create it before the MB gate |
| S11 | MAJOR | closed | §4.6 "A parked read is bounded, not detached": `static_reads < max_workers`, slot charged until the read result, `expire_requests`/`client_cancel` release view-less requests, `can_spawn_read(reads, max_workers)` a pure bound with both-direction tests (l.771, l.783) |
| S12 | MINOR | closed | §4.6 pump step 1: `remaining == 0` → `RESP_END` and release, **before** any read |
| S13 | MINOR | closed | §4.5 l.393: "changes when a rewrite changes the size or the mtime, which a same-byte rewrite may not" |
| S14 | MINOR | closed | §4.7: "before any listener is bound and before any role child exists (the only pre-boot fork is the read-only permission probe)". Residual: §5.1 l.587 still reads "before any fork" |
| S15 | MINOR | closed | §5.1 l.596: the read-open `fstat` must report `S_ISREG` **and** the same `(st_dev, st_ino)` |

## Pass 4
Re-quoted anchors against the moved text (§4.2 split, §4.6 pump, §5.1 boundary, §5.2 rows 1–38, §7.1/7.2 suites, §8 chain); pass-3 references to §4.6/§4.7/§8 line numbers are superseded by the ones in S16–S19 and this table.

- **(a) leading `//`** — closed, every fixture: `segments()` refuses on the *decoded* string before collapsing (`//x`, `/%2Fx` → `Absolute`; rows 5–6), `strip_script_name` cannot manufacture one (`/app//x` → `//x` → `Absolute`), `///`/`//` are `Absolute`, interior `//` collapses and is called out as "a `.split('/').filter(non_empty)` shortcut is a bug" (§4.3 l.389–398, §7.2 l.836). One decode means `/%252Fetc` stays a literal `%2F` filename.
- **(b) `same_inode`** — predicate closed; the *call site* is S16, still MAJOR.
- **(c) ADR oracle** — closed: ADR §5 (l.53–60) now states status-only disclosure; matches §5.1 (l.678–681). No oracle claim left to contradict.
- **(d) rows 37/37b/38** — closed: NFC serves the real file, NFD is an explicit miss row whose bytes must not be the near-miss file (l.742–744), and `/proclink/self/fd/0` is `ELOOP` at `proclink` with `RESOLVE_NO_MAGICLINKS` as the wall if `root` is itself a symlink to `/proc`. Residual is S17's filesystem assumption.
- **(e) module split** — closed and security-neutral: one `OwnedFd` owner (`decide` → `static_target` → `respond` → `StaticBody` → closure → `release`), no `dup`, `respond` never re-opens a path, so the split adds no second lookup window. ADR §1 wording is S18.
- **(f) re-drive** — closed: every `static_in` result re-drives all pumpable requests (l.540–546), so a stream held behind `static_reads == max_workers` is never stranded to `504`; §7.2 asserts all of `>max_workers` streams complete and the fd count does not grow. Residual is S19's release ownership.

| id | status | anchor / change |
|---|---|---|
| S1 | closed | §1.4 l.63–69, §4.7 step 5 l.585–592 (`static_roots` + `BTreeSet` probe), §5.1 l.631–635, ADR §1 |
| S2, S3 | closed | R3 l.101–103; R5 l.109–114 + cache rule l.435–446 |
| S4, S6, S7 | closed | §5.1 errno table l.665–681 (ADR §5 now admits it); R2 l.97–99; §5.1 l.660–664 |
| S5, S11 | closed | §4.6 l.490–564; **residual → S19** (single-release ownership) |
| S8 | closed | §8 l.867, l.897 (`MB.2.9`: five public docs + inventory), §9 l.909–911. Bookkeeping only: create `MB.2.9` in `plan/` |
| S9, S10 | closed | R11 l.134–143, §5.1 l.636–646; R9/§4.5 l.429–455 |
| S12, S13, S14, S15 | closed | §4.6 step 1 l.531–532 + l.835 test; l.429 + `utimensat` test; §4.7 l.575–577 and §5.1 l.634 no longer says "before any fork"; §5.1 l.642–643 |
| S16 | **open MAJOR** | new: `same_inode` call site has no hook and no test the audit can kill |
| S19 | **open MAJOR** | new: `release` vs read result both own the parked slot |
| S17, S18 | open MINOR | `url_path` source + rows 31/37b filesystem assumption; ADR §1 module name |

## Pass 5
Re-read the current design (930 lines) and ADR (84 lines); anchors below are against that text.

| id | pass-4 | status | anchor / change |
|---|---|---|---|
| S16 | MAJOR | **closed** | §4.2 l.267–270 `same_inode(probe: (u64,u64), read: (u64,u64))` is `#[doc(hidden)] pub`, so the row table is buildable from an integration test; l.303–308 `resolve_with(segments, after_probe: Option<&dyn Fn()>)` puts a hook between the `O_PATH` probe and the read open (`resolve` = `resolve_with(_, None)`); §7.2 l.850 swaps the leaf for a different inode in that hook and asserts `Hidden`, and l.855–858 names that test as what kills the call-site mutant. Faithful oracle: the probe fd keeps the old inode, the read open resolves the path anew. Hook is `None` on the production path only. |
| S19 | MAJOR | **open (narrowed)** | closed: double release (idempotent `release_static_slot`, §4.6 l.555–563) and the stray counter (l.547–553). Open: the guard is `request.slots` alone, while the timeout is both allowed to call the function and forbidden to free a parked read's slot — with that guard, timeout-while-parked frees early and the reader bound fails. Fix: `slots && !pumping`, single decrement by the read result, and an oracle for the timeout-while-parked arm (the §7.1 fd test parks on credit, so `pumping == false`). |
| S17 | MINOR | **open (narrowed)** | closed: fixture asserts case-sensitive + normalization-preserving (§7.1 l.833–837) as hard failures; row 38 / §5.3 l.765–768 narrow the magic-link claim honestly. Open: no sentence says `url_path` comes from the resolved segments, and `/assets//app.<hash>.js` has no row. Fix: one clause + one matrix row. |
| S1–S15 | — | closed (unchanged) | anchors unchanged from pass 3/4: §1.4, §4.7 step 5, §5.1, §4.2, §4.5–§4.6, §5.2 rows 1–38, §8 `MB.2.9` |

## Pass 6
Re-read the current design (957 lines) and ADR (84 lines); anchors below are against that text.

| id | pass-5 | status | anchor / change |
|---|---|---|---|
| S19 | MAJOR | **closed** | §4.6 l.574–589: `slot_releasable(holds_slot, pumping) = holds_slot && !pumping`, unit-tabled `(true,false)→true`, `(true,true)→false`, `(false,_)→false`; `release_static_slot` decrements only when it is true and clears `request.slots`; the read result clears `pumping` then calls it; expire/cancel calling it while parked is a documented no-op, and a second call after the result decrements nothing. The parked arm now has an oracle (the table), and l.538–541 makes `max_workers` the descriptor bound too by moving `resolve`+`decide` into `dispatch_static`. |
| S17 | MINOR | **closed** | §4.2 l.321–325: `url_path` is "Built from the **resolved segments** (after `//` collapse)" so aliases share one key; row 40 (§5.2 l.774) pins `/assets//app.4f1c2a9b7d3e.js` → same `url_path`/`ETag`/immutable on S/F/N; §7.2 l.875/l.877 carry row 40 in the matrix and positive twins. The pass-5 filesystem half (fixture asserts case-sensitive + normalization-preserving, §7.1 l.861–866) and row 39/§5.3's narrowed magic-link claim stand. |
| S20 | — | new MINOR (open) | "steps 5–12" vs the eleven-row §4.3 table (l.396–398, l.539). Cosmetic, in the paragraph that carries the deferral invariant. |
| S21 | — | new MINOR (open) | the `Forward` arm: the unconditional "never call `try_spawn` for its route" (l.542–543) and the unplanned body window between admission and `dispatch_static` (l.535–537, §4.4 l.423–426). Both are status-only under queue pressure, no leak. |
| S1–S16, S18 | — | closed (unchanged) | anchors unchanged from passes 3–5; S16's `resolve_with` oracle is untouched by the `dispatch_static` move (the hook lives in `resolve`, which the suite still drives directly). |

## Pass 7
Re-read the current design (973 lines) and ADR (84 lines); anchors are against that text. The audit the pass was asked for — one counted slot, one descriptor owner, one `Request` owner, across every exit — comes out clean:

| exit | slot (counted in `dispatch_static`, l.531) | descriptor / `StaticFile` | `Request` |
|---|---|---|---|
| `Serve` → last chunk / `RESP_END` | one; `release_static_slot` (`pumping == false` → true) then `release` | moved into `StaticBody` by `respond`, dropped by `release`; HEAD/304 drop it inside `respond` | dropped by `release` |
| `Serve`, timeout/cancel, not pumping | same, once | dropped by `release` | dropped |
| `Serve`, timeout/cancel, parked | `release_static_slot` → false (no-op); the read result clears `pumping`, calls it → true, then `release` | returned by the closure, dropped there | dropped by the result path, never twice (l.582–590) |
| `Reject` (400/404/405/413) | freed before the status write (l.546–547) | none escapes: `decide` consumes `StaticOutcome`, so a `Serve`-worth `StaticFile` on 405/413 is dropped inside `decide` | `release` |
| `Forward` | handed over **without** `slots += 1` (`dispatch_reserved`, l.542–545); ordinary `release` decrements once | none opened: the outcome is `Missing` (carries nothing) and `static_target` stays `None` | ordinary release |
| queued, never dispatched | none taken | none — `resolve` runs inside `dispatch_static` (l.548–550) | queue drop |

Spawn rule, fail-closed: `static_forward` is `true` only for policy + `spa` unset + an `app`; `false` (the default, and every pure static route) never spawns in either `dispatch_queued` walk or `Scheduler::start` (l.552–557). A static route therefore cannot reach a view except through the one `Forward` verdict that legitimately needs it.

| id | pass-6 | status | anchor / change |
|---|---|---|---|
| S19 | closed | **still closed** | l.574–590: `release_static_slot -> bool` is now the single decrementer *and* the trigger for `release`, so "the read result performs both" (slot, then body/fd/`Request`) with no double drop; `slot_releasable` and its three-row table are unchanged. |
| S20 | open MINOR | **open (one of two)** | §4.3 l.402–403 fixed to "steps 5–11"; §4.6 l.549 still says "steps 5–12". |
| S21 | open MINOR | **open (narrowed)** | (i) closed by `static_forward` + `dispatch_reserved`; (ii) pre-dispatch `body_plan` still unstated (§4.4 l.426–433); (iii) ADR §1 l.25 "keeps no view" omits the `Forward` handover; (iv) `static_forward` is not set in stage 1's three direct constructions (fail-closed, but a dev-mode `Forward` would have no view). |
| S1–S18 | closed | **still closed** | anchors unchanged; the new `is_dotfile` owner (l.272–274, called from the head path at l.402–403) and the miss-row index-sha256 assertion (l.791–793) strengthen two closed items rather than opening new ones. |

## Pass 8
Re-read the current design (983 lines) and ADR (84 lines); anchors are against that text. The question — any remaining slot/fd/double-release path, and is the spawn rule fail-closed in dev and prod — comes out clean on both halves.

**Slots.** The gate is now explicit and structural: `Request.static_pending` is set when the matched route has a policy and cleared by `dispatch_static` on every verdict; `admit_ready` takes the static branch only when `static_pending && !slots`; and `dispatch_static` `debug_assert!(!request.slots)` (§4.6 l.550–556). That closes the one path pass 7's table could not see: a buffered `Forward` re-entering `admit_ready` at `REQ_END` would otherwise re-resolve and take a second slot — now it carries neither the flag nor a free slot. The count itself is unchanged: `dispatch_static` sets it once, `dispatch_reserved` never re-takes it, `release_static_slot` is the only decrementer and `release` the only dropper, with the parked-read case deferred to the result. `max_workers = 1` makes it observable rather than argued: `Serve` then `Forward`, and two `Forward`s, must both answer `200`; a double count or leak shows as `503 queue_timeout` on the second (§7.2 l.904).

**Descriptors.** Still one owner and no queue-time fd: `resolve` runs inside `dispatch_static` (l.554–556), `StaticOutcome::Missing` carries nothing so a `Forward`/`Reject` cannot smuggle a `StaticFile` out, `decide` consumes the outcome and drops a `Serve`-worth file on a 405/413, and `respond` consumes the file into `StaticBody` (or drops it for HEAD/304). Verified as a new hold: a `Forward` that must wait for a buffered body keeps its slot only until `request.deadline`, which `dispatch_static` sets exactly as `dispatch` does (l.537–540) — so the new earlier admission is bounded by the same 30 s `BLASTIT_REQUEST_TIMEOUT`, not by `body_timeout`.

**Spawn rule.** `static_forward` is the conjunction (policy, `spa` unset, `has_app`) and `with_static(policy, has_app)` sets it in **all four** constructions, dev and `--learn` included (l.560–563), so prod and dev agree. The default is `false`, so every failure to set it over-closes rather than spawns: a pure static route can never reach a view in either mode. No path spawns a view for a route that cannot forward.

| id | pass-7 | status | anchor / change |
|---|---|---|---|
| S19 | closed | **still closed** | §4.6 l.550–556 (`static_pending` gate + `debug_assert`) removes the re-entry path without changing the single-decrementer rule (l.600–612); §7.2 l.904 replaces the argued invariant with an observable one. |
| S20 | open MINOR | **open (one occurrence)** | §4.3 l.403 and §4.6 l.541 say "steps 5–11"; §4.6 l.556 still says "`resolve` (steps 5–12)". |
| S21 | open MINOR | **open (prose only)** | (i) `static_forward`/`dispatch_reserved`, (ii) buffered-body-before-`Serve` → `413` + drop, and (iii) ADR §1's `Forward` handover are all closed. What remains is (iv): §4.7 l.657 and the `MB.2.6` leaf (l.962) still show the one-argument `.with_static(...)`. |
| S1–S18 | closed | **still closed** | anchors unchanged; the pass-8 additions are inside §4.6/§4.4 and reopen nothing (identity check, `O_PATH`-before-read-open, final refusals, `slot_releasable`, the docs leaf all untouched). |

## Pass 9
Re-read the current design (991 lines) and ADR (84 lines); anchors are against that text. Both questions come out clean.

**Can an unvalidated body reach a view on a `Serve` path?** No, twice over. (a) A `Serve` never has a view: `dispatch_static` sets `view = None`, a pure static route (`static_forward == false`) is never spawned, and a forward-capable one is spawned only from `dispatch_reserved` on a `Forward` verdict. (b) The body is decided at the head: a policy route is admitted with `body_ready = true` so `dispatch_static` runs before any `REQ_END`/transcode; `Serve` answers `405`/`413` from the head and "**never** waits for, buffers or transcodes the body" with the buffer dropped (§4.4 l.427–438). A codec-invalid body on a `Serve` target is therefore the page's `413`, never a `400` and never a view request — asserted in §7.2 (l.912: `Serve` + body → `413`, view never sees it; the same body on a `Forward` → `400` at `REQ_END`). The one path that still hands a view unvalidated bytes is a declared `body = "stream"` plan on a `Forward`, which is that codec's documented meaning (the app is the validator), unchanged by this design and reachable only after `decide` says `Forward`.

**Can a slot still leak or double-count?** No. The gate is ordered so a slot-holder can never take a second one: `if request.slots { dispatch_reserved } else if request.static_pending { dispatch_static } else { unchanged }` (§4.6 l.556–566). Enumerated: `Serve` → one take, `release_static_slot` at `RESP_END`/read result then `release`; `Forward` streaming → `dispatch_reserved` immediately, ordinary `release` decrements the one slot; `Forward` buffered → `body_ready = false`, the held slot survives `REQ_END`, the transcode, and re-entry through `dispatch_reserved` (no `+=`); `Reject` → slot freed before the status write; timeout/cancel queued → `release_static_slot` false (no slot) + `release`; timeout pumped → fresh release; timeout parked → no-op until the read result. The only `+=` sites remain `dispatch_static` and the untouched `dispatch`, and a request holding a slot cannot reach the latter. New hold checked: a buffered `Forward` waits for its body while holding a slot, bounded by `request.deadline` set in `dispatch_static` — `BLASTIT_REQUEST_TIMEOUT`, not `body_timeout`. ADR §1 also now separates the probe identity (`check`/`dev` as the invoking uid, production as the responder's), which keeps S1's closure honest.

| id | pass-8 | status | anchor / change |
|---|---|---|---|
| S19 | closed | **still closed** | §4.6 l.556–566: the ordered gate plus `debug_assert!(!request.slots)` makes the double count structurally impossible, not merely asserted; single decrementer/dropper unchanged (l.606–618). |
| S20 | open MINOR | **open (one occurrence)** | §4.3 l.403 and §4.6 l.547 say "steps 5–11"; §4.6 l.564 still says "`resolve` (steps 5–12)". |
| S21 | open MINOR | **closed** | §4.7 l.665 now passes `manifest.app().is_some()` to `.with_static`, closing (iv); (i)–(iii) closed in passes 7–8. |
| S1–S18 | closed | **still closed** | anchors unchanged; the pass-9 additions are inside §4.4/§4.6 and reopen nothing (identity check, `O_PATH`-before-read-open, final refusals, `slot_releasable`, `MB.2.9`). |

## Pass 10
Re-read the current design (994 lines) and ADR (84 lines); anchors are against that text. Only the two changes were judged.

**Can a codec error or a body path reach a view on `Serve`?** No. `plan_body`'s `Err(status)` is stored (`body_plan_error: Option<u16>`), and `Serve` answers `413` from the head with the buffer dropped — the stored error is answered only on `Forward` (§4.4 l.431–441), before any handback, and the view is reached only after that arm declines to answer. §7.2 (l.915) asserts the pair directly: a mismatched `Content-Type` is `413` on `Serve` and `400` on `Forward`, "and the view never sees it". A pure static route cannot even reach the stored error (R4 forbids `[route.ingress]` there, so `plan_body` has no codec to fail), and `Serve` still has no view to receive anything (`static_forward == false` → never spawned; a forward-capable route only spawns through the ordinary path after the handback).

**Can a slot still leak or double-count?** One path says yes: the new stored-error answer (S22). `dispatch_static` counts the slot, `decide` says `Forward`, and the Forward arm writes the stored `400` and returns — an exit the invariant enumerates as neither `Serve` (which keeps the slot to `RESP_END`), nor the handback ("gives the slot back"), nor `Reject` ("releases it before writing the status"). Read literally, that branch never frees the slot. Everything else is clean: the handback is `release_static_slot` → clear `static_pending` → `admit_ready`, i.e. one decrement then the ordinary `dispatch`'s one increment, never simultaneous; `static_pending` cleared means the `REQ_END` re-entry cannot re-resolve or re-count; `debug_assert!(!request.slots)` still guards `dispatch_static`; timeout/cancel on the re-queued request finds `slots == false` and frees nothing twice; and `Serve`/`Reject` are unchanged.

| id | pass-9 | status | anchor / change |
|---|---|---|---|
| S19 | closed | **still closed, one new exit** | the single-decrementer rule (l.608–626) is untouched, but the enumeration in l.549–557 now misses the `body_plan_error` exit → S22. |
| S22 | — | new MAJOR (open) | §4.4 l.431–441 + §4.6 l.549–557: the stored-error `400` must be treated as `Reject` for slot purposes. |
| S23 | — | new MINOR (open) | handback should clear `dispatched_at`/`deadline` and must not call `release`. The deferral `dispatch_reserved` removal itself is sound: one decrement, then the ordinary path's own increment. |
| S20 | open MINOR | **open** | §4.6 l.564 still says "steps 5–12" (the other two occurrences say 5–11). |
| S21 | closed | **closed** | unchanged; `.with_static(policy, has_app)` at §4.7 l.665. |
| S1–S18 | closed | closed | unchanged. |

## Pass 11
Re-read the current design (1007 lines) and ADR (84 lines); anchors are against that text. Every exit from `dispatch_static` is slot-safe:

| exit | slot |
|---|---|
| `Serve` | held from the take to `RESP_END` / the read result; `release_static_slot` (pumping false) → `release`; zero-length ends at pump step 1 before any read. |
| `Forward`, streaming plan | handback: `release_static_slot` → clear `static_pending` → `dispatched_at`/`deadline = None` → **no** `release` → `admit_ready`, i.e. one decrement, then the ordinary `dispatch`'s own increment. Never simultaneous. |
| `Forward`, `Buffer` plan | same handback **first**, then `queued_since = now`, `body_ready = false` and return: the wait for `REQ_END` holds no slot, and a client that never ends its body is expired by `queued_since` rather than parking a reader. `REQ_END` transcodes (`400` on a bad codec) and re-enters the ordinary buffered path. |
| `Reject` (`decide`: 400/404/405/413) | released before the status is written (l.577–580). |
| stored `body_plan_error` | released "exactly like `Reject`", before the status is written (l.578) — S22's gap. |

No path can reach `admit_ready` twice with a slot: `static_pending` is cleared on every verdict, `debug_assert!(!request.slots)` guards `dispatch_static`, and the gate is `if request.static_pending { dispatch_static } else { unchanged }` (l.581–583). `Serve` still has no view to receive a body, and a codec error is still answered only on `Forward` (§4.4 l.439–446).

| id | pass-10 | status | anchor / change |
|---|---|---|---|
| S22 | MAJOR | **closed** | §4.6 l.578 adds the stored-error exit to the invariant; §7.2 l.928 asserts the per-method statuses (`405`+`Allow` / `400` on `Forward`, `413` on `Serve`). |
| S23 | MINOR | **closed** | §4.6 l.566–574: the handback clears both clocks, records `queued_since` for `Buffer`, and says "**do not** `release`". |
| S20 | MINOR | **closed** | all three occurrences now read "steps 5–11" (§4.3 l.410, §4.6 l.557, l.585). |
| S24 | — | new MINOR (open) | ADR §1 still says `Forward` "hands that same counted slot" over, against §4.6's release-and-retake. |
| S1–S19, S21 | closed | closed | unchanged. |

## Verdict
Five exits, five named slot rules, one decrementer, one dropper, and no way back into `dispatch_static` with a slot held. The two changes this pass asked about are both improvements on what I last reviewed: `Forward` now gives the slot back before the view path takes its own (so the double-count question disappears rather than being argued), and a buffered `Forward` holds nothing while it waits for its body. The only open item is the ADR's now-stale description of the handover — prose, in the artefact a reader quotes when it disagrees with the design.

Attempt-3 re-anchor: the design grew at §4.2 (`STATIC_RESOLVE_FLAGS`, `resolve_with`) and R11/§5.3/§7.2 were reworded, so every design line number in the pass tables above shifts by up to +7 after §4.2; the section references are authoritative and the numbers above are the current ones.

VERDICT: PASS
