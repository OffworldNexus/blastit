# Fit it to your infra

blastit is configured by environment variables and one manifest file. Two
platforms have a preset; everything else is [the settings](../reference/settings.md).

## Picking a preset

| Preset | For | Privilege mode | Who can reach the port |
|---|---|---|---|
| [`digitalocean-paas`](digitalocean-paas.md) | DigitalOcean App Platform | `root` | DO's edge and its own services |
| [`compose-traefik`](compose-traefik.md) | Docker Compose behind Traefik v3 | `capabilities` (rootless) | the app's own compose network |

```console
$ blastit --preset digitalocean-paas
$ BLASTIT_PRESET=compose-traefik blastit
```

## What a preset is

A preset is a set of **defaults**, not a mode: built-in default < preset < every
`BLASTIT_*` variable that is *set* (and `PORT`). A set variable wins even when it
is empty — bare `BLASTIT_TRUSTED_PROXIES=` trusts nobody.

`--preset NAME` and `--preset=NAME` both work, and either wins over the variable.
The `boot` event names the preset in force, the settings it supplied
(`preset_settings`) and the ones you overrode (`preset_overridden`).

Two things it assumes, neither checked for you.

**The privilege mode.** blastit detects whether it is root or holds a small
capability set, and the first `boot` event says which. A contradicting preset
warns and runs.

**Who can reach the socket.** blastit cannot tell a hop it owns from a peer that
reached it another way: keeping this one true is your job. Both presets trust
every peer (`*`), so *nothing untrusted may reach blastit's port*. If the port is
published or the network carries containers you do not own, any peer can forge
the client address.

`*` reads the rightmost chain entry: what the closest hop added. A network list
is worse — it reads the leftmost entry, the client's invention, whenever every
entry is inside it.

Next: [App Platform](digitalocean-paas.md) · [Compose + Traefik](compose-traefik.md) ·
[Anything else](anything-else.md).
