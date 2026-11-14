# Log MB.3.13.1

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 1: PhpEmbedRuntime worker loop over view.chan: route_uid, EmbeddedView filter, max_requests recycling

## 2026-11-14T18:18 [lead]

status ready -> active

## 2026-11-14T18:18 [lead]

attempt 1/3 started

## 2026-11-14T18:18 [lead]

base 9e83cc0; milestone: a PHP fixture answers through view.chan. Prereqs: MB.3.11, MB.3.12, MB.3.5.4, MB.3.23, MB.3.25.

## 2026-11-14T18:20 [lead]

BLOCKED correctly (attempt 1): my brief claimed prereqs had landed, but MB.3.23 (WorkerRuntime), MB.3.24, MB.3.25 (EmbeddedView) are all 'ready', not done -- grep finds no WorkerRuntime/EmbeddedView in crates/. Second: the worktree lacked the architect's accepted docs and the post-merge plan commits because they were committed on release/0.2.1 AFTER the fold; merged now (ff8aba3), so ADR-0010/backend-runtimes.md are present. Corrected order: MB.3.23 (trait) -> MB.3.25 (EmbeddedView) -> MB.3.13.1 -> MB.3.13.2/.3 -> MB.3.13.4. Also: this leaf's paths need crates/blastit-view/Cargo.toml (php feature + blastit-php-sapi dep) which it does not have. Not re-dispatched until those land; no code was touched.

## 2026-11-14T18:20 [unknown]

edited fields

## 2026-11-14T18:20 [lead]

OWNERSHIP RESOLVED (owner, 2026-11-14): PhpEmbedRuntime + the php feature wiring are THIS leaf's; MB.3.13.4 owns only the protected suite. backend-runtimes.md §5 row 7 amended to say so, MB.3.13.4 retitled, and this leaf is the Cargo.toml/Cargo.lock lock owner for the split (paths now include crates/blastit-view/Cargo.toml).
