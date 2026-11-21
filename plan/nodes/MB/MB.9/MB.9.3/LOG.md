# Log MB.9.3

## 2026-11-14T01:54 [planner]

created (task) Port the WSGI forwarding path onto the injected Upstream trait

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-17T14:07 [planner]

status ready -> superseded: Superseded by MB.36: the view crate boundary is the view.chan protocol, not an Upstream/WorkerRuntime trait (ADR-0016)
