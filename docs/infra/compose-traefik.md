# Docker Compose behind Traefik

```console
$ blastit --preset compose-traefik
```

or, for a process you do not start yourself:

```console
$ BLASTIT_PRESET=compose-traefik blastit
```

This preset rests on a measurement of its own, not on the App Platform one:
Traefik **v3.7.16** is the ingress, blastit is behind it, and the app publishes
no port at all.

## What it sets

| Setting | Value | What the value assumes |
|---|---|---|
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | Traefik **overwrites** it from a peer it does not trust, and appends its own entry otherwise; the rightmost entry is what the closest hop saw in both cases |
| `BLASTIT_TRUSTED_PROXIES` | `*` | **no untrusted peer can reach blastit's port.** The app publishes no port, so the peers that can are the containers on its own compose network |
| `BLASTIT_FAIR_QUEUING` | `on` | as above: one client's burst must not starve the others |
| `BLASTIT_LOG_FORMAT` | `json` | stdout is the container's log |

and two assumptions it does not turn into settings:

| | | |
|---|---|---|
| privilege mode it assumes | `capabilities` | this is the rootless deployment: init runs as a dedicated uid holding six capabilities, not as root |
| deliberately not set | `BLASTIT_EXEC`, the `BLASTIT_PROXY_<NAME>_*` caps | both name something only the deployment knows: the capability-free copy of the binary, and the manifest's proxies |

## Why `x-forwarded-for`, and why `*`

