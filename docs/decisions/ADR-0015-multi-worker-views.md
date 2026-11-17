# ADR-0015: A pool of view processes per route, sized by `per_process_cap`

- **Status:** Proposed
- **Date:** 2026-11-17
- **Node:** MB.24.2
- **Owner:** designer
- **Analysis:** `docs/design/parallel-views.md` (the full design and its four reviews),
  `docs/design/parallel-views.review-dx.md` and `...review-ai-ergonomics.md` (the accepted
  DX, both `VERDICT: PASS`), `docs/how-it-works.md` and `docs/frameworks/php/index.md`
  `## Concurrency` (the sentences MB.24.1 landed), `ARCHITECTURE.md` §2.4/§8.

## Context

Four concurrent 200 ms requests to **one** `entrypoint php` route take about 800 ms while the
same four to a Python route take about 200 ms. The gap is not the runtime: distro `libphp` is
NTS, so one OS process runs one request (`crates/blastit-view/src/php.rs:10-17`). The gap is
the scheduler, which holds **at most one `View` per route** (`views: HashMap<usize, View>`,
`crates/blastit-scheduler/src/scheduler.rs:783`), so `can_dispatch` (`:2217`) and `reserve`
(`:2440`) admit one request at a time to the route whatever the runtime could do.

The identity plumbing already fits a pool: `ViewKey { route, pid }` (`:239`) keys `dispatch`,
`Retire` and `Exited`; `RLIMIT_NPROC` is already `view_worker_cap() + 16`
(`crates/blastit/src/php_view.rs:70-72`); the global `slots` budget, LRU `last_used`, idle
`idle_since`/`idle_timeout`, `max_requests` recycling, spawn backoff and the memory-derived
process cap all exist and must be reused. Users were told the truth already (MB.24.1), so the
code owes them a data model, not a new knob.

## Decision

1. **One pool per route, two concurrency facts, both boot data.** `Scheduler.pools:
   HashMap<usize, ViewSet>` holds `views: BTreeMap<u32 /*pid*/, View>`, `spawning:
   Option<(u64, Instant)>` and `pending_ready: u32`. `RouteInfo` gains `per_process_cap:
   Option<u32>` (requests one process runs at once; `None` = `BLASTIT_VIEW_MAX_WORKERS`) and
   `process_pool: bool` (may this route own more than one process). The composition root
   writes both from the admitted provider (`ADR-0009`): an NTS `libphp` route is `Some(1)` +
   `true`, a threaded WSGI route is `None` + `false`. The scheduler never asks what runtime a
   route is, and never infers the shape from a value (with `BLASTIT_VIEW_MAX_WORKERS=1` a WSGI
   route still owns one process).
2. **Selection is LRU among the workers that admit a request.** Ready, below
   `per_process_cap`, not stopping, not stale, not exited, channel open; ties by ascending
   pid. `last_used` stays the scheduler's one notion of recency, shared with eviction.
3. **One fork in flight per route, no spawn id — and the cap is measured between forks.**
   A route forks only what its requests need (`ceil((inflight + queued) / per_process_cap)`)
   and never more than one `SPAWN` is outstanding. Until the first real memory measurement
   exists, growth also waits for `pending_ready == 0`, so a cold burst is `fork → SPAWNED →
   READY → fork` and `BLASTIT_MAX_VIEW_PROCESSES` is re-derived between two forks instead of
   being priced from the estimate it is known to understate. `init` forks serially anyway, so
   several outstanding `SPAWN`s would buy no wall clock. Workers of one route are
   interchangeable and the pid is the identity, so the wire is unchanged (`0x40`/`0x41`).
4. **The memory-derived cap counts live processes,** not routes: workers plus the in-flight
   fork, across all routes. The measurement's denominator stays the `READY` workers, which
   over-estimates the per-process cost — the safe direction.
