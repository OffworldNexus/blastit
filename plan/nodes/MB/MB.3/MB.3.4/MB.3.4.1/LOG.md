# Log MB.3.4.1

## 2026-11-14T03:37 [planner]

created (task) Create the pure blastit-fastcgi crate: manifest, records, name-value codec, CGI head parser - design MB.3.4a

## 2026-11-14T11:02 [lead]

status ready -> active

## 2026-11-14T11:02 [lead]

attempt 1/3 started

## 2026-11-14T11:02 [lead]

base dbbf6196e4f7e933a2687928883380702542f361

## 2026-11-14T11:12 [lead]

DEVIATION: changed lines ~1430 vs design's <=550 cap, entirely rustdoc + 34 in-module tests over 4 source files; no record type, cap or refusal dropped. Accepted by lead as a recorded deviation (re-splitting a green 4-file crate now would only risk regression); reviewer to judge substance and may reopen if it finds padding.

## 2026-11-14T11:12 [lead]

status active -> review

## 2026-11-14T11:21 [lead]

REVIEW FAIL: [MAJOR] record.rs:114-133 + lib.rs:64 MAX_PARAMS_BYTES=65535 unreachable — Header::new rejects 65529..65535 because aligned padding pushes CL+PL over 65535 while Header::decode accepts CL=65535/PL=0, so a design-legal set fails as Truncated/502 instead of 400. [MINOR] cgi.rs:45 Field derives Debug so {:?} prints header values (Set-Cookie) unlike the hand-written log-safe Debug elsewhere. [MINOR] lib.rs:66-68 doc says budget minus eight length bytes (65527) but the constant is 65531. [MINOR] cgi.rs:374 test name a_non_token_name_is_refused vs design's cgi::tests::non_token_header_name_is_fault. [MINOR] record.rs:56-59 unknown type byte decodes as Unnamed without a typed error and without a test. [MINOR] RESULT.md mischaracterizes the budget split (572 prod / 492 tests / ~369 rustdoc).

## 2026-11-14T11:21 [lead]

status review -> ready

## 2026-11-14T11:30 [lead]

attempt 2 done: 65 535 PARAMS cap now reachable (zero padding at max size, decode accepts both), Field Debug is log-safe, pair-budget doc fixed, test renamed to non_token_header_name_is_fault, Unnamed decode test added, RESULT line split corrected. Budget deviation stands (585 production lines vs the design's 550); accepted, follow-up for MB.3.8 audit.

## 2026-11-14T11:30 [lead]

status ready -> review

## 2026-11-14T11:39 [lead]

attempt 2/3 started

## 2026-11-14T11:39 [lead]

attempt 2 review PASS (all 8 acceptance commands re-run by reviewer; 65 535 round-trip proven for all seven record types; budget deviation accepted). Three MINORs fixed in attempt 3: Head::content_type must store the Content-Type essence (design R18), Record::decode assertion on the max-size record, and an at-cap Ok head case.

## 2026-11-14T11:39 [lead]

status review -> ready

## 2026-11-14T11:47 [lead]

status ready -> review

## 2026-11-14T11:55 [lead]

status review -> done
