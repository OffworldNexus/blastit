---
id: MB.26.38
title: "e2e: Django collectstatic served by [static] beside the WSGI app"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Add the missing end-to-end leg for the case the owner actually cares about: **static files beside a normal Python app** — Django's `collectstatic` output served by `[static]` while the WSGI app still answers the dynamic routes. Today's static e2e fixture is a prebuilt SPA, so that shape is only proven by unit/protected tests.

# Context

`make e2e` already proves, against live containers, every promise of `docs/frameworks/static-and-spa.md` (SPA deep URL → `index.html`, a non-SPA fixture where it must not, hashed asset under the year-long cache, `HEAD` with no body, directory index, `/.well-known/` public, `.env`/`.git` 404, nine traversal/symlink refusals, `GET` with a body 413, `blastit check` + `--json`, all-static manifest refusal) plus the Django demo, SQLAlchemy, rootless capabilities, prebuilt wheel and venv smoke.

What no e2e covers: a **Django app whose `STATIC_ROOT` is served by `[static]`** in the same process topology, with the app still receiving dynamic routes — i.e. the ordinary deployment the docs are being rewritten to describe (MB.6). The static fixture is a build output with no upstream at all, which is why this gap exists.

# Acceptance

1. A new e2e leg (in `e2e/run.py`, reusing `check_logs.py`/`scenario.py` helpers) that:
   - builds or ships a Django app that has run `collectstatic` into a `STATIC_ROOT` (a small fixture is enough; no network installs inside the gate beyond what the other legs already cache);
   - runs it under `blastit` with `[static]` pointing at that root and a normal app route alongside;
   - asserts a static file is served by blastit (correct `Content-Type`, `200`, ETag/304), a dynamic route reaches the app, an unknown *static-looking* path falls through per the documented precedence, and `..`/dotfile attempts are still refused;
   - asserts the unhashed-original footgun explicitly: an `immutable` glob that would also match an unhashed file is not silently trusted — if the manifest rejects it, prove the rejection; if it is accepted, say what protects the user.
2. `make e2e` exit 0 with the new leg, and the documented `blastit check` path exercised for that fixture (static + app route in one manifest).
3. The docs page from MB.6 and this leg agree: any command or claim the leg proves must exist verbatim in the page, and vice versa (the static driver already reads the page; extend that pattern or cite the stronger of the two).
4. No protected test edited; no product code change unless the leg finds a real defect, in which case stop and report it as its own node.

# Notes

- Do not duplicate the SPA leg: this is the "app + static in one routes table" shape.
- Keep it fast (the whole e2e gate is ~30-60 s today) and Docker-only, like its neighbours.
- The PHP equivalent belongs to MB.3 once embedded PHP exists (`[route.php]`, ADR-0010); note it there rather than faking it here.

# Provenance

Re-filed from `MB.8` at the `v0.2.0` gate: combined Django `collectstatic` + WSGI container leg in `e2e/` (`make e2e` already covers static mounts, Django WSGI, and PHP+static `public/`).
