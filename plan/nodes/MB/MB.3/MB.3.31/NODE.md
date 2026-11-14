---
id: MB.3.31
title: "Purge the retired PHP transport vocabulary from live plan briefs (one-and-only-one-way invariant)"
kind: task
status: ready
owner: planner
priority: 1
depends_on: []
paths: ["plan/nodes/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T20:33"
updated: "2026-11-14T20:33"
---

# Goal

Remove the retired PHP-transport vocabulary from every **live** plan brief so
the invariant "one and only one way to run PHP" is true on disk, not just in
ADR-0010. This is the prerequisite for `MB.3.30`'s plan guard.

# Context

- Owner, 2026-11-14: *"NO FALLBACK to be clear, I want ONE AND ONLY ONE way to
  run PHP"*. The code path is gone (`MB.3.17`), ADR-0010 is the decision, and
  `MB.3.28` now refuses a `[route.php]` manifest until `MB.3.13.3` wires the
  worker — but 27 live `NODE.md` files still describe the retired transport,
  including the `MB.3` epic title and `MB.3.5`/`MB.3.6`/`MB.3.9` titles. An agent
  reading those briefs will implement a fallback that does not exist.
- Live = frontmatter `status` not in `{done, superseded}` (blocked/failed are
  live too). `LOG.md` and `RESULT.md` are **records** and are not scanned.
- Ban, in a live `NODE.md`: `php-fpm`, `PHP-FPM`, `php_fpm`, `route.fastcgi`.
  `FastCGI` is allowed **only** on a line that names the attack class
  (`attack`, `desync`, `parameter trick`, `smuggling`, `CVE`), the same
  distinction the docs guard makes for `docs/VISION.md`; a line about what
  Blastit runs may never say it.
- Do not touch `MB.3.30/NODE.md` (it defines the guard and its word list).
- **Vocabulary only.** Do not change a status, an owner, an attempt count, a
  dependency or a technical decision. Replace the transport word with what is
  true (embedded `libphp`, `[route.php]`, "the retired external PHP path
  (`MB.3.17`, superseded)"), and keep the sentence's meaning. Titles go through
  `tools/plan.py --agent planner edit <id> --title ...`; bodies may be edited as
  Markdown. Preserve every fact that is still true.
- The 27 files on disk at creation: `MB.12`, `MB.3` (epic), `MB.8`, `MB.9`,
  `MB.3.10` (will be `done` after it lands), `MB.3.20`, `MB.3.22`, `MB.3.26`,
  `MB.3.28` (will be `done`), `MB.3.30` (skip), `MB.3.5`, `MB.3.6`, `MB.3.7`,
  `MB.3.8`, `MB.3.9`, `MB.9.1`–`MB.9.4` and their children, `MB.3.5.2`,
  `MB.3.5.3`, `MB.3.6.4`. Re-derive the list; do not trust this one.

# Acceptance

```console
$ for f in $(grep -rl '^id:' plan/nodes/*/*/NODE.md plan/nodes/*/*/*/NODE.md plan/nodes/*/*/*/*/NODE.md 2>/dev/null); do
    st=$(grep -m1 '^status:' "$f" | awk '{print $2}');
    case "$st" in done|superseded) continue;; esac;
    case "$f" in *MB.3.30/NODE.md) continue;; esac;
    grep -nEi 'php[-_ ]?fpm|route\.fastcgi' "$f" && echo "HIT $f";
  done
$ uv run --no-project tools/plan.py validate
```
- Zero hits for `php[-_ ]?fpm` / `route.fastcgi` in live `NODE.md` files.
- Every remaining `fastcgi` in a live brief is on an attack-class line (list
  them in the RESULT, with the line, so the distinction is auditable).
- `plan.py validate` OK; no status/attempt/dependency changed; `MB.3.30` and
  every `done`/`superseded` node untouched.
- Spot-check the rewritten `MB.3` epic, `MB.3.5`, `MB.3.6`, `MB.3.9`,
  `MB.9.x`, `MB.12`: the meaning is the same and each points at ADR-0010 /
  `MB.3.17` for history.

## Historical banners (owner, 2026-11-14)

A guard exemption protects the build; it does not protect a reader. Agents open
`done`/`superseded` nodes for background, so every historical node that
**prescribes** the retired mechanism gets one line immediately after its
frontmatter, before `# Goal`:

> HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

- The named cases: `MB.3.1` (FastCGI research), `MB.3.2`, `MB.3.3`, `MB.3.4`,
  `MB.3.4.1`–`MB.3.4.6`, `MB.3.5.1`, `MB.3.6.1`–`MB.3.6.3` (and `.1.1`/`.1.2`/
  `.2.1`/`.2.2`), `MB.3.9.1`–`MB.3.9.5`. Apply judgement file by file; the
  survey's 63 is a token match, not the banner list.
- **Skip** a node whose match is only the attack class (php2shell/CGI argument
  injection) or a removal record that is still current (`MB.3.17`): those do
  not prescribe the retired mechanism. A banner on them would be false.
- Do **not** rewrite the substance. The record of what was rejected is
  valuable; the banner only stops it reading as current.
- Count: report how many banners were added and why any token-matched
  `done`/`superseded` node was skipped.

## No compatibility story (owner clarification, 2026-11-14)

The PHP transport was never released, so in live briefs also remove
"retired key" / "legacy" / "deprecated" / "migration" framing **about PHP** —
a manifest uses `[route.php]` or it is wrong; there is nothing to migrate from.
Keep the historical banner text exactly as given above (it labels a rejected
design, not a migration), keep attack-class language, and do not invent a
compatibility note anywhere.