Measured against Traefik v3 with blastit behind it (see
[the measurement](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#docker-compose-behind-traefik)).
Every row is one request from outside the compose network:

| Header the client sends | Traefik's defaults | `forwardedHeaders.trustedIPs` contains the client | `forwardedHeaders.insecure=true` |
|---|---|---|---|
| *(nothing)* | `X-Forwarded-For: <peer>`, `X-Real-Ip: <peer>`, `X-Forwarded-Proto: http` | same | same |
| `X-Forwarded-For: 6.6.6.6` | **overwritten**: `<peer>` | **appended**: `6.6.6.6, <peer>` | appended |
| `X-Real-Ip: 6.6.6.6` | **overwritten**: `<peer>` | **kept**: `6.6.6.6` | kept |
| `X-Forwarded-Proto: https` | **overwritten**: `http` | **kept**: `https` | kept |
| `Forwarded`, `True-Client-IP`, `X-Client-IP`, `CF-Connecting-IP`, `do-connecting-ip` | pass through **forged** | pass through forged | pass through forged |

From which:

* **`x-forwarded-for` is the header to read, not `X-Real-Ip`.** Traefik always
  adds its own observation to `X-Forwarded-For`, so the rightmost entry is the
  address it saw in *every* configuration above. `X-Real-Ip` is whatever the
  client sent, as soon as the client's address is in `trustedIPs`.
* **Every peer is trusted, and it is a rule about the *rightmost* entry.** The
  private ranges a Docker network uses were the other candidate, and they are
  worse here: when every entry of a chain is trusted the resolver takes the
  **leftmost**, so with the client's own address inside those ranges (the bridge
  gateway, every other container, a LAN client) a forged `X-Forwarded-For:
  6.6.6.6` *becomes* the client. Trusting a peer because its address is in
  `10/8` also says nothing about whether it is a hop this deployment owns.
* **`X-Forwarded-Proto` is followed**, which is `wsgi.url_scheme` in your app:
  the peer is trusted to say it, and Traefik overwrites it by default — so a
  client's own `https` cannot make a plaintext deployment report HTTPS, and
  behind Traefik's TLS entry point the app sees `https`.
* **Do not put a client's address in `forwardedHeaders.trustedIPs`** unless
  something that really is a proxy sits in front of Traefik: with the client's
  address in it, Traefik keeps the client's own `X-Real-Ip` and
  `X-Forwarded-Proto`. `X-Forwarded-For` stays the right header even then, which
  is the second reason the preset reads that one.
* A deployment that terminates TLS *in front of* Traefik is that other
  configuration, and it is the one case where you write the `trustedIPs` list
  yourself: the preset's values assume Traefik is the hop that decides.

!!! warning "The assumption is the deployment's to keep"

    Any container on the same compose network can reach blastit — it binds
    `0.0.0.0:8080` inside its own network namespace — and every peer is trusted.
    So:

    * publish no port for the app, only Traefik's;
    * keep containers you do not trust off that network. Your own sidecars are
      trusted by construction, which is what makes service-to-service calls work
      with no second rule.

    If either fails, any peer on the network can forge the client address.

## A full example

Three files, and two published ports: Traefik's. The app publishes nothing.

### `traefik.yml` — the file provider

```yaml title="traefik.yml"
http:
  routers:
    app:
      rule: Host(`shop.example.com`)
      entryPoints: [web]
      service: app
    app-tls:
      rule: Host(`shop.example.com`)
      entryPoints: [websecure]
      service: app
      tls: {}
  services:
    app:
      loadBalancer:
        servers:
          - url: http://app:8080
```

Both entry points forward to blastit's port. Traefik sets `X-Forwarded-Proto`
itself — it does not pass a client's own value through unless the client's
address is in `forwardedHeaders.trustedIPs` — so `http` arrives over the
plaintext entry point and `https` behind the TLS one.

### `compose.yaml`

```yaml title="compose.yaml"
name: shop

x-app-env: &app-env
  DATABASE_URL: postgresql://shop:${DB_PASSWORD:?}@db:5432/shop?sslmode=disable
  DJANGO_SECRET_KEY: ${DJANGO_SECRET_KEY:?}
  BLASTIT_PRESET: compose-traefik

services:
  db:
    image: postgres:18
    environment:
      POSTGRES_USER: shop
      POSTGRES_PASSWORD: ${DB_PASSWORD:?}
      POSTGRES_DB: shop
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "shop"]
      interval: 1s
      retries: 60
    volumes:
      - pgdata:/var/lib/postgresql/data

  # Schema changes run outside blastit, with the real credentials, and as a uid
  # of their own — never the one init runs as (see "Rootless" below).
  migrate:
    image: registry.example.com/shop:${TAG:?}
    user: "10001:10001"
    environment: *app-env
    command: ["sh", "-c", "python manage.py migrate -v0"]
    depends_on:
      db: { condition: service_healthy }

  app:
    image: registry.example.com/shop:${TAG:?}
    user: "10000:10000"
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FOWNER, KILL, SETGID, SETPCAP, SETUID]
    tmpfs:
      # /run belongs to root and a rootless init cannot create a directory in
      # it. This one is the runtime directory, owned by the uid init runs as.
      - /run/blastit:mode=0700,uid=10000,gid=10000
    environment: *app-env
    # The *copy* of the interpreter the image gave the capabilities to — never
    # the shared one, which a worker, `manage.py` and your shell also run.
    command: ["/usr/local/bin/python3.13-init", "-m", "blastit"]
    expose: ["8080"]
    depends_on:
      migrate: { condition: service_completed_successfully }

  traefik:
    image: traefik:v3.7.16
    command:
      - --entryPoints.web.address=:80
      - --entryPoints.websecure.address=:443
      - --providers.file.filename=/etc/traefik/traefik.yml
      - --log.level=INFO
    volumes:
      - ./traefik.yml:/etc/traefik/traefik.yml:ro
    ports:
      - "80:80"
      - "443:443"
    depends_on: [app]

volumes:
  pgdata:
```

Notes on the parts that are not obvious:

* **`expose: ["8080"]` and no `ports:`** on the app. That is the preset's
  assumption, expressed in the file.
* **`user:` on the app is the non-root identity**, and it is a **dedicated**
  uid: no worker, migration or shell may share it, because it is the uid that
  can run the capable copy — and running the copy is becoming init. The
  `migrate` service therefore uses a uid of its own.
* **`cap_add` is the bounding set**, not the capabilities the process gets.
  Docker hands a non-root `user:` process **no** capabilities at all; see below.
* **`command` is the copy**, `/usr/local/bin/python3.13-init`, and not
  `blastit`. A wheel's `blastit` console script has a shebang pointing at the
  shared interpreter, which stays capability-free.

### The Dockerfile — and the capabilities

```dockerfile title="Dockerfile"
FROM python:3.13-slim

WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
RUN pip install --no-cache-dir --pre blastit \
      --extra-index-url https://offworldnexus.github.io/blastit/simple/
RUN apt-get update && apt-get install -y --no-install-recommends libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
COPY . .

ENV PYTHONUNBUFFERED=1 \
    BLASTIT_APP=shop.wsgi:application \
    BLASTIT_PRELOAD=shop.urls \
    BLASTIT_MANIFEST=/app/blastit.toml

ARG BLASTIT_UID=10000
RUN useradd --uid "$BLASTIT_UID" --user-group --create-home --shell /usr/sbin/nologin blastit

# The dedicated copy of the interpreter, with the capabilities. Order matters:
# `chown` clears a file capability, so nothing re-owns the file after `setcap`.
# The copy sits *next to* the original, because CPython finds its stdlib and
# site-packages from the executable's own path.
RUN interpreter="$(readlink -f /usr/local/bin/python3.13)" \
    && cp "$interpreter" "${interpreter%/*}/python3.13-init" \
    && chown "$BLASTIT_UID:$BLASTIT_UID" "${interpreter%/*}/python3.13-init" \
    && chmod 0500 "${interpreter%/*}/python3.13-init" \
    && setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
         "${interpreter%/*}/python3.13-init"

EXPOSE 8080
# No `USER`: the identity of this deployment is the `user:` field of the compose
# service, which is where the non-root uid is declared.
```

**Why not `--cap-add` on a `user:` container.** Measured on Docker 29.5
(`docker compose` v5): `--cap-add`/`--cap-drop` set the container's *bounding*
set only, and a `user:` process starts with `CapPrm`, `CapEff` and `CapAmb` all
zero. Two mechanisms do hand capabilities to a non-root process, and this
example uses the first:

* **file capabilities on a dedicated copy** of the file init starts from
  (`setcap`), which is what the Dockerfile above does. In a wheel install that
  file is the **interpreter**: `blastit` is a console script.
* **ambient capabilities from a launcher** that starts as root and drops the
  uid itself. This is the spelling that works where `no_new_privs` is set on the
  container, since the kernel then ignores file capabilities:

    ```console
    $ setpriv --reuid=1000 --regid=1000 --init-groups \
          --inh-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
          --ambient-caps +setuid,+setgid,+chown,+fowner,+dac_override,+kill,+setpcap \
          blastit
    ```

Start it:

```console
$ DB_PASSWORD=… DJANGO_SECRET_KEY=… TAG=$(git rev-parse --short HEAD) docker compose up -d
$ curl -sS -o /dev/null -w '%{http_code}\n' -H 'Host: shop.example.com' http://127.0.0.1/
$ docker compose logs app | head -3
{"event":"boot","component":"init","stage":"stage0","seccomp":true,"privilege":"capabilities","capabilities":["cap_chown","cap_dac_override","cap_fowner","cap_kill","cap_setgid","cap_setuid","cap_setpcap"],"bounding_set":"dropped"}
```

## Rootless

blastit asks the kernel which privileges it has before it does anything, so it
can run as an unprivileged user holding only what the boot needs instead of
root. Its first event says which mode it chose, so a deployment that *thinks* it
granted a capability finds out at boot rather than at the first request.

The capabilities, and why each one is there:

| Capability | Needed for |
|---|---|
| `cap_setuid` | `setresuid` to a view's uid |
| `cap_setgid` | `setgroups(0, NULL)` and `setresgid` |
| `cap_chown` | the view directories and the pg sockets, chowned to the view uid |
| `cap_fowner` | the *mode* of those files, applied after the `chown` that made them the view's |
| `cap_dac_override` | opening a view's directory to bind its socket — the directory is already the view's, `0700` |
| `cap_kill` | `kill(2)` on a view process, whose uid is never init's |
| `cap_setpcap` | dropping every child's **bounding** set, so nothing can hand a capability back |

`cap_sys_resource` is **not** needed: blastit only ever *lowers* a `setrlimit`,
and binding `$PORT` needs no privilege. `cap_setpcap` is wanted rather than
required — without it the children's other sets are still cleared and
`no_new_privs` still stops an `execve` from granting what the bounding set
allows — and the boot then says `"bounding_set": "kept"` with one warning.
Missing one of the six required ones is refused at boot, with the names and the
recipe that fixes it.

### The capable file is init's alone

A file with capabilities turns **any** process of its owner's uid that can run
it into init: `CAP_SETUID`, `CAP_SETPCAP`, `setuid(0)`. So the capability never
goes on a file anybody else runs:

```text
/usr/local/bin/python3.13        root:root     0755   the interpreter `python` resolves to
/usr/local/bin/python3.13-init   10000:10000   0500   the copy init is started from, with
                                                    cap_chown,cap_dac_override,cap_fowner,
                                                    cap_kill,cap_setgid,cap_setuid,cap_setpcap
```

* the copy sits **next to** the original, because CPython finds its stdlib
  (`sys.prefix`) and its virtualenv from the executable's own path;
* **that uid is dedicated.** It is the only uid that can run the copy, so a
  worker, a migration or an operator's shell must use another one. The compose
  file gives them a uid of their own. The boot warns — it never refuses — when
  another process of init's uid is already running;
* the *shared* interpreter stays capability-free. The boot checks **every** name
  it can be started by (`python`, `python3`, `python3.X`,
  `sys._base_executable`, the resolved `sys.executable`) and compares it with
  init's file by device and inode rather than by name, so a symlink chain cannot
  hide the mistake, and it refuses with the recipe a deployment where the shared
  file carries them;
* a child cannot use the copy even if it could run it: its bounding set is empty
  and it sets `no_new_privs`, and the kernel refuses such an `execve` outright
  (`EPERM`, "insufficient to execute correctly").

### Two things a rootless deployment provides itself

* **A runtime directory the service user owns.** `/run` belongs to root, so a
  rootless init cannot create a directory in it:
  `--tmpfs /run/blastit:mode=0700,uid=1000,gid=1000`, or a volume, or a real
  directory owned by the service uid.
* **A service uid that is not one of the layout's.** `BLASTIT_UID_BASE`
  (default `20000`) hands out `BLASTIT_UID_BASE + 1…3` and one uid per route
  from `BLASTIT_UID_BASE + 1000`. The boot refuses a uid that the layout gives
  to another process.

### A standalone install

The same principle, one level up: `setcap` on the binary itself breaks
`--role=dbproxy`, which re-executes `/proc/self/exe` from a child whose bounding
set is empty. A rootless standalone deployment keeps two copies and names the
capability-free one:

```console
$ cp /usr/local/bin/blastit /usr/local/bin/blastit-init
$ chown 10000:10000 /usr/local/bin/blastit-init && chmod 0500 /usr/local/bin/blastit-init
$ sudo setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
      /usr/local/bin/blastit-init
$ BLASTIT_EXEC=/usr/local/bin/blastit /usr/local/bin/blastit-init
```

`BLASTIT_EXEC`'s target is checked at boot: owned by root or by init's uid,
writable by neither group nor others, in a directory that is not either. A uid
that could replace that binary would decide what the process holding the real
credentials runs next. `/tmp` is the usual trap, and the boot refuses it by
name.

## Next

[Anything else](anything-else.md) — the client-IP header and trusted proxies for
your own ingress, memory and process caps, fair queuing, logs, and worked
examples for Kubernetes, plain Docker, systemd and nginx.
