# Log MB.21

## 2026-11-16T00:38 [planner]

created (epic) Replace blastit.toml with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)

## 2026-11-16T00:38 [planner]

Decomposed into 5 children (2 tasks + 3 epics, 19 nodes total): DX docs MB.21.1 -> design+ADR MB.21.2 -> toolchain MB.21.3.{1..6} -> cutover MB.21.4.{1..4} -> docs/proof MB.21.5.{1..4}. Deviations: epic owner omitted because plan.py's OWNERS has no 'lead' (gate stays with lead); the Blastfile ADR is ADR-0012, since ADR-0011 is already accepted (raw-ingress boot refusal, MB.3.53); MB.20.depends_on extended with MB.21.5.1 so docs/frameworks is rewritten once.

## 2026-11-16T00:40 [conductor]

Owner-settled decisions for Blastfile (mandatory for 0.2.0): (1) NO workers knob in Blastfile — scheduler owns parallelism, [route.php] workers is removed completely; (2) NO global top-level groups — trait <name> attaches only via explicit 'use <name>' inside a view; (3) NO pooler/cloud/infra knobs in Blastfile.

## 2026-11-16T01:44 [conductor]

OWNER MANDATE — TOTAL BURIAL OF blastit.toml (same rule as FastCGI): Delete 'Coming from blastit.toml?' from docs/blastfile.md and delete the entire '## Migration' section from docs/reference/blastfile.md. NO migration guide, NO key-map table, NO blastit.toml/TOML detector or migration refusal sentence in code! Blastit simply reads ./Blastfile (or BLASTIT_MANIFEST); missing file says 'missing ./Blastfile'. Delete all TOML parsing/editing code and toml/toml_edit crate deps in MB.21.4.4, purge every reference to blastit.toml across docs/, README.md, VISION.md, ARCHITECTURE.md, crates/, python/, scripts/, e2e/, Makefile and live plan briefs, and enforce a permanent repo-wide make check guard refusing 'blastit.toml' anywhere.

## 2026-11-16T01:44 [lead]

OWNER DIRECTIVE 2026-11-16: total burial of blastit.toml (same law as FastCGI MB.3.17/MB.3.30). No detector, no sniffer, no migration sentence, no toml/toml_edit deps after cutover; only ./Blastfile + BLASTIT_MANIFEST; missing file names ./Blastfile; TOML input = ordinary Blastfile line:col syntax error; permanent make-check guard against the name returning. Actions: MB.21.2 redirected, MB.21.1.1 purge task created, MB.21.4.1/4.4/5.1/5.2 and epic acceptance rewritten

## 2026-11-16T01:49 [planner]

edited fields

## 2026-11-16T01:49 [planner]

