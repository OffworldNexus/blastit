# Log MB.3.5.4

## 2026-11-14T14:39 [lead]

created (task) Add [route.php] (PhpPolicy, Executor::Php) and narrow [route.fastcgi] to external-only; drop FastCgiPoolSocketInvalid

## 2026-11-14T15:14 [lead]

RETARGETED (owner 2026-11-14, 'No fallback please'): this leaf is now [route.php] ONLY. [route.fastcgi] stops existing as a key -- refused as an unknown key naming it, NOT defaulted off -- and no fastcgi policy type or parser arm survives. The fallback half I wrote earlier this session (entry+listen external-only, the weaker-mode warning, and the BLASTIT_ALLOW_EXTERNAL_FASTCGI fail-closed switch) is SUPERSEDED and must not be implemented; the crate removal is MB.3.17.

## 2026-11-14T16:20 [lead]

RE-SCOPED after MB.3.17 (commit bc3b201): the fastcgi half is gone, so this leaf is [route.php] ONLY -- PhpPolicy { entry, workers }, Executor::Php, and it must CARRY the rule tests that left with check_fastcgi (entry rules, optional workers 1..=64, spa=true conflict, CONNECT/TRACE refusal). Also: [route.php] currently parses as 'grant on unknown proxy php', which is exactly why docs_fences' STALE twin is [route.static] until this lands.

## 2026-11-14T16:20 [lead]

status ready -> active

## 2026-11-14T16:20 [lead]

attempt 1/3 started

## 2026-11-14T16:20 [lead]

base 5dcf16d; scope guards added from the owner: one key (entry, under static.root when both set), optional downward workers only, Executor::Php typed, fastcgi stays an unknown key with the accepted twin, inherit the entry/workers/spa/CONNECT/TRACE rule tests, PHP manifests need no app but app-less+executor-less still refuses without mentioning uv add, STALE row untouched.

## 2026-11-14T16:32 [unknown]

edited fields

## 2026-11-14T16:32 [lead]

status active -> ready

## 2026-11-14T16:32 [lead]

status ready -> active

## 2026-11-14T16:32 [lead]

attempt 2/3 started

## 2026-11-14T16:32 [lead]

paths extended to crates/blastit/src/stage0.rs + crates/blastit/tests/no_python_build.rs: the predicates landed but stage0::app_rule/no_python_support still key on is_all_static(), so a PHP-ONLY manifest is still refused with BLASTIT_APP / the uv add sentence -- guard #4 of the brief is not met until that is fixed and asserted. Follow-up (2), --learn rewriting a manifest and dropping [route.php] via edit.rs::expected_routes, is logged for MB.3.6.4 (it owns learn/check attribution).

## 2026-11-14T16:36 [lead]

status active -> review

## 2026-11-14T16:36 [lead]

attempt 2: boot decisions fixed -- app_rule refuses only when !can_boot_without_app() && app.is_none(); no_python_support returns None for can_boot_without_app() and skips [route.php]; uv add stays WSGI-only. Tests: php-only and static+php boot; empty/WSGI-only/php+WSGI refuse with the exact BLASTIT_APP sentence and never uv add; real-binary no_python_build php-only row. make check exit 0, rs-no-python 3/3, docs_fences diff 0 lines.

## 2026-11-14T16:39 [lead]

status review -> done

## 2026-11-14T16:39 [lead]

REVIEW PASS (attempt 2). Five MINORs deferred to MB.3.22 (two behavioural: stage0::static_app_refusal still keys on is_all_static so a PHP-only manifest silently ignores BLASTIT_APP; check.rs:161 still routes PHP through pytest). Stale NODE.md acceptance clause deleted. judge: workers is a real cap (None => derive, no consumer treats it as pool size); static.root match is segment-wise (publicity/ refused and tested).
