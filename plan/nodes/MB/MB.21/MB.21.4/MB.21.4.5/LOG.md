# Log MB.21.4.5

## 2026-11-16T02:16 [planner]

created (task) Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention

## 2026-11-16T16:07 [lead]

edited fields

## 2026-11-16T16:07 [lead]

paths widened before dispatch: crates/blastit-sqlpolicy/src/tables.rs::TableGrants::to_toml still renders the retired [route.db.tables] snippet (MB.21.4.7's follow-up), crates/blastit/src/audit_v1.rs still reads the retired plan type, and error.rs is where the re-spelled sentences live, which this sweep must leave alone.
