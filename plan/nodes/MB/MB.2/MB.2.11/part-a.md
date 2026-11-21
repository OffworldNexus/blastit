# DigitalOcean Managed PostgreSQL — connection pooler (Part A)

Sources fetched 2026-11-13 (all DigitalOcean-owned unless marked). Pooler is PgBouncer (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).

## A1. Enabling the pooler

- Console: **Databases** page → click cluster name (**Overview**) → **Connection Pools** tab → **Create a Pool** → set pool name, database, user, mode, size → **Create Pool** (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- There is **no cluster-wide "pooler: true" switch**: pooling is enabled by creating a pool *resource*, and "Once you've created a connection pool, you must use its specific information to force clients to connect to it" (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- **doctl CAN enable it**: `doctl databases pool create <database-cluster-id> <pool-name> --db <db> --size <n> [--mode transaction|session|statement] [--user <u>]`; `--db` and `--size` are required, `--mode` defaults to `transaction` (https://docs.digitalocean.com/reference/doctl/reference/databases/pool/create/).
- API equivalent: `POST /v2/databases/{database_cluster_uuid}/pools` (anchor `#databases_add_connectionPool`, https://docs.digitalocean.com/reference/api/reference/databases/), body `{"name","mode","size","db","user"}` (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- `doctl databases connection <cluster-id>` returns only the *cluster* connection (URI/Database/Host/Port/User/Password/SSL); it has no pool flag — use `doctl databases pool list <cluster-id> --format Name,URI` for the pool URI (https://docs.digitalocean.com/reference/doctl/reference/databases/connection/, https://docs.digitalocean.com/reference/doctl/reference/databases/pool/list/).

## A2. Connection details shape

- Each pool gets its own `connection` object: `{"database":"backend-pool","host":"db.example.com","password":"...","port":25061,"ssl":true,"uri":"postgres://doadmin:...@db.example.com:25061/backend-pool?sslmode=require","user":"doadmin"}` (https://docs.digitalocean.com/reference/api/reference/databases/#databases_list_connectionPools).
- Ports confirmed from docs: direct cluster = `25060`, pool = `25061` (cluster example `postgres://doadmin:...@db.example.com:25060/defaultdb?sslmode=require`, pool example `...:25061/...`) (https://docs.digitalocean.com/reference/api/reference/databases/#databases_get_cluster, https://docs.digitalocean.com/reference/api/reference/databases/#databases_list_connectionPools).
- **Hostname**: DO's API examples reuse the cluster host (`db.example.com`) for both ports and only change port + database name; docs do not otherwise describe pool hostname shape → treat as "verify per cluster" (https://docs.digitalocean.com/reference/api/reference/databases/#databases_list_connectionPools).
- **Database field trap**: in the pool URI the *database* segment is the **pool name**, not the target DB; the target DB is the pool's `db` field (`"db":"defaultdb"`, `"database":"backend-pool"`). So `psql -d <pool-name>`, and ORMs must not force the real dbname (https://docs.digitalocean.com/reference/api/reference/databases/#databases_list_connectionPools).
- Console exposes per-pool details under **Connection details** in three formats: Connection parameters, Connection string, Flags (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- App Platform bindables: `${db.HOSTNAME}`, `.PORT`, `.USERNAME`, `.PASSWORD`, `.DATABASE`, `.DATABASE_URL`, `.DATABASE_PRIVATE_URL`, `.JDBC_DATABASE_URL`, `.CA_CERT`; `DATABASE_URL` is the public string, `DATABASE_PRIVATE_URL` only works in the same VPC (https://docs.digitalocean.com/products/app-platform/how-to/use-environment-variables/).
- Pool-specific bindables use a **three-part** name: `(PostgreSQL Only) If your database uses connection pools, you can reference pool-specific variables using the syntax ${<database component name>.<pool name>.<VARIABLE_NAME>}. For example, ${mydb.mypool.DATABASE_URL}` (https://docs.digitalocean.com/products/app-platform/how-to/use-environment-variables/); worked example with `${example-db.DATABASE_URL}` vs `${your_database_name.your_database_connection_pool_name.DATABASE_URL}` (https://docs.digitalocean.com/products/app-platform/how-to/connect-pg-pools/).

## A3. Pooler mode and size

- Modes: transaction, session, statement; "Transaction mode is the most common option and is selected for you by default. This setting cannot be modified after creation" (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- Discrepancy to note: `doctl databases pool update` still exposes `--mode` and API `PUT /pools/{pool_name}` requires `mode` in the body — treat mode as effectively immutable and re-create the pool to change it (https://docs.digitalocean.com/reference/doctl/reference/databases/pool/update/, https://docs.digitalocean.com/reference/api/reference/databases/#databases_update_connectionPool). Pool name is likewise immutable: "it must be unique and it cannot be edited once the pool is created" (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- Size: "The pool size is the minimum number of connections the pool can handle. The maximum pool size varies based on the size of the cluster"; start at ~half of available connections (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- Backend budget: "25 backend server connections are allowed for every 1GB of RAM. Three are reserved for maintenance" (1 GiB → 22, 4 GiB → 97), "shared across all connection pools in a cluster" (https://docs.digitalocean.com/reference/api/reference/databases/#databases_add_connectionPool); per-plan table in https://docs.digitalocean.com/products/databases/postgresql/details/limits/. Caps: "21 PgBouncer pools and up to 1,000 connections, depending on the plan size" (same limits page).
- Advanced Edition clusters are *always* pooler-fronted (a platform pooler routes to the current primary) — this is separate from user-created pools and does not change the 25060/25061 split (https://docs.digitalocean.com/products/databases/postgresql/concepts/advanced-edition/).

## A4. TLS

- "Managed PostgreSQL clusters **require** TLS for all connections"; connection details return `ssl: true` and a URI with `sslmode=require` (https://docs.digitalocean.com/products/databases/postgresql/how-to/secure/).
- Pool connection object carries `ssl: true` and `?sslmode=require`, so `sslmode=require` works on 25061 (https://docs.digitalocean.com/reference/api/reference/databases/#databases_list_connectionPools).
- CA: Standard Edition clusters ship one downloadable CA cert used via `sslrootcert` for `verify-ca`/`verify-full`; Advanced Edition uses the system trust store (`sslrootcert=system`, libpq ≥ 16) (https://docs.digitalocean.com/products/databases/postgresql/how-to/connect/).
- Pool docs only point at the generic sslmode section for verification and show no pool-specific CA/verify-full string, so "same CA file on the pool port" is **not explicitly documented** (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/#connecting-to-the-pool).

## A5. Documented limitations (verbatim)

- "Using `pg_dump` on a connection pool causes errors when `pool_mode` is set to `transaction`. We recommend you use `pg_dump` directly against your database cluster, bypassing its connection pool(s)." (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/)
- Transaction mode: "clients must not use any session-based features, since each transaction ends up in a different connection and thus gets a different session state" (upstream: https://www.pgbouncer.org/config.html) — DO's wording: session mode "is useful when your application uses prepared statements, advisory locks, listen/notify, or other features which operate on a session level rather than a transaction level" (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- Statement mode: "only allows a single statement at a time… queries with multiple statements are not allowed and will fail" (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).
- `server_reset_query_always` (runs `DISCARD ALL` in all modes) is a cluster config knob, default false (https://docs.digitalocean.com/reference/api/reference/databases/#databases_patch_config). DDL/migrations: DO gives no rule beyond the pg_dump warning; the documented safe pattern is session mode for session-level features and going direct for dump/restore (https://docs.digitalocean.com/products/databases/postgresql/how-to/manage-connection-pools/).

## A6. Startup parameters (`options=-c timezone=UTC`)

- No DO page documents pooler startup-parameter handling; a docs search for pgbouncer + startup parameters returns only the pool how-to (https://docs.mcp.digitalocean.com/v1/search?q=pgbouncer+pooler+startup+parameters).
- The cluster config schema exposes `pgbouncer.ignore_startup_parameters` with an enum of **only** `extra_float_digits` and `search_path` ("List of parameters to ignore when given in startup packet") — `options` is not offered (https://docs.digitalocean.com/reference/api/reference/databases/#databases_patch_config).
- Upstream PgBouncer: unlisted startup params "will raise an error"; adding `options` itself to `ignore_startup_parameters` makes unknown params inside `options` ignored (https://www.pgbouncer.org/config.html). Consequence: passing `options=-c timezone=UTC` through DO's pooler is **expected to fail or be dropped, unverified**.
- Portable alternative, fully documented: set the cluster parameter instead — `doctl databases configuration update <db-id> --engine postgres --config-json '{"timezone":"UTC"}'` (config schema has `timezone` example `Europe/Helsinki`) (https://docs.digitalocean.com/reference/doctl/reference/databases/configuration/update/, https://docs.digitalocean.com/reference/api/reference/databases/#databases_patch_config).
- One concrete test a developer must run: `PGOPTIONS='-c timezone=UTC' psql "postgres://…@<host>:25061/<pool-name>?sslmode=require" -c 'SHOW TimeZone;'` — expect either `UTC` (passed through) or a startup-parameter error (rejected); repeat on 25060 to confirm the baseline.

## Unverified / could not confirm

- PgBouncer **version** DO runs: not stated on any docs page (pool how-to, limits, advanced-edition, API reference all omit it; version search returns nothing relevant — https://docs.mcp.digitalocean.com/v1/search?q=PgBouncer+version+prepared+statements).
- `max_prepared_statements`: not documented for DO; upstream default is 200 with 0 = disabled, so support is version-dependent (https://www.pgbouncer.org/config.html). Protocol-level prepared statements in *transaction* mode are therefore unverified — must be tested per cluster.
- Whether `options` is dropped or errors on 25061 (see A6 test); whether pool hostnames always equal the cluster hostname (examples only); whether the pool endpoint supports `verify-full` with the Standard Edition CA file (pool URI examples only show `require`).

## Developer Experience Implications

- Out-of-the-box: a cluster has no pool; one `doctl databases pool create <id> <name> --db <db> --size <n>` yields a working endpoint on `:25061` with TLS required and `sslmode=require` already in the URI — no PgBouncer config to write.
- The single highest-friction default: the pool URI's *database* is the pool name, so any framework that templates `dbname` from config (Django `DATABASES.NAME`, Rails `database:`) must use the pool name or nothing connects. Blastit should derive `dbname` from the URI, never re-inject the manifest's database name.
- Second friction: default transaction mode silently breaks prepared statements, advisory locks, LISTEN/NOTIFY, temp tables and `pg_dump`. A 12-factor developer should get one manifest line (`pool_mode = "session"`) and a startup warning if their driver sets `prepare_threshold>0` / server-side prepared statements while on a transaction pool.
- App Platform is one line (`${db.pool.DATABASE_URL}`); Blastit should mirror that with a single pooled-URL key and default to the VPC/private variant when the app and cluster share a VPC.
- Timezone/session GUCs belong in cluster config (`--config-json '{"timezone":"UTC"}'`), not libpq `options`, because the pooler's `ignore_startup_parameters` enum excludes `options` (https://docs.digitalocean.com/reference/doctl/reference/databases/configuration/update/, https://docs.digitalocean.com/reference/api/reference/databases/#databases_patch_config).
