# Log MB.56.4.1

## 2026-11-20T21:07 [lead]

created (task) e2e/apps + run_apps.py: PHP/PostgreSQL/static, Django & Flask, interpreter-free Static

## 2026-11-20T21:24 [lead]

status ready -> active

## 2026-11-20T21:24 [lead]

attempt 1/3 started

## 2026-11-20T21:24 [lead]

base dbc80515940bca2602156779ecf2685a4ee9d688

## 2026-11-20T21:49 [lead]

edited fields

## 2026-11-20T21:49 [lead]

REVIEW FIX: (B) e2e/live.py + e2e/rootless/run.py still exec deleted scenario.py/check_logs.py -> delete both with an explicit handoff to MB.56.4.2's run_prod.py and repoint Makefile/.github; (B) no ungranted INSERT leg -> add peek/catalog INSERT attempts (customers + orders.note) in PHP/Django/Flask asserting 403/42501; (M) assert_denied is vacuous (swallows OSError, accepts >=500) -> require 403 AND 42501, transport error = failure; (M) static leg: real .git/.php/secret fixtures + %00/backslash/%2e%2e/.git/ probes + documented 400-vs-404; (M) e2e/run.py deletion breaks ci.yml:557 + Makefile:501 and orphans docs_deploy.py -> repoint + re-home; (m) docker rmi app images; assert denial bodies credential-free. NOTE: Range/206 is H-017 (open human item) - do not assert.

## 2026-11-20T21:49 [lead]

REVIEW FIX 2 (ux fixtures): Flask trait needs psycopg's hstore OID query (+ prose: use_native_hstore=False alternative); Django static mount must follow django.md (STATIC_ROOT=staticfiles/STATIC_URL/INSTALLED_APPS + collectstatic) ; add manage.py; use dj_database_url two lines with conn_max_age=0; Flask create_engine(..., poolclass=NullPool); Flask static root 'static'; add Blastfile.splitstore leg (host/port/user/password/database) asserting 200 + db_password_in_env=false; e2e/README.md scratch path ${TMPDIR:-/tmp/opencode}/blastit-e2e-apps.

## 2026-11-20T21:57 [lead]

status active -> done
