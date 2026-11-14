# Log MB.3.6.3.3

## 2026-11-14T10:51 [planner]

created (task) Protected critical_fastcgi_pool suite: fd table, pgid, escapee and U1 - design MB.3.6b part 3

## 2026-11-14T14:39 [lead]

SUPERSEDED by ADR-0010 (docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md, owner decision 2026-11-14): PHP executes embedded (host NTS libphp.so + per-minor C SAPI shim) inside the per-route view worker; there is no php-fpm pool. This node's machinery (parentfd argv handoff, render_pool_conf, S0-S7/U1 bind window, PoolReady, pool discovery/cleanup) is deleted by the ADR, not deferred. History kept: MB.3.9.2's grammar (entry, no listen, optional workers capped by view_worker_cap) transfers verbatim to [route.php]; the rest is not to be implemented. Do not start.

## 2026-11-14T14:39 [lead]

status ready -> superseded
