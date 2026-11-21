# Log MB.21.4.10

## 2026-11-16T18:58 [planner]

created (task) Delete the last retired spellings from user-facing sentences and give the report's table scope a `blastfile` key

## 2026-11-16T18:58 [planner]

edited fields

## 2026-11-16T18:58 [planner]

created: the last user-facing retired spellings (validate.rs StaticDeadDeclaration + spa/prefork strings, the audit_v1 --learn hint, check.rs's two pytest-django hints, stage0's dead php_worker_budget sentence, host.rs's claim label, blastit-view php.rs's absolute-path refusal) and the report's table-scope value as Blastfile directives under table_grants.blastfile

## 2026-11-16T18:58 [planner]

amended per owner (2026-11-16): the report key is renamed table_grants.toml -> table_grants.blastfile, not kept - owner rule: zero .toml field names in CLI/JSON output; the leaf also records the rule in ADR-0012 (superseding that single field name in ADR-0003). Fact-check note: the lead's 'rename may be a no-op' measurement does not hold on this branch - measured at 7ba6d23 and again with 4.5.6's tree: crates/blastit-py/src/audit.rs:366 and :1552 still insert "toml" with to_toml(...) (11 hits, plus test_fold.py stubs and test_pytest_plugin.py:1115), while audit_v1.rs is clean and python/tests/critical/test_check_contract.py binds only pyproject.toml, so the protected contract suite is untouched and takes no covering glob. python/blastit/pytest_plugin.py (its :285-292 docstring publishes the key) and python/tests/{test_fold,test_pytest_plugin}.py are in paths for that reason alone.

## 2026-11-16T19:00 [planner]

edited fields

## 2026-11-16T19:00 [planner]

amendment 2 (measured surface folded in): the renderer is substantive, not a verification - tables.rs:504 to_toml(&self, prefix) renders [route.db.tables] + x = { select = [...] } and becomes to_blastfile(&self, locator) emitting select x(id, name) / select x / insert / update / delete x / functions a, b, with '# <locator>' (view <name> postgres[ store] | trait <name>) as one leading comment line; empty grant -> "". Report key "toml" -> "blastfile" at audit.rs:366 and :1552 with locators built from route.name(), the doc examples :110/:326 and the seven assertions :2071..:3494; fold.py:564 already passes 'postgres[ store]' so it becomes 'view <route> postgres...'; pytest_plugin.py:292 docstring. Acceptance now greps to_toml|"toml"|.toml" over crates/blastit-py + crates/blastit-sqlpolicy + python/blastit + the two Python suites, plus timeout 180 cargo test -p blastit-sqlpolicy --locked, make check, make test-critical, make docs-check.

## 2026-11-16T19:01 [planner]

amendment 2, corrections with measurement: (a) crates/blastit-sqlpolicy/tests/critical_policy_surface.rs:740,761 is a PROTECTED caller of to_toml and must be re-spelled (update = ["id"] to update t(id)); it is authorized by its own explicit paths entry and RESULT.md quotes the diff with every structural claim kept. (b) python/tests/runnable_docs/harness.py:3555 is not a field - it is the BLOCK_CHECKS fence-language validator map (yaml/toml/sql), which docs/{ci,getting-started,index,local}.md still need, and blastfile can never be a fence language (ADR-0012 12); renaming it would silently stop every TOML fence being parsed, so harness.py stays out of paths and out of the new grep, recorded in the body. (c) docs/reference/blastfile.md never names table_grants - it documents --json findings only - so there is no documented example to keep in step; the undocumented report field is a follow-up for MB.21.5.1. Build artefacts (_blastit*.so, __pycache__) match a bare grep: acceptance uses grep -I and forbids touching them.

## 2026-11-16T20:35 [lead]

status ready -> active

## 2026-11-16T20:35 [lead]

attempt 1/4 started

## 2026-11-16T20:35 [lead]

base fa7f2ef

## 2026-11-16T20:48 [lead]

attempt 1 landed the whole edit set on disk (13 Rust/Python/doc files, greps clean, blastit-sqlpolicy+blastit-py build) but the session ended with an empty report and no RESULT.md; attempt 2 finishes the evidence and the gate.

## 2026-11-16T20:48 [lead]

attempt 2/4 started

## 2026-11-16T21:10 [lead]

status active -> review

## 2026-11-16T21:15 [lead]

REVIEW FAIL: [MAJOR] to_blastfile interpolates table/column names raw, so a non-Word name renders a fragment the reader refuses (select audit.users(id) -> fmt rc=2) or extra directives (a newline name adds 'select secret(id)'), contradicting its own rustdoc and the test that pins the dotted form; need the escaping/advice rule the deleted string_toml gave, mirrored in _table_snippet, with both shapes in grant_fragment.rs. [MAJOR] audit_v1.rs:1258/1264 still tell the developer to write body = "json"/body = "raw" + allow_raw, pinned by critical_check_engine.rs:890-891. [MINOR] RESULT names the wrong twin file; stage0::php_boot keeps a dead _cap; the only _table_snippet empty-list test sits behind the PostgreSQL skipif.

## 2026-11-16T21:15 [lead]

status review -> ready

## 2026-11-16T21:15 [lead]

attempt 3/4 started

## 2026-11-16T21:30 [lead]

status ready -> review

## 2026-11-16T21:41 [lead]

reviewer PASS (round 2). Two MINORs recorded: (1) the DB-free _table_snippet tests run in CI's pytest job (.github/workflows/ci.yml:589), not in a make target — a Makefile change would need its own leaf; (2) grant_locator's named-store header is executed but never asserted (a mutant dropping {store} would survive) — handed to the MB.21.5.3 mutation audit. My gate: validate OK, guard OK, make check exit 0, 166 suites.

## 2026-11-16T21:41 [lead]

status review -> done

## 2026-11-16T21:41 [lead]

commit 335f564
