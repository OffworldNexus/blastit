# Deployments

Production Blastit is not `blastit dev`: it runs the app under real uids and
**refuses to start** without root or the capability set, and without a seccomp
filter — no silent fallback.

## Pick a preset

| Preset | For | Privilege | Who may reach the port |
|---|---|---|---|
| [`digitalocean-paas`](digitalocean-paas.md) | DigitalOcean App Platform | root | DO's edge and its own services |
| [`compose-traefik`](compose-traefik.md) | Docker Compose behind Traefik v3 | capabilities (rootless) | the app's own compose network |

```console
$ blastit --preset digitalocean-paas
$ blastit --preset compose-traefik
```

A preset only supplies defaults: any `BLASTIT_*` you set wins, even if it is
empty. The deploy files choose the preset with `BLASTIT_PRESET`;
[settings](../reference/settings.md) lists the rest.

## Two databases, two URLs

| Connects | To | DO port | Compose service | Used by |
|---|---|---|---|---|
| `app` | the **pool** | 25061 | `pgbouncer:5432` | every request |
| migrations | the **database** | 25060 | `db:5432` | the `migrate` job |
| grants | the **database** | 25060 | `db:5432` | the one-off grant |

DDL and session state do not survive transaction pooling, so only the app's
`DATABASE_URL` names the pool; migrations and grants name the database. On DO
both come from Connection Details (the pool URI's path is the pool name); the
Compose file already writes both.

### Pooler

**1. Both recipes already put the zone on the role** — DO roles default to `UTC`
and Compose's `db/grants.sql` runs the `ALTER`.

**2. Read it back as `app` on the direct port, never the pool** — open backends
keep the zone they logged in with. It must print exactly `UTC`; if it prints
anything else, set it on the role as the owner
(`psql "$OWNER_URL" -c "ALTER ROLE app SET timezone = 'UTC'"`) and read again.

```console
# DigitalOcean: the `app` user on the direct port (25060), not the pool
$ export APP_URL='postgresql://app:<password>@<host>:25060/defaultdb?sslmode=require'
$ psql "$APP_URL" -A -t -c 'SHOW TimeZone'
# Compose, from `db`: docker compose exec db psql -U app -d shop -A -t -c 'SHOW TimeZone'
```

**3. Changed a live role? Restart the pooler** — `docker compose restart
pgbouncer` drops in-flight transactions; DO creates the pool after the role.

Both recipes set the zone before your first `uv run blastit check`, so on a
database that says `UTC` the `connection` group's grant for
`set_config('TimeZone', …)` is a `dead_grant`: remove it there, and keep it only
where `check` runs against a database that says otherwise
([CI](../ci.md#what-it-fails-on)).

## What a preset cannot do for you

1. **Keep untrusted peers off the port.** Both presets trust every peer (`*`): on
   Compose publish no port and keep foreign containers off its network; on App
   Platform only DigitalOcean reaches it ([client address](../reference/settings.md#client-address)).
2. **Size the pool.** Each worker holds one upstream connection
   (`BLASTIT_MAX_WORKERS`, 16 by default), so workers × instances is the client
   count the pool must accept, and a managed pool's `size` is its minimum *server*
   connections ([pooling](../reference/settings.md#behind-a-connection-pooler)).
3. **Give the database a non-owner role.** The proxy holds the real credentials:
   keep `SUPERUSER` and `CREATEROLE` off, give migrations a role of their own, and
   narrow the recipe's broad backstop to one `GRANT` per table and verb after each
   migration ([DO](digitalocean-paas.md#cluster-roles-and-the-pooler),
   [Compose](compose-traefik.md#compose)).
4. **Read the ingress decision once it is live.** A header name is a guess until
   one real request has been through the ingress ([below](#measure-your-ingress)).

## Check that the ingress is live { #measure-your-ingress }

**1. Send one request to a routed route** (here `/products/`) carrying the header
the preset reads:

```console
# Compose: the preset reads the rightmost X-Forwarded-For
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/products/
# DigitalOcean: the edge overwrites do-connecting-ip with the real address
$ curl -sS -H 'do-connecting-ip: 6.6.6.6' https://myshop.ondigitalocean.app/products/
```

**2. Read `remote_addr` in the `request` event** — both presets log JSON, and the
sample is trimmed (`ts`, `level`, `component` and `request_id` elided):

```console
$ docker compose logs app | grep '"event":"request"' | tail -1    # Compose
$ doctl apps logs <app-id> --type run | grep '"event":"request"' | tail -1  # DO
```

```json
{"event":"request","method":"GET","path":"/products/","route":"catalog","status":200,"remote_addr":"203.0.113.7"}
```

It must be your public address, not `6.6.6.6` or a `10.x`/`172.x` container
address. If `6.6.6.6` appears the ingress trusted a client-supplied entry:
`BLASTIT_CLIENT_IP_HEADER` must name the header your edge **sets** —
`do-connecting-ip` on DO; on Compose keep `x-forwarded-for`
([client address](../reference/settings.md#client-address)).

## Before the first deploy

- [ ] `uv run blastit check` is clean.
- [ ] The first (`stage0`) `boot` event reports the `privilege` you expected; the
      second (`config`) event reports the `preset` you asked for (`preset` is
      `null` on `stage0`).
- [ ] The health path (`/_blastit/health`) answers.

## Where next

[App Platform](digitalocean-paas.md) · [Compose + Traefik](compose-traefik.md) ·
[Anywhere else](anything-else.md) · [Fair queuing](fair-queuing.md).
