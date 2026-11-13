# Testability review — docs/design/static-and-spa.md

Lens: testability

Pass 2 re-read of the rewritten design (778 lines), the rewritten ADR-0007 (78 lines) and the unchanged normative page. `MB.2.3`–`MB.2.7` were re-checked against §7 and §8; each finding is marked closed (with the section that closes it) or open (with the surviving defect).

### T1. The `immutable` rule could not be turned into one decidable assertion [BLOCKER → closed]
Where: §3 `R5` (`:109-114`), §4.5 cache rule (`:371-382`), matrix rows 34–35 (`:638-639`), §7.2 (`:728`).
What: the key is now `(AnswerKind, answering URL path)`: `Exact` only is glob-matched; `DirectoryIndex` and `SpaFallback` are always `no-cache`; fixture **D** (`immutable = ["/index.html"]`) gives `/index.html` immutable but `/` and `/dashboard` `no-cache`. The `/index.html` glob row (`:424`) now states the same restriction, so the two tables agree. The page's sentence is quoted verbatim (`:380-382`).

### T2. Row 36's device oracle and `classify`'s seam [BLOCKER → closed (pass 3)]
Where: §4.2 `classify` (`:250-251`), §5.2 row 36 (`:640`), §7.2 (`:728`), `plan/nodes/MB/MB.2/MB.2.4/NODE.md:45`.
What: row 36 is now correctly `MB.2.7`-only and `classify` is named, so the "impossible unprivileged suite" half is closed. Two defects survive. (a) `classify(kind: FileType, …)` cannot be tabled over `BlockDevice`: `std::fs::FileType` has no constructor, and a Docker `/dev` carries no block device, so that named row is vacuous or fails off-environment — the table needs `mode_t`/a local `FileKind`. (b) `MB.2.4`'s note still says "one case per row … FIFO/device" and "It must run unprivileged", i.e. it still orders the impossible case the design moved to e2e.
Fix: take `(mode: u32, len: u64, max: u64)` (assert on `S_IFMT`) and strike "device" from `MB.2.4:45`.

### T3. `permission denied` had no oracle and every candidate test was root-dependent [BLOCKER → closed]
Where: §5.2 row 25 (`:629`), §7.1 (`:716-718`), §7.2 (`:730`).
What: `mode000` is a fixture file, `/mode000` is row 25, the root must be unlistable/unreadable in the `MB.2.6` negatives, and the harness asserts `geteuid() != 0` as a hard failure (never a skip) exactly where `chmod 000`/`0o400` is used.

### T4. Positive rows were status-only [MAJOR → closed (pass 3)]
Where: §5.2 (`:642-645`), §7.2 (`:728`), §4.5 (`:367`).
What: every positive row now asserts the body sha256 and the answering file's `ETag`, and §7.2 adds the `/a.js`·`/a.js/`·`?v=2` `ETag` equality, so an empty or wrong `200` cannot pass. Residual: `X-Content-Type-Options: nosniff` (R9, `:127`) is in no suite row. Fix: add one assertion on a `200` and its `304`.

### T5. The closed `Content-Type` table [MAJOR → closed (pass 3)]
Where: §4.5 (`:391-406`), §7.2 (`:728`).
What: all 25 rows are now named, including none/uppercase/unknown. But the last row reads "anything else, no extension, or an uppercase name (lowercased first) → `application/octet-stream`", while the header says the lookup is "from the extension (ASCII lowercase)" — so `.PNG` is either `image/png` (lowercase first) or `application/octet-stream`. The named uppercase test row has two possible expected values, and its author is the same person who writes the code → the assertion cannot fail.
Fix: pick one (`application/octet-stream` for unknown-after-lowercasing, `image/png` for `.PNG`) and delete the parenthetical.

### T6. R11's `openat2`-absent fallback [MAJOR → closed (pass 3)]
Where: §4.2 (`:254-259`), §5.1 (`:550-554`), §7.1 (`:707`), §7.2 (`:728`).
What: `Backend::Walk` and a both-backend matrix close the "dead code" half. But the seam is declared `#[cfg(test)] pub fn open_with(…)`: integration tests (`crates/blastit-scheduler/tests/critical_static_assets.rs`) compile the crate as a dependency *without* `cfg(test)`, so `open_with` does not exist for the suite §7.1 credits with "`openat2` **and** `Walk`". As written the promise cannot compile.
Fix: drop the gate (`#[doc(hidden)] pub fn open_with`) or move the both-backend table into `src/static_asset.rs`'s `#[cfg(test)] mod tests`; then say which file owns it.

