# Log MB.3.32

## 2026-11-14T20:35 [planner]

created (task) Delete the dead php-fastcgi design corpus (18 files) and repoint every reference

## 2026-11-14T20:36 [lead]

edited fields

## 2026-11-14T20:36 [lead]

OWNER 2026-11-14 'deep cleanup of this idea': scope extended from the 18 design files to every prescriptive trace. Keep exactly (1) RETIRED_ROUTE_KEYS + its tests (protected one untouched) + one-line comment, (2) ADR-0008 as record, (3) attack-class language. Added crates/blastit-manifest/src/** (comments + model.rs repoint) and Makefile:404 ('four reserved CGI parameter names'). Crates triage measured 30 hits (not 37): manifest lib2/model4/parse3/critical_php_manifest8/docs_fences2/parse-tests5, stage0 1, critical_php_backend 1, no_python_build 4; the guard/absence assertions and protected tests stay, docs_fences is MB.3.20's.

## 2026-11-14T20:37 [lead]

edited fields

## 2026-11-14T20:37 [lead]

OWNER CLARIFICATION 2026-11-14: never released, no backward compat. RETIRED_ROUTE_KEYS + its curated self-naming branch/tests are DELETED; [route.fastcgi] is an ordinary unknown key. Protected critical_php_manifest.rs edit authorized by critical_guard --node MB.3.32 (prefer deleting the pointless case). ADR-0008 becomes a 2-3 line tombstone, decisions/README row updated. No #[deprecated]/legacy alias/old-path Option anywhere. Paths now include decisions/README.md and the two manifest test files.

## 2026-11-14T22:14 [lead]

status ready -> active

## 2026-11-14T22:14 [lead]

attempt 1/3 started

## 2026-11-14T22:14 [lead]

base 136f2d376089ac581de7794c08ea8689f42336c3

## 2026-11-14T22:14 [lead]

edited fields

## 2026-11-14T22:18 [planner]

edited fields

## 2026-11-14T22:23 [lead]

edited fields

## 2026-11-14T22:23 [lead]

lead: no_python_build.rs added to paths (2-line asserted-sentence update, refusal behaviour unchanged, authorized but previously out of scope)

## 2026-11-14T22:23 [lead]

status active -> review

## 2026-11-14T22:26 [lead]

status review -> done

## 2026-11-14T22:29 [lead]

commit e730527 (docs+manifest+Makefile); plan folder in the same commit
