# Consistency review (final verification) — MB.24.2 parallel views (`per_process_cap` pool, ADR-0015)

Scope: the revised `docs/design/parallel-views.md` and
`docs/decisions/ADR-0015-multi-worker-views.md`, checked against the same targets as before
(`ARCHITECTURE.md` 271-470/1458-1548/1660-1757, `settings.md` 64-124, `how-it-works.md`
44-47, `php/index.md` 57-66, `ADR-0009/0010/0012/0014`, `dev-fanotify-reload.md` §4.6,
`MB.24`/`MB.24.3`/`MB.24.4`/`MB.23.5`, and the cited code). Verification only.

## Resolution of the previous findings

| Finding | Status | Where |
|---|---|---|
| **B1** demand bound refused the second concurrent request | **Resolved** | §4.1 `route_requests` / `needed_processes` (`:319-334`), §4.3 `grow(route, wanted)` (`:464-478`); the 2-concurrent and 4-burst cases fork correctly and §4.9 `:1042-1062`, R1 `:1156`, R3 `:1158`, R12 `:1167` and ADR decisions 3/5 agree on the need formula. |
| **B2** `reload()` dead on the MB.24.3-first order | **Resolved** | §4.6 `:855-869` gives `MB.24.3` `Ctl::Reload` and the `on_ctl` arm; §8.3 `:1266-1268` names `ctl.rs`+`frame.rs`; §8.4 `:1277` adds the supervisor's ignore arm; ADR decision 7 (`:68-75`) agrees. The three `frame` files are recorded as a `paths` amendment in §1 `:127-133` — a request, not a defect. |
| **M1** rlimit as an undisclosed soft route cap | **Resolved** | §1 `:59-74` names the budget, the memory cap, the uid rlimit and, separately, init's non-knob `view_worker_cap()+16` ceiling; §4.1 `:374-378`, §4.8 `:1036`, §5 `:1112` and `ADR-0015:120-128` state the condition and the graceful refusal. |
| **M2** §4.10 omitted/mis-fixed `ARCHITECTURE.md` §2.6 and §8.2 | **Resolved** | §4.10 item 3 (`:1074-1080`) quotes the `ARCHITECTURE.md:471-473` clause and says to **replace** its second half with "each of those processes runs one request at a time, so a route's concurrency is its process count under `BLASTIT_MAX_WORKERS`"; item 5 (`:1085-1088`) replaces **both** §8.2 rows (`:1672-1673`) — `MAX_WORKERS` "threads" → "slots (a Python thread, or a PHP worker process)" and `VIEW_MAX_WORKERS` "per-view thread cap" → "requests per view process; also sets its `RLIMIT_NPROC`; it does not bound a route". Checked against the actual lines. |
| **M3** `settings.md:69` rlimit fact / review attribution | **Resolved** | §4.10 item 6 (`:1089-1097`) keeps "also sizes its `RLIMIT_NPROC`", cites F1 for `:68` and the accepted PHP page (`php/index.md:63-64`) for "does not bound a route", and says the `:69` wording is not AE-9's. |
| **M4** epic/leaf sentences still contradicted the design | **Resolved as a recorded request** | §1 `:101-136` names the prefork wording, `MB.24.4` acceptance 4 and Notes, `MB.24.3` acceptance 5, and — at `:121-127` — `MB.24:53`'s "a per-route **count**, not one slot", with the correction ("one `SPAWN` in flight per route, answered by `Spawned { route, pid }`"), the reason (init forks sequentially; a count is what let a cold burst fork unmeasured heap copies), the `frame.rs`/`lib.rs`/`decoder.rs` `paths` amendment, and the `PHP_SUITE_ARGS`/`e2e` follow-ups. It does not claim the briefs were edited. Per the stated rule, a brief wording change or a `paths` amendment is not a defect. |
| **M5** `provider_ceiling`/`ViewSet` vocabulary | **Resolved** | the per-route container is `ViewSet` in both documents (`parallel-views.md:231-241,281`; `ADR-0015:30-33`), ADR-0010 decision 2's own word and `backend-runtimes.md:158-162`'s; the derived number is `process_limit` (`:327-336`), so `ADR-0009:20`'s `provider_ceiling`/`route_allocation` no longer collide. |
| **M-3** R3's "one more per `READY`" | **Resolved** | R3 (`:1158`) now scopes the witness: `pool.rs` installs no memory probe, so `measured_view_mem` stays `None` and §4.3's gate holds for the whole test. Verified: `SchedulerConfig::default()` has `memory_usage: None`, `mem_total_bytes: None`, `max_view_processes: None` (`config.rs:300-320`); `measure_view_memory` returns before measuring (`scheduler.rs:3308-3313`); `Options::new` sets `memory_model: None` (`common/mod.rs:619-629`) and `cfg.memory_usage` is only set when a model exists (`:703`). So `grow`'s gate `measured_view_mem.is_none() && max_view_processes.is_none() && pending_ready > 0` (`:488-490`) holds whenever a fork is pending: `SPAWN → SPAWNED → gate → READY → fork`, i.e. one more per `READY`. With a probe attached the gate is off after the first `READY` and the post-measurement path releases the slot at `SPAWNED` (`scheduler.rs:4786-4795` pumps after every `on_ctl`), covered by the memory legs. |
| **M-4** supervisor's exhaustive `Ctl` match | **Resolved** | §8.4 (`:1272`) adds `Ctl::Reload` to `on_ctl` (`supervise.rs:514-527`); the file is in `MB.24.3`'s paths. |
| **M-5** §1 omitted init's ceiling | **Resolved** | §1 `:70-73` names the fourth, privileged bound; §4.5 and §5 carry it. |
| **M-6** §4.10 item 6 attribution | **Resolved** | item 6 (`:1089-1097`) as above. |
| **M-7** `RoutePool` vs `ViewSet` | **Resolved** | renamed in design and ADR; no `RoutePool` remains. |
| **M-8** MB.24.4 snippet before `boot_bindings` | **Resolved** | §8 MB.24.4 places the loop after `boot_bindings` (`stage1.rs:356`) and before `probe_bindings` (`:370`), guarded by `#[cfg(feature = "php")]`, reading `php`, with `let mut routes` noted. |
| **T-1** two cold routes could defer to each other for ever | **Resolved** | §4.4 `other_route_starved` (`:695-707`) now requires `other < route`. The order is acyclic: the lowest-indexed cold route defers to nobody and forks (a deferring `grow` returns at the starvation check, before `make_room`/`spawning`, so it changes no state), a route with a worker or a pending `SPAWN` is not `is_empty`, and a backoff route is excluded, so the other deferrals end in the same or the next pump. Deterministic (route index), no starvation. |

