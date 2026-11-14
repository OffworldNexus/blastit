# Log MB.3.7

## 2026-11-14T01:09 [planner]

created (task) Prove FastCGI, the documented commands and the php2shell suite end-to-end in Docker

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:06 [lead]

node body corrected (2026-11-14) after MB.3.9.1's review: the page no longer carries php-fpm.conf/mkdir/absolute-listen and no longer uses uv init/uv add, so the e2e replay clauses naming those are replaced. INI assertions now target the GENERATED pool file, and only if the submitted mechanism keeps a pool (if MB.12 lands embed, the equivalent is that dev starts the PHP worker with the derived config and two routes get two). Still HELD pending MB.12.

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T19:06 [lead]

INPUT from MB.3.25/native probe (2026-11-14): e2e/php-sapi/seccomp-exec installs SeccompRole::View{egress} because the harness predates MB.3.25. The PHP worker now installs EmbeddedView, and EmbeddedView denies execve (R14) BY DESIGN, so no harness can install it and then exec the probe ('Operation not permitted (os error 1)' at execve — itself correct). To make the shipped e2e test the exact PHP role, the probe must accept --role embedded-view and install the filter itself at startup (single-threaded, before php_module_startup), not inherit it across exec. Today the egress proof transfers because View and EmbeddedView share the identical socket/egress branch (MB.3.25's pinned tables); the exact-role proof is native in-process: AF_INET=EPERM(1), AF_UNIX=0, clone3=ENOSYS(38), execve=EPERM(1).
