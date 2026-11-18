# Log MB.34.4

## 2026-11-17T13:17 [planner]

created (task) blastit-manifest: carry each query's raw text and string Position, validate the template, and put every query refusal at its exact inner line:col

## 2026-11-18T01:25 [lead]

edited fields

## 2026-11-18T01:54 [lead]

edited fields

## 2026-11-18T01:54 [lead]

paths: added crates/blastit-manifest/tests/critical_query_sites.rs (new protected T1 suite the accepted design names; was out of lane).

## 2026-11-18T03:19 [lead]

status ready -> active

## 2026-11-18T03:19 [lead]

attempt 1/3 started

## 2026-11-18T03:19 [lead]

base 5f293985cc355b29dbac08421482b8ea84d1c677

## 2026-11-18T04:04 [lead]

status active -> review

## 2026-11-18T04:04 [lead]

owner done: QuerySite{at,interior,escapes,raw} + query_sites() one-per-entry, NONE for PgGrant::new; 3 refusal families with exact line:col; new protected critical_query_sites (4 T1 cases); 5 mutation reds; check --locked green except an evicted uv tool cache (LEAD FIXED: one online warm-up restored ruff+zensical offline). Reviewer dispatched.

## 2026-11-18T04:11 [lead]

reviewer VERDICT PASS: five positions (5,34)(5,72)(5,84)(4,23)(5,23) re-derived, protected diff is 5 added at: lines only, all four T1 cases load-bearing, uv cache confirmed repaired. One pre-existing MINOR (PgGrant derives Debug with unredacted queries) filed as a follow-up node.
