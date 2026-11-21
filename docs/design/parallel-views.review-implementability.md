# Implementability review (final) — `parallel-views.md` + `ADR-0015`

Lens: **cheap-model buildability**. Round-1 (`B1–B6, M1–M9`), round-2 (`N1–N8`), and round-3
(`B1, M1–M3` + minors) findings are re-verified against `docs/design/parallel-views.md` as it
now stands. Only the round-3 items are re-examined; no new scope.

Read: `docs/design/parallel-views.md`, `docs/decisions/ADR-0015-multi-worker-views.md`,
`plan/nodes/MB/MB.24/{MB.24.3,MB.24.4}/NODE.md` (`paths`), `crates/blastit-scheduler/src/
{scheduler,route,config,ctl}.rs`, `crates/blastit-frame/src/{ctl,frame,lib}.rs`,
`crates/blastit-scheduler/tests/common/mod.rs`, `crates/blastit/src/{supervise,stage1,php_view,
config,lib}.rs`, `Makefile`.

---

## Round 3 findings — verdict

| # | Finding | Status |
|---|---|---|
| **B1** | `Ctl::Reload` needs `frame.rs` + the two hand-written round-trip tables, none in `MB.24.3`'s `paths` | **RESOLVED as far as the design can.** §8.3 (`:1261-1268`) now names all four files and the exact items each gains — `ctl.rs` (variant, `type_code`/`code`, `write`/`read`, doc table), `frame.rs` (`TypeCode::Reload = 0x45`, `from_code`, `is_ctl`, `name`, `Frame::decode`), and the `lib.rs:218,233` / `decoder.rs:539` tables — which is precisely the set the code needs (`frame.rs:99-170`). §1 (`:126-131`) records the **plan amendment** for the three `paths`, and §9's round-3 row states the stop-and-ask rule. A recorded `paths` amendment is a plan request, not an implementability defect. |
| **M1** | `pending_ready` never released, so the pre-measurement gate stuck shut | **RESOLVED.** §4.6 (`:849-853`) states `retire` decrements it for a worker that was not `READY`, covering the reload sweep, stale-at-birth and the duplicate path; §8.2 (`:1254-1256`) lists it in `retire`'s rewrite. The accounting is now exactly-once: a worker either decrements at `READY` (`view_ready`) or on removal while `!ready` (`retire`, or `view_gone`'s own removal, which is mutually exclusive because `view_gone` early-returns once the entry is gone). R4's duplicate leg (`:1154`) asserts the release. |
| **M2** | `crate::php_view::VIEW_PROCESS_MARGIN` is private and `php`-gated (`E0603`) | **RESOLVED.** §4.5 (`:777-791`) defines a local `const VIEW_PROCESS_MARGIN: u32 = 16;` in `supervise.rs`, states that `php_view`'s private twin is the same number and sentence, and records unification as a follow-up; §8.4 (`:1270`) adds the const to the leaf's work. Nothing else in the leaf touches the `php`-gated module. |
| **M3** | Two **cold** routes each deferred to the other, so neither forked | **RESOLVED.** §4.4 (`:695-707`) now carries the deterministic tie-break `other < route`: for A and B both queued with empty pools, the lower-indexed one defers to nobody and forks, so the next iteration finds it no longer starved (`pools.get(&other).is_none_or(ViewSet::is_empty)` is false once it owns a worker) and the higher one forks too. Deferring `grow` still mutates nothing, the deferral graph is acyclic (strictly decreasing route index), and the `queued_routes()` order is sorted, so the same pump is deterministic. R6's witness keeps both halves ("A's next `SPAWN` does not happen; B's does", and the merely-busy negative case). §9's round-3 row records the fix. |
| **minors** | `emit_close_spawn` unnamed; init's `on_ctl` arm; `stage1` anchor/cfg; `RoutePool` naming | **RESOLVED.** `emit_warning` is named in §8.2 (`:1247`) and used at §4.7 (`:939`); §8.4 (`:1272-1273`) adds `Ctl::Reload` to init's "messages init sends" ignore arm and notes the exhaustiveness break; §8 MB.24.4 (`:1296-1320`) moves the loop **after** `boot_bindings` returns (`:356`) and before `probe_bindings` (`:370`), gates it `#[cfg(feature = "php")]`, uses the `php` vector and `let mut routes`; `RoutePool` is renamed `ViewSet` throughout (§4.1, §4.3, §4.4, §8.2, ADR decision 1). |