### T7. `MB.2.7` replay scope [MAJOR → closed (pass 3)]
Where: §7.3 (`:737-754`), `MB.2.7:24-25`.
What: the fence-by-fence map closes the "hand-picked commands" gap. Two expectations are still not executable as written. (a) Fence 8 is replayed for "exit `1`, stderr names the missing capabilities", but §7.1 places the replay "in a container" and the page's fence 8 is "In the container image, under an existing preset", whose promise is that a preset *runs* the site (page:136-141). In a capable container the command must not exit `1`; if it must exit `1`, "a preset already runs the site" stays unproven. (b) Fence 3 must reproduce `127.0.0.1:8080` exactly, while `MB.2.7`'s own note pins the port to `E2E_STATIC_PORT`.
Fix: state fence 8's execution context (host/unprivileged, expecting `1`, and then also assert the capable-container boot) and require port `8080` for the banner replay.

### T8. `→ 400/404` was not a binary row [MAJOR → closed]
Where: §7.3 (`:750-755`: "exact status … never `400/404`"), §5.2 rows 1–13.
What: the driver now asserts the matrix's exact status per row. `MB.2.7:25` still says `400`/`404` and needs a one-line sync, but the design is unambiguous.
Fix: sync the node text when the driver lands.

### T9. `docs_fences.rs` proved only that the page parses [MAJOR → closed]
Where: §7.2 (`:727`), §3 `R15` (`:149-151`).
What: the static entry now pins `is_all_static()`, `root`, `spa` and `immutable`, plus a canary that a changed shape fails, so a page edit that adds `app` or drops `spa` goes red.

### T10. Files the design names sit outside their node's `paths` [MAJOR → closed (pass 4), one residual MINOR]
Where: §8 (`:763-765`), §4.7 (`:498-505`), `plan/nodes/MB/MB.2/MB.2.4/NODE.md` paths, `MB.2.6/NODE.md` paths.
What: §8 gives `MB.2.4` `tests/common/static_fixture.rs` (the shared S/F/N/D builder §7.1 relies on for both suites) and `MB.2.6` `crates/blastit/src/stage1.rs` (app-optional `load`, the three `.with_static` chains, `manifest_dir`/`static_max_bytes`), but neither file is in the node's `paths`. `MB.2.4` must create the fixture out of lane or duplicate it, and `MB.2.6`'s `critical_static_cli.rs` cannot pass at all without `stage1` — an app-less `dev` still imports the app. The design calls `stage1` "Follow-up 1", but a path that must land *before* the node is not a deferral.
Fix: add both paths to the owning `NODE.md` (via `plan.py`) before the nodes start, and list them in the `paths` columns of §8 rather than as prose.

Pass 4: both paths are now in the plan (`MB.2.6` owns `stage1.rs`; the `MB.2.4` epic owns `static_fixture.rs` and both suites; the `MB.2.5` epic owns `critical_rewrite.rs`), and `MB.2.2/RESULT.md:98-120` states that the four child nodes still have to be created with §8's paths and acceptance. Residual MINOR: the epics' own acceptance lines are stale — `MB.2.4`'s `cargo test --test critical_static_assets` needs `rewrite::decide`, which only `MB.2.5.1` owns, so the 2.4.2→2.5.1 order in §8 must be encoded in the children's `depends_on`, not left to the epic graph (`MB.2.4 → MB.2.5`).

### T11. Tempdir strategy without `tempfile` [MAJOR → closed]
Where: §7.1 (`:712-720`).
What: unique leaf `format!("{module}-{pid}-{nanos}")`, exclusive creation, `Drop` cleanup and short `sun_path` names, with `cfg.manifest_dir`/`static_max_bytes` set by `World`-based tests. Nothing left to guess.

### T12. `**` was user-facing grammar the page never showed [MINOR → closed]
Where: §2 (`:84-85`), `R5` (`:111-112`), §4.5 (`:418-424`), §6.7 (`:693-695`), ADR §2 (`:33`).
What: `**` is now a non-goal in all four places, the vector table uses `/assets/*/*`, and the page's "`*` stops at `/`" is the only metacharacter documented.

### T13. `http_date` vectors and the `ETag` claim [MINOR → closed (pass 3)]
Where: §4.5 (`:365`, `:408-411`), §7.2 (`:728`), `MB.2.4:26`.
What: the three `http_date` vectors are now assigned. The `ETag` shape is not: `§4.5` promises `"<size:x>-<mtime_ns:x>"` "no inode or device disclosure", §7.2 asserts only stability and cross-URL equality, and `MB.2.4` still instructs `(dev, ino, size, mtime)`. An implementer following the node passes every named assertion while shipping device/inode bytes. "It changes on every rewrite" also has no assertion (a same-size rewrite can keep both fields).
Fix: assert one exact `ETag` string in the suite, fix `MB.2.4:26`, and drop or test the rewrite claim.

