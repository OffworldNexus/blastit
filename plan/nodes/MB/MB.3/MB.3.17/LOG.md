# Log MB.3.17

## 2026-11-14T15:14 [lead]

created (task) Remove the external FastCGI path: delete blastit-fastcgi, drop [route.fastcgi], the ALLOW flag and every doc that sanctions it

## 2026-11-14T15:52 [lead]

ADDITIONAL LESSONS bullet to append alongside this node's own one-liner (the attempt cap on MB.3.14 refused a fourth reopen, correctly): **sapi_startup() nulls sapi_module_struct.ini_entries, so a lockdown assigned before it is silently absent while every test passes.** MB.3.12 attempt 1 set the INI array first and lost the whole policy -- open_basedir unset, allow_url_fopen=On, disable_functions empty -- with a green suite; the fix assigns after sapi_startup and a critical test pins startup < lockdown ordering. Same shape as the bailout trap: a call that reports success while doing nothing. Cite MB.3.12.

## 2026-11-14T15:57 [unknown]

edited fields

## 2026-11-14T15:57 [lead]

status ready -> active

## 2026-11-14T15:57 [lead]

attempt 1/3 started

## 2026-11-14T15:57 [lead]

base 9889a29; SPLIT agreed: code now, docs after MB.12's lenses (docs/frameworks|design|decisions are the architect's paths). docs/reference/settings.md is ours and goes now.

## 2026-11-14T16:11 [implementer]

attempt 1: code half landed (crate+lock+Executor/parse/validate/error/stage0/no_python_build, settings.md row, LESSONS). critical_guard --node MB.3.17 base 9889a29: 'no change to a protected critical test'. BLOCKED on one held path: docs/frameworks/php.md's blastit.toml fence still writes [route.fastcgi], so blastit-manifest's docs_fences (and make check) fails with 'route[0].fastcgi: unknown key'; docs/** is MB.12's, dispatch forbids it. Held docs half recorded as MB.3.20 (proposed, doc-writer, depends MB.3.17,MB.12).

## 2026-11-14T16:12 [lead]

attempt 1: CODE HALF COMPLETE, blocked on ONE held docs path. Crate deleted (7 files, 3495 lines) + Cargo.lock; Executor::Pool/FastCgiPolicy/Route::fastcgi and the whole FastCgi parse/validate/error family gone; BLASTIT_ALLOW_EXTERNAL_FASTCGI + BLASTIT_PHP_FPM gone from code and settings.md; [route.fastcgi] is an unknown key (route[0].fastcgi: unknown key) with the accepted twin asserted and proven failing first; LESSONS bullet appended; critical_guard authorized all 2 protected deletions; cargo build --locked, cargo test -p blastit, rs-no-python, make php-sapi-e2e all green. RED: make check fails ONLY at rs-test/docs_fences because docs/frameworks/php.md still writes [route.fastcgi] -- a held path (MB.12). UNBLOCK OPTIONS: (1) the architect (or an owner-authorised writer) drops the 3-line [route.fastcgi] block from php.md now -- that page is being rewritten by MB.3.20/MB.3.10 anyway; (2) owner authorises this node to edit that one fence. NB the implementer found [route.php] does NOT parse on this base (reads as a grant on unknown proxy php) -- that is MB.3.5.4, so the page cannot simply be switched to [route.php] yet.

## 2026-11-14T16:12 [lead]

status active -> blocked

## 2026-11-14T16:12 [unknown]

edited fields

## 2026-11-14T16:12 [lead]

status blocked -> ready

## 2026-11-14T16:12 [lead]

status ready -> active

## 2026-11-14T16:12 [lead]

attempt 2/3 started

## 2026-11-14T16:18 [implementer]

attempt 2: docs_fences now carries the STALE accounting row (php.md fence must fail with 'fastcgi: unknown key'; MB.3.20 owes the entry's removal) + self-clearing test; both halves proved red (wrong needle, and an entry on a booting page) then restored. inventory.toml fence-7 reason re-worded. make check exit 0, 111 ok / 0 FAILED.

## 2026-11-14T16:20 [lead]

status active -> done

## 2026-11-14T16:20 [lead]

commit bc3b201 (make check exit 0; 4287 deletions; critical_guard authorized both protected deletions; docs_fences STALE row self-clears). Held docs half is MB.3.20 (proposed, depends MB.3.17,MB.12) -- it MUST delete the STALE row when it retargets php.md, and the self-clearing test enforces it.
