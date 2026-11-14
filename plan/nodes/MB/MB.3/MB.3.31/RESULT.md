# Result MB.3.31

## Summary

The live plan briefs now describe one PHP story only: embedded host NTS `libphp` in forked
per-route `view` workers over `view.chan`, the `[route.php]` block (`entry`, optional
`workers`), ADR-0010 as the decision, `MB.3.17` as the history. 33 live `NODE.md` files were
re-derived from disk and swept; 9 titles went through `plan.py edit`; 27 `done`/`superseded`
briefs that *prescribe* the retired mechanism got the one-line HISTORICAL banner. No status,
attempt, dependency or technical decision changed, and no code, docs or Makefile was touched.

## Evidence

### 1. Ban patterns (`php[-_ ]?fpm` / `route[.]fastcgi`) in live briefs — zero hits

Exemptions: `MB.3.30` (defines the guard's word list) and `MB.3.31` (this brief, which must
spell the pattern to state it).

```console
$ for f in $(find plan/nodes -name NODE.md | sort); do
    st=$(grep -m1 '^status:' "$f" | awk '{print $2}');
    case "$st" in done|superseded) continue;; esac;
    case "$f" in *MB.3.30/NODE.md|*MB.3.31/NODE.md) continue;; esac;
    grep -nEi 'php[-_ ]?fpm|route[.]fastcgi' "$f" && echo "HIT $f";
  done
$ echo "exit=$?"
exit=1            # no output: not one live brief carries a banned spelling
```

Spot check of the two exempt files: `MB.3.30` quotes the words on purpose (guard spec);
`MB.3.31` carries only the regex forms `php[-_ ]?fpm` / `route[.]fastcgi`, neither of which
matches itself.

### 2. `plan.py validate`

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

### 3. Bare `fastcgi` in live briefs — attack-class or deletion target only

```console
$ for f in $(find plan/nodes -name NODE.md | sort); do
    st=$(grep -m1 '^status:' "$f" | awk '{print $2}');
    case "$st" in done|superseded) continue;; esac;
    case "$f" in *MB.3.30/NODE.md|*MB.3.31/NODE.md) continue;; esac;
    grep -nEi 'fastcgi' "$f" && echo "-- $f";
  done
119:3. **Attack-class language** — `php2shell`, "CGI/FastCGI parameter trick",
120:   "HTTP/FastCGI desync" in security/research docs. That is the threat we
11:  - "docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md"
14:  - "docs/research/php-fastcgi-wire.md"
32:Delete the dead `php-fastcgi` design corpus (18 files, ~656 KB) and repoint
39:- Owner, 2026-11-14: *"the whole php-fastcgi design md files should be deleted
44:- Delete exactly: `docs/design/php-fastcgi.md`,
45:  `docs/design/php-fastcgi.resolve-architect.md`, and every
46:  `docs/design/php-fastcgi.review-*.md` (16 files) = 18.
50:1. `docs/design/README.md`: remove the `php-fastcgi.md` table row.
51:2. `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md`: **stays** as
56:   the superseded `php-fastcgi.md`; say "the retired design (deleted with
60:   banner line at the top of each: "HISTORICAL — cites `docs/design/php-fastcgi.md`,
63:5. `docs/research/INDEX.md`: remove the `php-fastcgi-wire` row, and delete
64:   `docs/research/php-fastcgi-wire.md` (see the decision below).
66:   rewrite — the audit is still true about `crates/blastit-fastcgi`, which was
69:   `docs/design/php-fastcgi.md` §4.1 as authority for the external-PHP key. Point it
73:**Decision on `docs/research/php-fastcgi-wire.md`: delete it.** Blastit speaks no
85:$ grep -rn 'php-fastcgi\.md\|php-fastcgi-wire\.md' docs crates README.md ARCHITECTURE.md ; echo "grep=$?"   # 1
120:3. **Attack-class language** — `php2shell`, "CGI/FastCGI parameter trick",
121:   "HTTP/FastCGI desync" in security/research docs. That is the threat we
-- plan/nodes/MB/MB.3/MB.3.32/NODE.md
```

Surviving lines, by justification — re-grepped against the file on disk today
(`grep -cEi 'fastcgi' plan/nodes/MB/MB.3/MB.3.32/NODE.md` -> `19`; the listing above is the
same run):