### T14. R12's `outcome` and the unobservable `views` map [MINOR → closed]
Where: §7.2 (`:729`), `R12` (`:140-142`).
What: `outcome == "ok"` for an answer, `"unmatched"` for a refusal, `log.spawns` unchanged and the route's script never sees `REQ_HEAD` — all observable from the existing harness.

### T15. `inventory.toml` reasons promise replays nothing performs [MINOR → closed (pass 5)]
Where: `python/tests/runnable_docs/inventory.toml:227-262`, §7.3 (`:860-877`), §8 (`:911`), `MB.2.2/RESULT.md:216`, `plan/nodes/MB/MB.2/MB.2.9/NODE.md:10-15`.
What: the design now routes the six reasons to `MB.2.9` (§8 `:911`), which closes the hand-off half. The guard did not survive: `MB.2.2/RESULT.md:216` still resolves T15 with "adds a harness check that a reason naming a node names an assertion in it", but `MB.2.9`'s `paths` have no `python/tests/runnable_docs/test_harness.py` (only `inventory.toml`), so the check cannot be written, and unlike T10 it is not listed in the Preconditions or in §8. The reasons are printed as the docs gate's justification, so an unimplementable guard means the drift returns the first time a page changes. Second, `MB.2.9` also edits `docs/reference/manifest.md`, which `MB.2.3` owns, while `depends_on: [MB.2.2]` lets it start before `MB.2.3` — two leaves on one file.
Fix: add `python/tests/runnable_docs/test_harness.py` to `MB.2.9`'s `paths` and to the Preconditions, or delete the claim from `RESULT.md:216` and state the drift risk; make `MB.2.9` depend on `MB.2.3`.

Pass 5: closed — `RESULT.md:216` now says the check needs `test_harness.py`, "which no leaf owns", and records Follow-up 6 with both honest options; `MB.2.9` only rewrites the six reasons, and the `manifest.md` overlap is Follow-up 7.

### T16. The re-drive invariant and its positive twin cannot fail [MAJOR → closed (pass 5)]
Where: §4.6 (`:535-557`), §7.1 (`:825-843`), §7.2 R6 (`:851`).
What: `static_reads ≤ max_workers` and slots ≤ `max_workers`, and only a slot-holding request spawns a read with `pumping = false`, so when a request wants a read its own absence from the in-flight count proves `static_reads < max_workers`. The "a request whose read could not be scheduled while `static_reads == max_workers`" case is therefore unreachable: the read-pool gate can never bind, and deleting the whole "re-drive every pumpable request" loop would leave `more than max_workers static streams all complete` green — that twin actually proves the admission queue drains (worth keeping), not the loop. `can_spawn_read`'s "false at the bound" table likewise pins a predicate that is never false at its call site. A cheap implementer will write ~40 lines of unreachable orchestration plus a test that cannot detect its removal — the fluff `testing` §3 rejects.
Fix: delete the re-drive requirement and the `can_spawn_read` gate (slots already bound the reader pool), rename the twin "more than `max_workers` static streams queue, then all complete", or, if the invariant is wanted, make reads schedulable for *queued* requests (so `static_reads` can exceed the dispatched count) and test that path.

Pass 5: closed — §4.6 `:547-551` deletes the counter and the queue ("There is no `static_reads` counter and no re-drive queue to get wrong"), and §7.1 `:840-843` / §7.2 `:851` now assert the structural bound: slot held to the read result, idempotent `release_static_slot`, flat fd count with `max_workers` parked, wave completion.

### T17. Rows 37/37b assume a normalization-preserving filesystem [MINOR → closed (pass 5)]
Where: §5.2 fixture (`:699-706`), rows 37/37b (`:745-746`).
What: both rows are binary and unprivileged — `café.js` is created with NFC bytes and the NFD request must miss — but on a normalization-insensitive filesystem (APFS, NTFS, some SMB mounts) the NFD lookup finds `café.js`, row 37b returns `200 file` instead of `200 index`, and the suite hard-fails with no diagnosis. Nothing asserted the precondition, and row 37's column **N** was also the only row whose `N` value depends on N sharing S's tree (§5.2 `:704` said only "an all-static, `spa = false` root").
Fix: after creating `café.js`, assert `symlink_metadata("cafe\u{301}.js")` is `ENOENT` and hard-fail "filesystem normalizes names — row 37b is meaningless here"; say in the fixture paragraph that S/F/N share one tree.

Pass 5: closed — §7.1 `:842-847` makes non-root, case sensitivity (`/DOCS` does not exist) and NFC/NFD preservation hard fixture failures "because rows 31/37/38 silently invert otherwise", and states all four fixtures are one tree with policy-only differences.

