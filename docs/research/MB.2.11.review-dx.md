# MB.2.11 — `dx` review, verdict pass (pooler-by-default presets)

Node MB.2.11 · Lens `dx` · goldfish, fresh context · 2026-11-13 (this pass). Read
`docs/deploy/index.md` (121), `digitalocean-paas.md` (150),
`compose-traefik.md` (259), `docs/research/connection-pooling.md` (105), plus
`e2e/docs_deploy.py`, `python/tests/runnable_docs/inventory.toml` and
`docs/VISION.md`. Gates: `e2e/docs_deploy.py` -> `40/40 documented pooler
promises hold`; `make docs-test` -> `47 fences: 28 executed, 19 accounted, 0
failed`, exit 0; `make docs-check` -> "No issues found". Pages frozen; notes only.

## The fence under review (`index.md:42-47`)

```console
$ export OWNER_URL='postgresql://doadmin:<password>@<host>:25060/defaultdb?sslmode=require'
$ export APP_URL='postgresql://app:<password>@<host>:25060/defaultdb?sslmode=require'
$ psql "$OWNER_URL" -c "ALTER ROLE app SET timezone = 'UTC'"
$ psql "$APP_URL" -A -t -c 'SHOW TimeZone'
```

Self-contained and paste-safe: both URLs are exported before they are read, both
carry port `25060`, database `defaultdb`, `sslmode=require`, and the readback is
`app` on the direct port — never the owner, never the pool. `docs_deploy.py`'s
`index_checks` pins that order and rejects an owner-role `-A -t` read, so the two
traps the page names ("the direct port and the real database — never the pool's
URI, whose path is the pool name") cannot silently regress. Local stays
zero-`sudo` (the capability copy is baked into the Compose Dockerfile), CI stays
one `blastit check`, and each preset is one command plus five checklist lines.

## MINOR (non-blocking)

| id | Note |
|---|---|
| n1 | The "never the pool's URI" warning is attached only to *the second* URL (`index.md:38-39`). `OWNER_URL` must be direct too: the `ALTER ROLE` runs as `doadmin`, and the pool cannot authenticate that role. Say "both", not "the second". |
| n2 | The fence is DigitalOcean-shaped (`doadmin`, `:25060`, `defaultdb`) with no caption, and the Compose equivalent is prose *after* it (`docker compose exec db psql ...`). One caption (`# DigitalOcean: the cluster's direct port`) stops a Compose reader pasting `:25060` into a `db` container. |
| n3 | `index.md:51-52` says "restart the pooler (`docker compose restart pgbouncer`)" — Compose-only; App Platform has nothing to restart. Add one clause that the DO page's order (role `ALTER` at `digitalocean-paas.md:67`, pool creation at `:85`) is what makes DO safe; otherwise a reader waits out `server_lifetime` with no instruction. |
| n4 | `digitalocean-paas.md:27,131` still ship `ca_cert` / `DATABASE_CA_CERT` while the app URL is `sslmode=require`; the page itself says `ca_cert` "only matters with `sslmode=verify-full`". A dead knob in a copy-paste spec — drop it or say when to switch. |
| n5 | `psql` is an unstated deploy-path prerequisite (fence 2 is `static`, so the gate never proves it exists). One clause fixes it, and it is also the place to say where `APP_URL`'s `<password>` comes from (the `app` user's one-time display or the pool URI), since `OWNER_URL`'s comes from Connection Details. |

## Observation, not a finding

Two early gate runs of this pass were red and unattributed: one `make
docs-test` reported 1 failed fence, and a separate wheel-build test hit
`BadZipFile` reading `dist/`. Three subsequent `make docs-test` runs were 47/0
exit 0, and no deploy fence was involved in either red run: consistent with a
concurrent session writing `dist/`/holding a port, not a documentation defect.

Zero `BLOCKER`, zero `MAJOR`: nothing on these pages makes a developer run
`sudo` locally, hand-edit a generated file to discover a route, or guess a port
without a readback that proves it. The remaining items are one-clause polish.

VERDICT: PASS
