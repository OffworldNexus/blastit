# Log MB.3.9.3

## 2026-11-14T13:18 [lead]

created (task) Amend design 4.8.1 + ADR-0008: dev spawns the pool via the runtime provider (discovery, cleanup, error contract)

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision. This node was written for a spawned php-fpm pool (generated conf, provider spawn, discovery via BLASTIT_PHP_FPM). In the libphp/embed model most of it evaporates: the request runs in blastit's own worker process, so there is no pool to spawn, no discovery, and per-request CPU is an exact clock_gettime(CLOCK_PROCESS_CPUTIME_ID) around the request instead of RUSAGE_SELF deltas over a socket. Do not start it; re-read the node if embed wins (it will shrink to 'dev starts the PHP worker' + no conf).