### T18. Row 38 does not isolate `RESOLVE_NO_MAGICLINKS` [MINOR → closed (pass 6)]
Where: row 39 (`:773`), §5.3 (`:792-795`), §5.1 (`:645-650`).
What: the row is binary and unprivileged (a `proclink -> /proc` symlink, `404`) and does catch the dangerous failure — a resolver that follows the root symlink reaches `/proc/self/fd/0` and answers `200`. It cannot, however, exercise `RESOLVE_NO_MAGICLINKS`, because `RESOLVE_NO_SYMLINKS` (and the `O_NOFOLLOW` walk) refuses at `proclink` first, so `ELOOP` is identical with the magic-link flag present or absent. §5.3 credited the CVE line to a flag the row cannot distinguish.
Fix: reword the row's `Why` as "a root symlink to `/proc` is refused" and keep `RESOLVE_NO_MAGICLINKS` as untested defence-in-depth in §5.1, not as a row's claim.

Pass 5: closed, one residual MINOR — §5.3 `:765-768` said exactly that (row 38 was `ELOOP`; a page-worthy magic-link root needs a privileged bind mount). Residual: "asserted by the flag set" named no suite row.
Pass 6: closed — §5.3 `:792-795` now names `critical_static_resolve.rs`, and §7.2 `:877` owns it: "the `RESOLVE_*` expression contains `BENEATH|NO_SYMLINKS|NO_MAGICLINKS` (the only unprivileged oracle for the flags)".

### T19. `same_inode`'s refusal path is covered by no test [MINOR → closed (pass 5)]
Where: R11 (`:141-143`), §4.2 (`:266-270`), §7.2 (`:850`, `:855-858`).
What: the unit table over real `fstat` results (same file twice, two different files) kills the predicate mutants; it cannot kill the *use*, because no unprivileged test can make the probe and read descriptors differ. Inverting the call site is caught by every successful serve, but deleting the `if !same_inode(…)` block survives with only a mutation-audit entry and no possible killer — so "its call site is what a mutation audit must kill" is an oracle the audit cannot supply.
Fix: state the expected survivor (`replace the guard with ()`) and record it as an accepted/enumerated mutant in the `cargo-mutants` invocation (`--exclude-re`), so the ≥ 75 % target and the `test-curator` report do not chase an unfixable mutant.

Pass 5: closed — `same_inode` takes `(u64, u64)` tuples (constructible; §4.2 `:266-270`) and is tabled (equal / inode differs / device differs), while `resolve_with(segments, Some(hook))` swaps the leaf between probe and read open so the arm must answer `Hidden`; §7.2 `:855-858` credits that substitution test, not the audit, with killing the call-site mutant.

### T20. Two anchor/oracle defects from the 40-row renumbering [MINOR → closed (pass 8)]
Where: §7.1 (`:864`), §7.2 (`:892-893`), §5.2 post-table (`:791-799`), row 36 (`:770`).
What: (a) §7.1's resolve row (`:846` in pass 6) said "rows 1–13/15–26/37–38's refusal half". (b) rows 14, 19 and 38 asserted only "never the near-miss file", so an empty `200` satisfied all three.
Fix: §7.1 → "rows 1–13, 15–17, 22–26, 39 refusals, rows 14/18–21/27–40 files"; miss rows also assert the index sha256.

