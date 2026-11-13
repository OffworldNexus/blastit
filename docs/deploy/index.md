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
when it is empty. Everything else is in [the settings](../reference/settings.md).

## Four things a preset cannot do for you

1. **Keep untrusted peers off the port.** Both presets trust every peer (`*`).
   App Platform reads `do-connecting-ip`, which its edge overwrites; Compose reads
   the rightmost `X-Forwarded-For`. Any peer that reaches Blastit is believed:
   publish no port, and keep containers you do not own off the network. The rules
   are in [client address](../reference/settings.md#client-address).
2. **Size the connections.** Each worker holds one upstream connection, so
   workers × instances must fit the database plan.
3. **Give the database a non-owner role.** The proxy holds the real credentials,
   so scope that role to the union of your grants and keep `SUPERUSER` and
   `CREATEROLE` off. Migrations use a role of their own.
4. **Read the ingress decision once it is live.** A header name is a guess until
   one real request has been through the ingress.

Migrations are a trusted-code boundary: they run unsandboxed with real
credentials, so run them as a one-off outside Blastit and pin the image.

## Measure your ingress

Deploy, then send one request carrying a header only you set:

```console
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/products/
```

Both presets log JSON; find the `request` event it produced and read
`remote_addr`:

```json
{"event":"request","method":"GET","path":"/products/","route":"catalog","status":200,"remote_addr":"203.0.113.7"}
```

`remote_addr` is what Blastit will believe. If it is `6.6.6.6`, the ingress
passes your header through: read another one with `BLASTIT_CLIENT_IP_HEADER`. If
it is a real client address, the preset is right. Never list a network that can
hold your clients.

## Before the first deploy

- [ ] `uv run blastit check` is clean.
- [ ] The first `boot` event reports the `privilege` you expected and the
      `preset` you asked for.
- [ ] The health path (`/_blastit/health`) answers.

## Where next

[App Platform](digitalocean-paas.md) · [Compose + Traefik](compose-traefik.md) ·
[Anywhere else](anything-else.md) · [Fair queuing](fair-queuing.md).
