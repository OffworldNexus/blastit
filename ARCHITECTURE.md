# blastit — Architecture

blastit is a zero-trust application harness for 12-factor web apps (Python WSGI,
embedded PHP 8.3–8.6, and static sites) that **sandboxes views**. Each view runs
under its own uid, and a view only gets the capabilities the `Blastfile` lists
for it. For now that means PostgreSQL: route X may run parameterised queries A
and B, route Y may run query C, and nothing else. If an attacker takes over a
view, they get that view's allowlist and nothing more.

It is built to run as the only thing in a container, behind a load balancer
that speaks HTTP/1.1. The target is DigitalOcean App Platform, which runs
containers under gVisor.

The feasibility experiments that support this design are archived in
[`docs/archive/2026-10-experiments.md`](docs/archive/2026-10-experiments.md).
The platform probes run on App Platform itself are written up in
[`docs/spikes/2026-10-do-app-platform.md`](docs/spikes/2026-10-do-app-platform.md).
Section 9 summarises the parts of both that matter here.

---

## 1. Goals and non-goals

**Goals**

* **Least privilege per view.** Each view process runs under its own uid. The
  database proxy uses that uid to decide which statements are allowed.
* **No real secrets in application memory.** The app sees credentials that only
  work against the local brokers. The real credentials live in one process that
  never runs application code ("environment variable swapping").
* **Memory-frugal.** Python and the application are imported once, before any
  fork, and the preloaded heap is made immortal. Every process shares that
  memory copy-on-write.
* **Bounded concurrency.** One component, the scheduler, owns the global limit
  on in-flight requests.
* **Runs in a constrained gVisor container** with no mounts, no namespaces, no
  cgroups and no firewall. We rely only on uids, file permissions, rlimits and
  (if available) seccomp.
* **Configured only through environment variables**, plus one manifest file
  that is shipped with the code.

**Non-goals**

* TLS, HTTP/2, HTTP/3, WebSockets and `Upgrade`. The load balancer terminates
  TLS and downgrades everything to HTTP/1.1. We answer `Upgrade` with `501`.
* Connection pooling. Each request thread opens its own upstream connection. If
  you want pooling, put it in front of the real database (for example
  PgBouncer or DO's connection pools).
* Protecting against malicious code that runs **at import time**. Imports run
  as root before the fork (see §7.1). The threat model covers compromise at
  request time: RCE in a view, SQL injection, deserialisation bugs.
* Network egress filtering per uid. This needs `CAP_NET_ADMIN`, which we don't
  have. The best we can do is a seccomp filter on socket creation (§7.3).

---

## 2. Components

A single serving Rust binary, `blastit`, plays every role. Most roles are chosen by
their position in the process tree. The exception is dbproxy, which is
re-exec'd with `--role=dbproxy` (§4); that flag grants nothing by itself,
because the secrets only arrive over an inherited pipe. CPython (`3.13`, `3.14`)
is `dlopen`ed in stage 1 via an fd-pinned per-minor `libblastit_py3{13,14}` shim
only when a Python route is active ([ADR-0017](docs/decisions/ADR-0017-universal-distribution.md)),
the host's NTS `libphp` is `dlopen`ed in a
PHP route (§2.6), a `static` mount is answered by the scheduler through the
`blastit-view-static` library (§2.4), so an all-static site needs no interpreter
at all. The database proxy is
built on the [`pregresql`](https://github.com/OffworldNexus/pregresql) crate,
used as a library. On the Python side there is no framework integration at
all: blastit loads one standard WSGI callable (`BLASTIT_APP`, or the file's
`entrypoint`). It knows nothing about the framework's routing. Which
process serves which request comes from blastit's own routing table in the
manifest (§6). A second binary, `blastit-lsp`, is the **editor** surface: it
plays no role in this process tree, and §2.7 says what it may and may not link.

**Execution modes.** Six entry points:

| Entry point | Mode | Runs |
|---|---|---|
| `blastit` | production | stage 0, then `execve` stage 1 under real uids or capabilities; refuses to start without them |
| `blastit dev` | single-uid simulation | the same five roles under `geteuid()`, loopback by default, with `fanotify` live reload (`--no-reload` turns it off); a non-loopback bind needs `--host` and `BLASTIT_DEV_EXPOSE=1`, and the banner then says `EXPOSED, no auth`; `--learn` allows what the manifest lacks and records it |
| `blastit check` (`blastit test`) | audit | `pytest` through dbproxy (or static-root audit on an all-static file); `--learn` proposes new views with their exact grants, never a grant on an existing view (`dev --learn` does that) |
| `blastit fmt` | formatter | lossless canonical formatting of `./Blastfile` (or `--check` in CI) |
| `blastit lsp` / `blastit-lsp` | language server | stdio JSON-RPC LSP for `Blastfile` diagnostics, formatting, `$?`/`...` hover, and SQL parameterisation Quick Fixes |

The component table below is the production topology; `blastit dev` runs the
same roles on one uid. A `static` mount adds no row: the scheduler answers it
itself (§2.4).

| Component | Count | uid | Interpreter? | Holds | Talks to |
|---|---|---|---|---|---|
| **init** | 1 (PID 1) | 0, or the uid it was started as (D6) | stage 1 only | listen socket, all internal socket ends while forking, preloaded app | everyone (control) |
| **dbproxy** | 1 (own `execve`) | `UID_BASE+3` | no | **the real DB credentials**, the query policy, one listener per view | views (Unix sockets), PostgreSQL (TCP/TLS) |
| **http** | 1 | `UID_BASE+1` | no | the listening TCP socket | load balancer, scheduler |
| **scheduler** | 1 | `UID_BASE+2` | no | the manifest's routing table, worker budget, one channel per view process | http, init, view processes |
| **view** | 1 per view (lazy) | `UID_BASE+1000+i` | CPython, or the host's `libphp` for `entrypoint php` (§2.6) | nothing but its own channel | scheduler, dbproxy |

`UID_BASE` defaults to `20000`. DO maps the full 32-bit uid range (the spike
used uids up to 2³²−2), but we stay below 65536 for portability. None of these
uids needs an `/etc/passwd` entry, because `setuid()` only needs `CAP_SETUID`
(experiment 1, confirmed on DO). Every process gets `gid == uid` and an empty
supplementary group list.

### 2.1 init (PID 1, root or capabilities)

init is the only process with privileges. It never parses HTTP, never runs a
view and never talks to the database. It does three things.

1. **Bootstrap.** Read the environment, parse the manifest, compute the swapped
   environment, create the runtime directories, bind `0.0.0.0:$PORT`, and start
   every other component (§4).
2. **Zygote.** After it preloads the app, init is the only process that can
   create a view process: it is root and it holds the preloaded interpreter.
   The scheduler asks init for view processes over the control channel. init
   forks, drops privileges in the child, and passes the scheduler's end of the
   new channel back with `SCM_RIGHTS`.
3. **Supervisor.** Reap every child, since PID 1 inherits orphans and gVisor
   gives us no init. Forward signals and handle graceful shutdown. Be the
   only writer to stdout: collect every child's events and output through
   pipes and print them as one event per line (§8.1).

**Invariant: init is single-threaded at every `fork()`.** It runs no tokio
runtime and no background thread. BLAS thread pools are pinned to one thread
(§9.3). Any runtime a child needs is created after the fork, in the child.

It needs two things of its identity, and they are not the same thing: the
capability to *become* each uid of the layout below, and the capability to
create and own their private files. As root both come from uid 0. When blastit
is started as an unprivileged user instead (D6), both come from a small
capability set, which the boot reads from `/proc/self/status` and reports:

* `cap_setuid`, `cap_setgid` — `setresuid`, `setresgid` and `setgroups` to the
  uid layout of §2;
* `cap_chown`, `cap_fowner` — `chown` and `chmod` of the view directories and
  their sockets, which belong to the view's uid by the time their mode is set;
* `cap_dac_override` — opening a view's private directory to bind the socket
  inside it, which is `0700` and already the view's;
* `cap_kill` — `kill(2)` on a view process, whose uid is never init's.

`cap_setpcap` is **wanted, not required**: with it the children's bounding set is
emptied as well, and without it every other set is still cleared and
`PR_SET_NO_NEW_PRIVS` still stops an `execve` from granting what the bounding set
allows. A container that cannot give it (`--cap-drop SETPCAP`, a runtime without
`PR_CAPBSET_DROP`) boots, with one warning and `"bounding_set": "kept"` in the
`boot` event. Nothing else is needed: blastit only ever *lowers* an `rlimit`, and
binding `$PORT` needs no privilege.

The mode is not configured but detected, and a process that is missing one of
those capabilities is refused at boot with the names of the missing ones and the
`setcap` line that fixes it. The first `boot` event says which mode is in force —
`"privilege":"root"`, or `"privilege":"capabilities"` with the set in
`"capabilities"` — because the two deployments look identical from the outside.

**The file that carries them is init's alone.** A file with file capabilities
turns any process of its owner's uid that can run it into init, and a *child*
cannot `execve` such a file at all once its bounding set is empty (the kernel
refuses with `EPERM`, "insufficient to execute correctly"). So a rootless
deployment gives the capabilities to a **dedicated copy** of the file init starts
from — a wheel's interpreter, a standalone binary — which is mode `0500`, owned
by the uid init runs as, and started by the entrypoint and by nothing else.

That uid is then a **dedicated** one: it is the only uid that can run the copy,
and running the copy is becoming init. A worker, a migration, an operator's shell
and every sidecar must use another uid, and the `boot` event warns (it never
refuses — a debug shell is legitimate) when another process of init's uid is
already running when it starts. The shared interpreter stays capability-free:
**every** name it can be started by (`python`, `python3`, `python3.X`,
`sys._base_executable`, the resolved `sys.executable`) is checked for the
attribute, and it is compared with init's file by device and inode rather than by
name, so a symlink chain cannot hide the mistake and a name that *is* init's own
copy is not a shared name. The boot refuses, with the recipe, a deployment where
the shared file carries them, or where a child's program does. `BLASTIT_EXEC`
names the capability-free copy of a standalone install's own binary (which
`--role=dbproxy` runs), and that file must be owned by root or by init's uid,
writable by neither group nor others, in a directory that is not either: a uid
that could replace it would decide what the credentials-holding process runs next.

**Distribution.** A tagged release publishes `py3-none-*` wheels to PyPI through
trusted publishing (`.github/workflows/publish-pypi.yml`), native `.deb`/`.rpm`
packages to signed APT/RPM repositories (`.github/workflows/packages.yml`), and
standalone archives; from an sdist, `maturin build --bindings bin` builds the same
universal `blastit` binary from the workspace inside it. [ADR-0017](docs/decisions/ADR-0017-universal-distribution.md)
(`docs/design/universal-distribution.md`) unifies every Linux package (`.deb`,
`.rpm`, `.tar.gz`, `py3-none-*` wheels, and the Composer wrapper) around one
`libpython`-free `blastit` binary that loads CPython (`3.13`, `3.14`) and NTS
`libphp` (`8.3`–`8.6`) via fd-pinned per-minor `dlopen` shims.

### 2.2 dbproxy

This is a pregresql `Proxy` with blastit's two hooks.

**Identity comes from which socket the connection arrived on, not from
`SO_PEERCRED`.** Under DO's gVisor, `SO_PEERCRED` on an accepted
Unix socket returns the *listener's own* credentials, so it can't identify the
client. Instead, each view has its own listener per `postgresql` proxy it has
grants on (§5), created in that view's private directory:

```
/run/blastit/views/<uid>/            0700, owned by the view uid
    .s.PGSQL.5432                    0600, owned by the view uid (proxy "analytics")
    .s.PGSQL.5433                    0600, owned by the view uid (proxy "db")
```

init binds all of these in stage 0 (it is root, and the set of views is known
from the manifest), `chown`s them, and hands the listening fds to dbproxy
across its `execve`. File permissions on Unix sockets are enforced at
`connect()`, which the spike verified: another uid gets `EACCES` both on a
`0600` socket and through a `0700` directory. So a connection on listener *L*
can only have come from *L*'s view uid, or from root.

As a second check, every listener has `SO_PASSCRED` set. dbproxy reads the
startup packet with `recvmsg` and requires the kernel-attached
`SCM_CREDENTIALS` uid to equal the listener's view uid. gVisor fills these in
correctly and rejects forged ones (`EPERM`). This also works with unmodified
libpq clients, because the receiver asks for the credentials and the client
doesn't have to send anything.

Every view sees the same rendered variables, because the socket directory is
`/proc/self/cwd` (§5.2) and each view's working directory is its own socket
directory. The
spike confirmed that `connect("/proc/self/cwd/.s.PGSQL.5432")` works under
gVisor.

* **`AuthHook`**
  * `client_auth_method` returns `Trust`. The identity is the listener plus
    the `SCM_CREDENTIALS` check, not a password.
  * `resolve_upstream` maps the listener tag to `(route, proxy)`. A
    credential mismatch is rejected with `FATAL 28000`. A route with no grant
    on that proxy has no listener at all. The real upstream comes from the
    proxy's inputs: host, port, role, password, database, TLS mode and CA.
* **`QueryHook`**
  * `authorize` checks the statement against the view's effective grants:
    the union of its own grants and those of every trait it `use`s (§6).
    Everything else is denied with SQLSTATE `42501` before it reaches the
    server, and logged as a `sql.denied` event (§8.1).

The proxy keeps no pool. One inbound connection is one upstream connection.
Each proxy's upstream connections are capped by
`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` (default: `BLASTIT_MAX_WORKERS`), so
the DB plan's connection limit is never exceeded (small DO plans allow about
20). dbproxy is the broker for the `postgresql` protocol and serves every
`postgresql` proxy; future protocols get their own broker process.

