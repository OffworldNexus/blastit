# Anything else

Kubernetes, a plain VM, Docker without Traefik, nginx: everything is a
combination of [the settings](../reference/settings.md).

## The settings that matter

| Setting | Default | Set it to |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | the header your ingress **overwrites**; [measure it](#working-it-out-for-your-own-ingress) |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | the addresses your ingress connects *from* |
| `BLASTIT_MAX_WORKERS` | `16` | at most the database plan's connection limit |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | the live view-process cap |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | what one view process costs (Django: about 140 MiB) |
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | the upstream cap of one proxy |
| `BLASTIT_UID_BASE` | `20000` | change it if the range collides in the container |
| `BLASTIT_FAIR_QUEUING` | `off` | `on` so one client's burst cannot starve the others |
| `BLASTIT_VIEW_SPAWN` | `lazy` | `eager`, or `prefork = true` on the hot routes |

## Client IP and trusted proxies

A peer that is not in `BLASTIT_TRUSTED_PROXIES` **is** the client, whatever its
headers say. From a trusted peer the chain is read from the right: trusted
entries are skipped, the first entry that is not trusted is the client, and if
every entry is trusted the leftmost is. So `*` is the rightmost entry — what the
hop closest to blastit added.

* An unreadable entry — `1.2.3.4:5678`, `example.com` — makes the whole header
  worthless, and the peer is the client.
* `127.0.0.1:8080` is a boot error, not an entry. An empty value trusts nobody.
* Never list a range that can hold your clients (`10/8`, the docker bridge,
  `100.64.0.0/10`): with every entry inside it, the resolver reads the leftmost
  one, which a client appended.
* `wsgi.url_scheme` follows the same rule: `X-Forwarded-Proto` is read from a
  trusted peer, and only when it is exactly `http` or `https`.

`BLASTIT_TRUST_FORWARDED` is deprecated: `1` means `*`, `0` trusts nobody, and it
warns at boot.

## Working it out for your own ingress

Add this view to your app, route it, and ask for it through the ingress:

```python title="whoami.py"
import json


def application(environ, start_response):
    body = json.dumps({
        "remote_addr": environ.get("REMOTE_ADDR"),
        "scheme": environ.get("wsgi.url_scheme"),
        "forwarded_for": environ.get("HTTP_X_FORWARDED_FOR"),
    }).encode()
    start_response("200 OK", [("Content-Type", "application/json")])
    return [body]
```

```toml title="blastit.toml"
[[route]]
name = "whoami"
path = ['/whoami']
```

```console
$ curl -sS -H 'X-Forwarded-For: 6.6.6.6' https://shop.example.com/whoami
```

If `remote_addr` comes back as `6.6.6.6`, the ingress passed your header through:
read another one.

## Root or rootless

| Mode | When you get it | What it needs |
|---|---|---|
| `root` | the container or unit starts blastit as uid 0 | the container's own set: `SETUID`, `SETGID`, `CHOWN`, `FOWNER`, `DAC_OVERRIDE`, `KILL` |
| `capabilities` | anything else | those six plus `cap_setpcap`, on a [dedicated copy](compose-traefik.md#rootless) of the file init starts from |

Pick `root` when your platform gives the container root: one less moving part.
Pick `capabilities` when a root process is not allowed — but Docker's `--cap-add`
gives a non-root `user:` process **nothing**, and a platform that sets
`no_new_privs` ignores file capabilities, so the way there is a launcher that
drops the uid itself:

```console
$ setpriv --reuid=1000 --regid=1000 --init-groups \
      --inh-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      --ambient-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
      blastit
```

The mode is detected, not configured: the first `boot` event says which one is in
force.

## Memory and process caps

* **`BLASTIT_MAX_WORKERS`** — request threads alive across every view. Keep it at
  or below the database plan's connection limit: each worker holds one upstream
  connection.
* **`BLASTIT_MAX_VIEW_PROCESSES`** — live view processes, derived from memory at
  boot and re-derived from what a view process really costs on your app (the
  `preload` `boot` event reports it). Retiring never fails a request: when
  nothing is evictable, the request waits.
* **`BLASTIT_VIEW_IDLE_TIMEOUT`** (`600s`) retires an idle view process, and
  `BLASTIT_VIEW_MAX_REQUESTS` (`0`, off) retires one after N requests — a leak
  valve at the cost of a fork per N requests.

Give the container a memory limit: `BLASTIT_MEM_HIGH_WATERMARK` (default `85%` of
it) retires idle processes under pressure.

## Kubernetes

blastit is one container and wants to be PID 1 in it, which is the default; the
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
            # The address the ingress connects from, measured with the route above.
            - name: BLASTIT_TRUSTED_PROXIES
              value: "10.244.0.0/16"
          readinessProbe:
            httpGet: { path: /_blastit/health, port: http }
            initialDelaySeconds: 15
          livenessProbe:
            httpGet: { path: /_blastit/health, port: http }
          securityContext:
            runAsUser: 0
            capabilities:
              drop: ["ALL"]
              add: ["SETUID", "SETGID", "CHOWN", "FOWNER", "DAC_OVERRIDE", "KILL"]
          resources:
            requests: { cpu: 250m, memory: 512Mi }
            limits: { memory: 1Gi }
```

The scheduler answers the health path itself, so a probe does not depend on any
view. `resources.limits.memory` becomes `total` for the derived cap; under the
`restricted` Pod Security Standard blastit does not run.

## Plain Docker

```console
$ docker run -d --name shop \
    -e DATABASE_URL='postgresql://shop:secret@db:5432/shop?sslmode=disable' \
    -e BLASTIT_TRUSTED_PROXIES=172.18.0.2 \
    -p 127.0.0.1:8080:8080 --memory 1g \
    --health-cmd 'python -c "import urllib.request;urllib.request.urlopen(\"http://127.0.0.1:8080/_blastit/health\")"' \
    registry.example.com/shop:1.2.3
```

No `--user`: Docker's default set is what root mode needs. `172.18.0.2` stands
for the address your ingress connects from; the health check uses `python`
because the slim image has no `curl`.

## systemd

Rootless, with systemd providing the capabilities and the runtime directory:

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
BLASTIT_APP=shop.wsgi:application
BLASTIT_MANIFEST=/srv/shop/blastit.toml
PORT=8080
BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1
```

`User=shop` must not be a uid the layout hands out. The environment file holds
secrets, so mode `0600`, owned by the service user.

## nginx in front

```nginx title="/etc/nginx/sites-enabled/shop"
server {
    listen 443 ssl;
    server_name shop.example.com;

    ssl_certificate     /etc/letsencrypt/live/shop.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/shop.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;

        # Stating the value instead of appending makes the chain one entry long.
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Host $host;
    }
}
```

`$proxy_add_x_forwarded_for` keeps what the client sent and appends nginx's
address after it; if you keep it, believe the rightmost entry. With nginx on this
host: `BLASTIT_CLIENT_IP_HEADER=x-forwarded-for`,
`BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1`.
