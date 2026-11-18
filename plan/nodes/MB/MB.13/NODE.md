---
id: MB.13
title: Rewrite the deployments index page for scannability
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths: [docs/deploy/index.md, python/tests/runnable_docs/inventory.toml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:55"
updated: "2026-11-18T20:36"
---

# Goal

Rewrite `docs/deploy/index.md` so it scans: one idea per short section, tables and short command blocks instead of multi-sentence paragraphs. The owner's verdict on the current page: **"much better although the `/blastit/deploy/` still probably needs a clean rewrite"** (after the DO page and static page were rewritten).

# Context

The page is 121 lines and does not look long — it reads slow. The problems:
- **"Pooler"** is a two-paragraph blob containing a `psql` block, a `dead_grant` aside, a `server_lifetime` explanation and a pooler-restart instruction, all in one run of prose.
- **"Four things a preset cannot do for you"** is four paragraphs masquerading as a list; each item mixes a rule, a mechanism and a remedy.
- Deep detail that belongs in `docs/reference/settings.md` / `docs/ci.md` is embedded here (the `connection`-group `dead_grant` story, `MAX_CLIENT_CONN` arithmetic, `BLASTIT_MAX_WORKERS`).

# Acceptance

1. **Structure with whitespace**, roughly:
   - **Intro** — 2–3 sentences: production is not `blastit dev`; it runs real uids and refuses to start without root/capabilities/seccomp. No silent fallback.
   - **Pick a preset** — keep the table; add one line that a preset is defaults and a `BLASTIT_*` override still wins.
   - **Two databases, two URLs** — a 3-row table (app → pool → port; migrations → database → port; grants job → database) and *one* sentence on why (DDL and session state do not survive transaction pooling). Then a numbered procedure: set the zone on the role, read it back on the direct port, restart the pooler — each a short block.
   - **What a preset cannot do for you** — four items, **≤ 2 sentences each**, each ending in a link to the page that owns the detail.
   - **Check that the ingress is live** — 2 steps (send a request with the header the preset reads; read `remote_addr`), keeping the one-line JSON example.
   - **Before the first deploy** — the checklist, unchanged.
   - **Where next** — unchanged.
2. **The `dead_grant`/`connection`-group story moves out.** One sentence here ("a database that already says `UTC` makes that statement a dead grant — CI explains when the group is required") with a link to `docs/ci.md`; the full reasoning and the `postgres:18` spelling nuance stay there, where they belong. Same for the pool sizing arithmetic: state the rule (workers × instances = client count the pool must accept) and link `docs/reference/settings.md` for `BLASTIT_MAX_WORKERS`/`MAX_CLIENT_CONN` detail.
3. **No sentence survives only because it was already written.** Anything that does not change what the reader types or decides is cut; anything that explains *why* stays but in one sentence.
4. **Facts stay true**: presets are defaults; `do-connecting-ip` is overwritten by the DO edge and `X-Forwarded-For`'s rightmost entry is read by the Compose preset; `*` is set by both presets; `/_blastit/health` exists; `preset` is `null` on the `stage0` event and set on `config`.
5. **Gates**: every fence registered at its new index in `python/tests/runnable_docs/inventory.toml` (positional — renumber), `make docs-unit`, `make docs-check`, `make docs-test`, `make check`; `dx` and `ai-ergonomics` critics **foreground** with the brief "would a developer about to deploy skim this and still know what to do next, or are they reading paragraphs to find one rule?".
6. Page stays <= 120 lines (it is 121 now — the rewrite must not grow it), and the two linked pages are not duplicated.

# Notes

- This is the landing page of `/blastit/deploy/`: it should read like an index with three procedures, not a chapter.
- Do not re-litigate the pooler decision or the two-URL rule; they stay, only the presentation changes.