> **pregresql support:**
> [OffworldNexus/pregresql#1](https://github.com/OffworldNexus/pregresql/pull/1)
> (open, CI green) adds what dbproxy needs:
> * `Proxy::add_unix_listener(listener, tag)`, for listeners init bound
>   before the `execve`;
> * `InboundConnection::listener_tag`;
> * `Transport::Unix.message_credentials`: the kernel-attached
>   `SCM_CREDENTIALS` of the first message, read with `recvmsg` into the
>   startup buffer so no bytes are lost. Linux only, `None` elsewhere;
> * `QueryContext::connection`.

### 2.3 http

This is the internet-facing parser, written in Rust (tokio + `httparse` or
hyper in HTTP/1-only mode). It has no Python, no secrets, and no file access
beyond what it inherited. Its only sockets are the inherited listening socket
and its channel to the scheduler.

* Accepts connections and parses HTTP/1.1, including keep-alive, `Expect:
  100-continue`, chunked request bodies and pipelining (answered strictly in
  order).
* Enforces limits on header size, body size, header read timeout (slowloris)
  and keep-alive idle time.
* Normalises the request: strips hop-by-hop headers and rejects any request
  that has both `Content-Length` and `Transfer-Encoding`. It derives the
  client address and scheme with one resolver: the header
  `BLASTIT_CLIENT_IP_HEADER` is believed only from a socket peer in
  `BLASTIT_TRUSTED_PROXIES` (§8.2). The address it derives is bare: no port, an
  IPv4-mapped IPv6 address normalised to IPv4, an IPv6 zone dropped. The app is
  told the address in `REMOTE_ADDR` and the connection's port in `REMOTE_PORT`
  (§2.5), which is CGI's split and what PEP 3333 keeps.
* Encodes responses. It validates header names and values (no CR/LF
  injection) and picks `Content-Length` or chunked encoding. Views produce a
  status line, headers and body chunks; they never write raw bytes to the
  client.
* Turns a client disconnect into `CANCEL` towards the scheduler.

### 2.4 scheduler

The scheduler is the only component that knows about every view process. It
is pure Rust, with no interpreter. It is forked before Python is loaded, like
http.

* **Routing.** It matches the request against the file's routing table
  (§6): each view's patterns, and optionally its methods, tried in
  file order; the first match wins. If nothing matches, the scheduler
  answers `404` itself and no Python runs. A catch-all view (`view fallback
  "/*" {}`, listed last) lets the app render its own 404s. Every routing
  decision is logged as a `request` event (§8.1).
* **Admission.** It keeps one global budget, `BLASTIT_MAX_WORKERS`, which is
  the number of live request slots across all view processes (each route owns
  one threaded Python process or a pool of single-request PHP processes, each
  admitting `per_process_cap` requests, forked one at a time up to
  `BLASTIT_MAX_WORKERS`). Excess requests wait in a bounded FIFO
  (`BLASTIT_QUEUE_SIZE`, `BLASTIT_QUEUE_TIMEOUT`); once that overflows, the
  answer is `503`. A request at the head that cannot be served — its route's
  view is not ready — does not hold up the requests behind it whose route can.
* **Fair queuing.** With `BLASTIT_FAIR_QUEUING=on` the same bounded queue is
  ordered by a two-level fair scheduler instead of by arrival. Every waiting
  request belongs to a **flow**: an **outer** flow keyed by the client's IP
  prefix (`BLASTIT_FQ_OUTER_PREFIX_V4`, default `24`, and
  `BLASTIT_FQ_OUTER_PREFIX_V6`, default `56`), and an **inner** flow keyed by a
  hash of the client address and of the headers and cookies the operator listed
  (`BLASTIT_FQ_INNER_HEADERS`, `BLASTIT_FQ_INNER_COOKIES`,
  `BLASTIT_FQ_INNER_INCLUDE_IP`) — one browser behind a NAT cannot starve its
  neighbours, one network cannot starve another. Both levels keep a virtual
  time, and both are dispatched lowest-first; a flow that becomes backlogged
  after being quiet restarts one `BLASTIT_FQ_BURST` (default `20ms`) behind the
  clock, so a new client may spend one burst before it queues, and every
  request then charges what it cost, up to one `BLASTIT_FQ_MAX_DEBT` (default
  `5s`) ahead of the clock, so no flow can borrow unboundedly against the
  future. Both tables are bounded (`BLASTIT_FQ_OUTER_MAX`,
  `BLASTIT_FQ_INNER_MAX_PER_OUTER`, `BLASTIT_FQ_INNER_MAX`): a flow that does
  not fit shares one overflow flow with every other one that does not, and a
  request whose client address cannot be read at all goes to that same overflow
  flow. Idle flows are collected once their debt is paid or after
  `BLASTIT_FQ_DEBT_TTL` (default `30s`, read as `1s` when it is shorter), so the
  tables cannot grow with the number of clients that have ever connected. The
  inner key is a per-process seeded hash of header and cookie values, which are
  never logged, and neither is the hash. A route whose view is not ready is not
  a candidate, so it never holds up another route. A knob that is out of range
  is *clamped*, not refused — a prefix cannot be longer than the address it
  masks and a table cannot hold zero flows — and the `boot` stage says so.
* **What a request costs.** The flow is charged the view process's own
  measurement — the wall time of `serve_request` (the WSGI call, the iteration
  of its iterable, the `close()` and the frames written back) and the thread's
  CPU time over the same span — reported in the response's last frame
  (`RESP_END_COST`). The report comes from a sandboxed process and is
  untrusted, so it is **clamped between** the time from dispatch to the view's
  first response frame — the one part of the request the view cannot hide
  without delaying its own client — **and** the time the scheduler observed
  between dispatch and the end of the response; a request whose end frame
  carries no report — a view built before D5, a `504`, a cancellation, a client
  that went away, a view that died — charges that observed time instead. The
  `request` event keeps both numbers: `cost_ns` (what was charged) and `cpu_ns`
  (what the worker reported, `null` when it reported nothing).
* **A `5xx` says which failure it was, and only a crash pays the wall.** A
  status alone cannot tell an application's own server error from a handler
  that never returned, and both leave the same `RESP_HEAD`, so `DONE` carries
  the missing fact (`MB.3.41`): `bailed` means a contained `exit()`/`die()`/a
  fatal was trapped by the SAPI shim. Three rules follow, one per kind:
  * an app that **answers** a `5xx` itself (a framework's `500`, a script's
    `http_response_code(500)`) returned from its handler: the request is
    `outcome: "ok"` — the view answered — with that `status`, and `cost_ns`
    keeps the clamp above, exactly as a `200` does;
  * a **contained bailout** answers its `5xx` without returning: `outcome:
    "bailed"`, and `cost_ns` is the request's own wall time (arrival to last
    byte) — `R19`'s crash ceiling at the one live thread such a worker has —
    so a crash loop can never cost less than the client waited, and the view's
    report is not believed at all;
  * a `5xx` the **scheduler** wrote itself (the `502` of a view that never
    answered, the `504` of `BLASTIT_REQUEST_TIMEOUT`, a `503` refusal) keeps its
    own outcome (`view_died`, `timeout`, `queue_full`/`queue_timeout`) and is
    charged dispatch to completion, because no view answered it and it never
    held the worker its arrival span covers. `view_died` therefore still means
    only "the view process died mid-request".

#### The static responder

A `static` mount is answered by the scheduler itself: same process, same uid,
same inherited listener — no new role, no new uid, no new socket. The
**resolution** (`static_asset`), the **decision** (`rewrite`) and the **head**
(`static_response`) are `blastit-view-static`'s, a library called by the
scheduler; the scheduler keeps admission, the slot and the descriptor pump, and
the answer leaves as `blastit-frame`'s `RespHead`/`RespBody` frames through the
scheduler's own `http.chan` writer — `blastit-view` reaches the scheduler only
as the request-target decoder it re-exports. Each root's
dirfd is opened once at boot (`O_PATH|O_DIRECTORY` on
`manifest_dir.join(root)`); a request's path is resolved beneath that descriptor
and never re-opened from a joined string.

One total order decides, first match wins: the health path; a route with no
policy is untouched; a malformed path (`400`) or a dotfile at any depth (`404`,
except `.well-known` as the first segment); an exact regular file; a directory's
`index`; `fallback` and `root/index`; the file's `entrypoint` when it declares
one; otherwise `404`. A refusal is terminal — never the app, never the fallback
— and the responder answers `GET`/`HEAD` only: a servable target gives `405`
with exactly `Allow: GET, HEAD`, and a `GET`/`HEAD` that declares a body gives
`413`. Only a file beneath `root` is reachable: `openat2` with
`RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS` (a per-segment
`O_NOFOLLOW` walk where the kernel has no `openat2`), an `O_PATH` probe
classified by `fstat`, then a read-open whose `(st_dev, st_ino)` must match the
probe — so a symlink, FIFO, socket, device or path swapped between the two opens
is `404` and is never read. `..`, NUL and the other malformed paths are `400`
before any syscall, so no refusal depends on the filesystem's state. The
year-long `Cache-Control` is keyed on the **answering** file's path: a directory
`index` and a SPA fallback are always `no-cache`.

#### Design choices

* **`*` means the rightmost entry, not the leftmost.** Wherever the edge
  *appends* to a header the client already sent — App Platform does — the leftmost
  entry is the client's own invention, so blastit takes the rightmost: what the
  closest hop added is the one thing that hop cannot be lying about.
