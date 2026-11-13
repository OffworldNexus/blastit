# Result MB.2.11

## Summary

Both deployment presets now assume a connection pooler. `digitalocean-paas`
shows the app on the pool's bindable (**25061**) and `migrate` on the cluster's
literal **25060** URL as `migrator`; `compose-traefik` adds a pinned `pgbouncer`
service (transaction mode, `max_prepared_statements`, SCRAM, health gate) with
the app on `pgbouncer:5432` and `migrate`/`grant` on `db:5432`. The session zone
is a **role default** (`ALTER ROLE app SET timezone = 'UTC'`), because the URL
cannot carry `options` and a transaction pool keeps no session state; the
`connection` group grant is then dead where the database says `UTC` and needed
where it says another spelling — derived from `test_check.py` and `audit_v1.rs`,
not memory. `e2e/docs_deploy.py` pins the documented wiring (41 promises) and
runs before Docker in `e2e/run.py`.

## Evidence

### 1. Research first, with citations

`docs/research/connection-pooling.md` (106 lines, 53 cited URLs) + INDEX entry.
Facts used: DO pool `:25061` vs direct `:25060`, pool URI names the pool,
`doctl databases pool create`, DO transaction-mode caveats; PgBouncer
`max_prepared_statements` since 1.21.0 (default 200 since 1.24.0), psycopg wants
≥ 1.22 + libpq ≥ 17; `edoburu/pgbouncer:v1.26.1-p0` listens on **5432** with no
shipped healthcheck; Django 5.2 sets `prepare_threshold=None` and compares the
startup `TimeZone` with the constant `UTC`.

```console
$ uv run --no-project e2e/docs_deploy.py | tail -1
docs_deploy: 41/41 documented pooler promises hold
```

### 2. DigitalOcean preset + page (≤ 150 lines)

```console
$ wc -l docs/deploy/digitalocean-paas.md
150 docs/deploy/digitalocean-paas.md
$ grep -c 'shop-pool\|25060' docs/deploy/digitalocean-paas.md
7
```

The two URLs are in the spec: `web` reads `${db.shop-pool.DATABASE_URL}` (25061);
the `migrate` job carries
`postgresql://migrator:…@…:25060/defaultdb?sslmode=require` (direct, DDL role).
The pool step is one instruction (console, name `shop-pool`) with the `doctl`
equivalent; `app` is a platform-managed user so the pool has a password the
platform knows.

### 3. Compose/Traefik preset + page

```console
$ grep -c '@pgbouncer:5432' docs/deploy/compose-traefik.md
1
$ grep -c '@db:5432' docs/deploy/compose-traefik.md
2
$ sed -n '86,103p' docs/deploy/compose-traefik.md
  pgbouncer:
    image: edoburu/pgbouncer:v1.26.1-p0
    environment:
      DB_HOST: db
      DB_USER: app
      DB_PASSWORD: ${DB_PASSWORD:?}
      DB_NAME: shop
      # md5 (the image default) fails against PostgreSQL 18's SCRAM passwords.
      AUTH_TYPE: scram-sha-256
      POOL_MODE: transaction
      MAX_PREPARED_STATEMENTS: "200"
      DEFAULT_POOL_SIZE: "20"
      MAX_CLIENT_CONN: "100"
    healthcheck:
      test: ["CMD", "pg_isready", "-h", "127.0.0.1", "-p", "5432", "-U", "app", "-d", "shop"]
      retries: 60
    depends_on:
      db: { condition: service_healthy }
```

The app `depends_on` the pool's health, the pool waits for `db`, the password
comes from `${DB_PASSWORD:?}` like the rest of the file, and `db/grants.sql`
sets the role's zone. The stack is static (`make docs-test` parses the YAML; no
Docker in the gate).

### 4. Runnable/static fences stay green

```console
$ make docs-unit
63 passed, 57 subtests passed in 0.35s

$ make docs-check
Build started
No issues found
Build finished in 0.20s

$ make docs-test 2>&1 | tail -1
47 fences: 28 executed, 19 accounted, 0 failed · 21.1s

$ uv run --no-project python python/tests/runnable_docs/run.py | grep 'deploy/index.md'
PASS  docs/deploy/index.md  fence 1  presets   each preset exists and fails closed without capabilities
PASS  docs/deploy/index.md  fence 2  static    statically checked every command, flag and snippet (… the psql flags are checked against the installed binary …)
PASS  docs/deploy/index.md  fence 3  example   accounted: Measures X-Forwarded-For through a real load balancer …
```