Verified against the real signatures while re-reading the new code: `ViewSet::len` counts a
`SPAWNED` worker exactly once (it moves from the slot into `views`); `grow`'s `Option<&ViewSet>`
borrow dies at `let owned`, so `make_room`/`entry` are free; `view_spawned`'s duplicate branch
(`:575-585`) now calls `retire` on the held key after `pool`'s last use, so the borrow is dead and
the four effects (fail the dead entry's requests, drop its queue, emit its event, signal the pid
that the newcomer now owns) go through one path; `retire_unheld` still covers the
unexpected-spawn case; `view_exited`'s `key.pid == 0` arm fits `on_ctl`'s existing
`Ctl::Exited → ViewKey { route, pid }`; `spawn_failed`/`forget_if_idle` are idempotent via
`spawning.take()`; `route_requests`/`needed_processes` (`div_ceil`, `u32`) do not double count;
`ChildTable::count_for` + `cfg.view_worker_cap()` (which exists, `blastit/src/config.rs:1639`)
give init's refusal a real bound with a local constant.

## Residual MINORs (unchanged, non-blocking)

- **m5.** §4.7 (`:956-960`) still writes `self.pools.get_mut(..).filter(..).map(..)` as a
  statement — an unused `Option` under `rs-lint`'s `-D warnings` (`Makefile:173-174`). Use `if let`;
  rustc names it.
- **m6.** §4.3 still says "`:1832`'s 'none keyed' branch disappears"; `:1832` is
  `self.dispatch(stream_id)`.
- **m7.** §7 R14 still lists `isolation` among the *unmodified* suites while R10 widens it.
- **m8.** R2's pid-tie-break leg cannot be built over `World` (one `SPAWN` in flight plus
  dispatch-on-`READY` means two never-dispatched `READY` workers never coexist); move it in-crate
  or drop it.
- **m9.** `Script::NeverReady` is used by R9 but is not in the harness-addition table.
- **m10.** `queued_for`'s body/`usize` return is still citation-only, and `route_requests`
  depends on it.
- **m11.** §4.6 (`:857-859`) still asks for "the zero-descriptor rule … in
  `crates/blastit-scheduler/src/ctl.rs`"; `finish_frame`'s `other =>` arm already requires zero
  descriptors for everything but `SPAWNED` — nothing to add there.

---

## Verdict

The three buildability items are closed: the wire's full file set is named with its exact edits
and its `paths` amendment recorded as a request (§1, §9), `pending_ready` is released on every
removal path, and init's ceiling uses an in-lane constant, all with in-lane compile shapes. An
implementer can now build `MB.24.3` and `MB.24.4` file by file from §8 without guessing a
signature. The correctness item returned in round 3 is now fixed in the design itself: §4.4's
starvation predicate carries the deterministic `other < route` tie-break, so two cold routes
cannot refuse each other and every queued route gets its first process. No BLOCKER and no MAJOR
remain.

VERDICT: PASS

---

## Designer resolution (round 5)

Appended by the designer; the review above and its verdict line are unchanged. Each item is
checked against `docs/design/parallel-views.md` as it now stands.

| Item | Status in this document | Where it is now |
|---|---|---|
| **m5** — `.map(..)` used as a statement in `view_gone` | **already fixed** (the previous round's finding, resolved in the text) | §4.7: `if !view.ready && let Some(pool) = self.pools.get_mut(&key.route) { … }` |
| **m6** — `:1832` cited for the "none keyed" branch | **already fixed** | §4.3 now cites `:1815`/`:2285`/`:2325`'s `!self.views.contains_key(&route)` guard and `:1801-1812`'s learn branch; `:1832` is no longer named |
| **m7** — R14 called `isolation` unmodified | **already fixed** | §7 R14: "unmodified **except the one widened leg R10 owns**" |
| **m10** — `queued_for` body unspecified | **already fixed** | §4.1 prints both helpers: `queued_for` = `queued_for_streams(route).count()` (`usize`), and `queued_for_streams` = `queue.ids()` filtered by `request.route` |
| **E0502 (new, attempt-2 review)** — `view_gone` iterated a `&self` iterator while calling `&mut self` methods | **real, and fixed**; the reviewer's rustc reproduction is acknowledged | §4.7 now collects first, exactly like today's `scheduler.rs:3733-3744`: `let waiting: Vec<u32> = self.queued_for_streams(key.route).collect();` then `for stream_id in waiting { … }`. The other printed bodies were re-audited (`grow`'s `Option<&ViewSet>` dies at `let owned`; `reload()` collects its keys; `reserve` touches `self.lru` after the last `view` use; `view_spawned`'s duplicate check is now a `bool` computed before the pool borrow) |
| m8, m9, m11 | still open as worded; not resolved by this node | m8: R2's cold-pool leg is stated as defensive in §7; m9: `Script::NeverReady` is named in §7's R9 row but not in the additions table; m11: §4.6 still names the zero-descriptor rule, which is harmless to restate |
