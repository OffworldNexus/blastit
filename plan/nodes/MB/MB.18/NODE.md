---
id: MB.18
title: "Restructure /deploy: architecture + topology model first, preset mirrors second, generic levers third"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths: ["docs/deploy/**", python/tests/runnable_docs/inventory.toml, e2e/docs_deploy.py]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T19:47"
updated: "2026-11-15T09:06"
---

# Goal

Restructure `/blastit/deploy/` around **the architecture and the choices**, not around two recipes interleaved. The index sets expectations and shows the topology; each preset page is that same picture for one platform; one generic page carries the levers.

Owner's verdict (2026-11-14), verbatim:
> "you're mixing up the instructions for DO and for docker compose, you're OBSESSED about the 'show timezone' and I don't even know why... The deployments page should be about explaining the overview of options that are available, the overall architecture that we imagined (the app starts within a single container, there is a pooler in front of the DB, ...), maybe some kind of nice mermaid schema to show the infra/network topology. Like, set up the expectations. Including in regards to fair queuing TBH. And then all the subsequent pages are the mirror of this on how to do that on different platforms. One for each preset, explaining what we thought the config was gonna be, and one generic giving all the pointers and levers to decide all the useful values."

# Context

What is wrong today: `docs/deploy/index.md` teaches DigitalOcean and Compose **in the same paragraphs** (pooler steps: "DO roles default to UTC and Compose's `db/grants.sql` runs the ALTER…"), and three of its six sections are about one database setting (`SHOW TimeZone`, `set_config`, `dead_grant`). A reader deciding "how do I deploy this?" gets a mixed procedure instead of a mental model.

# Acceptance

1. **`docs/deploy/index.md` becomes the model, not the recipe** (<= 120 lines):
   - **What you are deploying** — 3–5 sentences: one app container per route namespace, blastit as the front door (ingress gate → scheduler → view workers), a **pooler in front of the database**, credentials never in app memory, per-route sockets to the proxies.
   - **Mermaid topology diagram** (`graph`/`flowchart`): client → edge/LB → blastit (ingress · scheduler · dbproxy) → app view workers; blastit → pooler → database; and the outbound-proxy side. Keep it legible at mobile width; the site already renders Mermaid, so `make docs-check` and the visual check must pass with it.
   - **Fair queuing, set as an expectation** — what it means in production: the scheduler owns parallelism, per-flow CPU is the quantum, and a heavy route cannot starve a light one. Point to `fair-queuing.md` for depth; no knob-by-knob detail here.
   - **The choices you have to make** — a table (not prose): preset vs generic, pooler or not, how peers reach the port, where migrations run, TLS/edge termination. Each row links to the page that explains it.
   - **Where to go next** — one line per page.
2. **Each preset page is a mirror of that model for one platform** (`digitalocean-paas.md`, `compose-traefik.md`): same section order, same diagram shape (platform-specific), and **no instruction from the other platform**. A one-line link to the other preset is the only cross-reference allowed. "What we thought the config would be" — the concrete values (points, sizes, TLS, pooler mode, ingress header) with the reasoning for each.
3. **`anything-else.md` becomes the generic lever page**: every value you may need to decide (bind address, worker budget, pool sizing, client-IP header, egress policy, health path, capability/seccomp requirements), what the default is, when to change it, and what breaks if you get it wrong. This is the page the preset pages link to instead of re-explaining.
4. **The timezone/`dead_grant` material is demoted**: one short subsection in the generic page (or a "common pitfalls" list), with the reasoning intact and links to `ci.md` for the `connection` group. It must not occupy the index and it must not be duplicated in both preset pages.
5. **Run the docs gates and the DX lenses**: `make docs-unit`, `make docs-check`, `make docs-test` (fences registered at the new positions), plus `dx` and `ai-ergonomics` critics on all three pages with the brief "can a reader explain to a colleague what a blastit deployment looks like after 60 seconds on this page, and does any paragraph make them scroll back?".
6. **Visual**: the Mermaid diagram must render in light/dark and be readable at mobile width — run `visual-checker` on the deploy pages, since a diagram that renders as a broken box is worse than none.

# Notes

- The model is the product here: a reader who understands the topology can evaluate any platform. The presets are instances of it, not the thing itself.
- Do not invent new presets or change the deploy behaviour; this is a documentation restructure over what 0.2.0 ships.
- Keep every fact that is currently true (two URLs, pooler modes, `options=` refused, ingress trust, capability fail-closed); they just move to where they belong.
