# Log MB.3.6.3

## 2026-11-14T03:37 [planner]

created (task) stage0 no-app boot, parentfd freeze and supervise::spawn_pool single *at sequence + PoolReady - design MB.3.6b

## 2026-11-14T10:50 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T13:22 [lead]

BINDING OWNER CONSTRAINTS (2026-11-14, design-level -- record now, land later).

(1) THE SCHEDULER LIMITS PARALLELISM; THE POOL DOES NOT. php-fpm's pm.max_children and the child's RLIMIT_NPROC are CEILINGS, never the limiter. They must be >= the scheduler's per-route slot allocation (MB.3.9.2's derived value = Config::view_worker_cap(), config.rs:1407; explicit 'workers' is an override of that ceiling). If a provider under-provisions (explicit workers < the route's slot allocation), the answer is a BOOT-TIME REFUSAL naming both numbers ('fastcgi workers 2: below the route slot allocation 16'), never a request that stalls on a full pool. pm.max_requests stays leak hygiene only -- never a fairness/throttle knob. Admission and dispatch live in the scheduler; the pool never decides.

(2) CPU TIME ON THE EXISTING COMPLETION PATH. Reuse the scheduler's existing fair-queuing contract, do not invent a parallel one: fair.rs::Cost + the RESP_END_COST clamp (ARCHITECTURE.md ~305-320 and ~1497-1510): charge = clamp(reported, dispatch->first response frame, dispatch->end of response); no report => charge the observed time; the request event carries cost_ns (charged) and cpu_ns (reported, null when nothing was reported). Nothing in the pool may self-throttle.

(3) MEASUREMENT REALITY -- SAY IT HERE, NOT AT IMPLEMENTATION TIME. php-fpm gives no per-request CPU report, so the WSGI 'view reports its own number' shape cannot be reproduced. cgroup cpu.stat (usage_usec) is monotonic and PER-CGROUP, shared by every worker in that pool: a per-request delta requires sampling at admit and at complete AND a defined pro-rata split, and that split is biased because in-flight count changes inside the interval. Worse: there is NO cgroup for the pool in 'blastit dev'/'check' (single-uid, no cgroup setup) and the pool is a process group in production per ADR-0008 decision 6, not necessarily a cgroup of its own. So the defensible default is: CHARGE OBSERVED WALL TIME and report cpu_ns: null -- exactly the existing no-report case, no invented number. Any cpu.stat refinement must be production-only, must name its sample points (admit/complete) and the overlap split (two requests overlapping -> state the exact division), and must be clamped by the same wall-time bound so a bogus value cannot buy credit. Do not add a new credit path.

(4) FAILURE TO SETTLE. A request that dies without reporting is still charged at least its wall time (a crashed flow must not gain free budget) and releases its slot EXACTLY ONCE via the existing SessionDoneGuard Done(Fault) synthesis (ADR-0008 decision 1) -- no double release, no stranded slot.

(5) PROVIDER OWNERSHIP. MB.12's RuntimeProvider contract owns the GENERIC measurement interface (the thing that yields a per-request cost from a runtime blastit does not own) and the generic resolver; FastCGI is the first implementation, not a special case. If the provider model cannot express 'wall-time default + optional cgroup refinement', surface it to the lead rather than forking the scheduler.

## 2026-11-14T13:23 [lead]

SUPERSEDED (2026-11-14, owner): rule (3) of my earlier note -- 'charge observed wall time, cpu_ns: null, optional production-only cgroup cpu.stat refinement' -- is WITHDRAWN as unacceptable. 'the mechanism you are offering for FastCGI is totally unacceptable, you need to instrument PHP deeper than that. Also the target is NOT FastCGI, it is PHP.' The replacement is per-request CPU measured INSIDE the PHP worker by blastit-installed instrumentation: blastit generates the pool config, so it sets auto_prepend_file/auto_append_file to a blastit-shipped bootstrap in the run dir; RUSAGE_SELF delta from request start to shutdown; one fixed record (req_id, cpu_ns) over a per-pool reporting socket in the 0700 run dir; scheduler validates SO_PEERCRED + peer pid in the pool pgid (reuse S4/S6), drops and counts unknown/expired/duplicate ids and foreign peers; wall-time clamp and per-flow monotonicity still bound the damage. Rules (1), (2) and (4) of my earlier note STAND unchanged (scheduler is the only limiter; the report travels on the existing completion/Cost path; a crashed flow is charged >= wall time and releases exactly once). Design is MB.3.9.6; implementation MB.3.6.5/.6/.7. The old wall-time default survives ONLY as the stated fallback when instrumentation is unavailable (stock image, auto_prepend_file disabled), and that fallback must be visible (cpu_ns: null + a check warning), never silent.

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:39 [lead]

SUPERSEDED by ADR-0010 (docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md, owner decision 2026-11-14): PHP executes embedded (host NTS libphp.so + per-minor C SAPI shim) inside the per-route view worker; there is no php-fpm pool. This node's machinery (parentfd argv handoff, render_pool_conf, S0-S7/U1 bind window, PoolReady, pool discovery/cleanup) is deleted by the ADR, not deferred. History kept: MB.3.9.2's grammar (entry, no listen, optional workers capped by view_worker_cap) transfers verbatim to [route.php]; the rest is not to be implemented. Do not start.

## 2026-11-14T14:39 [lead]

status ready -> superseded