| Lines | Node | Why it survives |
|---|---|---|
| 120–121 | `MB.3.32` | Attack class only: `"CGI/FastCGI parameter trick"` / `"HTTP/FastCGI desync"` named as the threat we neutralise. Allowed by the rule. |
| 11, 14, 32, 39, 44, 45, 46, 50, 51, 56, 60, 63, 64, 66, 69, 73, 85 | `MB.3.32` | **Deletion targets (17 lines).** Each is a file path of the corpus to delete, a grep pattern over those paths, or the sentence that names the corpus — in the node whose whole job is deleting it. The brief says so explicitly ("The file names below carry the retired protocol word by construction: they are deletion targets, not transport claims"). |

Line-by-line: 11 and 14 are `paths` entries; 32/39/44–46/56/63–64/73 name the corpus files;
50–51 are the README row and the ADR-0008 tombstone step; 60 is the banner text to write into
the two review companions; 66 is the deleted crate; 69 is the `model.rs` comment to repoint;
85 is the acceptance grep over the deleted paths. Nothing there describes how Blastit runs PHP.

Prose that described *our* transport in `MB.3.32` was rewritten (Goal, the "retired
idea"/"external protocol" framing, `RETIRED_ROUTE_KEYS`, the Makefile wording, the
denial-verification clause, the ADR-0008 tombstone line), so the only remaining occurrences
are the ones above.

### 4. Banners

```console
$ grep -rl "HISTORICAL — written for the retired" plan/nodes --include=NODE.md | wc -l
27
$ # every bannered file is done/superseded
```

**27 banners added**, one line each, immediately after the frontmatter, before `# Goal`:

`MB.3.1`, `MB.3.2`, `MB.3.3`, `MB.3.4`, `MB.3.4.1`–`MB.3.4.6`, `MB.3.5.1`, `MB.3.5.4`, `MB.3.6.1`,
`MB.3.6.1.1`, `MB.3.6.1.2`, `MB.3.6.2`, `MB.3.6.2.1`, `MB.3.6.2.2`, `MB.3.6.3`,
`MB.3.6.3.1`, `MB.3.6.3.2`, `MB.3.6.3.3`, `MB.3.9.1`–`MB.3.9.5`.

Allowed skip reasons are the two in the brief: a match that is **only the attack class**, or a
**removal/denial record that is still current**. Token-matched `done`/`superseded` nodes
skipped, and why (3 of 30 after `MB.3.5.4` was bannered and `MB.3.12`'s single match was
removed):

| Node | Why no banner |
|---|---|
| `MB.3.17` | Removal record, still current: it is the node that deleted the external path, and every live rewrite points at it as history. |
| `MB.3.10` | Removal record, still current: the retarget that deleted the words from the pages (`[route.php]` + `BLASTIT_LIBPHP` + ADR-0010); every match is an instruction to delete a page/section. |
| `MB.3.28` | Denial record, still current: "no external route, no pool, no WSGI fallback" in the shipped fail-closed refusal. |

`MB.3.5.4` is **no longer skipped** — see Attempt 2 finding 1. `MB.3.12`'s only match was a
negation in the current shim build recipe, rewritten to "no external PHP server" so the
file carries no token at all (a banner there would be false: that shim work is current).

## Spot-checks