Replan under the 2026-11-16 burial directive: MB.21.1.1 created (purge the two language pages, doc-only, ready); MB.21.4.1 rewritten to discovery + ordinary syntax error (no detector, no migration sentence); MB.21.4.4 widened to crates/** and now deletes the v1 reader and the toml dependency; MB.21.5.1 rescoped to the site/README/ARCHITECTURE/live-brief sweep plus deleting the v1 reference page, inventory block and 7 inbound links; MB.21.5.2 owns the permanent test_the_real_tree_carries_no_retired_toml_manifest guard in python/tests/test_docs_assets.py (runs in make check via docs-unit). Epic title, acceptance and notes purged of the retired name; settled units 5 (body raw / body raw unvalidated) and 6 (no workers keyword, ADR-0012 12.3) added.

## 2026-11-16T06:05 [lead]

FOLLOW-UP for conductor/strategist (out of this epic's paths): crates/blastit-view's 1s frame-budget test flakes under load — observed in 2 of 4 make check runs during MB.21.3 (a flake outside every MB.21.3 leaf's paths; the sub-lead re-ran rather than touching it). Needs its own node before the MB.21 epic gate can be trusted repeatedly. Also: design §13.2 assigns six boot-warning re-spellings to MB.21.3.0 but they change sentences pinned by crates/blastit/tests/*, so they belong to MB.21.4.3's re-spelling sweep

## 2026-11-16T09:05 [lead]

INCIDENT (MB.21.4.1): implementer wedged 1002s on an unwrapped 'background: true' cargo test --test critical_static_cli; harness killed pid 3158340, EXIT=101. Root cause is a product defect, not only tooling: critical_static_cli::a_root_the_caller_cannot_list_is_refused spawns 'blastit dev' via .output() and expects a boot refusal; in the WIP cutover check_static_roots was not refusing on mounts in the dev boot path so dev booted and .output() hung. Rule enforced for every sub-lead/implementer: never background:true a cargo test without 'timeout 180', and use make (PYO3_PYTHON/LD_LIBRARY_PATH/PYTHONHOME) for python-feature targets

## 2026-11-16T09:09 [conductor]

Owner directive — make Blastfile budgets comfortable across code + reference + design: raise MAX_SOURCE_BYTES from 1 MiB to 16 MiB (16_777_216), MAX_TOKENS and MAX_TRIVIA to 4_194_304, MAX_PATTERN_BYTES from 256 to 4_096, MAX_LIST_ITEMS from 256 to 4_096, and app name length cap from 64 to 256 chars; also remove the false 'split the file' suggestion from token/trivia refusal messages since Blastfile has no multi-file include.

## 2026-11-16T11:43 [lead]

FLAKE #2 for conductor: crates/blastit/tests/critical_dev_and_fail_closed failed once under load during MB.21.4.2's gate and passed 3/3 in isolation. Same class as the blastit-view 1s frame-budget flake. Both are outside this epic's paths; neither is being papered over — each gate re-runs once and logs it. The MB.21 gate needs these two stabilised or explicitly waived by the conductor before a repeated full-tree make check can be trusted

## 2026-11-16T16:09 [lead]

HOST CONDITION for the epic gate: the machine is at ~91% MemTotal used by another project, above BLASTIT_MEM_HIGH_WATERMARK=85, so every blastit-dev-booting case refuses at boot by design (LESSONS: above-watermark refuses with 'memory: X of Y used' rather than serving 503s). MB.21.4.4's two failing cases are exactly those. The epic gate's make check must be run when the host drops below the watermark, or the two exclusions documented with the /proc/meminfo reading and the sub-lead's evidence

## 2026-11-16T17:55 [lead]

CONCURRENCY (epic gate): a sibling session (MB.22 / docs/research) is now writing inside this same worktree, so (a) every commit stages explicit node paths only and records the shared make check's suite count in the node LOG, (b) the MB.21 epic gate's make check is measured on a tree that may hold another epic's uncommitted files — the gate must record git status and note which non-MB.21 paths were present, and (c) no session may blindly stage or clean another's files

## 2026-11-17T04:53 [lead]

CONCURRENCY HAZARD confirmed: another session's uncommitted MB.22.1 edit to docs/local.md adds a 7th illustration, so docs_fences::every_documented_document_parses_and_validates fails on the shared tree and blocks any full make check here. The MB.21 epic gate must either wait for MB.22.1 to commit its docs_fences count or be measured on a tree where that node's files are committed. Do not update their count from MB.21

## 2026-11-17T05:01 [lead]

5.1-5.4 done and committed (809143d, 6bfd1cc, 6d2bd41, 06874ae). Post-0.2.0 follow-ups filed and prioritised: 5.6, 5.8-5.14. Pre-gate: 5.5 (purity gate nested-use evasion) and 5.7 (>=20-document corpus floor), both in flight with 5.11 (coverage pin). Epic gate after those three.

## 2026-11-17T05:21 [lead]

EXTERNAL BLOCKER for the epic gate: uncommitted MB.23.1 docs (docs/reference/cli.md:20,28 and docs/local.md:19,32) document 'blastit dev --no-reload' while crates/blastit/src/cli.rs has no such flag, so make docs-test fails fence 1 in docs/reference/cli.md. MB.21.5.7's reviewer proved this is not MB.21 work. The MB.21 epic gate cannot pass make check until MB.23 lands the flag (MB.23.2) or those four lines are dropped

## 2026-11-17T05:40 [planner]

Acceptance item 1 reconciled with the MB.21.5.3 audit ruling (AUDIT-MB.21.md 5.5): pre-gate children listed explicitly as done with RESULT evidence; the seven post-0.2.0 coverage nodes MB.21.5.6/5.8/5.9/5.10/5.12/5.13/5.14 excluded by id from the no-ready/active clause and carried on the release backlog. Notes bullet records the same ids and reason. No other acceptance item changed; no deferred node body touched.

## 2026-11-17T05:45 [lead]

GATE METHOD: another session's UNCOMMITTED docs/reference/cli.md + docs/local.md carry 'blastit dev --no-reload' (absent from HEAD and from cli.rs), which is the only red leg of make check here. Since never cleaning another session's files is a hard rule, the epic gate is measured on a detached worktree at HEAD (committed artefact), with the dirty-tree caveat recorded
