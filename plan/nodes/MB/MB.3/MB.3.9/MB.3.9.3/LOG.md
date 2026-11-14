# Log MB.3.9.3

## 2026-11-14T13:18 [lead]

created (task) Amend design 4.8.1 + ADR-0008: dev spawns the pool via the runtime provider (discovery, cleanup, error contract)

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision. This node was written for a spawned php-fpm pool (generated conf, provider spawn, discovery via BLASTIT_PHP_FPM). In the libphp/embed model most of it evaporates: the request runs in blastit's own worker process, so there is no pool to spawn, no discovery, and per-request CPU is an exact clock_gettime(CLOCK_PROCESS_CPUTIME_ID) around the request instead of RUSAGE_SELF deltas over a socket. Do not start it; re-read the node if embed wins (it will shrink to 'dev starts the PHP worker' + no conf).

## 2026-11-14T14:39 [lead]

SUPERSEDED by ADR-0010 (docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md, owner decision 2026-11-14): PHP executes embedded (host NTS libphp.so + per-minor C SAPI shim) inside the per-route view worker; there is no php-fpm pool. This node's machinery (parentfd argv handoff, render_pool_conf, S0-S7/U1 bind window, PoolReady, pool discovery/cleanup) is deleted by the ADR, not deferred. History kept: MB.3.9.2's grammar (entry, no listen, optional workers capped by view_worker_cap) transfers verbatim to [route.php]; the rest is not to be implemented. Do not start.

## 2026-11-14T14:39 [lead]

status ready -> superseded