| Node | Now reads | Points at |
|---|---|---|
| `MB.3` (epic, title `Embedded PHP Backend: host NTS libphp in per-route view workers & php2shell Defense (0.2.0)`) | "Ship first-class **PHP** backend support (`0.2.0`) — embedded host NTS `libphp` in per-route view workers, ADR-0010, no external PHP process and no fallback"; acceptance: forked `EmbeddedView` per route, `SCRIPT_FILENAME`/`DOCUMENT_ROOT` pinned in C, no parameter stream, so `PHP_VALUE`/`httpoxy`/CVE-2024-4577 are unrepresentable | ADR-0010 (×3), `MB.3.17` (historical) |
| `MB.3.5` (title `Parse and validate [route.php] (entry + workers)`) | "the `[route.php]` grammar exactly as `ADR-0010` §2 pins it (`entry` required, optional downward `workers`)"; children retargeted to `EntryProbe`/`check_php_entries`/`critical_php_manifest.rs`; no pool config | ADR-0010, `MB.3.17` |
| `MB.3.6` (title `Dispatch [route.php] over view.chan …`) | "hands the request to the route's PHP `view` worker over the unnamed `view.chan` `RawFrame` `socketpair`, drives `PhpEmbedRuntime`"; "there is no pool to synchronise"; superseded children tabulated by status | ADR-0010 §1–§2, ADR-0009, `MB.3.17` |
| `MB.3.9` (title `blastit dev runs, supervises and cleans up the embedded PHP workers itself`) | "discovers the host NTS `libphp`, forks, recycles, supervises, reaps, group-kills and cleans up the per-route `EmbeddedView` workers"; `entry`/`workers` only, no `listen`/`env`; `.9.3`–`.9.5` marked superseded with the lifecycle credited to `MB.3.13`/`MB.3.7` | ADR-0010, `MB.3.17` |
| `MB.9.x` | `MB.9` title "… static, WSGI and embedded PHP"; "`MB.3.6` then adds the `php` row"; `MB.9.1`/`.1.1` type list names `PhpPolicy`; `MB.9.1.2` `IMPLEMENTATIONS` = `static`/`wsgi`/`php`/`fake`; `MB.9.2`/`.2.1` use `with_php` and `"php"` registration; `MB.9.4` "a PHP implementation that behaves differently … cannot merge". Design-of-record citations moved from the deleted design to ADR-0008's surviving `Upstream` contract + ADR-0009 | ADR-0008/0009, ADR-0010, `MB.3.17` |
| `MB.12` | Table row is now "PHP — blastit's own forked view worker, host NTS `libphp` embedded through a per-minor SAPI shim (ADR-0010) … blastit (it is the server)"; consequences 2/3/5/6 rewritten to embedding and `view.chan`; the closing note no longer says blastit does not own PHP end to end | ADR-0010 §1–§2 |

Meaning preserved throughout: no acceptance threshold, budget, path list, dependency or
technical choice was relaxed — only the transport description and the citations changed.

## Files

Modified (all under `plan/nodes/**`; every `LOG.md` change is the `plan.py edit` audit trail):

- **Titles (9):** `MB`, `MB.12`, `MB.3`, `MB.3.5`, `MB.3.5.3`, `MB.3.6`, `MB.3.9`,
  `MB.3.32`, `MB.9`.
- **Live bodies (24 more):** `MB.3.15`, `MB.3.20`, `MB.3.22`, `MB.3.26`, `MB.3.5.2`,
  `MB.3.6.4`, `MB.3.6.4.1`, `MB.3.6.4.2`, `MB.3.7`, `MB.3.8`, `MB.8`, `MB.9.1`,
  `MB.9.1.1`, `MB.9.1.2`, `MB.9.2`, `MB.9.2.1`, `MB.9.2.1.1`, `MB.9.2.1.2`,
  `MB.9.2.1.3`, `MB.9.2.2`, `MB.9.3`, `MB.9.4`, `MB.3.31` (own brief), `MB.3.32`.
- **Banners (27):** the list in §4.
- **One historical body, vocabulary only:** `MB.3.12` — a parenthetical in its current shim
  build recipe reworded so the file carries no token (no banner: that work is current).

One acceptance block was made runnable again: `MB.3.20` grepped all of `docs/` for the
retired words, which can never be `# 1` while `ADR-0010`/`ADR-0008`/`backend-runtimes.md`
are the record (and `ADR-0008`'s file name carries the word). It now greps the user pages
(`docs/frameworks docs/reference README.md`) — the same set `MB.3.30`'s guard scans — and
returns `pages-grep=1` today:

```console
$ grep -rn "route[.]$w\|ALLOW_EXTERNAL_$w\|PHP.FPM" docs/frameworks docs/reference README.md ; echo "pages-grep=$?"
pages-grep=1
```

No `LOG.md`/`RESULT.md` content was hand-edited; no status/attempt/dependency/owner change
was made by this node. The lead drove the two nodes excluded from the frontmatter diff, and
their state on disk is exactly as the lead left it: `MB.3.31` is `status: active`,
`attempts: 3`; `MB.3.32` is `status: review`, `attempts: 1`. Excluding them, the diff reports
exactly `-owner: architect` (see follow-up 1).

## Follow-ups

1. **`MB.12` lost `owner: architect`** when its title went through `plan.py edit`:
   `tools/plan.py:952` deliberately strips `owner` from non-task nodes, so a container
   cannot carry one and `--owner` will not restore it. `validate` is green (containers do
   not need an owner) and the tree view never rendered it; if the lead wants it back, that
   needs a `plan.py` change, not a hand edit. Worth a `LESSONS.md` line.
2. **`MB.3.32` vs the coming guard.** Its 19 remaining occurrences are deletion-target
   paths. `MB.3.30`'s scan must exempt a node whose subject is deleting that corpus (same
   class as `MB.3.17`), or `MB.3.32` must be `done` before `MB.3.30` lands.
