# Log MB.3.7

## 2026-11-14T01:09 [planner]

created (task) Prove FastCGI, the documented commands and the php2shell suite end-to-end in Docker

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T13:24 [lead]

HELD pending MB.12's embed-vs-pool decision (owner 2026-11-14: 'use the goddamn libphp and bypass php-fpm if you need to'). Do not implement, do not fail, do not delete work: the node is frozen as-designed against the FastCGI/php-fpm model and will be re-read, amended or superseded once the architect decides. If a sub-item reads as 'needed regardless' (e.g. the pure codec crate), it is called out in the MB.3 inventory log.

## 2026-11-14T14:06 [lead]

node body corrected (2026-11-14) after MB.3.9.1's review: the page no longer carries php-fpm.conf/mkdir/absolute-listen and no longer uses uv init/uv add, so the e2e replay clauses naming those are replaced. INI assertions now target the GENERATED pool file, and only if the submitted mechanism keeps a pool (if MB.12 lands embed, the equivalent is that dev starts the PHP worker with the derived config and two routes get two). Still HELD pending MB.12.
