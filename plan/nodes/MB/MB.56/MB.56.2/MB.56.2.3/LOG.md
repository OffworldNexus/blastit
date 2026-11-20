# Log MB.56.2.3

## 2026-11-20T17:15 [lead]

created (task) Consolidate blastit-manifest §7 suite (15k -> <=1.5k LOC) incl. every docs/README Blastfile fence

## 2026-11-20T17:45 [lead]

status ready -> active

## 2026-11-20T17:45 [lead]

attempt 1/3 started

## 2026-11-20T17:45 [lead]

base ba279d8115788c8f0b0ec17a7a500b392047f836

## 2026-11-20T18:01 [lead]

edited fields

## 2026-11-20T18:01 [lead]

REVIEW FIX: (B) fence walker only matches 'text title="Blastfile"' -> misses 15/32 fences incl. all of docs/reference/blastfile.md; implement design's 3 fence classes (document: title=Blastfile|text starting with a keyword, lex+parse only; illustration: pinned count); (M) 5 public examples non-canonical (reference:88,112,160,174,387) -> expand to canonical form + move trailing comments; (M) per-page fence counts pinned not global >=15; (M) unpinned documented constructs: named-store unrestricted, OPTIONS, string escapes, duplicate trait/store, app name budget, $? outside query, index default, port/user/password/database/sslmode/ca_cert lowering, egress any warning; (M) pin the full blastit check stderr line shape

## 2026-11-20T18:02 [lead]

OWNER GUARDRAIL: canonicalise unformatted public docs Blastfile examples YES; NO per-page fence-count tables (brittle). Discover title="Blastfile" (incl. title="Blastfile: ...") + ```blastfile fences dynamically across README.md + non-excluded docs/**/*.md, assert parse + canonical fmt; allow an explicit tiny skip-list for deliberate bad-syntax demo fences. Keep <=1500 test LOC.

## 2026-11-20T18:09 [lead]

status active -> done