3. **`MB.3.31`'s own acceptance exempts `MB.3.30` *and* `MB.3.31`** — both must be able to
   spell the patterns. The first block also switched from `plan/nodes/*/…` globs (which
   missed the five-level `MB.3.6.4.*` briefs) to `find plan/nodes -name NODE.md`, so the
   deepest leaves are actually scanned now.
4. **`MB.3.20`'s premise was largely delivered by `MB.3.10`** (docs already clean, no `STALE`
   row). Rewritten in place to the remaining record work; consider superseding it at the
   next replan — status deliberately untouched here.
5. **One new protected file is named by a live brief** — `crates/blastit/tests/critical_php_learn.rs`,
   owned by the live `MB.3.6.4.2`, which will need the usual `critical_guard.py --node`
   authorization when it lands. `critical_php_dispatch.rs`, `conformance_php.rs` and
   `critical_head_refusals.rs` were dropped from `MB.3.6`'s `paths`, gate and notes in
   attempt 2: their would-be owners (`MB.3.6.1.1`/`MB.3.6.2.2`) are `superseded`, so no
   live node could ever create them.

## Attempt 2

Review findings (attempt 1 FAIL, see `LOG.md` 2026-11-14T22:22) and what changed. Every other
attempt-1 rewrite was left as it stood.

1. **MB.3.5.4 still prescribed the narrowed external fallback and got no banner.** Added the
   exact one-line HISTORICAL banner immediately after its frontmatter (before `# Goal`), so
   `MB.3.5.4` now reads as the record of a transition that `MB.3.17` finished. Its substance —
   Goal, Context, acceptance — is untouched. Banner count is 27, and the skip table above is
   back to the two allowed reasons only (3 nodes: `MB.3.17`, `MB.3.10`, `MB.3.28`, all still
   current removal/denial records). `MB.3.12`'s lone match was a negation in its current build
   recipe and was reworded away rather than bannered.
2. **MB.3.8 invented an ADR-0010 → ADR-0007 amendment.** Read both ADRs:
   - `ADR-0007` decision 1: *"The responder lives in the scheduler, as the scheduler's own uid
     … with the seccomp role that may not call `socket()`: no new process, role, uid, socket,
     supervisor or filter."*
   - the amendment that ever existed was `ADR-0008` decision 7 (git HEAD): *"The scheduler
     gains one `AF_UNIX` socket capability"*, i.e. *"Decision 7 supersedes decision 1's 'no new
     socket/filter' sentence for `SeccompRole::Scheduler` only"* — and that socket existed only
     to reach the pool.
   - `ADR-0010` §1: *"`SeccompRole::Scheduler` keeps zero `socket()`"* — it **restores**
     decision 1, and `ADR-0010` never mentions `ADR-0007` (`grep -n ADR-0007` on it: no hits).

   Conclusion: `ADR-0010` does **not** amend `ADR-0007`, so the banner requirement was
   **deleted**, not renamed. MB.3.8 now says "no banner" and explains why; its acceptance
   asserts the reverse way round (`may not call` + `socket()` present, `Amended by ADR-0010`
   absent). Verified: that command prints `ADR-0007 decision 1 stands unamended`.
