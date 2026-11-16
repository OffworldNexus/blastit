# Log MB.21.3

## 2026-11-16T00:38 [planner]

created (epic) Blastfile toolchain: bounded lexer, LL(1) parser, path compiler, fmt, surgical editor, docs_fences

## 2026-11-16T02:41 [lead]

Re-ordered the chain from design-order to build-order: 3.3 (pattern.rs + HEAD normalisation) now runs before 3.8, because lowering's step 4 compiles each view header's patterns (design §7.3), so a lowering without pattern.rs cannot produce a correct Route.path and the parity oracle cannot be green. 3.8 keeps designs 3.2b; the two pattern integration suites move to 3.8 (an integration test may only use the public entry point, which 3.8 lands — the rule 3.2 already follows for critical_blastfile_parser.rs).

## 2026-11-16T02:41 [lead]

Also recorded: StaticPolicy's fallback field + spa removal moved from 3.0 to 3.7, because every StaticPolicy literal in the workspace lives in 3.7's file list (scheduler route.rs, static_fixture.rs, critical_rewrite.rs, the two manifest suites, critical_manifest_edit_executor.rs) and adding the field in 3.0 would leave the workspace red outside its paths. 3.0 stays additive; 3.7 (design 3.0b) lands fallback, drops spa, and moves the literals. 3.7 gained model.rs/parse.rs/validate.rs/static_fixture.rs for that. 3.1 gained the Makefile line for the check_manifest_purity.py gate (§20 R20/R24) and 3.5's critical_learn_executor_blocks.rs path was corrected to crates/blastit/tests/.

## 2026-11-16T03:09 [lead]

Design gap resolved without a human: §4.3's Consumers column assigns the per-view accessor removal, the StaticOutcome rename and §9.5/§9.6 to MB.21.4.2, and 3.7 cannot land them while blastit.toml still drives the product (four protected contracts read the route->mount link). 3.7 re-scoped; MB.21.4.2's paths need widening (model.rs, parse.rs, edit.rs, scheduler route/rewrite/static_asset/static_response + their suites, blastit src, tests/parse.rs). H-010 open for the conductor to close.

## 2026-11-16T08:01 [lead]

Gate: all 9 leaves done; epic acceptance 1-5 verified (tree, make check exit 0 with 161 ok/0 FAILED, cargo test -p blastit-manifest 32 suites/527 tests, blastfile/ + 10 critical_blastfile suites present, lib.rs registers pub mod blastfile, empty Cargo/Lock diff, every changed pre-existing suite accounted for by an authorizing leaf). A test-curator audit is the parent's call per the brief; gaps measured here are listed in RESULT.md # Follow-ups (module coverage is complete, but the purity gate's substring needle and the P4 arms are worth a mutation pass).

## 2026-11-16T08:01 [lead]

status ready -> done
