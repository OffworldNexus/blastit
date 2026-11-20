# Log MB.42.3.3

## 2026-11-20T01:33 [unknown]

created (task) Ecosystem wrappers: 4 universal py3-none wheels + console scripts + py-sapi.yml + Composer package

## 2026-11-20T02:56 [lead]

status ready -> active

## 2026-11-20T02:56 [lead]

attempt 1/3 started

## 2026-11-20T02:56 [lead]

base HEAD

## 2026-11-20T03:37 [lead]

status active -> review

## 2026-11-20T03:39 [lead]

REVIEW PENDING (ux): BLOCKER exec loop hidden not fixed (RO site-packages -> _executable False -> falls through to the console script -> silent infinite exec loop; docstring claims otherwise); BLOCKER shipped Composer package refuses every leg (4/4 digest rows 'pending', both smokes set BLASTIT_RELEASE_BASE_URL so the default-origin path is never exercised); MAJOR uncached first run prints nothing; MAJOR unwritable cache leaks raw mkdir error; MAJOR curl:/tar: text + tool exit codes; MAJOR blastit-lsp --help/--version silence (split to MB.42.12); minors: chmod widens 0700->0755, comment names composer-sha256.sh. Docs counts for MB.42.4.

## 2026-11-20T04:23 [lead]

status review -> done
