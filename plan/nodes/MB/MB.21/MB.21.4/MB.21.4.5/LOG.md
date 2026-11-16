# Log MB.21.4.5

## 2026-11-16T02:16 [planner]

created (task) Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention

## 2026-11-16T16:07 [lead]

edited fields

## 2026-11-16T16:07 [lead]

paths widened before dispatch: crates/blastit-sqlpolicy/src/tables.rs::TableGrants::to_toml still renders the retired [route.db.tables] snippet (MB.21.4.7's follow-up), crates/blastit/src/audit_v1.rs still reads the retired plan type, and error.rs is where the re-spelled sentences live, which this sweep must leave alone.

## 2026-11-16T16:53 [lead]

edited fields

## 2026-11-16T16:53 [lead]

split: the leaf's planned remainder (src/{model,lib,edit,routing,validate} + tests/{parse,common/mod,mutation_gaps} + blastit-env) under-counted the v1 corpus still in the tree after 4.1/4.2. Measured at dispatch: ~150 'version = 1' fixture sites and 78 from_toml_str calls across 18 files -- crates/blastit-manifest/tests/{parse 1112 lines, critical_manifest_011 747, validate 781, critical_static_manifest 752, mutation_gaps 714, critical_php_manifest 445, grants 261, routing 259, uid 127, common/mod 172}, src/{model 7,edit 4,routing 2,validate 1,lib 1}, plus crates/blastit-env (6), crates/blastit-scheduler (src/lib, src/route, tests/{common/mod,critical_rewrite,critical_ingress_enforcement}), crates/blastit/src/{stage1,infer_routes}, crates/blastit/tests/critical_check_engine.rs. Far beyond one leaf and outside its listed paths; decomposed into children with disjoint paths, exactly one owning Cargo.toml/Cargo.lock.

## 2026-11-16T16:58 [planner]

edited fields
