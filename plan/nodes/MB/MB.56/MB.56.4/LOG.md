# Log MB.56.4

## 2026-11-20T16:02 [conductor]

created (task) Consolidate container & multi-platform GHA E2E suites (Laravel/Symfony PHP + Postgres + static, Django/Flask, Static, root/rootless prod & packaging)

## 2026-11-20T17:38 [lead]

From MB.56.2.2: e2e/php-sapi/Dockerfile:203 still runs 'cargo test -p blastit-php-sapi --test critical_file_sink' (deleted) and Makefile:748 rs-test-php doc + publish-pypi.yml:386 PY_SAPI_PROBE=0 are stale; fix when consolidating e2e/ + workflows

## 2026-11-20T17:41 [lead]

From MB.56.2.2 review: e2e/php2shell.py (24 rows) runs ONLY via 'make e2e' - no workflow invokes it and make check never runs it; wire php2shell into the php-sapi/nightly container legs so TESTING_PLAN §2 'php2shell exploit neutrality' is actually gated. Also critical_php_embed.rs:932 the_worker_cannot_be_built_without_the_entry_pin is an include_str! source-grep oracle, not behavioural - replace with a real assertion.