5. **No route starves another, and no route can hoard.** A route owns no worker its requests
   do not need; its process limit is `BLASTIT_MAX_WORKERS` when its provider serves one
   request per process and `1` when it threads. A route that has requests and **no process at
   all** is served before any route grows; a merely busy route never blocks a neighbour.
   Eviction only ever takes another route's idle worker. `BLASTIT_VIEW_MAX_WORKERS` therefore
   keeps exactly the meaning the accepted DX gives it: it caps one process (and sizes that
   process's `RLIMIT_NPROC`), and it is not a per-route request lever.
6. **The supervisor keeps one authority per child and its own bound.** `ChildTable` is a
   pid-keyed map of `ViewChild { route, uid }`; `Exited` is answered from that record's own
   route, `Retire` is refused for a pid that is not a live child of the named route, and
   `spawn` refuses a `SPAWN` when the route already owns
   `view_worker_cap() + VIEW_PROCESS_MARGIN` children — the bound `route_pids` used to give.
   `route_pids` and `pending_spawns` are deleted: they existed to enforce one pid per route,
   and both halves of their reason are void (each worker has its own `view.chan`; sharing the
   route's scratch directory is the pool's design).
7. **One reload, all pools, one owner for the message.** `MB.24.3` adds `Ctl::Reload` (`0x45`,
   empty payload — reserved by `ADR-0014` decision 3) and its arm; a reload retires every
   worker of every route that is idle or not yet `READY`, marks busy ones stale so nothing new
   reaches them, retires them at `DONE`, and retires on arrival any worker whose `SPAWN`
   predates the reload. `MB.24.3` retires with `RetireReason::Other("reload")` (the reason
   string MB.23.1 froze) because `crates/blastit-events` is outside its paths; MB.23.5 consumes
   the message and may substitute the typed variant. A save never leaves two versions of one
   route serving.
8. **A failed spawn costs one backoff, never a request and never the tree.** A fork that is
   refused or dies before `READY` is answered with `Ctl::Exited { pid: 0 }`; the scheduler
   frees the slot, books that route's `spawn_backoff` and leaves every sibling and queued
   request alone. A view fork is **never** `fail()`: one route's `EAGAIN`/`ENOMEM` must not
   exit the whole process tree. A wedged single-slot PHP worker is at its cap, so the route
   grows a sibling instead of wedging. The `LruCap` cool-down for a never-served worker is
   dropped (the need bound replaces its purpose, and it would freeze a *serving* route when a
   neighbour needed room); `MemoryPressure` stays.
9. **Python and PHP are unchanged where it counts.** WSGI keeps `per_process_cap =
   view_worker_cap()`, `process_pool = false` and one process per route; NTS PHP keeps
   `max_inflight() == 1`, `per_process_cap = 1` and one request per process. No second PHP
   transport, no degraded mode, no new dependency: `Cargo.toml`/`Cargo.lock` stay
   byte-identical.

**Measured promise.** Four concurrent 200 ms requests to one **warm** PHP route move from the
serialized floor (≥ 800 ms) to `< 450 ms` wall, asserted by the protected
`critical_parallel_views` suite. `serialized_then_parallel_php` measures its own PHP
`BLASTIT_MAX_WORKERS=1` control in the same function (≥ 800 ms) and asserts
`parallel <= control - 400 ms`, so neither a fast host nor a deleted pool can pass. "Warm" is
part of the claim, not a footnote: a `libphp` worker `dlopen`s the library and runs
`php_module_startup` after the fork (`ADR-0010` decision 2) and `prefork` warms only one
worker, so the fixture declares `prefork` **and** the suite sends a four-wide warm-up burst
before the clock (asserting four `200`s, four markers and four distinct pids), records that
burst's duration in `MB.24.4/RESULT.md`, and then asserts the measured window uses the same
four pids with no new `process.spawned`. A cold route's first requests pay the startup — that
is pre-existing behaviour, and `prefork` remains its lever.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| Thread one NTS `libphp` process | Zend `EG`/`CG`/`SG` are process globals; two threads corrupt them and segfault (`ADR-0010`). |
| A per-route `workers` directive | Removed, not deferred (`ADR-0012` §7): duplicate of the scheduler's budget, downward-only, unverifiable by `check`. |
| Pre-fork a fleet at boot | Reverses the lazy default and the memory promise; `prefork` stays one warm worker. |
| Several `SPAWN`s per pump ("parallel forks") | `init` forks sequentially, so it buys no wall clock; before the first measurement it would fork heap copies the cap never priced; and request 1 would be dispatched later, not earlier. |
| Derive the process limit from `BLASTIT_VIEW_MAX_WORKERS` | Turns that knob into a per-route lever, which the accepted DX denies exists. |
| Infer the provider shape from `per_process_cap > 1` | `BLASTIT_VIEW_MAX_WORKERS=1` is legal and would turn a threaded route into a Python heap pool; the shape is boot data. |
| A separate pooler role | A new uid, socket, reaper and threat model; init is the sole reaper and the scheduler the sole admission authority (`ADR-0009`). |
| A spawn id on the wire | Fixed-width payload change for state nothing reads; the pid is the identity and a stray `SPAWNED` is already retired. |
| Keep a failed view fork fatal (`fail()`) | One route's resource exhaustion would take every other route's availability — the availability half of `VISION.md`'s promise. |
| A static per-route share of the budget | Wastes a quiet app's budget that a busy route could use — exactly what WSGI threads do today. First-process-first growth is safer. |
| Raise `RLIMIT_NPROC` to `max_workers + margin` | Unnecessary for the shipped default; the limit is a per-uid backstop that costs one spawn's backoff. |
| A second PHP transport | Refused by `ADR-0010`; one embedded NTS path, no fallback. |

## Consequences

- The accepted pages become true verbatim, with no new flag, key or step: one PHP route runs
  concurrent requests in separate processes under the shared budget, and
  `BLASTIT_VIEW_MAX_WORKERS` still sizes one process rather than the route.
- Three bounds tune concurrency, all pre-existing and none a route lever: the budget, the
  memory-derived live-process cap, and the uid's `RLIMIT_NPROC` — the last one inherited from
  the deployment, which binds only when `VIEW_MAX_WORKERS` is set far below `MAX_WORKERS` and
  then costs one fork, never a request.
- A route's memory cost becomes load-dependent up to its limit, which is why the cap counts
  processes, why growth is charged one fork at a time, and why extras retire: a burst is a
  burst, not a new floor.
- `RLIMIT_NPROC` is per **uid**: routes that share a sandbox uid share the limit. Raising it
  is still refused.
- `idle_timeout` retires preforked workers too (unchanged), so a pool returns to **at most**
  the prefork count; warmth after a quiet spell is a restart, as today. A prefork floor would
  be a separate decision and would amend `ARCHITECTURE.md` §2.4.
- `ARCHITECTURE.md` §2.4/§2.6/§8/§8.2, `docs/reference/settings.md:68-69` and the `php.rs`
  module comment need the wording updates listed in design §4.10; they are follow-ups,
  recorded in `RESULT.md`, not changes smuggled into this epic's leaves.
- Implementation is two leaves: `MB.24.3` (the pool, `Ctl::Reload` `0x45`, `(route, pid)`
  supervision) and `MB.24.4` (the PHP shape in `stage1.rs` plus the protected wall-clock
  suite).
