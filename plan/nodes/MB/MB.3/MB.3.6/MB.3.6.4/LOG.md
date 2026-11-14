# Log MB.3.6.4

## 2026-11-14T03:37 [planner]

created (task) check::serve_and_run, the 3-way check branch order and PHP learn attribution - design MB.3.6c

## 2026-11-14T10:50 [planner]

edited fields

## 2026-11-14T13:22 [lead]

BINDING OWNER CONSTRAINTS (2026-11-14) for check/learn and the observation path -- these are design-level, record now and land later.

(1) The --learn/check stream must carry the SAME cost fields the fair queue charges (cost_ns charged, cpu_ns reported-or-null) and must never invent a per-request CPU number for a FastCGI route. For a route served by an external pool the honest value is the observed wall time with cpu_ns: null (see MB.3.6.3's measurement note); a learn/check document that claims a CPU figure php-fpm never provided is a lie in a security-adjacent artifact and is refused in review.

(2) Parallelism is the scheduler's: learn/check must not infer a route's concurrency from the pool's pm.max_children, nor report 'workers' as if it were the limiter. If a document needs a concurrency value, it is the scheduler's per-route slot allocation.

(3) The 3-way check branch order (all-static -> external-executor app-less -> pytest) and the external-executor path stay as accepted: 'check' must not spawn a pool inside the child dev it will kill. But a check run MUST still prove the runtime resolves and starts once (same resolver as dev/prod, MB.3.9.3), so a broken php-fpm install fails check at boot with the package+override message rather than as a per-request 503.

(4) A crashed flow in check/learn is charged at least its wall time and releases its slot exactly once (SessionDoneGuard Done(Fault)); the document must not show a route exercised by a request whose cost never settled.

## 2026-11-14T13:23 [lead]

SUPERSEDED (2026-11-14, owner): my earlier note (1) said a FastCGI route's honest value is observed wall time with cpu_ns:null. That is now the FALLBACK ONLY, not the design. The design is per-request CPU measured inside the PHP worker (auto_prepend/bootstrap in the generated pool conf, RUSAGE_SELF delta, record (req_id, cpu_ns) over a per-pool socket, verified by SO_PEERCRED + pool pgid membership). Consequences for check/learn that this epic must implement: (a) a run that cannot instrument reports cpu_ns:null AND warns ONCE per run that precise fair queuing is degraded -- never silently pretends to measure (MB.3.6.4.3); (b) the --json/pretty documents must never print a CPU figure for a request that had no verified report; (c) the degradation warning is a warning, not a failure: the suite still exercises the route. Notes (2), (3) and (4) of my earlier note stand.

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.
