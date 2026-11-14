# Log MB.3.5

## 2026-11-14T01:09 [planner]

created (task) Parse and validate [route.fastcgi] blocks in blastit-manifest

## 2026-11-14T10:50 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T13:13 [lead]

design-of-record is ADR-0008 + php-fastcgi.md 4.1 as accepted. Do NOT invent a key or a rule: three keys only (entry, listen, workers=4), no [route.fastcgi.env]. If the provider model (MB.12) later contradicts a rule implemented here, surface it to the lead; the manifest grammar is expected to survive as the per-runtime declaration surface, with the provider contract sitting behind Executor::kind().

## 2026-11-14T13:19 [lead]

GRAMMAR CHANGED 2026-11-14 (owner): [route.fastcgi] is now { entry, workers? } -- 'listen' is REMOVED (blastit derives <run>/<route>/fpm.sock inside a 0700 dir) and 'workers' is optional/derived from Config::view_worker_cap(). MB.3.9.2 is amending ADR-0008 4.1 + design 4.1/8.1/8.2; MB.3.5.1 is reworked to the new grammar. This epic's Acceptance/Context still describes the 3-key shape and MUST be re-synced to MB.3.9.2 before MB.3.5.2 is dispatched.

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T22:18 [planner]

edited fields
