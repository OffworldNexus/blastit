# Log MB.56.4

## 2026-11-20T16:02 [conductor]

created (task) Consolidate container & multi-platform GHA E2E suites (Laravel/Symfony PHP + Postgres + static, Django/Flask, Static, root/rootless prod & packaging)

## 2026-11-20T17:38 [lead]

From MB.56.2.2: e2e/php-sapi/Dockerfile:203 still runs 'cargo test -p blastit-php-sapi --test critical_file_sink' (deleted) and Makefile:748 rs-test-php doc + publish-pypi.yml:386 PY_SAPI_PROBE=0 are stale; fix when consolidating e2e/ + workflows

## 2026-11-20T17:41 [lead]

From MB.56.2.2 review: e2e/php2shell.py (24 rows) runs ONLY via 'make e2e' - no workflow invokes it and make check never runs it; wire php2shell into the php-sapi/nightly container legs so TESTING_PLAN §2 'php2shell exploit neutrality' is actually gated. Also critical_php_embed.rs:932 the_worker_cannot_be_built_without_the_entry_pin is an include_str! source-grep oracle, not behavioural - replace with a real assertion.

## 2026-11-20T18:33 [lead]

From MB.56.2.5 review: NO workflow runs 'cargo test -p blastit-lsp' - the lsp matrix legs (ci.yml:690-757) only run scripts/lsp_smoke.py (initialize+didOpen+one diagnostic+shutdown), so didChange/hover/formatting/quickfix never execute on macOS/Windows while TESTING_PLAN.md §7 promises them across Linux/macOS/Windows. Add the e2e_lsp crate test to those legs or narrow the plan wording.

## 2026-11-20T18:33 [lead]

From MB.56.2.5 review: no workflow runs cargo test -p blastit-lsp; ci.yml:690-757 lsp legs only run scripts/lsp_smoke.py, so didChange/hover/formatting/quickfix never run on macOS/Windows although TESTING_PLAN.md §7 promises them across Linux/macOS/Windows. Add e2e_lsp to those legs or narrow the §7 wording.

## 2026-11-20T20:12 [lead]

From MB.56.3.2.3: PHP concurrency (critical_parallel_views PHP half) may be handed to run_prod.py's fair-queueing leg - confirm it proves slow-PHP-view isolation there; WSGI concurrency lives in e2e_dev via tests/common/views.rs.

## 2026-11-20T20:13 [lead]

From MB.56.3.2.3: PHP concurrency (the critical_parallel_views PHP half) is handed to run_prod.py's fair-queueing leg - prove slow-PHP-view isolation there; WSGI concurrency lives in e2e_dev via tests/common/views.rs.

## 2026-11-20T20:18 [lead]

From MB.56.3.2.3: the SPLIT store form (postgres { host $DB_HOST port $DB_PORT user $DB_USER password $DB_PASSWORD database $DB_NAME }) has no runtime leg now - blastit-manifest §7 pins its lowering, but not the env rewrite + real connection. Prove it once end-to-end in the prod E2E (compose presets already use split stores).
