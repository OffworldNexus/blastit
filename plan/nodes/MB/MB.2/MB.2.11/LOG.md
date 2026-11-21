# Log MB.2.11

## 2026-11-13T22:17 [conductor]

created (task) Default the deploy presets to a connection pooler (DO managed pooler, Compose PgBouncer)

## 2026-11-13T22:17 [lead]

status ready -> active

## 2026-11-13T22:17 [lead]

attempt 1/3 started

## 2026-11-13T22:17 [lead]

base d37f765

## 2026-11-13T22:20 [doc-writer]

research: docs/research/connection-pooling.md in progress; decided trap 3 from code: dbproxy refuses a client 'options' startup param (ALLOWED_STARTUP_PARAMETERS) and blastit-env's ALLOWED_PARAMETERS refuses 'options' in DATABASE_URL, so the zone must be a role/database default; Django 5.2 already sets prepare_threshold=None.

## 2026-11-13T22:26 [doc-writer]

wrote docs/research/connection-pooling.md (researcher), both deploy pages (DO 150 lines, compose 240), index pooler rule, manifest/settings reference, e2e/docs_deploy.py (23/23) wired into e2e/run.py; make docs-unit/docs-check/docs-test green (47 fences, 0 failed); critics running.

## 2026-11-13T22:36 [doc-writer]

dx + ai-ergonomics pass 1: both FAIL; resolved all BLOCKERs/MAJORs (ALLOWED_HOST, platform-managed pool user, literal direct migrate URL, grant-stays-in-CI rule, probe must print UTC, DB_NAME, pool name, sizing, TLS wording, .env keys); pages frozen, second-pass critics running. make docs-test wheel build currently broken by another node's blastit-scheduler edits; harness green 47/0 against the built wheel.

## 2026-11-13T22:48 [doc-writer]

owner ruling applied: allowlist untouched; role default ALTER ROLE app SET timezone='UTC' in DO role SQL, Compose db/grants.sql and index; grant rule derived from test_check._django_manifest + audit_v1.dead_group_grants (dead where sessions say UTC, needed where Etc/UTC). Gates: docs-unit 63/57, docs-check clean, docs-test 47 fences 0 failed, e2e/docs_deploy 37/37. Final critics running on frozen pages.

## 2026-11-13T23:02 [doc-writer]

done: owner ruling applied (role default, allowlist untouched), docs-unit 63/57, docs-check clean, docs-test 47 fences 0 failed, e2e/docs_deploy 38/38; dx + ai-ergonomics final VERDICT: PASS (0 BLOCKER/0 MAJOR each). make e2e must be run by the lead (e2e/run.py + e2e/docs_deploy.py changed).

## 2026-11-13T23:06 [lead]

status active -> review

## 2026-11-13T23:16 [doc-writer]

attempt 2 review fixes: research note aligned (literal migrator URL, role default, no options), compose prose moved to settings.md (255 lines; full compose block is the recipe), DO pool-mode citation + app.yaml git warning, ai-ergonomics artefact rewritten with per-item evidence. docs-unit 63/57, docs-check clean, docs-test 47/0, docs_deploy 38/38; both artefacts VERDICT: PASS.

## 2026-11-13T23:30 [doc-writer]

attempt 2 final: direct-port zone read with its own APP_URL (pool URI path is the pool name), pool recycle/restart note, research 'two destinations' + /shop, compose [proxy.db] link, ca_cert verify-full, throwaway key listed. docs-unit 63/57, docs-check clean, docs-test 47/0 exit 0, docs_deploy 39/39; dx + ai-ergonomics VERDICT: PASS.

## 2026-11-13T23:36 [doc-writer]

final: self-contained zone fence (OWNER_URL/APP_URL exports), oracle + export-order pinned, inventory reason corrected; docs-unit 63/57, docs-check clean, docs-test 47/0 exit 0, docs_deploy 40/40; both artefacts VERDICT: PASS.

## 2026-11-13T23:40 [doc-writer]

final: gate pins APP_URL port/db + export order + owner-oracle rejection (41/41); research note qualified to new server sessions and cites server_lifetime; both artefacts VERDICT: PASS; docs-unit 63/57, docs-check clean, docs-test 47/0 exit 0.

## 2026-11-13T23:44 [lead]

REVIEW PASS attempt 2 (3 MINOR: one stale 'options is the supported way' phrasing in the research note; ai-ergonomics line refs drifted; single app.yaml git warning). GATE: make check exit 0 at fafae8a with these docs in the tree; make e2e exit 0 (364 ok, 31s) at this state.

## 2026-11-13T23:44 [lead]

status review -> done