Pass 7: (a) and (b) are both fixed — §7.1 `:872` now reads "rows 1–13, 15–17, 22–26, 39's refusal half" and §5.2 `:800-802` asserts "both the index's sha256 **and** that the bytes are never the near-miss file". One instance of the same defect survived: §7.2 `:893` still listed the resolve suite's positives as "rows 14, 18–21, **27–40**", which included row 36.
Pass 8: closed — §7.2 `:903` now reads "rows 14, 18–21, **27–35, 37–40**", so row 36 is out of the unprivileged resolve suite and stays where `:806-808` and `:220` put it (`MB.2.7`'s e2e row plus `classify`). The R5/R9/R10 row (`:902`) still says "the whole 40-row matrix **through `decide`**" and that is satisfiable without a device node: `decide(StaticOutcome::Hidden, …) → Reject(404)` is constructible, so row 36's *decision* is asserted and its *resolution* is not claimed.

### T21. The `Forward` slot assertion has no observable [MAJOR → closed (pass 8)]
Where: §7.2 R6 (`:904`), §4.6 admission (`:538-546`), ADR §1 (`:24-30`), `crates/blastit-scheduler/src/scheduler.rs:592` (`slots: u32`, private), `crates/blastit-scheduler/tests/common/mod.rs:506-520` (`World` fields).
What: "a front-controller `Forward` transfers the one counted slot through `dispatch_reserved` and `slots` returns to its starting value after the response" named a field no test can read: `slots` is a private `Scheduler` field, the `Scheduler` lives inside `run`'s future, and `World` exposes only `cfg`/`http`/`events`/`log`/`ctl` — no test file in `crates/blastit-scheduler/tests/` mentioned `slots`. The leak it guards (a `Forward` that keeps the slot `dispatch_static` took) had no other oracle: every forward test issued fewer than `max_workers` requests, so the scheduler would not starve until the suite was over.
Fix: restate it behaviourally — or, if the field must be read, add a `Ctl` query reachable from `World`. The second half was already fine: a pure static route (`static_forward == false`) that spawned puts an entry in the public `log.spawns`, because `try_spawn` (`scheduler.rs:2273-2301`) sends `Ctl::Spawn` with no app guard.

Pass 8: closed — §7.2 `:904` now states it as an observable: "a front-controller `Forward` transfers its one counted slot through `dispatch_reserved`, asserted observably with `max_workers = 1`: a `Serve` then a `Forward`, and two `Forward`s, all answer `200` — a double count or a leak surfaces as `503 queue_timeout` on the second". With one slot, both a leak (slot never released → the second request queues forever) and a double count (`slots = 2` → one release → the second still queues) collapse to a `503`, so the assertion fails for either bug and passes only when the transfer is exact; nothing waits on the queue timeout in the passing case. ADR §1 `:26-29` records the same handover.

## Pass 2

| Id | Pass-1 grade | Status |
|---|---|---|
| T1 | BLOCKER | closed — §4.5 answer-kind cache table, fixture D, rows 34–35 |
| T2 | BLOCKER | **open (MAJOR)** — `classify(FileType)` cannot be tabled for a block device; `MB.2.4:45` still orders the device row |
| T3 | BLOCKER | closed — row 25 `mode000`, `geteuid() != 0` hard failure, §7.2 `permission denied` |
| T4 | MAJOR | closed (one residual MINOR: `nosniff` unasserted) |
| T5 | MAJOR | closed (one row MINOR: uppercase `.PNG` has two possible values) |
| T6 | MAJOR | **open (MAJOR)** — `#[cfg(test)]` hides `open_with` from the integration suite |
| T7 | MAJOR | **open (MAJOR)** — fence 8's exit `1` vs "in a container"; fence 3 port 8080 vs `E2E_STATIC_PORT` |
| T8 | MAJOR | closed — §7.3 exact statuses (node text lags, one-line sync) |
| T9 | MAJOR | closed — §7.2 shape assertions on the page's fence |
| T10 | MAJOR | **open (BLOCKER)** — `tests/common/static_fixture.rs` and `stage1.rs` named by §8, absent from `MB.2.4`/`MB.2.6` `paths` |
| T11 | MAJOR | closed — §7.1 unique leaves, exclusive create, `Drop`, short socket names |
| T12 | MINOR | closed — `**` a non-goal in §2/`R5`/§6/ADR |
| T13 | MINOR | **open (MINOR)** — `ETag` shape unasserted and contradicted by `MB.2.4:26` |
| T14 | MINOR | closed — §7.2 `outcome` strings and `log.spawns` |
| T15 | MINOR | **open (MINOR)** — `inventory.toml` reasons no longer match §7.3 |

## Pass 3

| Id | Pass 2 | Status (pass 3) |
|---|---|---|
| T1, T3, T8, T9, T11, T12, T14 | closed | closed — no change since pass 2 |
| T2 | open MAJOR | closed — `classify`'s `_` arm is the total case (§4.2 `:255-259`), §7.2 tables the six constructible kinds and skips `BlockDevice`, `MB.2.4`'s device row moves to `MB.2.7` (Precondition 4) |
| T4, T5 | closed, residual | closed — `nosniff` on `200`/`304`, `.PNG → image/png`, unknown → `application/octet-stream` (§7.2 `:782`) |
| T6 | open MAJOR | closed — `#[doc(hidden)] pub fn open_with`, explicitly not `cfg(test)`, with the integration-crate rationale (§4.2 `:279-281`) |
| T7 | open MAJOR | closed — fence 3 replays on `127.0.0.1:8080` in its own container; fence 8 split into host (exit `1`) and capable-container (boots, answers `GET /`), which also proves the page's deploy promise (§7.3 `:797`, `:799`) |
| T10 | open BLOCKER | closed — `## Preconditions (plan edits that must land before MB.2.3 starts)` in `MB.2.2/RESULT.md:85-110` names `stage1.rs` for `MB.2.6`, `tests/common/static_fixture.rs` for `MB.2.4` and `MB.2.9`, and §8 marks all three `†` |
| T13 | open MINOR | closed — exact `ETag` from the fixture's `stat`, `utimensat` rewrite, `MB.2.4`'s `(dev, ino, …)` re-sync (Precondition 4), §4.5's rewrite claim now conditional on size/mtime |
| T15 | open MINOR | closed — `MB.2.9` owns the six inventory reasons plus a harness check that a reason naming a node names an assertion in it (Precondition 3, §8 `:827`) |

## Pass 4

| Id | Pass 3 | Status (pass 4) |
|---|---|---|
| T1, T3–T9, T11–T14 | closed | closed — no change since pass 3; §4.2/§4.5/§5.2/§7.2–§7.3 anchors re-quoted above |
| T2 | closed | closed — §4.2 `:273-277` (total `_` arm), §7.2 `:836` (six constructible kinds, `BlockDevice` needs no constructor) |
| T6 | closed | closed — §4.2 `:296-299` `#[doc(hidden)] pub fn open_with`, `Result` return, explicit "not `cfg(test)`" rationale |
| T7 | closed | closed — §7.3 `:852` own container on `127.0.0.1:8080`, `:857` two fence-8 contexts |
| T10 | closed, residual | closed — plan paths settled (`MB.2.4` epic owns both suites + `static_fixture.rs`; `MB.2.6` owns `stage1.rs`), children creation listed in `MB.2.2/RESULT.md:118-120`; residual MINOR: epic acceptance and `depends_on` still show `MB.2.4 → MB.2.5`, the reverse of §8's 2.4.2-after-2.5.1 order |
| T13 | closed | closed — §7.2 `:835` exact `ETag`, `utimensat`, `nosniff` on `200`/`304`, zero-length case |
| T15 | closed MINOR | **open MAJOR** — the harness check `RESULT.md:216` still promises needs `python/tests/runnable_docs/test_harness.py`, absent from `MB.2.9`'s `paths` (and from Preconditions); `MB.2.9` edits `docs/reference/manifest.md` owned by `MB.2.3` while depending only on `MB.2.2` |
| T16 | — | **open MAJOR** — the re-drive loop/`can_spawn_read` gate is unreachable (`static_reads ≤ slots ≤ max_workers`), so its positive twin and predicate table cannot fail |
| T17 | — | **open MINOR** — rows 37/37b assume a normalization-preserving FS with no precondition assertion; row 37's `N` column relies on N sharing S's tree |
| T18 | — | **open MINOR** — row 38 proves "root symlink refused", not `RESOLVE_NO_MAGICLINKS` (`RESOLVE_NO_SYMLINKS` fires first) |
| T19 | — | **open MINOR** — `same_inode`'s deletion mutant has no possible killer; the "mutation audit must kill the call site" claim is not an oracle |

## Pass 5

| Id | Pass 4 | Status (pass 5) |
|---|---|---|
| T15 | open MAJOR | closed — the claim is withdrawn: `MB.2.2/RESULT.md:216` now says the check needs `test_harness.py`, "which no leaf owns", and `MB.2.9` rewrites the six reasons only; recorded as Follow-up 6 with the two honest options (add the path or drop the gate) |
| T16 | open MAJOR | closed — §4.6 `:546-556` deletes `static_reads`, `can_spawn_read` and the re-drive queue ("only a request holding an admission slot can spawn a read … no re-drive queue to get wrong"); §7.1 `:841-843` and §7.2 `:851` now assert the real invariant (slot held to the read result, idempotent `release_static_slot`, fd count flat with `max_workers` parked, the wave-completion twin) |
| T17 | open MINOR | closed — §7.1 `:834-839` asserts non-root, a case-sensitive FS and NFC/NFD preservation as hard failures "because rows 31/37/37b silently invert otherwise", and states S/F/N/D are one tree with policy-only differences |
| T18 | open MINOR | closed, one residual MINOR open — §5.3 `:765-768` narrows the claim (row 38 is `ELOOP` at `proclink`; `NO_MAGICLINKS` needs a privilege-bearing bind mount) and §7.2 rewords the row as "a root symlink to `/proc` is refused"; residual: "asserted by the flag set" (`:767`) names no suite row — add "the resolved `ResolveFlags` contains `NO_MAGICLINKS`" to `critical_static_resolve.rs` in §7.2 |
| T19 | open MINOR | closed — `same_inode((u64,u64),(u64,u64))` is constructible and tabled (equal/inode-differs/device-differs, §4.2 `:270`, §7.2 `:850`), and `resolve_with(segments, Some(hook))` swaps the leaf between probe and read open so the TOCTOU arm must answer `Hidden`; §7.2 `:857-858` credits that test, not the mutation audit, with killing the call-site mutant |
| T10 residual | closed MINOR | closed — Follow-up 7 (`RESULT.md:237-242`) names the exact plan edits (`MB.2.9` deps on `MB.2.3`/`MB.2.6`; `MB.2.4.2` after `MB.2.5.1`), and §8's `MB.2.5.1` row now owns `lib.rs` |

## Pass 6

| Id | Pass 5 | Status (pass 6) |
|---|---|---|
| T1–T17, T19 | closed | closed — no change; all quoted anchors re-checked against the 40-row text (§4.2 `:266-270`, §4.5 `:455`, §5.2 `:705-774`, §7.2 `:875-880`) |
| T18 | closed, residual MINOR | closed — §5.3 `:792-795` names `critical_static_resolve.rs` and §7.2 `:877` owns "the `RESOLVE_*` expression contains `BENEATH|NO_SYMLINKS|NO_MAGICLINKS`", so the flag claim has a suite row |
| T10 residual | closed MINOR (plan-side) | still a lead action, not a design defect: the four children are absent and `MB.2.9` still shows `depends_on: [MB.2.2]`; Follow-up 7 (`RESULT.md:237-242`) carries both edits |
| T20 | — | **open MINOR** — §7.1 `:846` still calls rows 37–38 the "refusal half" (37/40 are positive, 38 is the NFD miss, 39 is the refusal; §7.2 `:877` is correct), and the three miss rows 14/19/38 assert only "never the near-miss file", not the index sha256 |

## Pass 7

| Id | Pass 6 | Status (pass 7) |
|---|---|---|
| T1–T19 | closed | closed — anchors re-checked against the 40-row text (§4.2 `:266-270`, §4.5 `:455`, §5.2 `:705-786`, §7.2 `:888-896`) |
| T20 | open MINOR | **still open MINOR** — the §7.1 refusal list (`:864`) and the miss-row index sha256 (`:791-793`) are fixed; §7.2 `:893` still lists row 36 inside "27–40" though `:770`/`:798-799` declare it e2e-only |
| T21 | — | **open MAJOR** — the new `Forward` clause asserts "`slots` returns to its starting value", but `slots` is private (`scheduler.rs:592`) inside `run`'s future and `World` exposes no handle to it, so the clause cannot be written; the leak it guards is otherwise untested |

## Pass 8

| Id | Pass 7 | Status (pass 8) |
|---|---|---|
| T1–T19 | closed | closed — anchors re-checked (§4.2 `:266-270`, §4.5 `:455`, §5.2 `:710-809`, §7.2 `:902-905`) |
| T20 | open MINOR | closed — §7.2 `:903` = "rows 14, 18–21, 27–35, 37–40" (row 36 excluded); §7.1 `:872` still the corrected "1–13, 15–17, 22–26, 39's refusal half"; §5.2 `:800-802` asserts the index sha256 for the miss rows. Row 36 remains reachable at the decision level (`decide(Hidden) → 404`, `:902`) and end-to-end (`:806-808`, `MB.2.7`) |
| T21 | open MAJOR | closed — §7.2 `:904` replaces the field read with `max_workers = 1` and "a `Serve` then a `Forward`, and two `Forward`s, all answer `200` — a double count or a leak surfaces as `503 queue_timeout` on the second"; ADR §1 `:26-29` records the same handover |

### T22. The head-time `plan_body` reject still wins over the page's `413` [MAJOR → closed (pass 11)]
Where: §4.4 (`:427-441`), §7.2 R6 (`:928`), §1.1 `:34`, `crates/blastit-scheduler/src/scheduler.rs:1178-1189`, `crates/blastit-ingress/src/lib.rs:276-317` (`plan_body`), `:495-499` (`is_bodyless_safe`), `:239-244` (`status()`).
What: `begin_raw` answered a plan error before any static decision (`Err(status) => return self.refuse_body(...)`), so on a forward-capable static route a `GET` body could yield the codec's `400` instead of the page's `413`.
Fix: store the error on a policy route and answer it only on `Forward`.

Pass 10: **half closed.** The mechanism is right — §4.4 `:437–439` stores `plan_body`'s `Err(status)` in `request.body_plan_error: Option<u16>` ("the page's method/body rule outranks a codec error on a path the responder answers"), and `Serve` answers `413` while `Forward` answers the stored status. But the twin's second half is unreachable: on a `GET`/`HEAD`, `plan_body` returns at the `is_bodyless_safe` branch (`:310-316`) *before* the content-type check, so a `GET` with `Content-Type: text/plain` and a body yields `BodyNotAllowed` (`413`), not `ContentTypeMismatch` (`400`) — `Forward` answers `413`, never the claimed `400 at REQ_END`. A `400` on `Forward` needs a `Buffer` plan, which needs a non-bodyless method (`POST`/`PUT`), and on that method a `Serve` target is `405`, not `413` — so no single method yields the claimed `413`/`400` pair. The only class where the stored error actually changes a `Serve` answer is a check that precedes the bodyless short-circuit: `Content-Encoding: gzip` (and a bad charset) on a `GET` (`:287-296`).
Fix: split the twin into two buildable pairs — (a) `GET` + `Content-Encoding: gzip` + body: `Serve` → `413` (pre-change it was `400`, so this row is the one that proves the stored error), `Forward` → `413`; (b) `POST` + json-invalid body: `Serve` → `405` + `Allow`, `Forward` → `400` at `REQ_END`. Drop "the same body … `400` on `Forward`" for a `GET`.

Pass 11: closed — §7.2 `:936` now states three pairs on the forward-capable `body = "json"` route and drops the impossible `GET`-`400`: (1) "`GET` + any body is `413` on `Serve` and on `Forward`" (`plan_body` → `BodyNotAllowed`, `ingress/lib.rs:310-316`); (2) "`POST` + `Content-Type: text/plain` is `405` + `Allow` on `Serve` and `400` on `Forward`" — the discriminating row, because the pre-change head-time refusal gave `400` on both, so only a `Serve` that consults the method gate first can produce `405`; (3) "`POST` + `application/json` + `{` is `405` on `Serve` and `400` at `REQ_END` on `Forward`" (`Buffer` plan → `body_ready = false` → transcode). §4.4 `:441–449` now pins the order — "the method gate first (`405` …), then the page's `413` … never `400` — and the buffer and the stored error are dropped" — which is exactly what pair 2 asserts. All three are binary, unprivileged (socketpair + `World`), and none asserts a status the code cannot produce.

### T23. The `body = "json"` fixture is not named [MINOR → closed (pass 10)]
Where: §7.2 R6 (`:915`), §5.2 fixtures (`:760-767`).
What: the twin says "on a forward-capable static route with `body = "json"`", but no fixture carried an ingress codec: F was "the same root with `spa` unset and an `app` declared" and nothing else.
Fix: name F with `[route.ingress] body = "json"`.

Pass 10: closed — §5.2 `:785–786` now reads "Fixture **F** = the same root with `spa` unset, an `app` declared and `[route.ingress] body = \"json\"` (front controller)", so the twin and the matrix share one manifest. The F column is unaffected for the bodyless rows: `plan_body` returns `AllowEmptyOnly` for a bodyless `GET` (`ingress/lib.rs:310-316`), so rows 19/27/29/31 still forward.

## Pass 9

| Id | Pass 8 | Status (pass 9) |
|---|---|---|
| T1–T21 | closed | closed — §7.1 `:880` "1–13, 15–17, 22–26, 39's refusal half", §7.2 `:914` "14, 18–21, 27–35, 37–40", matrix still 40 rows (`:807`) |
| T22 | — | **open MAJOR** — a plan `Err` was answered at `scheduler.rs:1183` before `dispatch_static`, so a `Serve` target could answer `400` where the page promises `413` |
| T23 | — | **open MINOR** — no fixture declared `[route.ingress] body = "json"` |

## Pass 10

| Id | Pass 9 | Status (pass 10) |
|---|---|---|
| T1–T21 | closed | closed — anchors unchanged; §8 `:972` now carries `body_plan_error` |
| T22 | open MAJOR | half closed — §4.4 `:437–439` stores the plan error and `Serve` answers `413`, but the twin's `400 on Forward` for a `GET` body was unreachable (`is_bodyless_safe` answers `413` first) |
| T23 | open MINOR | closed — F named with `[route.ingress] body = "json"` (`:785–786`) |

## Pass 11

| Id | Pass 10 | Status (pass 11) |
|---|---|---|
| T1–T23 | closed, one MAJOR | all closed — T22's three pairs are buildable and binary, and the anchors agree with the 40-row matrix |
| T22 | open MAJOR | closed — §7.2 `:936`: `GET`+body → `413`/`413`; `POST`+`text/plain` → `405`+`Allow` on `Serve` (the discriminating stored-error row: pre-change it was `400` on both) and `400` on `Forward`; `POST`+`application/json`+`{` → `405` on `Serve`, `400` at `REQ_END` on `Forward`; §4.4 `:441–449` pins the order ("the method gate first (`405` …), then the page's `413` … never `400`") |

## Verdict

Grades in the current design: **0 open BLOCKER, 0 open MAJOR, 0 open MINOR**. The three body pairs are binary, unprivileged and reachable on the named forward-capable fixture; the middle one is the only ordering that a codec-first implementation cannot fake, because a `Serve` that answers `400` at head fails it. The `max_workers = 1` slot test, the 40-row matrix and the row ranges in §7.1/§7.2 are unchanged and consistent.

Attempt-3 re-anchor: §4.2 gained `STATIC_RESOLVE_FLAGS` and R11/§7.2 were reworded, so the numbers quoted in passes 1–10 shift after §4.2; the pass-11 anchors above are current.

VERDICT: PASS