* **A lying view is bounded on both sides.** The worker's report of what the
  request cost is clamped **upwards** by the time the scheduler observed, and
  **downwards** by the time up to the view's own first response frame, so a view
  cannot report zero for a request it took real time to answer. It can only buy
  itself the gap between its first frame and its last one — that is, exactly the
  time its own client was waiting for it. A view that says nothing is charged what
  the scheduler saw. The one exception is a view that **bails out** of a `5xx` it
  had answered (`DONE`'s `bailed`): its report is not believed at all, and it is
  charged its own wall time instead, because the one report that must never be
  cheap is a crash loop's.
* **One lane per route, and one worker budget across routes.** The queue only
  ever serves the route a queued request belongs to, so the lanes are the routes;
  the best flow is compared across them because they all compete for
  `BLASTIT_MAX_WORKERS`. **Outer flows belong to the queue, not to a route** —
  `used` is one number per client however many routes that client has work for —
  so `(outer.used, inner.used)` is the same key inside a route and between routes,
  with the route index as the last tiebreak.
* **The root index is refreshed.** Every mutation of a flow's queues ends in
  `touch`, and `refresh_root` prunes the per-route root index when a flow is
  *removed* under it (an idle one collected, or an outer flow dropped): an entry
  that outlived its flow would be a stale candidate.
* **A peer whose address cannot be read goes to the overflow flow.** A `REQ_HEAD`
  may arrive with a peer string that is not an address at all (`peer_addr` empty,
  a Unix socket with no name). It is queued in the same overflow flow as every
  client that cannot be told apart, rather than in a flow of its own that would be
  one client's private lane.
* **`TrustedProxies` refuses what it cannot read.** An entry it does not
  understand fails the boot, with the variable named and the value never quoted —
  a trusted-proxy list that silently trusts nothing when the operator meant it to
  trust something is a deployment that believes a forged `X-Forwarded-For` from
  the very peer it meant to distrust. (`127.0.0.1:8080` is the mistake worth
  naming: a port is not a network.)
* **There is no `system` queue.** Nothing the scheduler does for itself (health
  checks, the `404` for an unmatched path, a `503`, a `504`) takes a worker slot
  or enters the admission queue, so there is no work that has to outrank a
  client's.
* **The budget counts threads, not answers.** A slot is released when the view
  reports `DONE`, meaning the thread has actually exited. A timed-out request
  gets a `504` right away, but its slot stays taken until the thread finishes.
  So stuck threads eat into the budget instead of piling up without limit. A
  view process with more than `BLASTIT_STUCK_THREADS` overdue threads is
  recycled: the scheduler asks init to kill it, fails its other in-flight
  requests with `502`, and lets the next request spawn a fresh one.
* **Lifecycle.** It asks init to `SPAWN` a view process the first time that
  view is requested (or at startup when `BLASTIT_VIEW_SPAWN=eager`). It
  `RETIRE`s a view process after `BLASTIT_VIEW_IDLE_TIMEOUT` with no requests,
  or after `BLASTIT_VIEW_MAX_REQUESTS` requests.
* **Health.** It answers `BLASTIT_HEALTH_PATH` itself, so health checks cover
  http → scheduler without depending on any view.

### 2.5 view processes

A view process serves exactly one manifest route under exactly one uid. It
does what it is told. ("View" in the rest of this document means a route and
its process.)

* A Rust I/O thread reads frames from the scheduler channel. Each `REQ_HEAD`
  is handed to a **pooled request thread** that attaches to the interpreter:
  one that has just finished a request parks and takes the next, retiring once
  it has been idle for a while, so a busy view does not create and destroy a
  thread per request. The pool, the WSGI `environ`, the response frames and the
  loop are `blastit-view-python`'s; `blastit-view` carries the frames and the
  credit windows, and `blastit` is the composition root that forks the view and
  passes it the forker. That thread builds a standard WSGI `environ` and calls
  the WSGI callable, the same one in every process, so the app does its own
  routing and middleware as usual. It then streams the response back. Because
  a worker serves many requests, per-thread application state such as
  `threading.local` persists across them, as in any threaded WSGI server.
  blastit adds `blastit.route` (the route's name) to `environ`, for logging
  only. `REMOTE_ADDR` is the resolved client
  address (§2.3) and `REMOTE_PORT` is the port the request arrived on: the
  peer's, when the peer had one — a client a trusted proxy named has no port of
  its own, so `REMOTE_PORT` is the connection's and `REMOTE_ADDR` is still who
  the chain says. A peer that is not an address at all (an unnamed socket) gets
  no `REMOTE_PORT` rather than a wrong one.
* When the thread exits, the view sends `DONE`. This happens after the WSGI
  iterable's `close()`, so `request_finished` handlers have already run and
  the DB connection is already closed.

All app code is mapped into every view process. **The sandbox comes from uid
privileges, not from code isolation**: view A's process can run view B's
code, but only with A's database allowlist. The same principle makes routing
mistakes safe. If the file's patterns and the app's own URL routing
disagree, a request lands in a process whose allowlist doesn't fit, and its
queries are denied. It fails closed; it never gains a privilege.

### 2.6 php (embedded)

An `entrypoint php` front controller runs (`entry`, for example
`public/index.php`) inside a normal view process: one unprivileged process per
slot under `UID_BASE+1000+i`, forked by the stage-1 supervisor exactly like a
CPython view (the scheduler's queue, the `ctl.chan` lifecycle, and a fresh
process whenever the scheduler's budget allows one). There is no `php-fpm`, no
FastCGI stream and no second server: NTS `libphp` serves one request per
process, so a PHP file's concurrency is the number of live view processes,
which `BLASTIT_MAX_WORKERS` caps for the whole app — and each of those
processes runs one request at a time (`BLASTIT_VIEW_MAX_WORKERS` sizes one
process and its `RLIMIT_NPROC`, never the route's process count). Nothing in
the `Blastfile` sets either: the scheduler owns parallelism.

**One transport, one runtime.** The worker is `blastit-view-php`'s concrete
`PhpEmbedRuntime` — there is no runtime trait, because the interface a view has
is `view.chan` and the process that speaks it — on the same unnamed `view.chan`
`socketpair` and the same frames as a WSGI view (§3.1): the scheduler's
upstream session forwards `ReqHead`/`ReqBody` — as `RawFrame`s wherever it does
not need to decode them — the runtime answers `RESP_HEAD`/`RESP_BODY` under the
response credit window, and the transport writes `RESP_END_COST` + `DONE`. The
boot's discovery gate, the `(st_dev, st_ino)` pins and the probe child live in
that crate too (`discovery`, `pinning`, `probe`); `blastit`'s stage 0 only
builds their inputs and formats their sentences. The
request body is buffered in full before PHP starts (the C ABI takes one slice),
bounded by `max_body_bytes`; `app.output` lines from the request thread reach
the log pipes like any view's stdout or stderr.

**The load is fd-verified twice.** Stage 0 resolves the host's NTS `libphp`
(static `.dynsym` inspection: `executor_globals` present, `tsrm_startup` absent)
and the matching per-PHP-minor shim (`libblastit_php8X-<arch>-<libc>.so`) by
opening and `fstat`ing them, records `(path, st_dev, st_ino)` in the
configuration, and never `dlopen`s. Stage 1 re-opens each
`O_RDONLY|O_CLOEXEC|O_NOFOLLOW`, asserts the identity against that record, and
passes the descriptors to the child in `ChildIo.keep`; the child `dlopen`s
`/proc/self/fd/<n>` (libphp `RTLD_NOW|RTLD_GLOBAL`, shim
`RTLD_NOW|RTLD_LOCAL`), so a file swapped after inspection cannot be loaded.
No `libphp` symbol is reachable in init, http, scheduler or dbproxy, and there
is no path variant to fall back to: a host without a usable NTS `libphp` is a
boot refusal naming the package (`libphp-embed`, `php85-embed`, `php-embedded`)
or `BLASTIT_LIBPHP`, never a degraded mode. Before the listener binds, stage 1
forks one contained probe child per `entrypoint php` view (`EmbeddedView`, the
route's uid, `PR_SET_PDEATHSIG`, `BLASTIT_PROBE_TIMEOUT_MS`) that runs
`blastit_php_worker_init` + `shutdown`; a shim that cannot start is that boot's
`exit 1`, not a first request's fault.

**`$_SERVER` is C memory, not a request stream.** The shim builds it from the
request and the manifest's fixed facts; `SCRIPT_FILENAME` and `DOCUMENT_ROOT`
come from `entry` and the application root at boot, never from the path or a
header. There is no argv, no FastCGI `PHP_VALUE`/`PHP_ADMIN_VALUE`, no
`REDIRECT_STATUS`, and a `Proxy:` header is dropped before it could become
`HTTP_PROXY`. The `ZEND_INI_SYSTEM` blob — `open_basedir`, `post_max_size` and
`upload_max_filesize`, `disable_functions` (`exec`, `shell_exec`, `proc_open`,
`dl`, the `pcntl`/`posix` process calls), `phar.readonly`/`phar.require_hash`,
`ffi.enable=0`, `allow_url_include=0`, the opcache and assertion switches — is
rendered once from `IniPolicy` and cannot be changed per request.
`php_module_startup` runs once per forked worker, on its own main thread, after
the privilege drop; the seccomp role (`SeccompRole::EmbeddedView { egress }`)
keeps every view rule and additionally refuses `execve`, `execveat`, `fork`,
`vfork` and a process-creating `clone` (§7.3).

**What it costs.** The shim measures `CLOCK_PROCESS_CPUTIME_ID` around
`php_request_startup`..`php_request_shutdown` outside PHP userland, so `cpu_ns`
cannot be forged by a script; a zero reading is `null`, never "free". A
contained `exit()`/`die()`/fatal trapped by `zend_first_try`/`zend_catch` ends
the request as `DoneOutcome::Bailed` (§2.4): the scheduler charges its wall time,
and `view_died` keeps meaning only that the process died.

### 2.7 The Rust workspace, crate by crate

`members = ["crates/*"]`; the rows are the members, the one thing each owns, and the direct
dependencies that decide where it can be built. "Any" means the crate's own code is
platform-neutral — its transitive C library (`pg_query`'s `libpg_query`) is compiled by `cc` on
each target, which is why a build host needs a C toolchain and `libclang` and an installed
artifact needs neither.

| Crate | Owns | Direct dependencies | Builds on |
|---|---|---|---|
| `blastit` | the composition root: `cli`, stage 0/1, the supervisor, `forking.rs` and every role below; **no per-language worker code, zero `pyo3`** | `blastit-dbproxy`, `blastit-env`, `blastit-events`, `blastit-frame`, `blastit-http`, `blastit-ingress`, `blastit-lsp`, `blastit-manifest`, `blastit-scheduler`, `blastit-sqlpolicy`, `blastit-sys`, `blastit-view`, optional `blastit-py-sapi` (`python`) and `blastit-view-php` (`php`), plus `libc`, `rustix`, `tokio`, `serde`, `serde_json`, `thiserror` | Linux |
| `blastit-sys` | the OS surface: fork/exec, uids, capabilities, seccomp, signals, `fanotify`, `/proc`, `elf`, `pin` (verified fd-pinning) | `libc`, `rustix`, `thiserror` | Linux |
| `blastit-dbproxy` | the PostgreSQL broker library: per-route listener, the query allowlist (`blastit --role=dbproxy`) | `async-trait`, `blastit-events`, `blastit-sqlpolicy`, `blastit-sys`, `pregresql`, `rustix`, `serde`, `serde_json`, `thiserror`, `tokio`, `zeroize`, `tokio-postgres` (dev) | Linux |
| `blastit-scheduler` | the routing table, the worker budget, fair queuing, the static mount's admission and dispatch (`static` feature) | `blastit-events`, `blastit-frame`, `blastit-ingress`, `blastit-manifest`, `blastit-view`, optional `blastit-view-static` (`static`, default here), `rustix`, `serde_json`, `thiserror`, `tokio` | Linux |
| `blastit-http` | HTTP/1.1 head parsing, the listener, the client-address resolver | `blastit-events`, `blastit-frame`, `httparse`, `thiserror`, `tokio` | Linux (`#[cfg(unix)]`) |
| `blastit-view` | **transport only**: `wire`, `flow`, `stream`, `loaded`, `error`, `config`, `sync`, `path` (the one request-target decoder) and `spawn` (the fork seam); no worker, no `pyo3`, no SAPI | `blastit-frame`, `blastit-sys`, `thiserror` | Linux |
| `blastit-view-python` | the WSGI worker: `environ`, `input`, `request`, `response`, `serve` (the pool), `loaded` (the Python tracker), `immortal`, `zygote` (the fork and child half) | `blastit-frame`, `blastit-sys`, `blastit-view`, `pyo3` | Linux; linked by `blastit-py-shim` only |
| `blastit-py-sapi` | stage-0 Python discovery (`pyvenv.cfg`/ELF gate), `blastit_py_abi_v1` types, and the stage-1 `dlopen` loader (zero `pyo3`) | `blastit-manifest`, `blastit-sys`, `rustix`, `serde`, `serde_json`, `thiserror` | Linux; `blastit` feature `python` |
| `blastit-py-shim` | per-minor `libblastit_py3{13,14}-<arch>-<libc>.so` `cdylib` shim (`dlopen`ed in stage 1, never linked into `blastit`) | `blastit-py-sapi`, `blastit-sys`, `blastit-view`, `blastit-view-python`, `pyo3`, `serde`, `serde_json` | Linux (`make py-sapi`) |
| `blastit-view-php` | the embedded worker: `worker` (`PhpEmbedRuntime`, `view.chan` loop), `discovery` (stage-0 ELF gate), `pinning` (library/shim + entry pins), `probe` (the boot probe child), `spawn` (the PHP fork and child half) | `blastit-frame`, `blastit-manifest`, `blastit-php-sapi`, `blastit-sys`, `blastit-view`, `rustix`, `serde_json` | Linux; `blastit` feature `php` |
| `blastit-view-static` | `static_asset` (`openat2` resolution), `static_response` (MIME/`ETag`/`immutable`), `rewrite` (SPA answer) — a library the scheduler calls, no process | `blastit-manifest`, `blastit-view`, `rustix`, `thiserror` | Linux; `blastit-scheduler` feature `static` |
| `blastit-php-sapi` | the C SAPI shim's Rust half (the shim is `dlopen`ed, never linked) | none at runtime — a build script compiles the per-minor C ABI | Linux |
| `blastit-env` | the `BLASTIT_*` / `ALWAYS_STRIP` barrier and the swapped `envp` | `blastit-manifest`, `percent-encoding`, `thiserror`, `url`, `zeroize` | any |
| `blastit-manifest` | the `Blastfile`: lexer, parser, model, `validate`, `fmt`, warning sites, and the query vocabulary it re-exports from `blastit-sqlpolicy` | `blastit-ingress`, `blastit-sqlpolicy`, `regex`, `serde`, `thiserror` | any |
| `blastit-sqlpolicy` | the SQL policy: `pg_query` parse, `parameterize`, table grants | `pg_query`, `serde`, `serde_json` | any |
| `blastit-ingress` | body codecs and the ingress policy type | `percent-encoding`, `serde`, `serde_json`, `thiserror`, `url` | any |
| `blastit-frame` | the internal frame codec (`RawFrame`, `Ctl`) | `thiserror` | any |
| `blastit-events` | the log and event shapes | `serde_json`, `thiserror`, `time` | any |
| **`blastit-lsp`** | the **editor surface** only: the LSP 3.17 subset on stdin/stdout for `Blastfile`s — bin `blastit-lsp`, lib `blastit_lsp::run()` | **exactly** `blastit-manifest` and `serde_json` | **Linux, macOS, Windows** — five targets |

**The editor surface is cross-platform; serving is Linux-only.** `blastit-lsp` is the one artifact
Blastit publishes for a developer's own machine rather than for a server: it reads framed LSP on
stdin, writes it on stdout, opens no file or socket **in its own code**, executes nothing, and
links no crate that can acquire a uid, a capability or a seccomp filter — `blastit-sys`, `rustix`,
`nix`, the `libc` crate, `pyo3` and `blastit-php-sapi` are absent from its dependency graph. That
is what makes the portable build honest (`docs/design/blastfile-lsp-zed.md` §1.3) and why it
**cannot become a serving binary by accident**: the serving crates are not in its graph, so there
is no listener to bind and no privilege to drop. It is supported unprivileged only: the release
ships no file capabilities and no page installs it with `setcap` or `sudo`, and the linked
manifest crate's file API and `libpg_query`'s parse-only C are in the image (the honest claim is
about the code path the server runs, not about every symbol in the binary). `blastit lsp` is a
delegation to the same `blastit_lsp::run()`
(`crates/blastit/src/lib.rs`: `pub use blastit_lsp as lsp;`), so Linux has one server, not two.
Everything in §2.1–§2.6 — uids, `execve`, seccomp, `libphp` — exists only on Linux, and the
editor user must not read this table as a promise of a Windows server.

---

## 3. Process tree and channels

```mermaid
flowchart TB
    lb(["Load balancer<br/>HTTP/1.1, TLS terminated upstream"])
    pg[("PostgreSQL<br/>e.g. DO Managed Database")]

    subgraph container["Container (gVisor sentry)"]
        init["<b>init</b><br/>PID 1 · uid 0<br/>reaper · supervisor · zygote · log collector"]
        http["<b>http</b><br/>uid B+1 · tokio · no Python"]
        sched["<b>scheduler</b><br/>uid B+2 · manifest routing table"]
        subgraph views["View processes (lazy, one per manifest route)"]
            v0["<b>view 0</b><br/>uid B+1000"]
            v1["<b>view 1</b><br/>uid B+1001"]
            vn["<b>view N</b><br/>uid B+1000+N"]
        end
        dbproxy["<b>dbproxy</b><br/>uid B+3 · one listener per view<br/>listener + SCM_CREDENTIALS → allowlist"]
    end

    lb -- "TCP 0.0.0.0:$PORT" --> http
    http <-- "① http.chan" --> sched
    sched <-- "② ctl.chan<br/>SPAWN / SPAWNED+fd / RETIRE / EXITED" --> init
    sched <-- "③ view.chan" --> v0
    sched <-- "③" --> v1
    sched <-- "③" --> vn
    v0 -- "④ /proc/self/cwd/.s.PGSQL.5432<br/>(its own 0600 socket)" --> dbproxy
    v1 -- "④" --> dbproxy
    vn -. "④ no listener if no grant" .-> dbproxy
    dbproxy -- "⑤ TCP + TLS, real credentials" --> pg

    init -. "fork + execve, empty env (stage 0)" .-> dbproxy
    init -. "fork (pre-Python)" .-> http
    init -. "fork (pre-Python)" .-> sched
    init -. "fork × N" .-> views
```

Solid arrows are data channels; dotted arrows from init are parenthood (who
forks whom). `B` is `BLASTIT_UID_BASE`.

| # | Channel | Type | Created by | Ends | Carries |
|---|---|---|---|---|---|
| ① | `http.chan` | `socketpair(AF_UNIX, SOCK_STREAM)` | init | http ↔ scheduler | request/response frames, muxed by `stream_id` |
| ② | `ctl.chan` | `socketpair(AF_UNIX, SOCK_STREAM)` | init | init ↔ scheduler | `SPAWN`, `SPAWNED`+fd (`SCM_RIGHTS`), `RETIRE`, `EXITED`, `DRAIN`, `RELOAD` |
| ③ | `view.chan` | `socketpair(AF_UNIX, SOCK_STREAM)`, one per view process | init, at spawn | scheduler ↔ view | request/response frames, muxed by `req_id`; `DONE` |
| ④ | pg sockets | named Unix sockets, one per (view, store) with a grant, in `/run/blastit/views/<uid>/` (`0700`), each `0600` and owned by the view uid, `SO_PASSCRED` set | init (stage 0), then inherited by dbproxy | one view → dbproxy | PostgreSQL wire protocol; identity = which listener, checked against `SCM_CREDENTIALS` |
| ⑤ | upstream | TCP (+TLS) | dbproxy | dbproxy → PostgreSQL | PostgreSQL wire protocol, real role |
| — | `secrets.pipe` | `pipe2`, inherited across dbproxy's `execve` | init stage 0 | init → dbproxy, one-shot | each store's inputs (real credentials), listener fd → (view, store) table, the file's grants |
| — | log pipes | `pipe2`: stdout + stderr for every child, plus an events pipe for http, scheduler and dbproxy | init | child → init | app output and structured events; init is the only writer to stdout (§8.1) |
| — | listen socket | TCP, `SO_REUSEADDR`, backlog `BLASTIT_BACKLOG` | init | → http | inbound connections |

Rules that apply to every channel:

* **Internal traffic never uses TCP.** Unix sockets and socketpairs work the
  same everywhere. They also avoid gVisor's empty-netns and loopback problem
  (experiment 3, finding 2). DO does provide `lo`, but nothing depends on it.
* **Every fd is `O_CLOEXEC`, and each child closes everything it doesn't
  own** right after the fork. A view inherits its own `view.chan` end, its log
  pipes and nothing else. In particular, it doesn't get the listening socket,
  `http.chan`, `ctl.chan` or other views' channels.
* **Socketpairs, not named sockets**, wherever both ends are known at fork
  time. Nothing can connect to them later. The pg sockets are the only named
  sockets, because Python drivers need a path. There, a per-view socket with
  file permissions plays the role that fd inheritance plays elsewhere.
* **Never trust `SO_PEERCRED` under gVisor.** It returns the local socket's
  own credentials. Use `SCM_CREDENTIALS` (`SO_PASSCRED`) or the identity of
  the endpoint.
* **`RELOAD` is one all-views transaction.** init sends the empty message with
  no route; the scheduler retires every idle worker of every route and marks
  the busy ones stale, so no two views ever serve different versions of a
  shared or preloaded module.

### 3.1 Framing

Channels ① and ③ use the same frame format: `u32 length | u8 type | u32 id |
payload`, in little-endian, with a hard maximum frame size of
`BLASTIT_FRAME_MAX` (default 1 MiB). Larger bodies are split into several
`*_BODY` frames. The codec is `blastit-frame`'s; the flow-control primitives,
the loaded-file datagram and the one request-target decoder are
`blastit-view`'s, and no view crate implements a second framing (§2.7).

| Direction | Frames |
|---|---|
| towards the app | `REQ_HEAD {method, target, headers, remote_addr, peer_addr, scheme, server_name/port}`, `REQ_BODY {bytes}`, `REQ_END`, `CANCEL` |
| towards the client | `RESP_HEAD {status, reason, headers}`, `RESP_BODY {bytes}`, `RESP_END` / `RESP_END_COST {cost_ns, cpu_present, cpu_ns}`, `RESP_ABORT {reason}` |
| view → scheduler only | `DONE {req_id, outcome}`, which releases the worker slot; `READY` (id 0), once, when the view can take requests |
| both ways | `WINDOW {bytes}`: flow-control credit for this stream's body (see below) |
| `ctl.chan`, init → scheduler | `RELOAD` (`0x45`), empty payload, id 0: every view reloads together (§3) |

`RESP_END` and `RESP_END_COST` are one frame kind with two wire forms: a view
that measured what the request cost it sends the second, one that has nothing
to report — an older build, a response the view's I/O thread wrote itself —
sends the first. The scheduler accepts both, so the two ends are not lockstep
and a lost report is a fallback rather than a protocol error. A code this
version does not know is still forwarded byte for byte by the raw layer.

`RESP_END_COST`'s `cpu_ns` is optional and says whether it is there
(`cpu_present` is 0 or 1): a view whose interpreter has no
`time.thread_time_ns` reports the absence of a CPU measurement rather than a
zero it did not take. The scheduler clamps `cost_ns` between the time up to
this view's first response frame and the time it observed itself, and only
reports `cpu_ns` — never schedules on it.

`DONE`'s `outcome` is `completed`, `aborted`, `errored` or — since `MB.3.41` —
`bailed`: a contained `exit()`/`die()`/fatal the SAPI shim trapped, so the
handler never returned. It is the one fact a response's status cannot carry,
and both a `5xx` an app answers itself and a `5xx` a dying handler leaves behind
look identical on `RESP_HEAD`. So a `5xx` a view answered is labelled and
charged when its `DONE` arrives (one frame later, on the same channel): `bailed`
for the crash, and `ok` with that status for an answer. See "What a request
costs" above for the three charge rules.

`REQ_HEAD`'s `peer_addr` is the address of the socket the request arrived on,
exactly as the kernel reported it — `ip:port`, or an address alone for a peer
that is not one. It is never a header's value, so a client cannot forge it, and
it is what the trust decision for `remote_addr` and for the fair scheduler's
flows is made on. `remote_addr` stays what the app sees in `REMOTE_ADDR` — the
address alone, whoever the client turned out to be: the peer's IP, normalised
(an IPv4-mapped IPv6 address is IPv4, an IPv6 zone is dropped), or the address a
trusted client-IP chain names. **No port is ever in it.** CGI's split is kept:
the port the connection arrived from is a key of its own, `REMOTE_PORT`, which
the view reports from `peer_addr` when the peer has a port (§2.5).

The scheduler rewrites ids between ① and ③. A view only ever sees its own
`req_id`s and cannot address any other stream. Bodies stream in both
directions. A view starts work as soon as `REQ_HEAD` arrives, and `wsgi.input`
blocks until body frames arrive. This handles large uploads (DO closes uploads
at 600 s) without buffering them in memory.

**Flow control.** The channels are shared by many streams, so no stream may
make a reader wait: every reader always keeps reading, and every per-stream
buffer is bounded by a credit window instead.

* Each body, in each direction, starts with `INITIAL_WINDOW` (1 MiB) of
  credit. A sender never sends body bytes beyond its credit.
* Request bodies: the view sends `WINDOW` as the app consumes `wsgi.input`;
  the scheduler forwards it to http, which reads more from that client's
  socket only when it has credit. A slow app slows its own client down, over
  TCP, and nothing else.
* Response bodies: http sends `WINDOW` as it writes to the client socket. It
  does not return every byte as it goes: a view's remaining credit is allowed
  to fall to a 64 KiB low-water mark, and it is then granted everything owed in
  one frame, so a response smaller than that never costs a `WINDOW` at all. The
  scheduler forwards the grant to the view, where a request thread that runs out
  of credit blocks in its write (as it would on a socket). A slow client slows
  its own request thread down, and nothing else.
* A peer that sends beyond its credit has broken the protocol: a view is
  recycled, and http aborts the stream.

**Batching.** Frames are small and the work per request is dominated by the
syscalls and wakeups around them, so neither side writes one frame and waits:
a writer drains what is already queued on its channel into one buffer and
writes it with a single `write_all`, capped at 64 frames or 256 KiB so a
`CANCEL` behind a burst waits for one batch and no more. A `REQ_HEAD` goes to
the view as the bytes http sent, decoded only as far as routing and the
fair-queue key need, and the front end turns Nagle's algorithm off on each
accepted socket: nothing on these connections gains from coalescing a write
that is already whole.

### 3.2 Request lifecycle

```mermaid
sequenceDiagram
    participant C as load balancer
    participant H as http
    participant S as scheduler
    participant V as view process<br/>(uid of the view)
    participant T as request thread
    participant D as dbproxy
    participant P as PostgreSQL

    C->>H: HTTP/1.1 request
    H->>H: parse, enforce limits, normalise headers
    H->>S: REQ_HEAD (+ REQ_BODY…, REQ_END)
    S->>S: match the path against the file's patterns → view (404 if none)
    alt worker budget exhausted
        S->>S: wait in queue (BLASTIT_QUEUE_TIMEOUT)
        S-->>H: 503 if the queue is full or the wait times out
    end
    opt no live process for the view
        S->>S: SPAWN via init, see §4
    end
    S->>V: REQ_HEAD (req_id rewritten)
    V->>T: pooled request thread, attached to the interpreter
    T->>T: build standard WSGI environ
    T->>T: app(environ, start_response): the app routes it
    T->>D: connect /proc/self/cwd/.s.PGSQL.5432 (only its uid may)
    D->>D: listener → view, SCM_CREDENTIALS uid must match
    D->>P: connect with the real role over TLS
    loop each statement
        T->>D: Parse / Bind / Execute (or Query)
        alt granted to the view (its own grants, or a trait it uses)
            D->>P: forward
            P-->>D: result
            D-->>T: result
        else not allowed
            D-->>T: ErrorResponse 42501, never sent upstream
            D->>D: log sql.denied {route, proxy, sql}
        end
    end
    T-->>V: status, headers, body chunks
    V-->>S: RESP_HEAD, RESP_BODY…, RESP_END
    S-->>H: RESP_HEAD, RESP_BODY…, RESP_END
    H-->>C: encoded HTTP/1.1 response
    S->>S: log request {method, path, route, status, durations}
    T->>T: iterable.close() · request_finished · DB connection closed
    V->>S: DONE {req_id}: worker slot released
```

---

## 4. Startup sequence

Startup has two stages separated by an **`execve` barrier**. The real secrets
are read in stage 0, handed to dbproxy, and then gone from the process image
for good. The application is only loaded in stage 1, so even the original
`envp` block (which `fork` would otherwise copy into every view) has never
held a real credential there.

The barrier isn't optional. Under gVisor, **any uid can read any process's
`/proc/<pid>/environ`** (and `cmdline`, `maps`, `auxv`, the fd list), and
`PR_SET_DUMPABLE(0)` doesn't change that. The file shows the process's
*initial* environment block, which `fork` inherits and only `execve`
replaces. The spike confirmed both halves: a forked child keeps showing its
parent's original block, and after an `execve` the old variables are gone. This matters because DO delivers every
app-spec variable, `SECRET`s included, in PID 1's environment. The spike
proved the scheme end to end with a real DO `SECRET`: after the barrier, a
view uid found it neither in any `environ`/`cmdline`, nor in its own
inherited memory, nor on disk, while a deliberately un-exec'd control child
leaked it
([`docs/spikes/2026-10-do-app-platform.md`](docs/spikes/2026-10-do-app-platform.md#a-do-injected-secret-hidden-from-view-uids)).
So two rules apply:

* No process may ever have a real secret in its `environ` or `argv`.
* dbproxy is `fork` + `execve`'d with an **empty** environment and gets the
  secrets over a pipe, not by plain `fork`.

```mermaid
sequenceDiagram
    autonumber
    participant I0 as init (stage 0)<br/>uid 0 · real env
    participant D as dbproxy
    participant I1 as init (stage 1)<br/>uid 0 · swapped env
    participant H as http
    participant S as scheduler
    participant V as view processes

    Note over I0: single-threaded
    I0->>I0: read env, parse BLASTIT_*, parse + validate manifest
    I0->>I0: read each store's variables from env → upstream config<br/>render the local endpoint (§5)
    I0->>I0: build uid table (http, scheduler, dbproxy, one per route)
    I0->>I0: create /run/blastit/views/<uid> (0700, owned by each view uid)
    I0->>I0: bind one pg listener per (route, proxy) granted<br/>chown to the view uid, 0600, SO_PASSCRED
    I0->>D: fork → setuid(B+3) → execve(--role=dbproxy, env = {})
    activate D
    Note over D: inherits only secrets.pipe, the pg listeners, log pipes<br/>fresh image: its /proc/<pid>/environ is empty
    I0->>D: secrets.pipe: proxy inputs + listener table + grants (then closed)
    Note over D: zeroize buffer · start tokio<br/>serve the inherited listeners
    D-->>I0: ready
    I0->>I0: app env = platform env − inputs − BLASTIT_* + outputs (§5)
    I0->>I1: execve(/proc/self/exe, --stage=1, swapped_env)
    Note over I0,I1: execve barrier: address space replaced, no secret survives,<br/>still PID 1, dbproxy is still its child (pid passed in argv)

    I1->>I1: bind 0.0.0.0:$PORT
    I1->>H: fork (before CPython is initialised)
    activate H
    Note over H: setuid(B+1) · start tokio<br/>serve listen fd ↔ http.chan
    I1->>S: fork (before CPython is initialised)
    activate S
    Note over S: setuid(B+2) · compile the file's patterns<br/>serve http.chan ↔ ctl.chan
    I1->>I1: init CPython · import BLASTIT_APP and BLASTIT_PRELOAD modules<br/>run BLASTIT_WARMUP (optional)
    I1->>I1: close DB connections · gc.collect()
    I1->>I1: immortalize every reachable object (§9.2) · gc.freeze()
    opt BLASTIT_VIEW_SPAWN=eager, or views that declare prefork
        I1->>V: fork × N (see below)
    end
    Note over I1: supervise: waitpid loop, ctl.chan, log pipes, signals
```

Two things hold at every step: init is single-threaded whenever it forks, and
no secret exists in stage 1.

Every view process is created the same way, whether at startup in eager mode
or on demand. The fork itself is `blastit-view-python`'s `zygote` for a WSGI
view and `blastit-view-php`'s `spawn` for a PHP view; `blastit`'s `forking.rs`
is the one `fork_for` they are handed, and the ordered child sequence below is
`blastit_sys::child::enter`.

```mermaid
sequenceDiagram
    participant S as scheduler
    participant I as init (zygote)
    participant V as new view process

    S->>I: ctl.chan: SPAWN {view}
    I->>I: socketpair() → view.chan
    I->>V: fork
    activate V
    I->>S: ctl.chan: SPAWNED {view, pid} + scheduler end (SCM_RIGHTS)
    I->>I: close its copy of both ends
    Note over V: close all fds except its view.chan end and log pipes<br/>PyOS_AfterFork_Child() (GIL held, §9.3)<br/>setrlimit NPROC (threads + margin), NOFILE, CORE=0<br/>umask 077 · chdir + HOME + TMPDIR = /run/blastit/views/<uid><br/>drop capabilities: ambient and inheritable first, then the bounding<br/>set while cap_setpcap is still effective (skipped without it, M1);<br/>then setgroups([]) · setgid(uid) · setuid(uid); then permitted and effective<br/>PR_SET_PDEATHSIG SIGKILL · PR_SET_NO_NEW_PRIVS<br/>seccomp filter (§7.3)
    V->>S: view.chan: ready
    Note over S: queued requests for the view are dispatched
    V--xI: exit (crash, or kill on RETIRE)
    I->>S: ctl.chan: EXITED {view, pid, status}
    deactivate V
```

Every child drops its capability sets itself, in the order the kernel requires,
and checks that they are gone. The ambient set goes first, because the kernel's
invariant is `ambient ⊆ permitted ∩ inheritable` and a `capset` that broke it
would be refused; then the inheritable set, and the bounding set when
`cap_setpcap` is held, because dropping a bounding capability needs it in the
**effective** set and a root process loses that set the moment `setresuid` leaves
uid 0; then `setgroups`, `setresgid` and `setresuid`; then the permitted and
effective sets.

A rootless init is why this is explicit rather than implied by the uid change:
the kernel clears a process's capability sets only when all of its uids cross 0,
and a rootless init's uid never was 0, so a child that dropped nothing would keep
`cap_setuid` — a way back to any uid in the container. The check is part of the
drop: every set the process *holds* must be empty afterwards, and a child whose
sets are not does not start. The bounding set is checked when it was dropped, and
reported as kept when it could not be (M1).

If anything in a stage fails, the whole container exits non-zero. A
half-started sandbox is worse than a crash loop that the platform can see.

**The serve path's manifest gates are a value, not a statement.** The production
serve path parses and validates the manifest, then takes its gates before it
binds a view socket: `ServeGates::clear` is the only constructor and it *is*
`raw_ingress_gate` (`ADR-0011`), and `bind_view_sockets` takes `&ServeGates`. A
boot therefore cannot bind without the gates having passed in this process, and
deleting the call site is a compile error rather than a silent fail-open. The
gate's process behaviour is measured through stage 1's re-read of the same
manifest (`critical_raw_ingress`), because an unprivileged `cargo test` cannot
reach a stage-0 gate that lives after the capability check; the ordering half is
the type, which is why it is the witness.

---

## 5. Proxies and environment variable swapping

Configuration comes **only** from environment variables. Before stage 1, init
rewrites the environment that the application will see. The rewrite is
driven by the store sections of the `Blastfile` (§6).

### 5.1 Stores

A **store** is a named broker for one external service. The protocol is the
keyword, and the file names the variables it owns without holding their values:

```text
postgres $DATABASE_URL                 # the default store, one libpq URL
postgres analytics $ANALYTICS_URL      # a named second store
postgres {                             # the default store, split slots
    host $DB_HOST
    port $DB_PORT
    user $DB_USERNAME
    password $DB_PASSWORD
    database $DB_DATABASE
}
```

With DO's environment, the app ends up seeing:

```
platform: DATABASE_URL=postgresql://doadmin:s3cr3t@db-xyz.db.ondigitalocean.com:25060/defaultdb?sslmode=require
          ANALYTICS_URL=postgresql://reader:t0ps3cr3t@db-abc.db.ondigitalocean.com:25060/analytics?sslmode=require
app:      DATABASE_URL=postgresql://blastit@/defaultdb?host=/proc/self/cwd&port=5433
          ANALYTICS_URL=postgresql://blastit@/analytics?host=/proc/self/cwd&port=5432
```

**Rules:**
* **Every input variable keeps its name and gets the local value.** A slot's
  variable is the `$IDENT` written beside it, and `password` and `ca_cert` are
  removed because they have no local value. The real values are read once, in
  stage 0, and handed to dbproxy over a pipe.
* **A URL or the split slots, never both.** A URL's query parameters are
  allowlisted, so a URL cannot smuggle a session setting or a credential past
  the broker.
* **A grant names a store, and a store is declared once.** Routes and traits
  grant `postgres { … }`, `postgres analytics { … }`, and so on (§6); a grant on
  a store the file never declared aborts the boot.
* **The file holds no credential.** A `Blastfile` carrying a password — or a
  quoted literal where `$IDENT` belongs — is a syntax error, so no credential
  can be committed with the policy.

The local endpoint is **rendered once, in stage 0**, and the fields it is built
from describe only the *local* endpoint plus a few non-secret facts. Passwords,
upstream hosts and CA material never reach the renderer, so no rendered value
can leak them.

### 5.2 Protocol `postgresql`

Served by dbproxy (§2.2). One listener per (route, proxy) for routes that
have a grant on that proxy. Each `postgresql` proxy gets its own local port:
`5432 + k`, *k* being the proxy's index among the `postgresql` proxies sorted
by name (so the order a file declares its stores in doesn't matter).

| Input slot | Content | Notes |
|---|---|---|
| `url` | libpq URI (`postgres://` or `postgresql://`) | the usual case, DO's `${db.DATABASE_URL}` |
| `host`, `port`, `user`, `password`, `database`, `sslmode` | individual parameters | an alternative to `url` (DO's split bindables); setting both forms is an error |
| `ca_cert` | PEM text | optional upstream CA, DO's `${db.CA_CERT}` |

| Template field | Value |
|---|---|
| `url` | `postgresql://{{ user }}@/{{ database }}?host={{ socket_dir }}&port={{ port }}` — the socket directory in a **query parameter**, the one spelling libpq, `dj-database-url` and SQLAlchemy all resolve |
| `socket_dir` | `/proc/self/cwd`: each view's working directory is its own socket directory (§2.2) |
| `port` | this proxy's local port (`5432 + k`) |
| `socket_path` | `/proc/self/cwd/.s.PGSQL.<port>` |
| `database` | the upstream database name (not a secret) |
| `user` | `blastit`; the proxy ignores it, because identity is the socket |

**Default outputs:**
* `url` → the same variable, rendered as `{{ url }}`;
* `host` → `{{ socket_dir }}`, `port` → `{{ port }}`, `user` → `{{ user }}`,
  `database` → `{{ database }}`;
* `sslmode` → `disable`, since the local leg is a Unix socket;
* `password` and `ca_cert` → removed.

**Grant contents:** the exact `query` lines and whether `transactions` is granted (§6).

The socket directory travels in a **query parameter** because that is the one
spelling every consumer resolves: libpq reads `host` and `port` from the query
when the authority carries none, `dj-database-url` passes unknown parameters
through to the driver, and SQLAlchemy's `postgresql://` and
`postgresql+psycopg://` dialects hand them to it as keyword arguments. The
percent-encoded authority form is what libpq and `dj-database-url` write, and
SQLAlchemy does not decode it — it looks up the literal `%2Fproc%2Fself%2Fcwd`
as a host. libpq connects to `<host>/.s.PGSQL.<port>`, so the **port selects the
proxy** and `/proc/self/cwd` selects the view. The rendered values are identical
in every process. That matters because frameworks read their database settings
once, in init, before the fork. No output contains a password, because dbproxy
authenticates by socket identity.

### 5.3 The rest of the environment

1. **Strip.** All `BLASTIT_*` variables, every store input, and anything
   listed in `BLASTIT_ENV_STRIP` (for example `PGPASSWORD`, or DO bindables
   you injected but don't route through a store) are removed.
2. **Inject.** The process-safety defaults below are added, unless the
   variable is already set:

   | Variable | Value | Why |
   |---|---|---|
   | `OPENBLAS_NUM_THREADS`, `OMP_NUM_THREADS`, `MKL_NUM_THREADS`, `NUMEXPR_NUM_THREADS`, `VECLIB_MAXIMUM_THREADS` | `1` | thread pools don't survive `fork` (§9.3) |
   | `PYTHONDONTWRITEBYTECODE` | `1` | views can't write to the code tree |
   | `LC_ALL` | `C.UTF-8` | no locale files needed |
   | `HOME`, `TMPDIR` | replaced per view | private scratch directory |

Every path on that interpreter's `sys.path` — its own entries, the working
directory blastit was started in, whatever `PYTHONPATH` names, whatever a `.pth`
file added — is checked at boot, and so is **every directory above one up to
`/`**: a uid blastit drops to that may write any of them can plant a module
another route imports, or `rmdir` an entry and put its own directory back. A
sticky directory above an entry (`/tmp` is the one every deployment has) is not
a way in when the entry below it belongs to nobody blastit drops to, and it is
one when the entry is missing — a sticky directory lets a new name through. The
kernel is asked, once per uid; the boot refuses and names the path to fix, and a
`sys.path` that does not answer within a second stops the boot rather than
hanging it.

Stage 1 runs `python -I`, which implies `-E` and `-P`: the interpreter ignores
every `PYTHON*` variable, and it starts with neither `PYTHONPATH` nor the
working directory on `sys.path`. Two things are put back before the first import
(D2). The console script's own entries — what stage 0 recorded — are restored in
front, so the application's modules are found where the user's interpreter found
them. And the **working directory blastit was started in goes first**: gunicorn,
uwsgi and granian all make the directory they were started in importable, so a
project that is not installed serves its own `wsgi:application` with no
`PYTHONPATH`, and `hello:application` is found in `hello.py` next to the
operator. Neither entry crosses the barrier the other way: no `PYTHON*` variable
changes how the interpreter starts, which is why the `PYTHONDONTWRITEBYTECODE`
above does not reach it — the modules stage 1 imports before the fork are
compiled into `__pycache__` directories owned by the user blastit started as,
which is why a wheel serves an application directory it may write to.

Proxies are the general pattern for every future capability: **replace the
credential with a local endpoint served by a broker that checks the
manifest**. A new protocol (Redis, S3-style object storage, outbound HTTP
APIs) adds a protocol definition: input slots, template fields, default
outputs, grant contents and a broker process. The `Blastfile` syntax doesn't
change. Secrets that the app itself must hold, such as Django's
`SECRET_KEY`, can't be swapped and stay visible to every view.

---

## 6. The `Blastfile`

The `Blastfile` is a file in the image (`BLASTIT_MANIFEST`, default
`./Blastfile`, no upward search). It is versioned with the code, because it
describes the code. dbproxy reads it in stage 0; init and the scheduler read it
in stage 1. Its grammar, keywords and error shapes are the
[Blastfile reference](docs/reference/blastfile.md); this section is what the
rest of the process tree does with it.

`entrypoint python` (with its optional `preload` block), `entrypoint php`, or no
executor at all decide what runs; `BLASTIT_APP` and `BLASTIT_PRELOAD` override
the first two. A view that takes a body declares a codec with `body json`,
`form`, `none` or `raw`: `json` and `form` are decoded and re-encoded before the
app sees them (a body that does not fit is a `400`), `none` rejects any body,
and `raw` passes untouched and also needs `unvalidated`. A body to a view with
no `body` is a `413`.

It is **blastit's own routing table**, independent of the framework. Besides the
stores (§5), it has three kinds of entries.

* **Views** are sandboxes: one process, one uid each. A view says which requests
  go to it (patterns, optionally methods), what it may do, whether it may open
  outbound connections, and whether it is preforked. Its grants are written
  **inside the view** (`postgres { … }`, the usual case), taken from traits, or
  both.
* **Traits** are optional named sets of the same grants, for statements several
  views share. A trait grants nothing until a view attaches it with `use`.
* **Mounts** (`static "/…" { … }`) serve files from one directory and need no
  process of their own.

A grant, in a view or in a trait, is per store (§5): for a `postgres` store, the
statements allowed and whether transaction control is allowed.

The app is never asked anything. It receives ordinary WSGI requests and routes
them itself, exactly as it would behind any other WSGI server.

```text title="Blastfile"
app "myshop"

entrypoint python "myshop.wsgi:application" {   # WSGI callable; BLASTIT_APP overrides it
    preload "myshop.urls"                       # imported once, before the fork
}

postgres $DATABASE_URL                          # see §5 for stores and their variables

postgres analytics $ANALYTICS_URL

# Connection setup, plus what middleware does on every request.
trait session {                                 # grant on the "db" store (§5)
    postgres {
        transactions                            # BEGIN/COMMIT/ROLLBACK, SAVEPOINT/RELEASE sp_*
        query "SET TIME ZONE 'UTC'"
        query 'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $? AND "django_session"."session_key" = $?) LIMIT 21'
    }
}

trait catalog {
    postgres {
        query 'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $? LIMIT 21'
    }
}

view product_detail GET "/products/{id:int}" {  # full match against the path
    use session, catalog                        # shared with checkout; nothing else needed here
    prefork                                     # hot view: fork at startup, skip first-hit latency
}

view checkout POST "/checkout" {
    body form                                   # decoded and re-encoded before the app sees it
    egress any                                  # may open TCP connections (payment API); see §7.3
    use session, catalog
    postgres {                                  # a grant only this view has
        query 'INSERT INTO "shop_order" ("product_id", "qty") VALUES ($?, $?) RETURNING "shop_order"."id"'
    }
}

view admin GET "/admin/*" {                     # staff-only back-office: its SQL is too varied to list
    postgres unrestricted                       # any statement on "db"; flagged at boot
}

view about GET "/about/" {
    postgres {                                  # the view's own block: no trait involved
        query 'SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $? LIMIT 21'
    }
}

view restaurants GET "/restaurants/*" {
    postgres {                                  # instead of listing every statement
        select restaurants_restaurant(id, slug, name)
        select restaurants_cuisinetype          # every column
        select orders_order                     # a bare table reads any column
        insert orders_order
        update orders_order(status, accepted_at)
        delete orders_order
        functions st_distance, unaccent         # beyond the built-in safe set
    }
    postgres analytics {
        select events(id)
    }
}

view fallback "/*" {}                           # catch-all: the app renders its own 404s
                                                # without it, unmatched paths get blastit's own 404
                                                # only the traits it uses; add grants if error pages need the DB
```

* **Effective grants.** A view's allowlist for a store is the union of its own
  block, every trait it `use`s, and nothing else: a `Blastfile` has no
  file-level grant. `transactions` is on if any contributor turns it on;
  `unrestricted` is on if any contributor turns it on, but a *trait* that turns
  it on aborts the boot (below), so in practice it is only ever a view's own. A
  view with no grant for a store gets no listener for it at all (§2.2).
* **Table and column grants.** A `postgres` grant may say which **tables and
  columns** a view may read and write, instead of — or next to — the statements
  it may run. `select <table>(<columns>)` takes a column list, a bare
  `select <table>` reads every column, `insert` and `update` take a column list,
  and `delete <table>` deletes rows; `functions` lists the function calls
  allowed beyond the built-in safe set. A statement that is not listed exactly
  is parsed with PostgreSQL's own parser (`libpg_query` — a second
  implementation would be a parser differential, and any text the two read
  differently is a bypass) and allowed when every table, column and function it
  touches is granted. Anything else is refused: another statement kind, a
  construct the analysis does not understand, a function nobody granted. Reading
  a column anywhere — a `WHERE`, an `ORDER BY`, a join condition — is reading
  it; a bare `select <table>` covers every column it expands to, through derived
  relations; `count(*)` reads no column; an unqualified column has to be granted
  on every relation in scope that could provide it; `public.t` is `t` and any
  other schema keeps its qualified name; `pg_catalog` and `information_schema`
  are never readable this way; a function call must be in the built-in safe set
  or in `functions` (`pg_catalog.x` is `x`). A view's tables are the union of its
  own and its traits', and `unrestricted` next to a table or `functions` grant is
  a boot error. Every base table a statement names must appear in the grant, even
  when no column of it is read (a row count or an EXISTS reveals it); unqualified
  `pg_*` names are never grantable as tables, since `pg_catalog` comes first on
  every search path — statements on the catalogue stay exact `query` lines; casts
  to `reg*` and to types other than the built-in plain ones need a `functions`
  entry. A row lock is the same shape: a locking clause (`FOR UPDATE`, `FOR NO
  KEY UPDATE`, `FOR SHARE`, `FOR KEY SHARE`, with or without `OF …`, and through
  a derived relation) needs an `update` entry on every base table it locks — but
  on **any one column**, because PostgreSQL asks for the UPDATE privilege on a
  single column of a locked table — so `update <table>(id)` is enough for a view
  that only reads the rows it locks, while the columns it reads still need
  `select`; the denial reads `update on t (any column, to lock its rows) not
  granted`.
* **Rewriting a grant from a test suite.** The pytest plugin (D3b) reports, per
  `(view, store)`, the smallest `tables`/`functions` grant covering every
  statement the view ran, as the `blastfile` snippet to paste into that view's
  `postgres` block — the directives `--prefer-tables` prints — so a view can be
  moved from exact `query` lines to table grants in one step. A table a statement only
  locks gets an `update` entry on one column the view already reads (`id` when it
  reads one, otherwise the first), never every column — a lock needs the UPDATE
  privilege on *any one* column, so a bare grant would hand out every one — and
  the snippet says so in a comment. A statement that locks a table without
  reading a column of it (`SELECT 1 FROM t FOR UPDATE`, `SELECT * FROM t FOR
  UPDATE`) names no column at all: the covering grant leaves the `update` entry
  out, the snippet says to name one (the primary key, say), and the statement
  stays an exact `query`, which is what the tooling writes for it.
* **The test database.** A `pytest-django` suite is the one case where the
  secret-free environment is not enough: `django_db_setup` has to create its
  `test_*` database on the real server, or every migration and flush lands in
  the developer's own database. When `pytest-django` is importable and the
  project sets `DJANGO_SETTINGS_MODULE` or has a `./manage.py`, the orchestrator
  writes the real store inputs to a one-shot `0600` file instead; the plugin is
  imported by `-p` *before* Django builds its settings, so it reads that file,
  names it in `DJANGO_SETUP` and unlinks it, and applies the real values only
  long enough for `DATABASES` to be evaluated. `os.environ` is then scrubbed back
  to the secret-free `swapped` values, so no test import or body sees a real
  credential. When `django_db_setup` runs, matching `test_*` aliases are pointed
  at dbproxy's socket, `set_db` names the database to the proxy, and a LIFO
  finalizer on `django_db_setup` closes proxied sessions and restores the saved
  dictionaries just before `teardown_databases` drops `test_*` on the real
  server (R30, ADR-0004).
* **Traits, in the report and in the tooling.** A statement a view is granted
  only through a trait it `use`s is never in that view's `unused` when another
  view that uses the same trait ran it: the report keeps an `unused_global`
  section for a grant no request needed anywhere, and a statement appears there
  only when *no* view matched it — the key stays in the document for `--json`
  consumers. A trait's table grant is reported the same way, once, when no
  observed statement needed it. A view whose only evidence is a live server's log
  (below) reports no `unused` grants at all: the log records the statements that
  were **denied**, never the ones that ran, so no grant of it can be called dead.
* **A live server, served by blastit itself.** A suite that drives a live server
  — Playwright against `live_server` — makes its traffic in a server process of
  its own, which the plugin cannot see from the test process. `blastit_live_server`
  (and `--blastit-live-server`, which serves pytest-django's own `live_server`
  the same way) starts the real blastit on a free port, with the project's
  `Blastfile` and the *test* database, waits for the health path, and on teardown
  folds the log's `request` and `sql.denied` events into the same observations
  the report is built from — so real front-end traffic is checked against the
  file and a missing grant comes out as `missing`. It runs production
  `blastit` with root or passwordless `sudo`, and otherwise `blastit dev` on one
  uid (R39); it skips when the project passes no `--blastit-manifest`, names no
  WSGI callable, has a `postgres` store that does not take its URL from one
  variable, or has no `django_db_setup` fixture to create the test database in.
  It preloads the project's root URLconf, which is what this section asks a
  Django project to do anyway; and it serves the project from a copy in a
  temporary directory when the view uids cannot read it where it lives, never
  changing a mode outside the project. A denial carries a view and no request
  id, so it is attached to the first request the log shows for it.
* **`unrestricted`** in a view's own grant (`postgres unrestricted`) means
  exactly what it says: that view may run any statement on that store,
  transaction control included. It is still a sandbox — the listener, the uid
  check, the other stores' grants and the credential barrier all stay — it
  simply has nothing left to check on this one, so dbproxy logs no statement
  for it. It is accepted **in a view only**: a trait can be attached to many
  views at once, which would turn much of the file into one sandbox, so
  `unrestricted` in a trait is a boot error, and every unrestricted
  `(view, store)` is reported as a `manifest.warning` at boot.
  Its user is a staff-only back-office (Django's admin) whose SQL is too
  varied to list, where the guarantee that remains is that the process still
  never sees the credentials.
  A grant that lists `query` lines next to `unrestricted` is a boot error
  too: the list would read as a restriction, and it isn't one.
* **Validation at boot.** An unknown trait, a duplicate view name, an invalid
  pattern, `unrestricted` in a trait or a grant on an undeclared store aborts
  the boot. A trait no view `use`s, and a view that is unrestricted on a store,
  are reported as warning events.
* **Routing.**
  * Views are tried in file order, and the first one whose method and pattern
    match wins; a `static` mount whose prefix matches and holds the file answers
    before any view (§2.4).
  * A quoted pattern matches the whole path (`{id:int}`, `{id:uuid}` and `*` are
    its three forms); `regex '…'` is a full regular expression. Both use Rust
    `regex` syntax, which guarantees linear-time matching, so a hostile URL
    can't stall the scheduler.
  * `BLASTIT_SCRIPT_NAME` strips a mount prefix first, as WSGI does.
* **Getting the routing wrong is safe, if annoying.** A misrouted request runs
  in a process whose allowlist doesn't fit its statements, so they're denied
  (§2.5), and every denial is logged (§8.1).
* **uid assignment** is `UID_BASE + 1000 + the sandbox's index in the file`.
  Every process derives it from the same file, and init also hands the table to
  dbproxy. A multi-endpoint `view` is one sandbox, so its endpoints share one
  uid. uids only live as long as the container, so reordering the file between
  deploys doesn't matter.

* **Matching** is exact, on the normalised SQL text: trimmed, trailing `;`
  removed, whitespace collapsed (the pregresql demo's rule), and — because a
  real app's SQL is not the same bytes twice — with the *shape* of the text
  normalised too, by the same rules for a grant and for the text on the wire:
  * every bind placeholder `$<digits>` outside quoted regions and comments
    becomes `$?`. The numbering carries no meaning for the policy (values
    always travel separately) and it shifts whenever a statement is assembled
    dynamically, so one template means one grant.
  * a parenthesised list right after `IN`/`NOT IN` that holds nothing but
    placeholders becomes `IN ($?, ...)`, whatever its length, so
    `prefetch_related` and `__in` match one grant for every page size. A list
    with anything else in it — a string, a subquery, a nested tuple — is left
    alone.
  * after `VALUES`, a run of identical rows `(…), (…), …` becomes the single
    row followed by `, ...`, so `bulk_create` matches one grant for every
    number of objects. A single row is left as it is, rows that differ from
    each other do not collapse, and a function call's argument list never
    collapses: its arity picks the overload.
  A `Parse` that matches authorises the `Bind`/`Execute` that follow it, and
  pregresql tracks this. Simple-protocol `Query` messages go through the same
  check, so a route can't get around the policy by switching protocols. `COPY`,
  `LISTEN` and the function-call sub-protocol are denied unless they are
  listed.
* **What the app must do.** blastit can't check these, because it doesn't
  look inside the app; violations show up as `sql.denied` events.
  * **Send parameters separately from the SQL.** psycopg2, or Django's
    default psycopg 3 client-side binding, inlines literals into the SQL text,
    so no allowlist could ever match. Django needs
    `DATABASES[...]["OPTIONS"]["server_side_binding"] = True` with psycopg 3.
  * **Don't keep connections across requests** (`CONN_MAX_AGE = 0` in
    Django). Request threads are thrown away after each request, so a
    persistent connection would be orphaned.
  * **Load lazily-loaded code before the fork.** Frameworks often load their
    URL configuration on the first request. Anything imported only after the
    fork is private to one process. List such modules in `BLASTIT_PRELOAD`
    (for Django, the root URLconf), or touch them in `BLASTIT_WARMUP`.
* **Writing the file.** blastit owns three writers and no fourth: `blastit
  dev --learn` records the views and the exact statements the app ran,
  `blastit check --learn` proposes a view for a path the suite requested, and
  `blastit fmt` puts the result in the canonical layout. Every edit goes through
  the same lossless tree and is re-parsed and validated before it is written, so
  a comment above a view stays above it and a refusal leaves the file untouched.
  Every denied statement is logged with its view, store and normalised SQL (§8.1);
  each `--learn` pass may only reveal the first denial of each request, because a
  denied statement usually aborts the request.

---

## 7. Security model

### 7.1 Trust boundaries

| If this is compromised… | …the attacker gets | …and cannot |
|---|---|---|
| a view (request-time RCE) | that view's allowlisted statements; its scratch dir; egress only if granted; other processes' `environ`/`cmdline`/`maps` (which hold no secrets, §7.2) | run other statements; read real DB credentials; read or ptrace other processes' memory; connect to other views' pg sockets; reach other views' channels; hold more threads than its `RLIMIT_NPROC` |
| http | the raw bytes of in-flight HTTP traffic | reach the DB (its uid isn't in the table); run Python; spawn processes |
| scheduler | it can send any request to any view | do anything a view couldn't already be made to do by an HTTP request |
| dbproxy | the real DB credentials | — this is the crown jewel. It runs no app code, is not dumpable, and is the smallest Rust component we can make it |
| init | everything | — it runs only blastit code after stage 1 imports, and handles no external input |
| import-time app code | root, before any sandboxing | — **out of scope**. Imports must run before the fork for COW sharing. Installed as a wheel, blastit's stage 0 is itself the virtualenv's interpreter, so the venv's `.pth` files and `sitecustomize` run there too, with the real credentials still in the environment: they are import-time code of the same kind |

### 7.2 Mechanisms that work without extra privileges

* **Distinct uids, `gid == uid`, no supplementary groups.** Different uids
  can't signal each other, `ptrace` each other, or read each other's
  `/proc/<pid>/{mem,io,cwd,root}`. The views never get `CAP_SYS_PTRACE`.
* **What gVisor does *not* protect** (measured on DO, where it differs from
  Linux): another uid can read `/proc/<pid>/{environ,cmdline,maps,auxv,stat,
  status}`, list `/proc/<pid>/fd` and `readlink` its entries.
  `PR_SET_DUMPABLE(0)` changes the file owner but not the `0444` mode. So:
  * nothing secret goes in `environ` or `argv` (§4);
  * fd targets (file paths, `socket:[…]`) and memory layouts are visible to
    every view. ASLR between views is meaningless anyway, because they are
    all forks of one image.
* `PR_SET_DUMPABLE(0)` on dbproxy and init still matters on real Linux, where
  it guards `environ` and `mem`.
* **Filesystem.** The code tree is read-only to views (owned by root, mode
  `0755` / `0644`). Each view gets a private `0700` scratch directory and
  `umask 077`. `/tmp` stays `1777` but is no longer the default scratch dir.
* **A boot that names what a dropped-to uid cannot reach.** Every path a view
  imports *through* — the interpreter's prefixes, the standard library,
  `platstdlib`, `site-packages`, and the directory the application's own module
  comes from (`importlib.util.find_spec`) — has to be readable and traversable by
  the uids blastit drops to, or a lazy import fails inside one view after the
  fork (`ModuleNotFoundError: No module named 'stringprep'`), one request at a
  time. A Python installed under a private home is the case this exists for:
  `uv python install` puts one in `~/.local/share/uv/python`, and `~` is often
  `0750`, so `libpython`'s built-ins keep working and every pure-Python module
  fails. The file a child has to `execve` — `BLASTIT_EXEC`, or a wheel's
  `_bin/blastit` — has to be executable by dbproxy's uid, or stage 0 dies
  with `exec dbproxy: Permission denied (os error 13)` and nothing else. blastit
  asks the kernel with `access(2)`, as the uid in question, before either can
  happen, and refuses the boot naming the first directory that blocks, its mode
  and both fixes: move the interpreter (`UV_PYTHON_INSTALL_DIR=/opt/uv-python`
  before `uv python install`), or `chmod o+rx` that directory and every one above
  it.
* **rlimits.** `RLIMIT_NPROC` per view uid, plus `RLIMIT_NOFILE` and
  `RLIMIT_CORE=0`. It counts threads as well as processes, summed over every
  process of the uid, on both Linux and DO's gVisor, so it is a hard backstop
  under the scheduler budget. With a limit of 16, Linux allowed 15 new
  threads and gVisor 16: gVisor doesn't count the main thread, so leave one
  unit of margin.
* **`PR_SET_PDEATHSIG(SIGKILL)`.** No process outlives init.
* **Capabilities.** Every child clears every capability set it holds and sets
  `PR_SET_NO_NEW_PRIVS`. The flag is what makes an `execve` of a file with file
  capabilities grant nothing: with it set, the new permitted set may only be a
  subset of the old one, and the kernel refuses the `execve` outright (`EPERM` —
  "insufficient to execute correctly") when the file's capabilities cannot be
  honoured. So the file init starts from (D6) may carry the boot's capabilities
  without a view being able to use them: a view holds nothing, and its bounding
  set is empty. The dedicated copy is also mode `0500` and owned by init's uid, so
  another uid cannot run it in the first place.

### 7.3 seccomp

Unless `BLASTIT_SECCOMP=0`, each child installs a BPF filter after dropping
privileges. This needs only `PR_SET_NO_NEW_PRIVS`, no capability — which is
what makes it work on DO, where the spike measured `socket(AF_INET)` and
`socket(AF_INET6)` going to `EPERM` while `AF_UNIX` kept working. Because of
that it is the default rather than an option, and the `boot` event says
whether the filters are on.

The filter is a **refusal list**: the default action is allow, and what it
refuses it refuses with `EPERM` — `clone3` is the one exception, below — so a
syscall it says nothing about cannot be what breaks a request. It is installed
while the child is still single-threaded — in the zygote or the PHP fork before
a view serves anything, and in http, scheduler and dbproxy before their runtime
exists — because it cannot be removed and applies to every thread, present or
future.

* **Every role** refuses `ptrace`, `process_vm_readv`, `process_vm_writev`,
  `bpf`, `keyctl`, `add_key`, `request_key`, `perf_event_open`, `userfaultfd`,
  the three `io_uring` calls, `kexec_load`, `kexec_file_load`, `init_module`,
  `finit_module`, `delete_module`, `mount`, `umount2`, `pivot_root`, `swapon`,
  `swapoff`, `reboot`, `setns`, `unshare`, `open_by_handle_at`, `pidfd_getfd`,
  `kcmp`, `process_madvise` and the new mount API (`open_tree`, `move_mount`,
  `fsopen`, `fsconfig`, `fsmount`, `fspick`). They are the calls that read or
  steer another process, load code into the kernel, or move between namespaces
  and mounts; none is part of serving a request.
* **`socket()`** is checked on the address family in `args[0]`:
  * **views:** `AF_UNIX` only. A view whose `Blastfile` entry declares
    `egress any` may also use `AF_INET`/`AF_INET6`. Every other family —
    `AF_NETLINK` and the rest — is `EPERM`.
  * **http, scheduler:** no `socket()` at all. They only use inherited fds:
    both run on an inherited listener, an inherited `AF_UNIX` channel pair and
    pipes, and nothing they do calls one.
  * **dbproxy:** `AF_UNIX`, `AF_INET` and `AF_INET6`. It adopts the pg
    listeners init bound and connects to the real PostgreSQL over TCP, so it
    needs both; a host *name* is resolved by `getaddrinfo`, which falls back
    to `/proc` when its `AF_NETLINK` socket is refused.
* **`socketpair()`** stays `AF_UNIX`-only for every role, http and scheduler
  included: a pair of descriptors the process already owns is not a new way
  out of it, and std and tokio create one internally on some platforms.
* **`clone3` is the one refusal that is not `EPERM`.** It fails with `ENOSYS`
  so glibc's `pthread_create` falls back to the still-allowed `clone`; a
  `clone` that requests any `CLONE_NEW*` flag (including `CLONE_NEWTIME`) is
  `EPERM` for every role, because a process may not reach for a namespace.
  `setsid` and `setpgid` are `EPERM` too: a worker cannot leave init's
  supervision by starting a new session or process group. A view's own
  `setpgid(0, 0)` runs inside `blastit_sys::child::enter`, between
  `ChildIo::prepare` and the filter install, so the call being refused is never
  the legitimate one.
* **`SeccompRole::EmbeddedView` (a PHP view, §2.6)** keeps every rule above and
  the view's `socket()` families, and additionally refuses `execve`, `execveat`,
  `fork`, `vfork` and a `clone` without `CLONE_THREAD` with `EPERM`. Its workers
  are forked **before** the filter goes on, so nothing legitimate in a request
  creates a process; a request that tries is escaping the process-wide CPU
  charge, which is the one thing that would make a crash loop cheap (`R14`).
  Thread-creating `clone` stays allowed: PHP extensions and glibc itself start
  threads.
* **A foreign architecture is fatal.** The filter compares
  `seccomp_data.arch` against the build's `AUDIT_ARCH_*`
  (`AUDIT_ARCH_X86_64`/`AUDIT_ARCH_AARCH64`) and kills the process when it
  does not match, and on x86_64 an x32 syscall number
  (`nr & __X32_SYSCALL_BIT`) is refused the same way. A syscall number from
  another ABI means a number-to-syscall mapping unlike the one the refusal
  list is written in, so there is nothing safe to compare it against; no
  legitimate caller produces either.

This is our only lever for egress control, and it is what `egress` in the
manifest buys.

---

## 8. Operations

**Failures.**

| Event | Response |
|---|---|
| view process dies | scheduler answers its in-flight requests with `502`; the next request respawns it |
| view thread exceeds `BLASTIT_REQUEST_TIMEOUT` | client gets `504`; the slot stays held until the thread exits; recycle once too many threads are stuck |
| queue full, or queue wait exceeds `BLASTIT_QUEUE_TIMEOUT` | `503` with `Retry-After` |
| a proxy at `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | the new connection is refused (`53300`, too many connections), which surfaces as a DB error in the view |
| http, scheduler or dbproxy dies | fail fast: init kills everything and exits non-zero, and the platform restarts the container |
| `SIGTERM` to PID 1 | http stops accepting and finishes in-flight requests until `BLASTIT_GRACEFUL_TIMEOUT`, then `SIGTERM` → `SIGKILL` to everyone, then exit |

**Sizing.** Choose `BLASTIT_MAX_WORKERS` ≤ the DB plan's connection limit,
because each live request slot (thread or PHP worker process) can hold one
upstream connection.

The fair queue's tables are bounded by configuration, not by the number of
clients: `BLASTIT_FQ_OUTER_MAX` outer flows per worker, and
`BLASTIT_FQ_INNER_MAX` inner flows across them (`BLASTIT_FQ_INNER_MAX_PER_OUTER`
for one outer flow). A client that arrives on an address that cannot be read —
an unnamed socket — is one request of the same overflow flow as every other
such client, which is the honest accounting: they cannot be told apart.

Memory decides how many view processes can be alive at once. Under DO's
gVisor, every forked process costs a floor of **about 14–18 MiB**, against
about 1 MiB on Linux. On top of that, each 2 MiB chunk of shared memory it
writes to gets copied (§9.2). So:

* `BLASTIT_VIEW_SPAWN=lazy` (the default) means only views that are actually
  hit cost memory. Views that declare `prefork` are forked at
  startup, which removes the first-hit fork latency (12–60 ms on DO).
* `BLASTIT_MAX_VIEW_PROCESSES` caps the number of live view processes (the
  pools' workers plus any fork in flight, charged one fork at a time). When a
  new one is needed, the least-recently-used **evictable** one is retired: a
  process that is `READY`, has nothing in flight and has no request waiting in
  the admission queue for its own route. A process that has not answered the
  request it was forked for — including one that has not sent `READY` yet — is
  never retired, and when nothing is evictable the request stays in the queue
  instead: the cap may never fail a request by killing the process that would
  have served it. The eager or `prefork` burst at startup is bounded by the
  same cap instead of forking every route the manifest names. The default is
  `max(1, (total − usage after preload − reserve) / estimate)`: at least one
  view process, because a cap of zero would refuse every request. The reserve
  is `max(64 MiB, 10 % of total)`, kept for the kernel, the platform's agents
  and the next fork. `estimate` starts as `BLASTIT_VIEW_MEM_ESTIMATE` and is
  replaced by the cost the scheduler **measures**: after each view process
  reports `READY` it samples aggregate usage, keeps a moving average of the
  rise per view process and re-derives the cap from it. That matters because a
  view copies most of the preloaded heap — about 140 MiB under gVisor for a
  Django + Wagtail app on CPython 3.14, immortalised or not (marking saves
  ~40 % per view on native Linux, but under gVisor's 2 MiB copy-on-write
  chunks such an app writes to nearly every chunk anyway), against the 32 MiB
  default — so
  a cap derived from the configured estimate (≈45 on a 2 GiB instance) bounds
  nothing. The measurement only ever replaces the *derived* default;
  `BLASTIT_MAX_VIEW_PROCESSES` still wins, and an estimate that is never
  measured stays where the operator put it. The scheduler takes that reading
  twice — once before the burst, which is *before* init has finished
  preloading, and again at the first view process init reports, which is the
  first moment §4's "usage after preload" exists — so a burst that turns out
  too big for the second reading is trimmed as later views are needed. When the
  machine's memory cannot be read at all there is no derived cap, and init says
  so once, as a `warn` line of its own output (§8.1).
* `total` is `min(MemTotal, cgroup limit)`: `MemTotal` comes from
  `/proc/meminfo`, the limit from cgroup v2 `/sys/fs/cgroup/memory.max` with
  cgroup v1 `memory/memory.limit_in_bytes` as the fallback; `max` — or v1's
  page-rounded `LONG_MAX` — means "no limit". `used` is what the kernel cannot
  reclaim: cgroup v2 `memory.current` minus `(file − shmem − file_mapped) −
  slab_reclaimable` from `memory.stat`, so page cache and reclaimable slab are
  left out while anonymous, tmpfs and mapped memory stay counted. A
  `memory.stat` without `file`/`shmem` falls back to
  `inactive_file + slab_reclaimable`; cgroup v1 keeps its raw `usage_in_bytes`
  (spelled differently, and accurate on DO — `MemTotal` *is* the instance size,
  §9.2 #10); a machine without cgroups keeps `MemTotal − MemAvailable`, which
  already excludes cache. The `boot` event reports `mem_total`, `mem_used` and
  the `max_view_processes` they buy, so the sizing decision is in the log.
* The scheduler also watches aggregate usage on its own, which is the only
  reliable number (`VmRSS` doesn't move under gVisor). A boot already above
  `BLASTIT_MEM_HIGH_WATERMARK` (a percentage of `total`, over the same reading)
  cannot start a view process, so every request would wait out
  `BLASTIT_QUEUE_TIMEOUT` and answer `503`; `blastit dev` refuses that boot
  instead, and prints one WARN line when it starts above the `85%` default (the
  knob was raised), naming the setting, the measured value and the reclaimable
  cache it left out. During a run, above the watermark it checks every 2 seconds
  and retires **one** idle view process per check, oldest-first; a view with a
  request in flight is never retired for memory. While usage is above the
  watermark a new view process is only spawned if an evictable one can be
  retired first, and otherwise the request waits in the queue: pressure cannot
  become retire-then-respawn thrash. A view retired for memory pressure is not
  spawned again for its route until the spawn backoff has passed (§2.4's "don't
  spin", the same cool-down a route whose spawn failed already gets).

### 8.1 Logs

stdout is blastit's only output, and it is an **event stream: one event per
line**. Events cover two things: what blastit decided (routing, denials,
process lifecycle) and what the app printed. The `sql.denied` events are what an
operator or a CI job reads offline to add the `Blastfile` grants a run proved
missing (§6).

**Formats.** `BLASTIT_LOG_FORMAT=json` (the default, for the server) prints one
JSON object per line. `pretty` prints the same events as coloured, aligned,
human-readable lines, for running blastit by hand. Both carry the same fields.
There is one human form: an event a component forwarded is read back into the
event it came from and rendered the same way, so `request` and `sql.denied` look
alike whichever component emitted them. A line this build cannot read — an
`event` a newer component emitted — falls back to a field dump, which loses
nothing.

```json
{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"request","component":"scheduler","request_id":"01JB9…","method":"GET","path":"/products/42/","route":"product-detail","rule":0,"status":200,"outcome":"ok","queued_ms":0.1,"duration_ms":18.4,"bytes_in":0,"bytes_out":5120,"cost_ns":18400000,"cpu_ns":9100000,"remote_addr":"203.0.113.7"}
{"ts":"2026-10-25T14:02:12.007Z","level":"warn","event":"sql.denied","component":"dbproxy","route":"product-detail","pid":57,"proxy":"db","kind":"parse","sqlstate":"42501","in_transaction":false,"sql":"SELECT \"shop_product\".\"price\" FROM \"shop_product\" WHERE \"shop_product\".\"id\" = $?","reason":"select on shop_product.price not granted"}
```

```text
14:02:11.384 INFO  request     GET /products/42/ → product-detail  200  18.4ms
14:02:12.007 WARN  sql.denied  product-detail  db  parse  SELECT "shop_product"."price" FROM …
```

**Single writer.** init is the only process that writes to stdout, so lines
never interleave.
* http, scheduler and dbproxy send structured events to init over a
  dedicated events pipe.
* Every child's stdout and stderr also go to init over pipes, and init wraps
  each line in an `app.output` event.
* init stamps `component` and `route` according to *which pipe* the data came
  from. So a compromised view can't forge another component's events: the
  most it can do is print lines that are labelled as its own app output.

**Line size.** DO truncates forwarded lines at 2048 bytes, and a truncated JSON
line is unparseable. So every event is kept to at most 2048 bytes:
* long fields (`sql`, `path`, `line`) are cut and listed in
  `"truncated": [...]`;
* long app output is split into several `app.output` events carrying
  `part` and `parts`.

**Fields common to every event:** `ts` (UTC, milliseconds), `level`, `event`,
`component`; plus `route` and `request_id` when they apply.

| Event | From | Level | Fields | Emitted when |
|---|---|---|---|---|
| `request` | scheduler | info (warn for 5xx) | `request_id`, `method`, `path`, `route` (null if unmatched), `rule` (index of the matching `path` regex), `status`, `outcome` (`ok`, `unmatched`, `queue_full`, `queue_timeout`, `timeout`, `view_died`, `bailed`, `client_gone`), `queued_ms`, `duration_ms`, `bytes_in`, `bytes_out`, `cost_ns`, `cpu_ns`, `remote_addr` | once per request, at the end; this is the routing decision |
| `sql.denied` | dbproxy | warn | `route`, `pid` (view process), `proxy`, `kind` (`query`, `parse`, `function_call`, `copy`, …), `sql` (normalised), `sqlstate`, `in_transaction`, `reason` (the policy's explanation, for example `select on auth_user.password not granted`; never a parameter value) | a statement was rejected |
| `pg.refused` | dbproxy | warn | `route`, `proxy`, `reason` (`credentials_mismatch`, `too_many_connections`, `upstream_unreachable`, …) | a connection to the proxy was refused |
| `http.rejected` | http | info | `remote_addr`, `status` (400, 413, 431, 501…), `reason` | a request was refused before reaching the scheduler |
| `process.spawned` / `process.exited` / `process.retired` | init, scheduler | info (warn for crashes) | `route`, `pid`, `uid`, `fork_ms`, `status` or `signal`, `reason` (`idle`, `max_requests`, `stuck_threads`, `memory_pressure`, `reload`) | view process lifecycle |
| `process.cap` | scheduler | info | `estimate_bytes` (the measured cost of one view process), `cap` (the live view process cap it buys) | the measured cost or the cap changed (§8 Sizing); never once per request |
| `app.output` | init | info (stdout) / warn (stderr) | `stream`, `line`, `part`, `parts` | the app or a library printed something |
| `runtime` | init | info | `kind` (`python`, `php`, `static`), plus `version` and `environment` (`python`), `version`, `libphp` and `php_ini` (`php`), or `mounts` (`static`) | once at boot and after each `dev.reload` |
| `reload` | init | info | `directories` (how many directories the watch pass marked; the human form spells `1 directory` in the singular) | once, by a `blastit dev` boot whose marking pass placed its marks (§4.7) |
| `dev.reload` / `dev.reload.unavailable` | init | info / warn | `scope`, `changed`, `duration_ms` (`dev.reload`); `reason` (`dev.reload.unavailable`) | `blastit dev` `fanotify` live reload |
| `boot`, `ready`, `shutdown`, `manifest.warning` | init | info / warn | stage, counts, durations, `seccomp`, `privilege` (`root`, `capabilities`, or `unprivileged-dev`, D6) with `capabilities` (the set init holds, rootless mode only) and `bounding_set` (`dropped` or `kept`, M1), the preset in force with the settings it supplied and the ones an explicit variable overrode (`preset`, `preset_settings`, `preset_overridden`; E1 — `preset` is written as `null` in a `boot` that has none, and the supplied settings' values are the preset's own, never the operator's), the rendered output variable *names* per proxy (never values), the warning text (for example a trait no view uses) | lifecycle of the whole server |

A `remote_addr` field is the same address the app sees in `REMOTE_ADDR`: a
bare IP, never `ip:port` (§2.5). A port is never a log field at all — it is a
key of `environ`, not of an event, and it reaches the app and nothing else.

`cost_ns` is what the request was charged for fair queuing: the view
process's own measurement, clamped between the time from dispatch to the
view's first response frame and the time the scheduler observed to the end
of the response, or that observed time when no report arrived. It is always a
number. Three shapes fall outside that clamp, and none of them is decided by
the status alone: a `5xx` a view **bails out of** (`outcome: "bailed"`) is
charged its own wall time, arrival to last byte, so a crash loop cannot cost
less than the client waited; a `5xx` an app **answers** itself
(`outcome: "ok"` with that status) is charged the clamp like any other answer;
and a `5xx` the scheduler wrote itself (`502`/`503`/`504`, `outcome:
"view_died"`/`"queue_full"`/`"timeout"`) is charged dispatch to completion, so
a request the scheduler failed never pays the queue wait it spent before it
reached a worker. `cpu_ns` is the thread CPU time the view measured over the
same span, and `null` when the view had none to report — no `RESP_END_COST`, or
no thread clock on its interpreter — so a reader never has to guess which
happened. `duration_ms` still ends at the last response byte, whether or not the
number arrives with it.

The `boot` event of the **`preload`** stage is the one that says what the app
cost before the fork: `immortalized` and `immortalize_ms` for §9.2 #9's pass
over the heap, and the memory the view process cap was derived from —
`mem_total`, `mem_used` and `max_view_processes` (§8 Sizing). Byte counts are
integers.

**The three `boot` lines.** The first is `stage0`'s: *made* before anything can
fail, so it says the §7.3 seccomp setting, read straight from its variable, and
nothing about a configuration that was not read yet — including `preset: null`.
It is *printed* once the settings that decide how lines are printed have been
read, and in the format they name: the lines stage 0 makes that early wait in
order and go out together, and a boot whose settings do not parse writes them as
JSON and then the error.
The `config` line is also stage 0's, emitted as soon as the configuration parsed
and *before* the manifest is read, so a boot that fails later still says which
preset it was failing with; it carries the mode init detected, the filters as
configured, and the preset. The `preload` line is stage 1's (§8 Sizing, §9.2 #7).
A `warn` line follows the `config` one when the preset's assumptions about the
platform do not hold for this boot (`preset_expects_root`,
`preset_expects_rootless`) — stage 0 prints that one itself rather than carrying
it to stage 1, for the same reason the `config` line exists.

**Data that is never logged:** query strings, header values, request and
response bodies, and SQL parameter values. They routinely carry personal data
and secrets. `path` is the routed `PATH_INFO`.

**Correlation.** dbproxy doesn't see request ids, because the app opens its
database connections itself. `sql.denied` carries the route and the view
process `pid`, which together with `ts` is enough for tooling to line it up
with `request` events.

### 8.2 Configuration reference

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8080` | set by DO; we bind `0.0.0.0:$PORT` |
| `BLASTIT_APP` | — | WSGI app, `module:attr` |
| `BLASTIT_PRELOAD` | — | extra modules imported before the fork (comma-separated) |
| `BLASTIT_MANIFEST` | `./Blastfile` | the file Blastit reads |
| `BLASTIT_SCRIPT_NAME` | — | mount prefix stripped before routing, passed as `SCRIPT_NAME` |
| `BLASTIT_LOG_FORMAT` | `json` | `json` (one event per line) \| `pretty` (coloured, for humans) |
| `BLASTIT_LOG_LEVEL` | `info` | minimum level of events printed |
| `BLASTIT_UID_BASE` | `20000` | base of the uid layout |
| `BLASTIT_MAX_WORKERS` | `16` | global number of live request slots (a Python thread, or a PHP worker process) |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | requests per view process; also sets its `RLIMIT_NPROC`; does not bound a pooled route's process count |
| `BLASTIT_QUEUE_SIZE` / `BLASTIT_QUEUE_TIMEOUT` | `256` / `10s` | admission queue |
| `BLASTIT_FAIR_QUEUING` | `0` | `1` orders the admission queue fairly (§2.4) instead of by arrival; `0` is exactly the FIFO |
| `BLASTIT_FQ_OUTER_PREFIX_V4` / `_V6` | `24` / `56` | how much of a client address an outer flow keeps; read as `32` / `128` when longer |
| `BLASTIT_FQ_OUTER_MAX` | `65536` | outer flows per worker; the rest share one overflow flow; read as `1` when `0` |
| `BLASTIT_FQ_INNER_MAX_PER_OUTER` | `1024` | inner flows one outer flow may hold; read as `1` when `0` |
| `BLASTIT_FQ_INNER_MAX` | `262144` | inner flows per worker; read as `1` when `0` |
| `BLASTIT_FQ_BURST` | `20ms` | what a client may spend before it starts queueing behind the others |
| `BLASTIT_FQ_MAX_DEBT` | `5s` | how far ahead of the queue's virtual time one flow may borrow |
| `BLASTIT_FQ_DEBT_TTL` | `30s` | how long an idle flow with debt is remembered; read as `1s` when shorter |
| `BLASTIT_FQ_INNER_HEADERS` | `user-agent,accept-language,sec-ch-ua,sec-ch-ua-platform,sec-ch-ua-mobile,authorization` | headers hashed into the inner flow key (an empty value hashes none); every entry is checked to be a header name |
| `BLASTIT_FQ_INNER_COOKIES` | `sessionid,session` | cookies hashed into the inner flow key; every entry is checked to be a cookie name |
| `BLASTIT_FQ_INNER_INCLUDE_IP` | `1` | whether the full client address is part of the inner flow key |
| `BLASTIT_REQUEST_TIMEOUT` | `30s` | time until the client gets a `504` |
| `BLASTIT_STUCK_THREADS` | `4` | overdue threads before a view process is recycled |
| `BLASTIT_VIEW_SPAWN` | `lazy` | `lazy` \| `eager` (`prefork` in the file overrides per view) |
| `BLASTIT_VIEW_IDLE_TIMEOUT` / `BLASTIT_VIEW_MAX_REQUESTS` | `600s` / `0` (off) | view process recycling |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | live view process cap (LRU retirement) |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | expected cost per view process: the starting divisor of the derived cap, and the floor of the measured one |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | of `total` (the smaller of `MemTotal` and the cgroup limit), over the non-reclaimable reading of §8 Sizing; above it one idle view process per 2 s check is retired, and `blastit dev` refuses to boot |
| `BLASTIT_IMMORTALIZE` | `auto` | `auto` \| `1` \| `0`: mark the preloaded heap immortal before the fork (§9.2 #9), each version with the marker CPython itself writes there. `auto` and `1` both do it on 3.12–3.14 and skip any interpreter whose marker blastit does not know (32-bit, big-endian, free-threaded, `Py_TRACE_REFS`, or a version outside that range), with the reason in the `boot` event; `0` never |
| `BLASTIT_MAX_HEADER_BYTES` / `BLASTIT_MAX_BODY_BYTES` | `64 KiB` / `100 MiB` | HTTP limits |
| `BLASTIT_HEADER_TIMEOUT` / `BLASTIT_KEEPALIVE_TIMEOUT` | `10s` / `75s` | HTTP timeouts (keep-alive above the LB's) |
| `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | header the client address is read from, and only from a trusted peer; a chain entry is a bare or bracketed address, so `1.2.3.4:5678` makes the whole header unreadable and the peer is the client |
| `BLASTIT_TRUSTED_PROXIES` | `127.0.0.1,::1` | peers whose client-IP header may be believed: IP addresses, CIDR networks or `*`; a port is an error, and an empty list trusts nobody. **Changed** (§2.3, D5): before D5 an unset `BLASTIT_TRUST_FORWARDED` meant the rightmost `X-Forwarded-For` entry was believed from any peer. A deployment behind a non-loopback proxy must now set `BLASTIT_TRUSTED_PROXIES`, or `REMOTE_ADDR` becomes the proxy and `wsgi.url_scheme` becomes `http` |
| `BLASTIT_TRUST_FORWARDED` | — | **deprecated** (§2.3, D5): `0` is `BLASTIT_TRUSTED_PROXIES` empty, `1` is `*`, which is what it always meant — the rightmost entry of anybody's `X-Forwarded-For`. Either way it is reported once as a warn `app.output` line at startup, and `BLASTIT_TRUSTED_PROXIES` wins when both are set |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | answered by the scheduler |
| `BLASTIT_BACKLOG` | `1024` | TCP listen backlog init binds with (§3) |
| `BLASTIT_MAX_CONNECTIONS` | `1024` | client connections http serves at once; sizes http's `RLIMIT_NOFILE` |
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | upstream connection cap of proxy `<NAME>` (upper-cased) |
| `BLASTIT_ENV_STRIP` | — | extra variables hidden from the app |
| `BLASTIT_SECCOMP` | `1` | the §7.3 filters (verified on DO) |
| `BLASTIT_GRACEFUL_TIMEOUT` | `25s` | drain time on `SIGTERM` |
| `BLASTIT_WARMUP` | — | optional `module:callable` run before the fork |
| `BLASTIT_PRESET` | — | a named set of defaults (see **Presets** below); `--preset NAME` (or `--preset=NAME`) sets the same thing and wins over this variable when the two disagree. The name is trimmed; a value that is only whitespace is unset. Every variable that is *set* — and `PORT` — wins over the preset, so precedence is built-in default < preset < environment. An unknown name is a startup error that lists the presets |

`BLASTIT_UID_BASE` also decides which uid init may **not** run as (D6): the boot
refuses a uid the layout hands to another process — the http, scheduler and
dbproxy uids, and one per route — because that process would share init's
identity. `BLASTIT_EXEC` (the standalone install only) names the capability-free
copy of the binary that `--role=dbproxy` runs; a wheel names its own interpreter
through the console script. There is deliberately no variable for the privilege
*mode*: it is read from `/proc/self/status` at boot and reported in the `boot`
event, because a deployment that believes it granted a capability has no other way
to find out that it did not.

**Presets.** A preset is a set of *defaults* for one platform: the two below are
the two deployments that were measured end to end, and everything else is a
combination of the variables in this table. Because a preset never wins over a
variable that is set, `BLASTIT_PRESET=…` is always safe to add to a deployment
that already configures blastit by hand. `blastit --help` lists them, and the
`boot` event names the preset in force, the settings it supplied and the ones an
explicit variable overrode (`preset`, `preset_settings`, `preset_overridden`,
§8.1).

Each preset carries two kinds of assumption, and neither is checked for the
operator:

* the **privilege mode** its values were written for. The mode is detected, not
  configured (§2.1), so a boot that contradicts one **warns once and runs** —
  root in a rootless deployment still works, it is simply more authority than the
  preset asked for. Stage 0 prints that line (before the manifest is read, so a
  boot that fails before stage 1 still says it).
* **which peers can reach the socket**, for the client-IP setting it supplies.
  That one is the deployment's to keep: blastit cannot tell a hop it owns from a
  peer that reached it some other way, so the table says what each value assumes.

| Preset | Setting | Value | What the value assumes |
|---|---|---|---|
| `digitalocean-paas` | `BLASTIT_CLIENT_IP_HEADER` | `do-connecting-ip` | the one header DO's edge **overwrites** with the real client address, so a client cannot put anything in it (§10) |
| | `BLASTIT_TRUSTED_PROXIES` | `*` | every peer is trusted, and the assumption is that **nothing untrusted can reach the socket**: an App Platform service is only reachable through DO's load balancer and the app's own internal services, and the header being read is one the edge overwrites. Trusting every peer is then the simplest rule that also covers internal service-to-service calls (§10) |
| | `BLASTIT_FAIR_QUEUING` | `on` | one client's burst must not starve the others on a single small instance (§2.4, D5); the defaults for the `BLASTIT_FQ_*` knobs |
| | `BLASTIT_LOG_FORMAT` | `json` | the platform forwards stdout to its own log service and truncates a line at 2048 bytes (§8.1) |
| | *assumption* | `root` | App Platform starts the container as root and cannot grant capabilities to a non-root user, so this preset's deployment is a root one (§2.1, §10) |
| | *not set* | `PORT`, `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | the platform owns the port, and a connection cap names a proxy that only the manifest knows (§8 Sizing) |
| `compose-traefik` | `BLASTIT_CLIENT_IP_HEADER` | `x-forwarded-for` | Traefik v3 **overwrites** it from a peer it does not trust, and appends its own entry otherwise; the rightmost entry is what the closest hop saw in both cases (the measurement beside §10) |
| | `BLASTIT_TRUSTED_PROXIES` | `*` | the assumption is that **no untrusted peer can reach blastit's port**: the app publishes no port, so the peers that can are the containers on its own compose network — **any service on that network can reach it and will be believed**, so a deployment keeps containers it does not trust off that network (its own sidecars are trusted by construction, which is what makes service-to-service calls work). `*` means *the rightmost entry*, which is the one thing the hop in front cannot be lying about — a network list would take the *leftmost* entry whenever the client's own address is inside it (the measurement beside §10) |
| | `BLASTIT_FAIR_QUEUING` | `on` | as above |
| | `BLASTIT_LOG_FORMAT` | `json` | stdout is the container's log |
| | *assumption* | `capabilities` | this is the rootless deployment of D6: init runs as a dedicated uid holding six capabilities, not as root |
| | *not set* | `BLASTIT_EXEC`, the `BLASTIT_PROXY_<NAME>_*` caps | both name something only the deployment knows: the capability-free copy of the binary (a wheel names its own interpreter), and the file's stores |

---

## 9. Findings this design relies on

These come from two places: the archived experiments (bare Linux and local
gVisor) and the App Platform spike (DO's gVisor, 1 vCPU / 1 GiB). The spike
is written up in
[`docs/spikes/2026-10-do-app-platform.md`](docs/spikes/2026-10-do-app-platform.md).

### 9.1 Privileges and identity

1. **Root can `setuid()` to any uid without an `/etc/passwd` entry**, and
   doing so doesn't break COW (experiment 1). On DO, the container runs as
   root, as PID 1, with Docker's default capabilities minus `NET_RAW`.
   `SETUID`, `SETGID`, `CHOWN` and `FOWNER` are all there, and the uid map
   covers all of 0..2³²−2.
2. **`SO_PEERCRED` is wrong under gVisor.** It returns the local endpoint's
   credentials, for root and non-root listeners alike, on both `SOCK_STREAM`
   and `SOCK_SEQPACKET`. **`SO_PASSCRED` / `SCM_CREDENTIALS` is right**, and
   forging a uid fails with `EPERM`. **Unix socket file permissions are
   enforced** at `connect()`. These three facts are why dbproxy uses per-view
   listeners plus a credentials cross-check (§2.2).
3. **`/proc/<pid>/environ` (and `cmdline`, `maps`, `auxv`, the fd list) is
   readable across uids under gVisor**, even with `PR_SET_DUMPABLE(0)`. It
   shows the initial environment block, which only `execve` replaces, and
   `execve` does clear it. That's why the barrier is mandatory and dbproxy
   gets its own `execve` (§4).
4. **seccomp filters installed by the app are enforced on DO**, and so is
   `PR_SET_NO_NEW_PRIVS`. `RLIMIT_NPROC` is per uid and counts threads (with
   gVisor leaving out the main thread). `PR_SET_PDEATHSIG` and
   `PR_SET_CHILD_SUBREAPER` work. `SCM_RIGHTS` works across uids, and
   `execve("/proc/self/exe")` works. `pidfd_open` is `ENOSYS`.
5. **A non-root `--user` process gets no capabilities from `--cap-add`.** On
   Docker 29.5 (`docker compose` v5), `--cap-add`/`--cap-drop` set the
   container's *bounding* set only: a `user:` process starts with `CapPrm`,
   `CapEff` and `CapAmb` all zero (measured locally and on a Droplet in ams3).
   Two mechanisms do hand capabilities to a non-root process: **file
   capabilities** on the file whose `execve` should have them
   (`setcap 'cap_setuid,…+ep'`), and a launcher that sets the inheritable and
   ambient sets before it changes the uid (`setpriv --inh-caps … --ambient-caps
   …`). A container that sets `no_new_privs` (the App Platform, §10) ignores
   file capabilities entirely, so there starting as root stays the way.
6. **A file with capabilities is init's alone, and a child cannot `execve` one.**
   `get_file_caps` returns `EPERM` when a file's permitted set cannot be honoured
   — which is what every child of a capability-carrying file sees, its bounding
   set being empty — and a rootless init keeps its capabilities across
   `setresuid`, because the kernel only clears the sets when a uid crosses 0.
   Together they mean the capability belongs on a **dedicated copy** of the
   `blastit` binary init starts from (mode `0500`, owned by init's uid, whose uid
   is then a dedicated one), and that both wheel and package installs serve
   `--role=dbproxy` from the capability-free `blastit` binary named in
   `BLASTIT_EXEC`. The check is by **device and inode** so a symlink chain cannot
   hide a shared binary carrying file capabilities, because the kernel grants the
   attribute of the file an `execve` resolves to, not of the name it was given.

### 9.2 Memory

7. **Preload-then-fork is still the big memory win, but gVisor raises the
   cost of each process.** On Linux, 100 forked workers of a Django +
   scientific-stack parent (≈173 MiB) cost 575 MiB of PSS in total
   (experiment 2). On DO, each forked process costs a floor of about
   14–18 MiB however small or large the parent is. That covers its own
   stack, malloc arenas and interpreter state.
8. **gVisor breaks COW in chunks of about 2 MiB.** After a fork, touching
   0.1% of the objects in a 67 MiB Python heap, at random, made each child
   copy **82 MiB** (on Linux: about 3 MiB). Touching 10% contiguously cost
   only about 8 MiB. Python refcounts write to every object a request reads,
   so without countermeasures each view process would end up copying most of
   the preloaded heap.
9. **Immortalizing the preloaded heap fixes it.** Before `gc.freeze()`, init
   walks every reachable object (`gc.get_objects()` plus `gc.get_referents()`)
   and writes CPython's own immortal marker over its reference count, the pair
   `_Py_SetImmortalUntracked` writes: the low 32 bits of `ob_refcnt` —
   `0xFFFFFFFF` on 3.12 and 3.13, where `Py_INCREF` adds one to that half and
   skips the write when it wraps — and, on 3.14, `0xC0000000` plus
   `_Py_IMMORTAL_FLAGS` in the `u16` `ob_flags` at offset 6 (PEP 683). Incref
   and decref then never write to those objects. Measured on DO:

   | per-child cost on DO | plain | immortalized |
   |---|---:|---:|
   | touch nothing | 17.8 MiB | 17.8 MiB |
   | touch 0.1% / 1% of objects at random | 87 / 89 MiB | **18.8 / 18.8 MiB** |
   | touch 100% | 94 MiB | 26 MiB |
   | call 30 warmed-up functions × 100 | 21 MiB | 22 MiB |

   Immortalizing 1.6 M objects took 1.8 s on DO. What remains is real writes:
   in-place mutation of shared dicts and lists, caches (URL resolver,
   templates, `lru_cache`), and bytecode specialization of code that wasn't
   warmed up. That's why `BLASTIT_WARMUP` exists.

   **3.14 needs its own marker.** The count alone is not enough there, because
   3.14 has incref and decref paths that do not test immortality at all.
   `_Py_INCREF_MORTAL` — behind `LOAD_CONST_MORTAL`, `PyStackRef_DUP`
   (`LOAD_FAST`, `COPY`, `FOR_ITER`, …) and `PyStackRef_FromPyObjectNewMortal` —
   is a bare `ob_refcnt++` on the low half, so `0xFFFFFFFF` wraps to `0` under
   ordinary traffic, `_Py_IsImmortal` then says "mortal", and a later decref frees
   an object that is still referenced. On a real Django + Wagtail app under
   CPython 3.14, views segfault in `_PyObject_Malloc` on their first request, 5 of
   5 processes; bisecting showed that marking the instances of one class is
   enough, and the same crash is reproducible without blastit by writing
   `0xFFFFFFFF` with ctypes. `PyStackRef_FromPyObjectSteal` also decides whether a
   reference is already immortal by `ob_flags & _Py_IMMORTAL_FLAGS`
   (`Py_TAG_REFCNT`), not by the count, and `PyStackRef_CLOSE` on an untagged
   reference goes through `_Py_DECREF_MORTAL`, equally unchecked.
   `0xC0000000` with the flag answers both: it leaves 2^30 of headroom in each
   direction, so no mortal-only path can wrap it inside a container's life, and
   the flag is what tells 3.14's stack that the object is already immortal. On
   that pair a 3.14 app served 3,000 requests across 5 processes with no drift and
   no crash — and the immortalised heap numbers above are unchanged, because the
   marker is never written to on either version. So `BLASTIT_IMMORTALIZE=auto`
   (the default) marks 3.12, 3.13 **and** 3.14; `BLASTIT_IMMORTALIZE=1` asks for
   the same interpreters, and differs only in who asked; and
   `BLASTIT_IMMORTALIZE=0` turns it off everywhere.

   The marker also depends on the CPython version, so any other interpreter —
   32-bit, big-endian, outside 3.12–3.14, free-threaded, `Py_TRACE_REFS` — is
   skipped the same way, with the reason in the `boot` event, rather than
   refusing to start. Immortal objects are never freed, which is fine for a
   heap that lives as long as the container.

10. **Measure memory on aggregates only.** gVisor has no `smaps_rollup`, and
   `VmRSS` doesn't change after allocations. On DO, `/proc/meminfo` reports
   the instance (`MemTotal` = 1 GiB), and cgroup v1
   `memory/memory.usage_in_bytes` is accurate. (Local `runsc` showed the
   host's `meminfo`; DO doesn't.) `os.cpu_count()` returns 8 on a 1 vCPU
   instance, so never size anything from it.

### 9.3 Python and fork

11. **Threads and `fork` don't mix.** BLAS/OMP thread pools started before a
   fork don't exist in the child, and the child deadlocks: exactly the KMeans
   views hung with multi-threaded BLAS, on Linux and under gVisor alike.
   Hence the single-threaded-at-fork invariant and the forced
   `*_NUM_THREADS=1`.
12. **Fork and GIL ordering with pyo3.** `PyOS_AfterFork_Child()` must run
    *while holding the GIL*; otherwise CPython aborts. init's stage 1 is the
    only thread and holds the GIL for its whole supervision loop, so it forks
    between `PyOS_BeforeFork()` and `PyOS_AfterFork_Parent()`, and the child
    inherits the GIL, calls `PyOS_AfterFork_Child()`, and then only calls
    `blastit_view_python::serve`, which releases it for its I/O loop. The fork
    lives in `blastit-view-python`'s `zygote`; `blastit` supplies the forker and
    the child sequence (`blastit_sys::child::enter`), never a second
    `PyOS_AfterFork_*` call.
13. **Embedded CPython can't find the distro's `site-packages` by itself.**
    Append them to `sys.path` before the first import.

### 9.4 Latency and robustness

14. **gVisor makes syscalls and forks expensive.** Syscall-heavy requests take
    about twice as long (27 → 50 ms in the PoC). A fork takes 12–60 ms on DO,
    against 0.5–7 ms on Linux. blastit adds two internal hops each way
    (http → scheduler → view), and the DB adds one more. Frames are large and
    batched, and hot views are forked at startup.
15. **A receive timeout on every worker channel** turned a deadlocked worker
    into a single `502` instead of a hung server. It is generalised here as
    the scheduler's timeouts and recycling.

---

## 10. Platform constraints (DigitalOcean App Platform)

All of these were measured on a live App Platform service unless they're
marked *(docs)*.

| Constraint | Consequence for blastit |
|---|---|
| gVisor (reports kernel `4.4.0`); some syscalls missing (`pidfd_open` → `ENOSYS`) | AF_UNIX only internally; no `pidfd`, `io_uring` or namespaces; check every syscall we use against gVisor's compatibility table |
| Root, PID 1, Docker-default capabilities minus `NET_RAW` | enough: `SETUID`, `SETGID`, `CHOWN`, `FOWNER`, `DAC_OVERRIDE`, `KILL`. The image must **not** set a non-root `USER`, and this is also why D6's rootless mode is not for this platform: a non-root `user:` gets no capabilities from `--cap-add`, and `no_new_privs` makes the kernel ignore file capabilities too |
| `no_new_privs` set on every container (seen on the pilot project: `sudo` refuses to start) | blastit must *start* as root (`USER root`, then `CMD ["blastit"]`); it only ever drops privileges, which `no_new_privs` allows. Other components of the same image that should not run as root drop to their user themselves (`setpriv --reuid=… --regid=… --init-groups …`). A *rootless* start would need the capabilities as ambient capabilities: `no_new_privs` alone does not take away what a process already holds, but it does stop `execve` from granting file capabilities |
| No `CAP_SYS_ADMIN` / `CAP_NET_ADMIN` | no mount or user namespaces, no cgroups of our own, no per-uid firewall. Isolation is uids + file modes + rlimits + seccomp |
| `SO_PEERCRED` broken; `/proc/<pid>/environ` readable across uids | per-view pg listeners + `SCM_CREDENTIALS`; `execve` barrier; dbproxy exec'd with an empty env |
| COW in ~2 MiB chunks; ~14–18 MiB floor per process | immortalized heap, lazy spawning, LRU cap on live view processes |
| HTTP/1.1 from the edge, TLS terminated there *(docs)* | http speaks plaintext HTTP/1.1 only |
| The socket peer is DigitalOcean's ingress (`10.244.x.x` on this cluster, `100.127.x` on others), not the client | `BLASTIT_PRESET=digitalocean-paas` reads the client from `do-connecting-ip` and trusts **every** peer |
| An App Platform service is only reachable through DO's load balancer and the app's own internal services | nothing untrusted can reach the socket, so trusting every peer is the simplest rule that also covers service-to-service calls; what makes it safe is not the peer list but that the edge **overwrites** `do-connecting-ip`, so no client can put anything in the header being read. The old `10.0.0.0/8,100.64.0.0/10` list is gone: a trusted *peer list* was only ever needed to stop a client from *appending* to a header an upstream appends to, and `do-connecting-ip` is not that header — `X-Forwarded-For` is, and the preset does not read it (§10's appending row) |
| `X-Forwarded-For` arrives as `<client-supplied…>,<real client>,<Cloudflare edge>`: the edge **appends** | the resolver walks the chain from the right and stops at the first address that is not trusted, so a forged leftmost entry never becomes the client |
| `do-connecting-ip` is overwritten by the edge with the real client address, and `X-Real-IP` is stripped | `do-connecting-ip` is the one header an operator can believe on App Platform; `X-Real-IP` is not read at all |
| `X-Forwarded-Proto` is **overwritten** by the edge with `https`: a client's `http`, `https, http`, `gopher`, `HTTPS`, or a second `X-Forwarded-Proto` line all arrive as exactly one `X-Forwarded-Proto: https`, and the edge adds one when the client sent none | `wsgi.url_scheme` is `https` on App Platform and a client cannot change it: Django's `SECURE_PROXY_SSL_HEADER`, `request.is_secure()` and absolute URLs built from the scheme hold here. The only `http` a view sees is a service-to-service call that did not traverse the edge, and is plaintext |
| A plain `http://` request to the app's domain is answered `301` to `https://` by the edge, and never reaches the container | the app only ever sees requests that arrived over TLS, so there is no plaintext path on which a scheme could be given |
| `Forwarded`, `True-Client-IP`, `X-Client-IP`, `X-Forwarded-Host` and `X-Forwarded-Port` pass through **forged**, and the `X-Forwarded-*` host/port pair is absent when the client sends nothing | none of them is read: the client address comes from `BLASTIT_CLIENT_IP_HEADER` and the scheme from `X-Forwarded-Proto`, each only from a trusted peer |
| A client-sent `CF-Connecting-IP` is refused by the edge (Cloudflare) with `403`, `error code: 1000`, and never reaches the container; an empty value passes | the client cannot even deliver this one, so it can never become `REMOTE_ADDR`. Where the edge is not Cloudflare (compose behind Traefik, a published port) the request arrives and the header passes through forged like the others, and it is not the one blastit reads |
| `100.64.0.0/10` is also CGNAT client space, and a client behind a carrier NAT can arrive *from* it | it no longer matters, and it is *not* treated as a proxy range anywhere: a client behind a carrier NAT may be `100.64.0.0/10`, and with the header overwritten by the edge that is simply the address blastit reports (the live e2e asserts `REMOTE_ADDR` is a client's, explicitly allowing `100.64.0.0/10`). The trap the old row warned about — a forged entry sitting inside a trusted range — cannot arise from a list that has no entries |
| `PORT` set by the platform (`8080` by default) | bound by init |
| Root filesystem is a writable 9p overlay; `/tmp` and `/dev/shm` are tmpfs (count as RAM); 4 GiB, ephemeral, no volumes *(docs)* | runtime dirs under `/run/blastit` (disk, not RAM); recreated on every start |
| Forwarded log lines ≤ 2048 bytes *(docs)* | init re-chunks every log line |
| amd64 only *(docs)* | single Rust binary built in the same image as `libpython` |
| `MemTotal` = instance size; cgroup v1 memory usage available; `cpu_count()` overstates | memory cap derived at startup; never size from the CPU count |
| Managed PG connection limits (about 20 on small plans) *(docs)* | `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` |
| Kubernetes underneath (`KUBERNETES_*` variables in the env) | stripped from the app env like everything else not needed |

### Docker Compose behind Traefik

`compose-traefik` is the second preset, and it rests on a measurement of its own
rather than on the App Platform one: the ingress is **Traefik v3.7.16**, blastit
is behind it, and the app publishes no port at all.

The preset's second assumption is about *reachability*: no untrusted peer can
reach blastit's port. Any container on the same compose network can — blastit
binds `0.0.0.0:8080` inside its own network namespace, and every peer is trusted
— so a deployment of this preset publishes no port for the app (only Traefik's)
and keeps containers it does not trust off that network. Sidecars of the same
deployment are trusted by construction, which is what makes service-to-service
calls work without a second rule. (`digitalocean-paas` makes the same
assumption, from the platform's side: see the table above.)

What Traefik does with the headers it manages, as observed from the app (every
row is one request from outside the compose network; the client's address as
Traefik saw it was the network's gateway, `172.29.0.1`, and Traefik itself was
`172.29.0.3`):

| Header the client sends | Traefik's defaults | `forwardedHeaders.trustedIPs` contains the client | `forwardedHeaders.insecure=true` |
|---|---|---|---|
| *(nothing)* | `X-Forwarded-For: 172.29.0.1`, `X-Real-Ip: 172.29.0.1`, `X-Forwarded-Proto: http` | same | same |
| `X-Forwarded-For: 6.6.6.6` | **overwritten**: `172.29.0.1` | **appended**: `6.6.6.6, 172.29.0.1` | appended |
| `X-Real-Ip: 6.6.6.6` | **overwritten**: `172.29.0.1` | **kept**: `6.6.6.6` | kept |
| `X-Forwarded-Proto: https` | **overwritten**: `http` | **kept**: `https` | kept |
| `X-Forwarded-Host` / `-Port` / `-Server` | overwritten | kept | kept |
| `Forwarded`, `True-Client-IP`, `X-Client-IP`, `CF-Connecting-IP`, `do-connecting-ip` | pass through **forged** | pass through forged | pass through forged |

From which:

* **`x-forwarded-for` is the header to read, not `X-Real-Ip`.** Traefik always
  adds its own observation to `X-Forwarded-For`, so the rightmost entry is the
  address it saw in *every* configuration above; `X-Real-Ip` is whatever the
  client sent as soon as the client's address is in `trustedIPs`.
* **Every peer is trusted** (`BLASTIT_TRUSTED_PROXIES=*`), which is a rule about
  the *rightmost* entry: `*` takes `chain.last()`. The private ranges a Docker
  network uses were the other candidate, and they are worse here — when every
  entry of a chain is trusted the resolver takes the **leftmost**, so with the
  client's own address inside those ranges (the bridge gateway; every other
  container; a LAN client) a forged `X-Forwarded-For: 6.6.6.6` *becomes* the
  client. Trusting a peer because its address is in `10/8` also says nothing
  about whether it is a hop this deployment owns.
* **`X-Forwarded-Proto` is followed** (`wsgi.url_scheme`), because the peer is
  trusted to say it and Traefik overwrites it by default: a client's own
  `https` cannot make a plaintext deployment report HTTPS, and behind Traefik's
  TLS entry point the app sees `https`.
* **`forwardedHeaders.trustedIPs` must not name a client** unless something that
  really is a proxy sits in front of Traefik: with the client's address in it,
  Traefik keeps the client's `X-Real-Ip` and `X-Forwarded-Proto`. Even then
  `X-Forwarded-For` stays the right header for the reason above, which is the
  second reason the preset reads that one.
* A deployment that terminates TLS *in front of* Traefik is that other
  configuration, and it is the one case where the operator has to write the
  `trustedIPs` list themselves: the preset's values assume Traefik is the hop
  that decides.

---

## 11. Open questions

Settled by the spike (see §9): S1 capabilities ✅, S2 peer identity ⚠️ (works
only via listener + `SCM_CREDENTIALS`), S3 seccomp ✅, S4 uid range ✅, S5
`RLIMIT_NPROC` ✅, S6 memory ⚠️ (works only with immortalization).

Still open:

* **A real app's heap.** The memory numbers come from a synthetic heap. Next
  step: repeat the measurement with Django + the scientific stack preloaded,
  immortalized and warmed up, and check how much a realistic request
  dirties.
* **Parameter constraints.** Should the manifest ever constrain parameter
  values (for example "`$1` must equal the session's user id")? Out of scope
  for `0.2.0`.