3. **MB.3.6 named suites no live leaf owns.** Removed `crates/blastit-scheduler/tests/critical_php_dispatch.rs`
   and `conformance_php.rs` from `paths` and from the epic gate (the gate now runs
   `cargo test -p blastit-scheduler --locked`), and removed `critical_head_refusals.rs` from
   `paths` for the same reason (same class — pool-era file, owners `MB.3.6.1.1`/`MB.3.6.2.2`
   `superseded`). The notes now name `critical_php_learn.rs` as the one new protected file,
   owned by the live `MB.3.6.4.2`; no node was invented. RESULT follow-up 5 was corrected to
   match.
4. **MB.3.22 item 5 was softened into a no-op.** Restored an instruction that actually
   removes the text: "**Delete that clause** (plan text, not code; the file is in this node's
   `paths`)", with the banner left in place above it. `MB.3.5.4:61` itself is untouched here —
   `MB.3.22` is its owner, and this node only edited the vocabulary of `MB.3.5.4`'s title/body
   elsewhere plus the banner.
5. **MB.3.8's security paragraph added new technical content.** The invented "path wall is the
   per-route uid plus `open_basedir`" sentence and the "amends ADR-0007 decision 1" claim are
   gone. The bullet now states **only** what `ADR-0010` §1 says (`SeccompRole::Scheduler` keeps
   zero `socket()`; the PHP route runs under `SeccompRole::EmbeddedView { egress }`), with the
   citation, and says in so many words that this node writes no new security decision.
   (Restoring the original text was not possible: it described the deleted `AF_UNIX` pool socket
   and the `0600` scheduler-owned socket, so the "cite only ADR-0010 §1" option was taken.)

Re-ran after the fixes: the ban scan is still clean (`A-exit=1`), the only live bare-word hits
are `MB.3.32`'s deletion targets, banner count `27`, `plan.py validate` -> `plan OK`.

## Attempt 3

Review findings (attempt 2 FAIL, `LOG.md` 2026-11-14T22:24) and what changed. Nothing else was
touched: the live-brief rewrites, the 27 banners and the attempt-2 fixes all stand.

1. **MB.3.22 item 5 was factually wrong on disk.** Verified `MB.3.5.4:61` today: it reads
   "`[route.fastcgi]` is refused as an unknown key naming it (the block no longer exists);
   `[route.php]` parses." — the current, correct unknown-key bullet (the pre-`MB.3.17` clause
   was already gone). **Item 5 deleted**; the remaining items keep their numbers (1–4); the Goal
   no longer says "five" ("Close the open MINORs … Two are behavioural; the other two are a dead
   field and a missing accessor"); the Notes no longer say "item 5 is this node's file only";
   and `plan/nodes/MB/MB.3/MB.3.5/MB.3.5.4/NODE.md` was dropped from `MB.3.22`'s `paths` with
   `plan.py --agent planner edit MB.3.22 --paths …` (the other five entries kept verbatim).
   `MB.3.5.4` itself is untouched apart from the attempt-1 banner.
2. **MB.3.8 cited the wrong ADR-0010 section.** The zero-`socket()` statement is in §2
   (Consequences: "…and one set of docs (`SeccompRole::Scheduler` keeps zero `socket()`)"),
   while the embedded worker/`EmbeddedView` statement is §1. The bullet now says
   "`SeccompRole::Scheduler` keeps zero `socket()` (§2, Consequences)" and cites
   "`ADR-0010` §1 and §2"; the ADR-0007 bullet likewise says "§1–§2 restore decision 1's
   zero-`socket()` scheduler". No requirement changed, only the citation.
3. **Stale MB.3.32 line numbers in the RESULT.** Re-ran
   `grep -nEi 'fastcgi' plan/nodes/MB/MB.3/MB.3.32/NODE.md` and pasted the real output plus a
   line-by-line split: **19 hits** — attack class on 120–121, deletion targets on 11, 14, 32,
   39, 44–46, 50–51, 56, 60, 63–64, 66, 69, 73, 85. The audit claim is checkable again.
4. **Wrong status description for the two lead-driven nodes.** RESULT now states them exactly as
   they are on disk: `MB.3.31` is `status: active`, `attempts: 3`; `MB.3.32` is
   `status: review`, `attempts: 1`; and says the lead drove both (this node changed neither).

Re-ran after the fixes: ban scan clean (`A-exit=1`), only `MB.3.32`'s deletion targets as live
bare-word hits, banner count `27`, `plan.py validate` -> `plan OK`.

