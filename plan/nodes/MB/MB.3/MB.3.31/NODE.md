---
id: MB.3.31
title: "Purge the retired PHP transport vocabulary from live plan briefs (one-and-only-one-way invariant)"
kind: task
status: done
owner: planner
priority: 1
depends_on: []
paths: ["plan/nodes/**"]
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-14T20:33"
updated: "2026-11-14T22:26"
---

# Goal

Remove the retired PHP-transport vocabulary from every **live** plan brief so the invariant
"one and only one way to run PHP" is true on disk, not just in ADR-0010 — and so an agent
reading a live brief cannot implement a fallback that does not exist. This is the
prerequisite for `MB.3.30`'s plan guard.

# Context

- Owner, 2026-11-14: *"NO FALLBACK to be clear, I want ONE AND ONLY ONE way to run PHP"*.
  The code path is gone (`MB.3.17`), ADR-0010 is the decision, and `MB.3.28` refuses a
  `[route.php]` manifest until `MB.3.13.3` wires the worker — but dozens of live `NODE.md`
  files still described the retired transport, including the `MB.3` epic title and the
  `MB.3.5`/`MB.3.6`/`MB.3.9` titles. An agent reading those briefs would implement a
  fallback that does not exist.
- The truth every rewrite states: PHP runs as embedded host NTS `libphp` inside forked
  per-route `view` workers over `view.chan`; the manifest block is `[route.php]` with
  `entry` and an optional `workers` cap (ADR-0010 §1–§2); the external PHP path is history
  (`MB.3.17`, superseded).
- Live = frontmatter `status` not in `{done, superseded}` (blocked/failed are live too).
  `LOG.md` and `RESULT.md` are **records** and are not scanned.
- Ban, in a live `NODE.md`, four retired spellings: the lower-case fpm hyphenation, its
  under-score and all-caps twins, and the dotted external route key. The pattern in the
  `# Acceptance` command is the definition of record (it is written so that this brief does
  not match itself).
- The wire-protocol word may remain **only** on a line that names the attack class
  (`attack`, `desync`, `parameter trick`, `smuggling`, `CVE`) — the same distinction the
  docs guard makes for `docs/VISION.md`. A line about what Blastit runs may never carry it.
- Do not touch `MB.3.30/NODE.md` (it defines the guard and its word list).
- **Vocabulary only.** No status, owner, attempt count, dependency or technical decision
  changes. Titles go through `tools/plan.py --agent planner edit <id> --title ...`; bodies
  are Markdown.
- The list of files on disk at creation was a snapshot; re-derive it, do not trust it.

# Acceptance

```console
$ for f in $(find plan/nodes -name NODE.md | sort); do
    st=$(grep -m1 '^status:' "$f" | awk '{print $2}');
    case "$st" in done|superseded) continue;; esac;
    case "$f" in *MB.3.30/NODE.md|*MB.3.31/NODE.md) continue;; esac;
    grep -nEi 'php[-_ ]?fpm|route[.]fastcgi|fastcgi' "$f" && echo "HIT $f";
  done
$ uv run --no-project tools/plan.py validate
```
- Zero hits for the fpm spellings / the dotted external route key in live `NODE.md` files.
  The loop exempts `MB.3.30` (the guard's word list) and this brief, because both must be
  able to spell the patterns; audit those two by eye against the same patterns.
- Every remaining occurrence of the wire-protocol word in a live brief is on an attack-class
  line, **or** names a file that node exists to delete (list them all in the RESULT with the
  line, so the distinction is auditable).
- `plan.py validate` OK; no status/attempt/dependency changed; `MB.3.30` and every
  `done`/`superseded` node's substance untouched (banners only).
- Spot-check the rewritten `MB.3` epic, `MB.3.5`, `MB.3.6`, `MB.3.9`, `MB.9.x`, `MB.12`:
  the meaning is the same and each points at ADR-0010 / `MB.3.17` for history.

## Historical banners (owner, 2026-11-14)

A guard exemption protects the build; it does not protect a reader. Agents open
`done`/`superseded` nodes for background, so every historical node that **prescribes** the
retired mechanism gets one line immediately after its frontmatter, before `# Goal`. The
line is applied verbatim in those nodes — copy it from the first one,
`plan/nodes/MB/MB.3/MB.3.1/NODE.md` — and it never appears in a live brief.

- The named cases: `MB.3.1` (PHP wire research), `MB.3.2`, `MB.3.3`, `MB.3.4`,
  `MB.3.4.1`–`MB.3.4.6`, `MB.3.5.1`, `MB.3.6.1`–`MB.3.6.3` (and `.1.1`/`.1.2`/
  `.2.1`/`.2.2`), `MB.3.9.1`–`MB.3.9.5`. Apply judgement file by file; a token survey is
  not the banner list.
- **Skip** a node whose match is only the attack class (php2shell / CGI argument injection),
  a denial/negation, or a removal record that is still current (`MB.3.17`): those do not
  prescribe the retired mechanism, and a banner on them would be false.
- Do **not** rewrite the substance. The record of what was rejected is valuable; the banner
  only stops it reading as current.
- Count: report how many banners were added and why any token-matched `done`/`superseded`
  node was skipped.

## No compatibility story (owner clarification, 2026-11-14)

The PHP transport was never released, so in live briefs also remove "retired key" /
"legacy" / "deprecated" / "migration" framing **about PHP** — a manifest uses `[route.php]`
or it is wrong; there is nothing to migrate from. Keep the historical banner text exactly
as applied in the historical nodes (it labels a rejected design, not a migration), keep
attack-class language, and do not invent a compatibility note anywhere.