`docs/deploy/index.md` fence 2 is the new role-default console fence
(`psql "$POOLER_URL" -c "ALTER ROLE app SET timezone = 'UTC'"` plus the
`SHOW TimeZone` probe); it is registered in `inventory.toml` as `static`. The SQL
snippet in `db/grants.sql` and the DO heredoc are parsed by the same `static`
fences (the harness checks every `sql` block a page shows).

### 5. Preset assertions

```console
$ make docs-test 2>&1 | tail -30
PASS  docs/deploy/digitalocean-paas.md  fence 1  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/deploy/compose-traefik.md  fence 1  presets               0.0s  each preset exists and fails closed without capabilities
```

`docs/reference/manifest.md` documents the URL-parameter allowlist (`options`
refused — unchanged product behaviour), `docs/reference/settings.md` says how to
size the proxy cap behind a pooler, and `e2e/docs_deploy.py` asserts the
two-URL rule, the direct migrate URL/role, the pool name/user, `ALLOWED_HOST`,
the `.env`, and the grant-SQL block. ADR-0005 is unchanged: the presets' own
setting table and the seccomp/privilege contract did not change, only the
deployment wiring the docs assume.

### The three traps, answered

1. **Two URLs.** DO: `${db.shop-pool.DATABASE_URL}` (25061) for the app, a
   literal `…:25060/…` as `migrator` for `migrate`; Compose: `@pgbouncer:5432`
   once for the app and `@db:5432` twice (`migrate`, `grant`). Asserted by
   `e2e/docs_deploy.py` (one pooler URL, two direct ones, the direct URL's role
   and port). Source: DO connection-pool API (`:25060`/`:25061`, pool name as
   the URI's database), `manage-connection-pools`.
2. **Prepared statements.** Django 5.2's PostgreSQL backend sets
   `prepare_threshold=None`, so the documented app prepares nothing; a raw
   psycopg 3 client (threshold 5) needs PgBouncer ≥ 1.22 + libpq ≥ 17 with
   `max_prepared_statements > 0` (set to `200`) or `prepare_threshold=None`.
   DO's transaction pool is documented as *not* the one for prepared statements
   (session mode is), so the DO page says a raw psycopg app needs a session
   pool. Sources: Django `postgresql/base.py`, psycopg `prepare.html`,
   PgBouncer changelog/config, DO pool how-to.
3. **Session state.** `options=-c timezone=UTC` is not available to a Blastit
   app: `blastit-env`'s `ALLOWED_PARAMETERS` refuses `options` in the input URL,
   and `blastit-dbproxy`'s `ALLOWED_STARTUP_PARAMETERS` refuses it in the client
   startup packet (the allowlist is correct and stays). A transaction pool also
   does not preserve `set_config`. So the zone is the **role default**
   (`ALTER ROLE app SET timezone = 'UTC'`), applied once: PostgreSQL reports it
   at startup, Django sends no statement, and `check`-time dead-grant rules
   follow. See "Timezone ruling".

## Timezone ruling (owner, 2026-11-13)

The owner ruled: keep the `DATABASE_URL` parameter allowlist closed (`options`
stays refused — it would let a URL set arbitrary runtime parameters), and make
the deployment answer a role (or database) default applied once.

What changed, per page:

* **`docs/deploy/index.md`** — the Pooler section now shows
  `psql "$POOLER_URL" -c "ALTER ROLE app SET timezone = 'UTC'"`, then the
  `SHOW TimeZone` probe, then the rule: set it **before** `blastit check`;
  a `connection` group granting `set_config('TimeZone', …)` is a `dead_grant`
  where the database says `UTC` (an audit error) and is required where it says
  another spelling (a fresh `postgres:18` says `Etc/UTC`, which is why
  `docs/ci.md` teaches the group).
* **`docs/deploy/digitalocean-paas.md`** — `ALTER ROLE app SET timezone = 'UTC'`
  is in the role SQL (the `app` role is a platform-managed user, so the pool has
  a password the platform knows); the prose states the same dead/live rule.
* **`docs/deploy/compose-traefik.md`** — `ALTER ROLE app SET timezone = 'UTC'`
  is in `db/grants.sql` (the server-level `-c timezone=UTC` was removed so both
  recipes use the same lever); the prose states the same rule.

Evidence (code, not memory):

* `python/tests/test_check.py::_django_manifest()` connects with psycopg, reads
  `connection.info.parameter_status("TimeZone")`, and returns `MANIFEST`
  (no connection group) only when it equals `"UTC"`; otherwise `DJANGO_MANIFEST`
  (with `[group.connection.db] queries = ["SELECT set_config('TimeZone', $1,
  false)"]`). Its comment says a grant for a statement the server never sends is
  a `dead_grant` error, "not extra safety".
* `crates/blastit/src/audit_v1.rs::dead_group_grants` pushes
  `Finding::DeadGrant { severity: Error, group: Some(name), … }` for a group
  query no exercised route matched.
* `docs/ci.md` keeps the `add_group` story for CI, where the database reports
  `Etc/UTC`; the deploy pages now say when that grant applies instead of
  removing it blindly.

Nothing in `crates/**` was touched; the allowlist is unchanged.

## Attempt 2 review fixes

1. **Research note aligned with the shipped answer** (`docs/research/connection-pooling.md`):
   the recommendations no longer keep `${db.DATABASE_URL}` (the `doadmin` owner
   URL) for the migrator nor `options=-c timezone=UTC` as belt-and-braces. They
   now say: app -> pool bindable, migrate -> a literal
   `postgresql://migrator:…@<host>:25060/defaultdb?sslmode=require`, zone ->
   `ALTER ROLE app SET timezone = 'UTC'` with PostgreSQL `ALTER ROLE` and
   ParameterStatus citations; the `options` line is deleted and the note records
   that `crates/blastit-env`'s allowlist refuses it.
2. **Compose prose shrunk**: the 27-line post-snippet block (sizing, prepared
   statements, TLS) moved to `docs/reference/settings.md#behind-a-connection-pooler`;
   the deploy page keeps two short paragraphs (two destinations + `ALLOWED_HOST`,
   then the zone/dead-grant rule and one TLS pointer). The page is 255 lines, not
   150, because the full seven-service compose block (~100 lines), Traefik's
   dynamic config (22) and the image Dockerfile (24) are the recipe itself; only
   the prose was reducible, and it now runs ~9 lines around the snippet.
3. **DO pool-mode limitation cited**:
   `digitalocean-paas.md` links "pool modes" to DO's connection-pools how-to, the
   same page the research note cites.
4. **`app.yaml` secrets**: the DO fill paragraph now says `app.yaml` holds
   secrets and must stay out of git, matching the Compose `.env` warning.
5. **`ai-ergonomics` artefact** rewritten as a full final pass (verified
   promises with file:line evidence, residual MINORs, `VERDICT: PASS`), not a
   six-line note.
6. **Zone probe on the direct port** (`Z1`): a pool's open backends keep the
   zone they logged in with, so a pooled `SHOW TimeZone` can answer for an old
   connection. `docs/deploy/index.md` now runs the `ALTER` as the owner and reads
   `SHOW TimeZone` on the **direct** port as `app` (`$OWNER_URL`/`$APP_URL`),
   says why (`server_lifetime`, one hour), and gives the Compose direct read with
   a `pgbouncer` restart after the change; `docs/reference/settings.md` repeats
   the recycle note. The DO recipe already sets the role before the pool exists.
7. **Minors**: the research note says "two destinations" (not "two URLs"); the
   Compose page links the `[proxy.db]` block; the DO page says `ca_cert` only
   matters with `sslmode=verify-full` and lists the job's throwaway key.
8. **The zone fence is self-contained** (final pass): it exports `OWNER_URL`
   and `APP_URL` in the fence itself, so copying it cannot fall back to `psql`
   role defaults; `e2e/docs_deploy.py` pins the export-before-use order and
   rejects an owner-role oracle (`psql "$OWNER_URL" -A -t` must not appear), and
   `inventory.toml`'s reason no longer names the retired `POOLER_URL` or says the
   pool answers.
9. **The gate pins the direct URL itself** (f2): `e2e/docs_deploy.py` now
    regex-pins `export APP_URL='postgresql://app:…:25060/defaultdb`, so a
    pool-URI `APP_URL` or an owner-role read is red (41/41). The research note's
    startup claim is qualified to "each new server session" and
    `server_lifetime` (3600 s) is cited (`connection-pooling.md:13,22,44,82`).
10. **The direct read is its own URL** (X1/m1): `APP_URL` is now exported in
   `docs/deploy/index.md` as
   `postgresql://app:<password>@<host>:25060/defaultdb?sslmode=require` — "never
   the pool's URI, whose path is the pool name" — so a port swap cannot reuse the
   pool name. `e2e/docs_deploy.py` asserts `APP_URL`/`:25060/defaultdb` and labels
   the check "direct port, not guessed"; the Compose restart has its command and
   a dropped-transaction warning; the research note's Compose example uses
   `/shop`.

## Files

- `docs/research/connection-pooling.md` (new), `docs/research/INDEX.md`
- `docs/deploy/index.md`, `docs/deploy/digitalocean-paas.md`,
  `docs/deploy/compose-traefik.md`, `docs/deploy/anything-else.md`
- `docs/reference/manifest.md`, `docs/reference/settings.md`
- `e2e/docs_deploy.py` (new), `e2e/run.py`
- `python/tests/runnable_docs/inventory.toml` (index fence 2 static; DO reasons
  3/4 swapped back)
- `docs/research/MB.2.11.review-dx.md`,
  `docs/research/MB.2.11.review-ai-ergonomics.md` (review artefacts; `docs/design/`
  is outside this node's paths)
- `plan/nodes/MB/MB.2/MB.2.11/part-a.md`, `part-b.md`, `part-c.md` (raw research
  inputs kept as evidence)

## Follow-ups

1. **Runnable-docs doctl table** (`python/tests/runnable_docs/harness.py`, out of
   lane): add `("databases", "pool", "create"|"list")`, `("databases",
   "connection")`, `("databases", "user", "create")` so the pool bootstrap can be
   checked console fences instead of prose. Until then those steps are verified
   against the doctl reference, not the gate.
2. **dbproxy prepared statements** (`crates/blastit-dbproxy`): confirm
   `Parse`/`Bind` names pass through unchanged; that decides whether
   `max_prepared_statements` is load-bearing for a raw psycopg app behind
   Blastit (it is not for Django, which sends none).
3. **DO pooler version/`options`**: DO publishes neither; the pages do not rely
   on either.
4. **e2e fixture**: `e2e/rootless/compose.yml` still connects the app straight to
   `db` (it proves the D6 capability path, not the pooler recipe). Making it
   match the docs is a new node and needs a `make e2e` run.
5. **`docs/knowledge/LESSONS.md`** (out of lane): extend the Django `TimeZone`
   bullet with the pooler half — the spelling comes from the role/server
   default, and a transaction pool drops a connect-time `set_config`.
6. **`docs/ci.md`** (out of lane): it still teaches the `connection` group for a
   fresh `postgres:18` (`Etc/UTC`), which is correct there; a note pointing at
   the deploy rule ("set the role default first, then `check`") would close the
   loop.
7. **Review minors** (`docs/deploy/**`): add `REPLACE_WITH_THROWAWAY_SECRET` to
   the DO fill list; add `-v ON_ERROR_STOP=1` to the DO role heredoc (Compose's
   `grant` already has it); name the log command (`doctl apps logs`, `docker
   compose logs`) next to the two log checks in `docs/deploy/index.md`.
8. **Runnable-docs compose table** (`python/tests/runnable_docs/harness.py`, out
   of lane): add `exec` so the Compose zone probe can be a checked fence instead
   of prose.

## Review Resolution

Artefacts live in `docs/research/` because `docs/design/` is outside this node's
paths. Pass 1 (`dx` 4 BLOCKER/6 MAJOR; `ai-ergonomics` 3/9) and pass 2
(`dx` 0/2; `ai-ergonomics` 0/6) were fully resolved; the final pass runs against
the frozen pages and the owner's ruling, and its verdict is recorded below.

Final pass on the frozen pages (attempt 2, after the five review fixes):
`dx` — **0 BLOCKER / 0 MAJOR / 3 MINOR, VERDICT: PASS**; `ai-ergonomics` —
**0 BLOCKER / 0 MAJOR / 6 MINOR, VERDICT: PASS** (both re-ran the gates). Both artefacts end with
`VERDICT: PASS`. The remaining MINORs are recorded, not blocking: the throwaway
secret is not in the DO fill list, the DO heredoc lacks `ON_ERROR_STOP=1`, the
log-retrieval command is left to the platform's docs, the DO `ALTER ROLE` line
also appears in the shared fence, and `.env` is sourced as well as passed by
`--env-file`. They are cheap follow-ups (items 7–8 below).
