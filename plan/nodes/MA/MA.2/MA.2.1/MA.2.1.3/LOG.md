# Log MA.2.1.3

## 2026-11-12T13:12 [planner]

created (task) Leaf 3: sqlpolicy literal redaction, learnable verdicts and introspection setup

## 2026-11-12T13:25 [lead]

status ready -> active

## 2026-11-12T13:25 [lead]

attempt 1/3 started

## 2026-11-12T13:25 [lead]

base 2438c36b5f00f4fa5b9531f2a812bc6b20507c8a

## 2026-11-12T13:35 [lead]

status active -> review

## 2026-11-12T13:40 [lead]

REVIEW FAIL: (1) learn.rs Unparseable carries libpg_query 'at or near "<literal>"' => user literal bytes in --learn diagnostics; strip/redact + critical test; (2) is_introspection_setup_sql bare-matches information_schema table names so SELECT * FROM tables/columns/schemata is admitted => only pg_catalog.* bare, information_schema.* qualified + negative tests; (3) drop pg_get_userbyid (reads pg_authid); (4) find_dangerous_function misses CALL funccall and >2-part names.

## 2026-11-12T13:40 [lead]

status review -> ready

## 2026-11-12T13:40 [lead]

status ready -> active

## 2026-11-12T13:40 [lead]

attempt 2/3 started

## 2026-11-12T13:46 [lead]

status active -> review

## 2026-11-12T13:48 [lead]

status review -> done
