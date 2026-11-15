# Log MB.3.56

## 2026-11-15T10:05 [lead]

created (task) Pin [route.php] entry by (st_dev, st_ino) at boot: the ADR wins over the gap (H-007)

## 2026-11-15T10:11 [lead]

CITATION CORRECTION (the decision does not change): MB.3.7's reviewer checked where the entry-pin claim actually lives. ADR-0010 §3 claims `(path, st_dev, st_ino)` recording + stage-1 equality for `libphp` and the shim only — it makes NO pin claim for `[route.php] entry`. The claim comes from `crates/blastit/src/php_view.rs:89` ("Absolute `SCRIPT_FILENAME`, pinned at boot (`[route.php] entry`)") plus MB.3.7's node §7.3. So the gap is a code doc comment that is false today, and the pin is now the way to make it true. The owner's reasoning stands on its own merits: consistency with the pair, and a process able to write `manifest_dir` swapping the executed script between requests is a different trust boundary. H-006 was a duplicate of H-007 and is closed.

## 2026-11-15T10:35 [lead]

edited fields

## 2026-11-15T10:35 [lead]

FOLDED IN from MB.3.7's review (same files, same purpose — an e2e suite where every assertion can fail), so this node is the only one touching them before the release:
(a) `e2e/php2shell.py:70` `import urllib.parse` is unused (ruff F401) — delete it.
(b) `e2e/php2shell.py:747,913,1016` and `e2e/php/entry_probe.py:25` carry `# noqa: S603/S310` for rules that are not enabled (RUF100); `php2shell.py:798`'s `__enter__ -> Server` is PYI034 — drop the stale noqa (or enable the rules) and return `Self`; `e2e/rootless/php_entry_swap.py:191,278,284` are not `ruff format` clean.
(c) TWO ASSERTIONS THAT CANNOT FAIL: `php2shell.py:989-992` (`'"event":"ready"' in text or "ready " in text`) and `:1008-1011` (`command_kind("blastit dev --learn") == "dev-learn"` — a pure function asserted against itself). Replace the first with the real `event":"ready"` JSON event read from `server.events()`, and drop the second (or assert something a broken run could actually violate).
(d) `RESULT.md:103` says E11's body "**is** the twelve-key JSON"; the probe asserts a parseable dict carrying `script_filename` (the exact-twelve assertion lives in `check_static`, `:952`) — say "a JSON document carrying `script_filename`".

## 2026-11-15T10:35 [lead]

status ready -> active

## 2026-11-15T10:35 [lead]

attempt 1/3 started

## 2026-11-15T10:35 [lead]

base c24d0c5

## 2026-11-15T10:50 [lead]

status active -> review

## 2026-11-15T10:57 [lead]

edited fields

## 2026-11-15T10:57 [lead]

REVIEW FAIL (attempt 1) — one MAJOR, and it is the substance of the node: the pin is IDENTITY-ONLY.
MAJOR (`crates/blastit/src/php_view.rs:328`): `verify_entry` compares only `(st_dev, st_ino)`, so the entry's CONTENT is not pinned. The reviewer ran a live `blastit dev` PHP route, rewrote `public/index.php` IN PLACE (inode unchanged — what `cp`/`tar -x`/a deploy script does) and the next GET answered `200 IN-PLACE-DECOY-RAN`; and `rm` + recreate can hand the freed inode back (measured 4/20 on xfs) — the exact hazard `replace_by_rename`'s own doc at `:1152-1156` names. Both protected/e2e witnesses only exercise RENAME, the one shape the mechanism catches, so "the script a route executes cannot be swapped between requests" is still false.
FIX: (i) record `st_size` + `st_ctime` (sec, nsec) in `PhpBinding` and compare them in `verify_entry`; (ii) hold an `O_PATH | O_CLOEXEC` fd on the entry from boot (`ChildIo.keep`, as `pinned_fds()` already does for the pair) so the recorded inode cannot be freed and reused; (iii) add an IN-PLACE-REWRITE leg to `critical_php_embed` that must be refused, alongside the rename leg — a witness that only covers the shape the mechanism happens to catch is how this survived.
MINOR: `php_view.rs:341` says "the instant before the script is handed to libphp" — false: the client-paced body read (`blastit-view/src/php.rs:243`) and `php_request_startup` sit between check and load, so the window is up to `max_body_bytes`/`request_timeout`. Move `verify_entry` into `PhpEmbedRuntime::serve` immediately before `worker.handle.request` (`blastit-view/src/php.rs:273`), or correct the sentence.
MINOR: `e2e/php2shell.py:1010` — `command_kind(" ".join(server.arguments))` is the constructor's own vector, not the child's argv; it fails only if `Server.__init__` changes, never if `blastit` ignores `--learn`. Read `/proc/<pid>/cmdline` while running, or assert a `--learn`-only observable.
MINOR: `docs/knowledge/LESSONS.md` and `plan/nodes/MB/MB.3/MB.3.7/RESULT.md` are outside this node's paths — both are now added to `paths` by the lead; no action beyond knowing they are covered.
ACCEPTED and not to be redone: pin placement (in the forked child, before every REQ_HEAD, nothing skips it), the descriptor stop condition (verified in `blastit_php_sapi.c:559,587`), the inside-decoy flip, the protected red, the four folded e2e cleanups.

## 2026-11-15T10:57 [lead]

status review -> ready

## 2026-11-15T10:57 [lead]

status ready -> active

## 2026-11-15T10:57 [lead]

attempt 2/3 started

## 2026-11-15T11:09 [lead]

status active -> review

## 2026-11-15T11:09 [lead]

edited fields

## 2026-11-15T11:09 [lead]

OWNER RULING: SHIP it, with the residual documented where an operator reads it. The reasoning to record (not to re-derive): exploiting the coarse-`st_ctime` window needs a writer with write access to `manifest_dir` inside one 1–4 ms tick with a same-size payload; `MB.3.40`'s provenance gate denies exactly that to route uids in production and there is no `[route.php]`-reachable writer otherwise, so the remaining actor is the operator/root, who could replace the whole deployment anyway and is outside the threat model. It is a documented residual, not a ship blocker: the control defeats every in-scope writer, and the out-of-scope writer defeats more than this control.
TWO REQUIREMENTS TO LAND BEFORE THE COMMIT (docs/security.md is now in this node's paths):
1. `docs/security.md` — one or two sentences an operator will find: what a same-size in-place rewrite within one clock tick can do, and that it requires `manifest_dir` write access (which production denies to route uids).
2. The code comment at the pin record — the same thing in ONE line, so the next reader does not "simplify" the metadata comparison away thinking it is exact.

## 2026-11-15T11:09 [lead]

status review -> active

## 2026-11-15T11:09 [lead]

attempt 3/3 started

## 2026-11-15T11:31 [lead]

Accepted residual, recorded rather than chased: `docs/security.md:155` says "as the boot's own `stat`", but the tick is anchored to the FILE's recorded status-change time — a rewrite inside the boot's tick but after the file's own ctime tick is refused, so the sentence OVERSTATES the residual (never the protection). Exact replacement for whoever next edits that page: "inside the same coarse-clock tick (1–4 ms) as the file's recorded status-change time". Not worth a fourth attempt on this node; it belongs with the post-tag docs tail.

## 2026-11-15T11:31 [lead]

status active -> done
