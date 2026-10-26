# Fit it to your infra

blastit is configured entirely through environment variables, plus one manifest
file. There is nothing to install at the edge, nothing to configure in a control
plane, and no configuration file to mount.

Most deployments, though, start from a named set of defaults.

## Picking a preset

Two deployments were measured end to end, and each has a name:

| Preset | For | Privilege mode it assumes | Who can reach the port |
|---|---|---|---|
| [`digitalocean-paas`](digitalocean-paas.md) | DigitalOcean App Platform | `root` | nobody untrusted: DO's load balancer and the app's own internal services |
| [`compose-traefik`](compose-traefik.md) | Docker Compose behind Traefik v3 | `capabilities` (rootless) | nobody untrusted: the containers on the app's own compose network |

```console
$ blastit --preset digitalocean-paas     # DigitalOcean App Platform
$ blastit --preset compose-traefik       # Docker Compose behind Traefik
```

`blastit --help` lists them with a one-line description:

```text
presets (`--preset NAME`, or `BLASTIT_PRESET=NAME`; every BLASTIT_* variable that is set wins over one):
  digitalocean-paas: DigitalOcean App Platform: root, do-connecting-ip, every peer trusted
  compose-traefik: Docker Compose behind Traefik: rootless, X-Forwarded-For, every peer trusted
```

**Anything else** — Kubernetes, a plain VM, Docker without Traefik, nginx in
front — is a combination of the variables in
[the settings reference](../reference/settings.md). Start with
[the parameters, and how they articulate](anything-else.md).

## What a preset is

A preset is a set of **defaults**, not a mode. Precedence is: the built-in
default, then the preset, then every `BLASTIT_*` variable that is **set** (and
`PORT`).

!!! note "Set is not the same as non-empty"

    `BLASTIT_TRUSTED_PROXIES=` trusts nobody, which is different from leaving
    it unset. A variable that is set wins even when it is empty.

Because a preset never wins over an explicit setting, `BLASTIT_PRESET=NAME` is
always safe to add to a deployment that already configures blastit by hand: it
changes nothing the deployment named itself.

`--preset NAME` and `--preset=NAME` both work, and `--preset` wins over
`BLASTIT_PRESET` when the two disagree — the boot says so, rather than picking
one silently. `BLASTIT_PRESET=NAME` is for a process you do not start yourself.

The `boot` event of the `config` phase says what happened:

```json
{"event":"boot","component":"init","stage":"config","seccomp":true,"privilege":"root",
 "preset":"digitalocean-paas",
 "preset_settings":{"BLASTIT_CLIENT_IP_HEADER":"do-connecting-ip","BLASTIT_FAIR_QUEUING":"on","BLASTIT_LOG_FORMAT":"json","BLASTIT_TRUSTED_PROXIES":"*"},
 "preset_overridden":["BLASTIT_LOG_FORMAT"]}
```

`preset_settings` carries the preset's own values — never the operator's — so an
overridden setting is named and the value that won is not printed.

## The two assumptions a preset carries

A preset rests on two things about the platform it was written for, and neither
is checked for you.

**The privilege mode.** The mode is *detected*, not configured: blastit reads
its own capabilities at boot and reports `root` or `capabilities`. A boot that
contradicts the preset — root where the preset expects a rootless deployment, or
the other way round — **warns once and runs**. Root in a rootless deployment
still works; it is simply more authority than the preset asked for.

**Who can reach the socket.** This one the preset cannot check at all: blastit
cannot tell a hop it owns from a peer that reached it some other way. So keeping
the assumption true is the deployment's job, and each preset's page says exactly
what it is assuming.

Both presets here set `BLASTIT_TRUSTED_PROXIES=*`, and both mean the same thing:
*nothing untrusted can reach blastit's port*. If that is false for your
deployment — the port is published, or the network has containers you do not own
on it — then any peer can forge the client address, and the fair queue's flows
and your logs key on a value an attacker chose.

A network list is not a safer default. A list makes the resolver take the
**leftmost** entry whenever every entry of the chain is inside it, and the
leftmost entry is the one an appending proxy puts the client's own invention in.
`*` takes the **rightmost** entry instead, which is what the hop closest to
blastit added, and the one thing that hop cannot be lying about.

## Next

* **[DigitalOcean App Platform](digitalocean-paas.md)** — the app spec, the
  Dockerfile, and why trusting every peer is safe there.
* **[Docker Compose behind Traefik](compose-traefik.md)** — the compose file,
  Traefik's config, and the rootless capability recipe.
* **[Anything else](anything-else.md)** — the client-IP header and trusted
  proxies, root versus rootless, memory and process caps, fair queuing, logs,
  the uid base, and worked examples for Kubernetes, plain Docker, systemd and
  nginx in front.
