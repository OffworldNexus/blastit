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
