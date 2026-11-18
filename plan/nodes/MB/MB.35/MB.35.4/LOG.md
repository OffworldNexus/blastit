# Log MB.35.4

## 2026-11-17T13:17 [planner]

created (task) Extract crates/blastit-lsp (bin blastit-lsp + thin blastit lsp delegation) with the protected critical_lsp suite unchanged

## 2026-11-18T06:31 [lead]

edited fields

## 2026-11-18T06:31 [lead]

paths added by lead from the accepted design: crates/blastit-manifest/src/lib.rs (re-export parameterize) and crates/blastit/tests/critical_lsp.rs (4 include_str! literals move with the crate — so "byte-identical" is replaced by "assertions untouched", recorded in the design §8.1 deviation).

## 2026-11-18T06:31 [lead]

status ready -> active

## 2026-11-18T06:31 [lead]

attempt 1/3 started

## 2026-11-18T06:31 [lead]

base 143afaf

## 2026-11-18T06:43 [lead]

status active -> review

## 2026-11-18T06:43 [lead]

owner done: crate extracted (98% renames), blastit lsp is `pub use blastit_lsp as lsp`, deps exactly blastit-manifest + serde_json (0 libc/rustix/nix/pyo3 in cargo tree), new protected critical_lsp_stdio (3 cases), critical_lsp 52 passed unchanged assertions, make check EXIT=0. Reviewers dispatched.

## 2026-11-18T06:51 [lead]

reviewer: VERDICT PASS — 98% renames, body byte-identical but for the 4 design-mandated edits, closure is blastit-manifest(+sqlpolicy/pg_query/regex)+serde_json with 0 forbidden hits, protected diff exactly 4+/4- and zero removed asserts, cli.rs untouched, stdio suite asserts an exact {4,8}-{4,12} severity-1 range with env_clear(). One MINOR (RESULT diff arithmetic) fixed.

## 2026-11-18T07:06 [lead]

status review -> done

## 2026-11-18T07:06 [lead]

committed 1126395 (renames in da04c46)
