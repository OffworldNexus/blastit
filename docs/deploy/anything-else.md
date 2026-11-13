# Anywhere else

Kubernetes, a plain VM, Docker without Traefik: everything is a combination of
[the settings](../reference/settings.md). There is no mode to pick.

## The settings that matter

| Setting | Default | Set it to |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the header your ingress **overwrites**; [measure it](index.md#measure-your-ingress) |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | the addresses your ingress connects *from* |
| `BLASTIT_MAX_WORKERS` | `16` | at most the database plan's connection limit |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | the live view-process cap |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | what one view process costs (Django: about 140 MiB) |
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | the upstream cap of one proxy |
| `BLASTIT_UID_BASE` | `20000` | change it if the range collides in the container |
| `BLASTIT_VIEW_SPAWN` | `lazy` | `eager`, or `prefork = true` on hot routes |

## Root or rootless

| Mode | When you get it | What it needs |
|---|---|---|
| `root` | the container or unit starts Blastit as uid 0 | `SETUID`, `SETGID`, `CHOWN`, `FOWNER`, `DAC_OVERRIDE`, `KILL` |
| `capabilities` | anything else | those six plus `cap_setpcap`, on a [dedicated copy](compose-traefik.md#rootless) of the file init starts from |

The mode is detected, not configured: the first `boot` event says which one is in
force, and production refuses to start when it can get neither.

Pick `root` when your platform gives the container root: one less moving part.
Pick `capabilities` when a root process is not allowed. Docker's `--cap-add`
gives a non-root `user:` process **nothing**, and a platform that sets
`no_new_privs` ignores file capabilities, so the way there is a launcher that
drops the uid itself:

```console
$ setpriv --reuid=1000 --regid=1000 --init-groups \
      --inh-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      --ambient-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      blastit
```

To exercise that path on your laptop, run the [Compose + Traefik
recipe](compose-traefik.md) with rootless Docker: it needs no root and covers the
uid, capability and seccomp path end to end.

## Memory and process caps

* **`BLASTIT_MAX_WORKERS`** — request threads alive across every view. Keep it at
  or below the database plan's connection limit: each worker holds one upstream
  connection. With a pooler in front, it is the pool's client count instead.
* **`BLASTIT_MAX_VIEW_PROCESSES`** — live view processes, derived from memory at
  boot and re-derived from what a view really costs on your app. Retiring never
  fails a request: when nothing is evictable, the request waits.
  `BLASTIT_VIEW_IDLE_TIMEOUT` (`600s`) and `BLASTIT_VIEW_MAX_REQUESTS` (`0`, off)
  retire one.
* **Give the container a memory limit.** `BLASTIT_MEM_HIGH_WATERMARK` (default
  `85%` of `total`, the smaller of `MemTotal` and the cgroup limit; the
  non-reclaimable reading is what the percentage is compared against) retires
  idle processes under pressure, and a container already above it serves `503`s. Size
  `BLASTIT_MAX_BODY_BYTES` (default `100 MiB`) so the largest body × concurrent
  uploads still fits.

## Plain Docker

```console
$ docker run -d --name shop --env-file .env \
    -p 127.0.0.1:8080:8080 --memory 1g \
    --health-cmd 'python -c "import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:8080/_blastit/health\")"' \
    registry.example.com/shop:1.2.3
```

No `--user`: Docker's default set is what root mode needs. `.env` holds
`DATABASE_URL` and `BLASTIT_TRUSTED_PROXIES=172.18.0.2` (the address your ingress
connects from), so neither is in your shell history; the health check uses
`python` because the slim image has no `curl`.

## systemd

```ini title="/etc/systemd/system/shop.service"
[Unit]
Description=shop (blastit)
After=network-online.target

[Service]
User=shop
Group=shop
WorkingDirectory=/srv/shop
EnvironmentFile=/etc/shop/blastit.env
ExecStart=/srv/shop/.venv/bin/blastit
Restart=always

# The six capabilities the boot needs, plus the one that empties the children's
# bounding set — as *ambient* capabilities, so no file carries them.
AmbientCapabilities=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_KILL CAP_SETPCAP
NoNewPrivileges=true

# /run/blastit, owned by User=, which a rootless init cannot make itself.
RuntimeDirectory=blastit
RuntimeDirectoryMode=0700
ReadWritePaths=/run/blastit
ProtectSystem=strict

[Install]
WantedBy=multi-user.target
```

```ini title="/etc/shop/blastit.env"
DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop
DJANGO_SECRET_KEY=…
PORT=8080
BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1
```

`WorkingDirectory` is where Blastit finds `./blastit.toml`. `User=shop` must not
be a uid the layout hands out. The environment file holds secrets, so mode
`0600`, owned by the service user.

## Kubernetes

Blastit is one container and wants to be PID 1 in it, which is the default; the
part to decide is `securityContext`.

```yaml title="deployment.yaml"
apiVersion: apps/v1
kind: Deployment
metadata: { name: shop }
spec:
  replicas: 2
  selector: { matchLabels: { app: shop } }
  template:
    metadata: { labels: { app: shop } }
    spec:
      containers:
        - name: shop
          image: registry.example.com/shop:1.2.3
          ports: [{ name: http, containerPort: 8080 }]
          env:
            - name: DATABASE_URL
              valueFrom: { secretKeyRef: { name: shop-db, key: url } }
            # Only the ingress may reach this port; the NetworkPolicy below
            # enforces it, so every peer that does arrive is trusted.
            - name: BLASTIT_TRUSTED_PROXIES
              value: "*"
          readinessProbe:
            httpGet: { path: /_blastit/health, port: http }
            initialDelaySeconds: 15
          livenessProbe:
            httpGet: { path: /_blastit/health, port: http }
          securityContext:
            runAsUser: 0
            capabilities:
              drop: ["ALL"]
              add: ["SETUID", "SETGID", "CHOWN", "FOWNER", "DAC_OVERRIDE", "KILL", "SETPCAP"]
          resources:
            limits: { memory: 1Gi }
```

```yaml title="networkpolicy.yaml"
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: shop }
spec:
  podSelector: { matchLabels: { app: shop } }
  ingress:
    # Name your ingress controller's namespace: without this policy every pod
    # in the cluster can reach :8080 and forge the client address.
    - from:
        - namespaceSelector:
            matchLabels: { kubernetes.io/metadata.name: ingress-nginx }
      ports: [{ protocol: TCP, port: 8080 }]
```

The scheduler answers the health path itself, so a probe does not depend on any
view. `resources.limits.memory` becomes `total` for the derived cap; under the
`restricted` Pod Security Standard Blastit does not run.
