---
id: MB.19
title: "Restructure /frameworks: one 'adapting your app' page + nine capability slots per framework"
kind: task
status: ready
owner: "doc-writer"
priority: 2
depends_on: []
paths: ["docs/frameworks/**", python/tests/runnable_docs/inventory.toml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T19:48"
updated: "2026-11-14T19:49"
---

# Goal

Restructure `/frameworks/` the way `/deploy/` is being restructured (MB.18): **one page that teaches the considerations**, then **one page per framework filling the same slots in the same order**. A reader should be able to adapt a new framework by reading the intro and copying the shape, not by reverse-engineering one example.

Owner's direction (2026-11-14):
> "one initial page to introduce what are the considerations to take into account to adapt your application, and then framework by framework the same structure of how to fill up each 'trait'"

# Context

Today the framework pages are independent: each one explains its own wiring, in its own order, and a concept explained in `django.md` (ingress bodies, or `--learn`) is silently assumed or re-explained differently in `flask.md`, `php.md`, `static-and-spa.md`. There is no place that says *what blastit needs to know about an application*, so "adapt your app" is folklore.

# Acceptance

1. **`docs/frameworks/index.md` becomes "Adapting your application"** (<= 130 lines) and defines the **capability slots** every framework page fills, each with two sentences on *why blastit needs it* and what goes wrong if it is missing:
   1. **Executor** — how blastit runs the app: WSGI callable, PHP entry, static root, (later) Node server. Which manifest block; what "no `app`" means for non-WSGI.
   2. **Static assets** — where built files live, which paths are immutable (hashed) vs revalidated, and the `root`/`public/` rule.
   3. **Database** — the env var blastit owns, driver/prepared statements, the two-URL rule, and why the app never sees the real credential.
   4. **Ingress** — bodies: `form` / `json` / `raw` (+ `allow_raw`), size limits, and what happens without a declaration (`413`).
   5. **Environment & secrets** — what the app may read, what is stripped at `execve`, and the "do not read `DATABASE_URL` from app code" rule.
   6. **Health** — the path and what it must not touch.
   7. **Migrations** — a separate role, run outside blastit, why they are a trusted boundary.
   8. **Learn & check** — the one command that fills the manifest, the one that audits it, and how to read a finding (missing grant vs dead grant vs unrouted request vs missing ingress).
   9. **Deployment notes** — process model (one worker per route), worker budget, anything the framework changes about that.
2. **Every framework page uses those nine headings, in that order**, and only says what is *specific* to that framework. If a sentence would be true of every framework, it lives in the intro, not repeated on four pages. Target <= 120 lines each.
3. **Existing pages conformed**: `django.md`, `flask.md`, `php.md` (already being retargeted to `[route.php]` by `MB.3.10` — fold it into the shape rather than rewriting twice), `static-and-spa.md` (as the "no executor" case). Node/JVM pages inherit the shape when MB.4/MB.5 land — say so in one line in the intro.
4. **A capability matrix table in the intro** — framework × executor kind × static story × DB path — so a reader can see at a glance what changes and what does not.
5. **Gates**: `make docs-unit`/`docs-check`/`docs-test` with fences re-registered per page index, `dx` + `ai-ergonomics` critics on the intro and on each conformed page (brief: "can I adapt my framework from this page without opening another one, and does any page repeat the intro?"), and no page exceeds its line budget.
6. **One canonical example per slot**: if the intro shows a `[route.ingress]` snippet, the framework pages show theirs in the same shape, not a different one. Divergence in snippet shape is the defect this restructure exists to remove.

# Notes

- This is structure over content: do not invent new product behaviour, and do not change any snippet's meaning — move it, or link to where it now lives.
- The intro must be honest that "adapting your application" is usually *zero code changes*: a manifest block, a command, and not reading secrets. That is the DX promise, and today it is buried.
- Sequence after `MB.3.10` (the php retarget) so the PHP page is edited once, in its final shape.
