---
id: MB.2.11
title: "Default the deploy presets to a connection pooler (DO managed pooler, Compose PgBouncer)"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths: ["docs/deploy/**", "docs/research/**", "docs/reference/**", "e2e/**", "docs/decisions/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T22:17"
updated: "2026-11-13T23:44"
---

# Goal

Make the two deployment presets assume a **connection pooler by default** instead of low per-route connection limits: DigitalOcean's managed pooler (a built-in feature, just enable it) and a PgBouncer service in the Compose/Traefik recipe. Update the presets, the pages and any preset assertions so the documentation matches what a developer actually deploys.

Owner's direction (2026-11-13):
> "In the DigitalOcean example as well as in the Traefik one, the idea is to use a pooler instead of having some kind of low limit. It's a default feature DigitalOcean so it just needs to be enabled. And on the docker compose example, add a pgBounce or something in the mix. Make the presets and documentation match this assumption."

# Context

Today both pages wire the app straight to the database (`db:5432` in Compose, direct database URLs in the App Platform spec), so the docs implicitly assume the app's own connection count is the limit. Pooling changes three things that must be documented rather than assumed:

1. **Which URL each role uses.** The app connects to the **pooler**; migrations connect **directly**, because DDL and session features do not survive transaction pooling. Two URLs, stated explicitly for both presets.
2. **Prepared statements.** `psycopg` 3 (and therefore Django/Flask/SQLAlchemy) prepares a statement after a few executions. PgBouncer in transaction mode needs `max_prepared_statements` (>= 1.21) or the app fails with "prepared statement _pg3_N already exists". DigitalOcean's managed pooler has its own documented limitations to cite, not guess.
3. **Session state, including the one we just shipped a fix for.** `SET`/`set_config('TimeZone', …)` is per-session and is not reliably preserved across pooled transactions — Django sets it at connect. The honest fix is a startup parameter (`options=-c timezone=UTC`) that the pooler passes through, and that changes whether the `[group.connection.db]` grant documented in `docs/deploy/index.md` is still needed (see MB.1.6/MA.8). Verify with the pooler in place instead of leaving a grant that no longer fires (or a statement that now fails).

Also keep: Blastit's dbproxy still owns authentication and per-route allowlists, and the pooler sits **between blastit and Postgres**. Presets must still fail closed without capabilities/seccomp, and connection counts are now the pooler's job — document the small per-route pool sizes blastit opens and why that is safe now.

# Acceptance

1. **Research first, with citations** (`docs/research/connection-pooling.md`): DigitalOcean managed pooler (how to enable it in the console/`doctl`, the pooler host/port shape, supported/unsupported features, TLS), PgBouncer (`pool_mode=transaction`, `max_prepared_statements`, `default_pool_size`, `auth_type`, `userlist`/`auth_query`, healthcheck), and what psycopg 3 / Django need. Every claim cites a URL. No pooler advice without a source.
2. **DigitalOcean preset + page.** The App Platform spec shows the app `DATABASE_URL` pointing at the pooler and the migrator using the direct URL, with the pooler step as one explicit instruction (and `doctl` equivalent if it exists). The page stays lean (<= 150 lines) and leads with the working recipe.
3. **Compose/Traefik preset + page.** A `pgbouncer` service added next to `db` (pinned image tag, transaction pooling, `max_prepared_statements`, healthcheck, `depends_on` with condition, secrets from the environment like the rest of the file), with the app pointing at it and the migrator still direct. The stack stays honest: it is statically checked, not run.
4. **Runnable/static fences stay green**: every command and snippet in both pages is registered in the runnable-docs inventory and passes `make docs-test` / `make docs-check` (38 fences currently: 26 executed / 12 accounted / 0 failed).
5. **Preset assertions updated** where they exist (`e2e/` deployment checks, `docs/reference/*` tables, `ADR-0005` if the preset contract changes): `blastit serve --preset digitalocean-paas|compose-traefik` still fails closed without capabilities, and any documented `[group.connection.db]` / timezone advice matches what actually happens behind the pooler.
6. `make check` exit 0; `make e2e` exit 0 if any deployment fixture changed; `dx` + `ai-ergonomics` critic pass on both pages (the DX here is "one extra service you must not forget, and the two URLs you must not swap").

# Notes

- The trap to avoid: a page that says "use a pooler" while showing a single `DATABASE_URL`, or a Compose file whose app still talks to `db`. The two-URL rule must be visible in the snippet, not only in prose.
- If the DO pooler turns out not to support something the current docs rely on (e.g. session settings), say so and change the preset rather than papering over it — and record the limitation in the page's short "what the pooler does not do" note.
- Keep both pages lean: the pooler explanation is 3–5 lines plus the snippet, not an architecture essay (that belongs in `ARCHITECTURE.md`).
