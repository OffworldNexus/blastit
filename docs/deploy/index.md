# Deployments

Production Blastit is not `blastit dev`: it runs the app under real uids, and it
**refuses to start** if it cannot get root or the capability set it needs, and a
seccomp filter. There is no silent fallback.

## Pick a preset

| Preset | For | Privilege | Who may reach the port |
|---|---|---|---|
| [`digitalocean-paas`](digitalocean-paas.md) | DigitalOcean App Platform | root | DO's edge and its own services |
| [`compose-traefik`](compose-traefik.md) | Docker Compose behind Traefik v3 | capabilities (rootless) | the app's own compose network |

```console
$ blastit --preset digitalocean-paas
$ blastit --preset compose-traefik
```

A preset is defaults, not a mode: a `BLASTIT_*` variable you set still wins, even
when it is empty. `--preset` runs the production server with them; the deploy
files select the same preset with `BLASTIT_PRESET`. Everything else is in [the
settings](../reference/settings.md).

## Pooler

Both recipes put a pooler between Blastit and PostgreSQL: DigitalOcean's managed
pooler, or the `pgbouncer` service in the Compose file. Two destinations, not two
literal URLs: the app's `DATABASE_URL` points at the pool; migrations and the
one-off grant job point at the database, because DDL and session state do not
survive transaction pooling.

A transaction pool keeps no session state, and Blastit refuses `options` in a
`DATABASE_URL` ([manifest](../reference/manifest.md#protocol-postgresql)), so
set the zone where every session inherits it — on the role, once. Do it as the
owner and read it back on the **direct** port as `app`: a pool's open backends
keep the zone they logged in with, so a pooled `SHOW TimeZone` can answer for an
old connection (`server_lifetime`, one hour, recycles them). Fill both URLs from
the cluster's Connection Details; the second is the direct port and the real
database — never the pool's URI, whose path is the pool name. Compose puts the
`ALTER ROLE` in `db/grants.sql`.

```console
$ export OWNER_URL='postgresql://doadmin:<password>@<host>:25060/defaultdb?sslmode=require'
$ export APP_URL='postgresql://app:<password>@<host>:25060/defaultdb?sslmode=require'
$ psql "$OWNER_URL" -c "ALTER ROLE app SET timezone = 'UTC'"
$ psql "$APP_URL" -A -t -c 'SHOW TimeZone'
```

It must print exactly `UTC`; on Compose read it the same way with
`docker compose exec db psql -U app -d shop -A -t -c 'SHOW TimeZone'` (direct,
new session). After changing the role, restart the pooler
(`docker compose restart pgbouncer`; it drops in-flight transactions). Django
compares that startup value with the constant `UTC` (`USE_TZ`), so it then sends
no `set_config('TimeZone', …)`. Set it **before** `blastit check`: a
`connection` group granting that statement is a `dead_grant` on a database that
says `UTC` (an audit error), and required on one that says another spelling — a
fresh `postgres:18` says `Etc/UTC`, which is why
[CI](../ci.md#for-agents-json) teaches the group.

## Four things a preset cannot do for you

1. **Keep untrusted peers off the port.** Both presets trust every peer (`*`).
   App Platform reads `do-connecting-ip`, which its edge overwrites; Compose reads
   the rightmost `X-Forwarded-For`. Any peer that reaches Blastit is believed:
   publish no port, and keep containers you do not own off the network. The rules
   are in [client address](../reference/settings.md#client-address).
2. **Size the pool.** Each worker holds one upstream connection
   (`BLASTIT_MAX_WORKERS`, 16 by default), so workers × instances is the client
   count the pool must accept (`MAX_CLIENT_CONN`). A managed pool's `size` is
   its *minimum* server connections; the maximum follows the cluster's RAM
   (about 25 per GB, 3 reserved), so raise the plan when the pool's `size`
   approaches that.
3. **Give the database a non-owner role.** The proxy holds the real credentials,
   so keep `SUPERUSER` and `CREATEROLE` off and give migrations a role of their
   own. The recipes' SQL is a **broad backstop** (all four DML verbs on every
   table): the per-route grants stay the wall, and narrowing the role is one
   `GRANT` per table and verb after each migration.
4. **Read the ingress decision once it is live.** A header name is a guess until
   one real request has been through the ingress.

Migrations are a trusted-code boundary: they run unsandboxed with real
credentials, so run them as a one-off outside Blastit and pin the image.

## Measure your ingress

Deploy, then send one request carrying the header the preset reads:

```console
# Compose: the preset reads the rightmost X-Forwarded-For
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/products/
# DigitalOcean: the edge overwrites do-connecting-ip with the real address
$ curl -sS -H 'do-connecting-ip: 6.6.6.6' https://myshop.ondigitalocean.app/products/
```

Both presets log JSON; find the `request` event it produced and read
`remote_addr`:

```json
{"event":"request","method":"GET","path":"/products/","route":"catalog","status":200,"remote_addr":"203.0.113.7"}
```

On Compose, `remote_addr` must be the address you really connected from (your
public address, not a `10.x`/`172.x` container address): `6.6.6.6` never appears,
because `*` reads the rightmost entry Traefik appended. On DigitalOcean the edge
overwrote yours for the same reason. If `6.6.6.6` does appear, the ingress is
trusting a client-supplied leftmost value: read the header it really overwrites
with `BLASTIT_CLIENT_IP_HEADER`, and never list a network that can hold your
clients.

## Before the first deploy

- [ ] `uv run blastit check` is clean.
- [ ] The first (`stage0`) `boot` event reports the `privilege` you expected; the
      second (`config`) event reports the `preset` you asked for (`preset` is
      `null` on `stage0`).
- [ ] The health path (`/_blastit/health`) answers.

## Where next

[App Platform](digitalocean-paas.md) · [Compose + Traefik](compose-traefik.md) ·
[Anywhere else](anything-else.md) · [Fair queuing](fair-queuing.md).
