# Log MB.56.4.2

## 2026-11-20T21:07 [lead]

created (task) e2e/run_prod.py: root + rootless presets, isolation, fair queueing, drain

## 2026-11-20T21:57 [lead]

status ready -> active

## 2026-11-20T21:57 [lead]

attempt 1/3 started

## 2026-11-20T21:57 [lead]

base 2392890b3f1230f5bd71750767a4c028af2b1ea8

## 2026-11-20T22:21 [lead]

edited fields

## 2026-11-20T22:21 [lead]

REVIEW FIX 2 (ux): (B) the two legs prove the OTHER page's store: root leg must use the DO block (host/port/user/password/database/sslmode/ca_cert, no DATABASE_URL) and rootless must use postgres $DATABASE_URL naming pgbouncer; (B) docs/security.md:158-160 names deleted e2e/run.py + e2e/rootless/run.py. (M) root leg needs child_containment; tmpfs /run/blastit must be 0700/140MiB/mem_limit 1g per compose page; shim capability probe globs a stale /opt path (shim now sits by the binary); trusted X-Forwarded-Proto positive row; e2e/README.md scratch-dir/prereq contradictions; no make target runs run_prod.py.

## 2026-11-20T22:27 [lead]

status active -> done
