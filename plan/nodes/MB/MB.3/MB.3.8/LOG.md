# Log MB.3.8

## 2026-11-14T01:09 [planner]

created (task) Release prep 0.2.1: version bump, dated changelog, ARCHITECTURE sync

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.
