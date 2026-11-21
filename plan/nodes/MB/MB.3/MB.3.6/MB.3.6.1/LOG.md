# Log MB.3.6.1

## 2026-11-14T03:37 [planner]

created (task) Pool ctl frames, workspace deps/lock and scheduler AF_UNIX seccomp - design MB.3.6a1

## 2026-11-14T10:50 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T15:14 [lead]

SUPERSEDED by owner decision 2026-11-14 ('No fallback please'): there is NO external FastCGI path. [route.fastcgi] stops existing as a key (unknown-key refusal, not a default-off switch); the BLASTIT_ALLOW_EXTERNAL_FASTCGI switch and every doc sentence sanctioning the fallback are deleted. Removal is MB.3.17 (depends MB.3.12). Kept: crates/blastit-php-sapi, PhpEmbedRuntime, view.chan framing, the Upstream/WorkerRuntime contract and MB.9.3 -- this removes one backend implementation, not the abstraction.

## 2026-11-14T15:14 [lead]

status ready -> superseded
