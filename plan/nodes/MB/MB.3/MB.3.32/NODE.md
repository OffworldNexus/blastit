---
id: MB.3.32
title: Delete the dead PHP design corpus (18 files) and repoint every reference
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.17]
paths:
  - "docs/design/**"
  - "docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md"
  - docs/decisions/README.md
  - docs/research/INDEX.md
  - "docs/research/php-fastcgi-wire.md"
  - "docs/testing/AUDIT-MB.3.4.md"
  - "docs/testing/AUDIT-MB.3.4-gate.md"
  - "crates/blastit-manifest/src/**"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - "crates/blastit-manifest/tests/parse.rs"
  - "crates/blastit-view/src/runtime.rs"
  - crates/blastit/tests/no_python_build.rs
  - Makefile
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T20:35"
updated: "2026-11-14T22:26"
---

# Goal

Delete the dead `php-fastcgi` design corpus (18 files, ~656 KB) and repoint
every reference, so the rejected design stops being the largest source of
PHP naming confusion in the repo. Keep the **decision record** (ADR-0008) and the
research that is still why we embed (`php2shell`, `php-embed-sapi`).

# Context

- Owner, 2026-11-14: *"the whole php-fastcgi design md files should be deleted
  as well"*. It is a 349 KB design for a design ADR-0010 replaced, plus 17
  review/resolve files. The code it described was deleted in `MB.3.17`.
- The file names below carry the retired protocol word by construction: they are deletion
  targets, not transport claims.
- Delete exactly: `docs/design/php-fastcgi.md`,
  `docs/design/php-fastcgi.resolve-architect.md`, and every
  `docs/design/php-fastcgi.review-*.md` (16 files) = 18.

# Consistency work (a deleted file still cited is a 404 that reads like missing content)

1. `docs/design/README.md`: remove the `php-fastcgi.md` table row.
2. `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md`: **stays** as
   the historical decision. Replace every link/citation to the deleted design
   with one line: "the design this ADR described was deleted when ADR-0010
   replaced it" — record, not rebuild. Keep the ADR's own substance.
3. `docs/design/backend-runtimes.md` (~line 359): the plan-sync sentence names
   the superseded `php-fastcgi.md`; say "the retired design (deleted with
   ADR-0010)" instead.
4. `docs/design/php-dev-local.review-{dx,ai-ergonomics}.md`: these are review
   **records** with many text citations. Do not rewrite the citations; add one
   banner line at the top of each: "HISTORICAL — cites `docs/design/php-fastcgi.md`,
   deleted when ADR-0010 replaced it; the citations are to the record, not a
   live design."
5. `docs/research/INDEX.md`: remove the `php-fastcgi-wire` row, and delete
   `docs/research/php-fastcgi-wire.md` (see the decision below).
6. `docs/testing/AUDIT-MB.3.4.md` and `AUDIT-MB.3.4-gate.md`: banner line, not a
   rewrite — the audit is still true about `crates/blastit-fastcgi`, which was
   deleted in `MB.3.17`.
7. `crates/blastit-manifest/src/model.rs:327`: the comment cites
   `docs/design/php-fastcgi.md` §4.1 as authority for the external-PHP key. Point it
   at `ADR-0010` (or drop the sentence) — a comment citing a deleted file costs
   someone an hour.

**Decision on `docs/research/php-fastcgi-wire.md`: delete it.** Blastit speaks no
external PHP wire protocol (the codec crate was deleted in `MB.3.17`); the attack class is
covered by `php2shell.md`, the accepted path by `php-embed-sapi.md`, and the
wire note's only consumer is gone. Keeping it as rejected-history would preserve
a protocol write-up with no reader.

Keep: `docs/research/php2shell.md` (attack class — still why we embed) and
`docs/research/php-embed-sapi.md` (the accepted design's evidence).

# Acceptance

```console
$ grep -rn 'php-fastcgi\.md\|php-fastcgi-wire\.md' docs crates README.md ARCHITECTURE.md ; echo "grep=$?"   # 1
$ make docs-check
$ make docs-unit
$ make docs-test
$ make check
$ uv run --no-project tools/plan.py validate
```
- 18 design files + the wire research file are gone; no path/link to any of
  them remains (`grep` exits 1). The word may appear only in the banners/record
  lines that say the file was deleted.
- `docs/design/README.md` has no row for it; ADR-0008 keeps its substance and
  carries the one-line record; both audits and both php-dev-local reviews carry
  their banner; `model.rs` cites ADR-0010.
- `make docs-check` strict is green (no dead link/anchor), and `make check`
  (including `docs-test`) is green; `plan.py validate` OK.
- Report: deleted paths, every repointed link as `file:line -> what`, what was
  kept and why, and the grep/docs-check results.

## Owner: deep cleanup, keep exactly three labelled things (2026-11-14)

Delete every **prescriptive** trace of the retired external-PHP idea. Keep only:

1. **NOTHING.** Owner clarification 2026-11-14: *"this thing was never pushed
   to production… there is no backward compatibility to maintain"*. So
   `RETIRED_ROUTE_KEYS` and its special branch, and the tests that
   assert a curated self-naming message, are **deleted**. The retired route key
   becomes an **ordinary unknown key** like any typo — no list, no explanatory
   sentence. Editing (prefer deleting the now-pointless case from) the
   protected `crates/blastit-manifest/tests/critical_php_manifest.rs` is
   authorized by `critical_guard.py --node MB.3.32`; record that authorization
   in RESULT. Any `#[deprecated]`, `legacy_*` alias, or `Option`-shaped "old
   path" branch for PHP is deleted too: with nothing released, an alias is pure
   debt.
2. **ADR-0008** — the historical decision (see above), links stripped, one
   record line; its surviving content is the `Upstream` contract.
3. **Attack-class language** — `php2shell`, "CGI/FastCGI parameter trick",
   "HTTP/FastCGI desync" in security/research docs. That is the threat we
   neutralise, not our transport.

**`crates/**` triage (no behaviour change):** this node owns
`crates/blastit-manifest/src/**` only. Repoint `model.rs:327` at ADR-0010; in
`lib.rs`, `model.rs`, `parse.rs` delete or shorten every explanatory
comment about the retired external-PHP path to at most one line. Leave
the banned-word / absence assertions in `blastit/src/stage0.rs`,
`blastit/tests/critical_php_backend.rs`, `blastit/tests/no_python_build.rs`
alone (they are the guard); leave the protected manifest tests and
`docs_fences.rs` alone (`.20` owns the STALE row).

**`Makefile:404`:** the comment names the retired protocol; reword it to "the four
reserved CGI parameter names" — it is the param-lockdown test, not a transport.

**Verify the denials:** read ADR-0009 and ADR-0010 and report that every mention of
the retired protocol *denies* it (no "weaker mode", no fallback recipe). Fix any
that offer one.

## ADR-0008 outcome: tombstone (owner clarification)

Replace the ADR body with two or three lines, no design content and no links to
deleted files:

> Rejected before release: PHP was never served over an external protocol. Superseded by
> ADR-0010 (embedded `libphp`). The `Upstream` contract survives in ADR-0009.

Update `docs/decisions/README.md`'s row to a tombstone status/label so the index
agrees (no "Partially Superseded" that implies a living external design). Keep
ADR-0009/0010 mentions to *denials* only — verify and fix any offer.

Also drop every "retired key" / "legacy" / "deprecated" / "migration" framing
for PHP from this node's surfaces: there is no migration story; a manifest uses
`[route.php]` or it is wrong. Keep the `stage0`/`no_python_build` banned-word
assertions (today's refusal must name no alternative) and attack-class language.
