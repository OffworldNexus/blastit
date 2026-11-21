# Log MB.3.9.1

## 2026-11-14T13:17 [lead]

created (task) Rewrite the PHP page Local section for the automatic dev pool (two commands, dx + ai-ergonomics acceptance)

## 2026-11-14T13:20 [unknown]

edited fields

## 2026-11-14T13:22 [lead]

status ready -> active

## 2026-11-14T13:22 [lead]

attempt 1/3 started

## 2026-11-14T13:22 [lead]

base 2ad094b; this leaf is now the unblocker for MB.3.5.1: docs_fences boots docs/frameworks/php.md, which still shows listen = run/fpm.sock and workers = 4. MB.3.5.1's parser already refuses listen (FastCgiListenDerived) and its 7->8 edit to docs_fences.rs is uncommitted in the worktree; do not revert or duplicate it.

## 2026-11-14T13:50 [lead]

status active -> review

## 2026-11-14T14:09 [lead]

status review -> done

## 2026-11-14T14:09 [lead]

commit 0bbb877 (atomic with MB.3.5.1: each alone leaves docs_fences/parser red). Open follow-ups: (a) install story uv tool vs uv add -- belongs to MB.12 distribution, recorded on MB.3.9; (b) MB.3.7 node body corrected (stale mkdir/listen/php-fpm.conf replay clauses); (c) H1/nav still say PHP-FPM -- rename after MB.12 decides.