## Remaining findings

None. The two MAJORs from the previous pass are verified at their current lines (§4.10 items
3/5 at `:1074-1089`; `MB.24:53` at `:121-127`), R3's oracle is now true of the probe-less
`pool.rs` path it names, and the cold-route tie-break is sound.

## Answers to the six questions

1. **Yes** — `process_limit` is boot data driven by `process_pool` (`:327-336`), so
   `VIEW_MAX_WORKERS` keeps "caps one process, not the route"; §1 `:59-74` names the three
   knobs and the privileged ceiling, and §4.7 makes the bound cost one fork.
2. **Yes** — the need bound, one fork in flight, `pending_ready` and process-counting are in
   §4.3/§4.4/§4.8; §4.10 items 1–5 cover §2.4, §2.6 and both §8.2 rows.
3. **Yes** — `MB.24.3` owns `Ctl::Reload`, its decode and the arm; the supervisor's ignore arm
   is named (`:1272`); `MB.23.5` keeps the watcher, debounce, send and optional typed reason.
   One reload path.
4. **Clean** — no re-opened decision: `workers` removed-not-deferred, one embedded PHP
   transport, no degraded mode, no new dependency, no retired manifest format; the only wire
   addition is `Ctl::Reload` `0x45`, reserved by `ADR-0014` decision 3.
5. **Yes** — §1 `:111-127` reconciles the epic's `==` prefork wording to "at most" and records
   `MB.24:53`; R13 asserts the pid set (`:1168`); the briefs are requests, not edits.
6. **Yes** — `ViewSet`, `process_limit`, `process_pool`, `pending_ready`, `ChildTable`,
   `per_process_cap`, `live_processes`, `RELOAD`, `spawn_backoff` agree between design, ADR
   and code (`config.rs:18,266,335`; `events/src/lib.rs:312-339`; `scheduler.rs:932,1148`).

## Verdict

All round-1, round-2 and verification findings are resolved. The ARCHITECTURE reconciliation
now replaces the §2.6 clause and both §8.2 rows, the epic's per-route-count sentence is
recorded as a plan request with its correction and reason, R3's witness is correctly scoped
to the probe-less `pool.rs` path (so its "one more per `READY`" is true of the code it runs),
and the `other < route` tie-break makes cold-route growth acyclic and deterministic. No
coherence defect remains.

VERDICT: PASS
