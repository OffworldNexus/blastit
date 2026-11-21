# Log MB.31.2

## 2026-11-17T12:33 [planner]

created (task) Lower, validate and print static immutable through pattern.rs: bare All, quoted patterns crossing /, regex '...', one refusal set

## 2026-11-17T22:46 [lead]

status ready -> active

## 2026-11-17T22:46 [lead]

attempt 1/3 started

## 2026-11-17T22:46 [lead]

base 30b864f; inherits MB.31.1 review MINORs: document the pattern-name rule in the reference, the Django 'reaches every depth' sentence is only true once this lands, and design/static-and-spa.md's 256-byte table is stale

## 2026-11-17T22:48 [lead]

edited fields

## 2026-11-17T22:48 [lead]

attempt 1 blocked honestly: a public field type change is source-breaking at every struct literal naming it, and six needed files were out of lane (including two protected critical tests). paths extended, with the explicit rule that the protected tests take a FIXTURE MIGRATION ONLY -- no assertion weakened, deleted or ignored -- plus static_asset.rs/static_response.rs so bare All can be live. Re-dispatching.

## 2026-11-17T22:48 [lead]

attempt 2/3 started

## 2026-11-17T23:06 [lead]

attempt 2 done: immutable parses/lowers/validates/prints through pattern.rs (bare => All live end to end, patterns compiled once at lower, 32-cap refused at the 33rd node with line:col), the 256-byte and bespoke per-pattern validate rows deleted, docs table rewritten. 521 + 283 + 7 tests; make check EXIT=0; critical_guard authorizes all 5 protected changes as fixture migrations. cross-/ stays MB.31.3 (glob still stops at /) and is stated, not claimed.

## 2026-11-17T23:06 [lead]

status active -> review

## 2026-11-17T23:19 [lead]

REVIEW FAIL attempt 2 (3 MAJOR, 2 MINOR): every Patterns entry -- quoted OR regex -- is handed to the retired glob(), so a regex's text is re-read as a glob and cache_control emits immutable for a path the manifest's own matcher DENIES; two matchers disagree and the interim over-caches instead of failing closed. Fix: record Spelling on StaticPattern and build ImmutableGlob only from quoted entries. Also: the design note claims the scheduler never keeps a second matcher (false at HEAD), and the public /*-crosses-/ sentences are not live until MB.31.3.

## 2026-11-17T23:19 [lead]

attempt 3/3 started

## 2026-11-17T23:31 [lead]

attempt 3: Spelling on StaticPattern and ImmutableGlob built only from quoted entries; red proof captured (removing the filter yields left immutable / right no-cache at critical_static_assets:725), reviewed prose corrected. 284+522 tests, make check EXIT=0, docs-test 0 failed, critical_guard authorizes 5 protected changes.

## 2026-11-17T23:31 [lead]

status review -> review

## 2026-11-17T23:39 [lead]

reviewer attempt 3 VERDICT PASS (3 MINOR: StaticPattern equality is text-only so a spelling flip compares equal -- handed to MB.31.3; the two public /*-crosses-/ sentences are MB.31.3's; a harness SIGINT race). Red proof reproduced by the reviewer. Committing.

## 2026-11-17T23:39 [lead]

status review -> done
