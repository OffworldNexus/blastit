# blastit — Architecture

blastit is a WSGI server for Python that **sandboxes views**. Each view runs
under its own uid, and a view only gets the capabilities the manifest lists for
it. For now that means the database: route X may run parameterised queries A
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

A single Rust binary, `blastit`, plays every role. Most roles are chosen by
their position in the process tree. The exception is dbproxy, which is
re-exec'd with `--role=dbproxy` (§4); that flag grants nothing by itself,
because the secrets only arrive over an inherited pipe. CPython is embedded
through `pyo3`, as in the `pywsgi` experiment. The database proxy is
built on the [`pregresql`](https://github.com/OffworldNexus/pregresql) crate,
used as a library. On the Python side there is no framework integration at
all: blastit loads one standard WSGI callable (`BLASTIT_APP`) and calls it. It
knows nothing about the framework's routing. Which process serves which
request comes from blastit's own routing table in the manifest (§6).

| Component | Count | uid | Python? | Holds | Talks to |
|---|---|---|---|---|---|
| **init** | 1 (PID 1) | 0 | stage 1 only | listen socket, all internal socket ends while forking, preloaded app | everyone (control) |
| **dbproxy** | 1 (own `execve`) | `UID_BASE+3` | no | **the real DB credentials**, the query policy, one listener per view | views (Unix sockets), PostgreSQL (TCP/TLS) |
| **http** | 1 | `UID_BASE+1` | no | the listening TCP socket | load balancer, scheduler |
| **scheduler** | 1 | `UID_BASE+2` | no | the manifest's routing table, worker budget, one channel per view process | http, init, view processes |
| **view** | 1 per view (lazy) | `UID_BASE+1000+i` | yes | nothing but its own channel | scheduler, dbproxy |

`UID_BASE` defaults to `20000`. DO maps the full 32-bit uid range (the spike
used uids up to 2³²−2), but we stay below 65536 for portability. None of these
uids needs an `/etc/passwd` entry, because `setuid()` only needs `CAP_SETUID`
(experiment 1, confirmed on DO). Every process gets `gid == uid` and an empty
supplementary group list.

### 2.1 init (PID 1, root)

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
  * `authorize` checks the statement against the route's effective grants:
    the union of its inline grants, its groups and the global groups (§6).
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
  client address and scheme from `X-Forwarded-For` / `X-Forwarded-Proto`, but
  only when `BLASTIT_TRUST_FORWARDED=1`.
* Encodes responses. It validates header names and values (no CR/LF
  injection) and picks `Content-Length` or chunked encoding. Views produce a
  status line, headers and body chunks; they never write raw bytes to the
  client.
* Turns a client disconnect into `CANCEL` towards the scheduler.

### 2.4 scheduler

The scheduler is the only component that knows about every view process. It
is pure Rust, with no interpreter. It is forked before Python is loaded, like
http.

* **Routing.** It matches the request against the manifest's routing table
  (§6): each route's path regexes, and optionally its methods, tried in
  manifest order; the first match wins. If nothing matches, the scheduler
  answers `404` itself and no Python runs. A catch-all route (`path = ".*"`,
  listed last) lets the app render its own 404s. Every routing decision is
  logged as a `request` event (§8.1).
* **Admission.** It keeps one global budget, `BLASTIT_MAX_WORKERS`, which is
  the number of request threads alive across all view processes. It can also
  enforce a per-view cap. Excess requests wait in a bounded FIFO
  (`BLASTIT_QUEUE_SIZE`, `BLASTIT_QUEUE_TIMEOUT`); once that overflows, the
  answer is `503`.
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
  starts a **new OS thread that attaches to the interpreter**. That thread
  builds a standard WSGI `environ` and calls the WSGI callable, the same one
  in every process, so the app does its own routing and middleware as usual.
  It then streams the response back. blastit adds `blastit.route` (the route's
  name) to `environ`, for logging only.
* When the thread exits, the view sends `DONE`. This happens after the WSGI
  iterable's `close()`, so `request_finished` handlers have already run and
  the DB connection is already closed.

All app code is mapped into every view process. **The sandbox comes from uid
privileges, not from code isolation**: view A's process can run view B's
code, but only with A's database allowlist. The same principle makes routing
mistakes safe. If the manifest's regexes and the app's own URL routing
disagree, a request lands in a process whose allowlist doesn't fit, and its
queries are denied. It fails closed; it never gains a privilege.

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
| ② | `ctl.chan` | `socketpair(AF_UNIX, SOCK_STREAM)` | init | init ↔ scheduler | `SPAWN`, `SPAWNED`+fd (`SCM_RIGHTS`), `RETIRE`, `EXITED`, `DRAIN` |
| ③ | `view.chan` | `socketpair(AF_UNIX, SOCK_STREAM)`, one per view process | init, at spawn | scheduler ↔ view | request/response frames, muxed by `req_id`; `DONE` |
| ④ | pg sockets | named Unix sockets, one per (route, `postgresql` proxy) with a grant, in `/run/blastit/views/<uid>/` (`0700`), each `0600` and owned by the view uid, `SO_PASSCRED` set | init (stage 0), then inherited by dbproxy | one view → dbproxy | PostgreSQL wire protocol; identity = which listener, checked against `SCM_CREDENTIALS` |
| ⑤ | upstream | TCP (+TLS) | dbproxy | dbproxy → PostgreSQL | PostgreSQL wire protocol, real role |
| — | `secrets.pipe` | `pipe2`, inherited across dbproxy's `execve` | init stage 0 | init → dbproxy, one-shot | each proxy's inputs (real credentials), listener fd → (route, proxy) table, the manifest's grants |
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

### 3.1 Framing

Channels ① and ③ use the same frame format: `u32 length | u8 type | u32 id |
payload`, in little-endian, with a hard maximum frame size of
`BLASTIT_FRAME_MAX` (default 1 MiB). Larger bodies are split into several
`*_BODY` frames.

| Direction | Frames |
|---|---|
| towards the app | `REQ_HEAD {method, target, headers, remote_addr, scheme, server_name/port}`, `REQ_BODY {bytes}`, `REQ_END`, `CANCEL` |
| towards the client | `RESP_HEAD {status, reason, headers}`, `RESP_BODY {bytes}`, `RESP_END`, `RESP_ABORT {reason}` |
| view → scheduler only | `DONE {req_id, outcome}`, which releases the worker slot; `READY` (id 0), once, when the view can take requests |
| both ways | `WINDOW {bytes}`: flow-control credit for this stream's body (see below) |

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
* Response bodies: http sends `WINDOW` as it writes to the client socket; the
  scheduler forwards it to the view, where a request thread that runs out of
  credit blocks in its write (as it would on a socket). A slow client slows
  its own request thread down, and nothing else.
* A peer that sends beyond its credit has broken the protocol: a view is
  recycled, and http aborts the stream.

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
    S->>S: match path against the manifest regexes → view (404 if none)
    alt worker budget exhausted
        S->>S: wait in queue (BLASTIT_QUEUE_TIMEOUT)
        S-->>H: 503 if the queue is full or the wait times out
    end
    opt no live process for the view
        S->>S: SPAWN via init, see §4
    end
    S->>V: REQ_HEAD (req_id rewritten)
    V->>T: new OS thread, attached to the interpreter
    T->>T: build standard WSGI environ
    T->>T: app(environ, start_response): the app routes it
    T->>D: connect /proc/self/cwd/.s.PGSQL.5432 (only its uid may)
    D->>D: listener → view, SCM_CREDENTIALS uid must match
    D->>P: connect with the real role over TLS
    loop each statement
        T->>D: Parse / Bind / Execute (or Query)
        alt granted to the route (inline, its groups or global groups)
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
    I0->>I0: read each [proxy.*] input from env → upstream config<br/>render output templates (§5)
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
    Note over S: setuid(B+2) · compile the manifest regexes<br/>serve http.chan ↔ ctl.chan
    I1->>I1: init CPython · import BLASTIT_APP and BLASTIT_PRELOAD modules<br/>run BLASTIT_WARMUP (optional)
    I1->>I1: close DB connections · gc.collect()
    I1->>I1: immortalize every reachable object (§9.2) · gc.freeze()
    opt BLASTIT_VIEW_SPAWN=eager, or views with prefork = true
        I1->>V: fork × N (see below)
    end
    Note over I1: supervise: waitpid loop, ctl.chan, log pipes, signals
```

Two things hold at every step: init is single-threaded whenever it forks, and
no secret exists in stage 1.

Every view process is created the same way, whether at startup in eager mode
or on demand:

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
    Note over V: close all fds except its view.chan end and log pipes<br/>PyOS_AfterFork_Child() (GIL held, §9.3)<br/>setrlimit NPROC (threads + margin), NOFILE, CORE=0<br/>umask 077 · chdir + HOME + TMPDIR = /run/blastit/views/<uid><br/>setgroups([]) · setgid(uid) · setuid(uid): all capabilities dropped<br/>PR_SET_PDEATHSIG SIGKILL · PR_SET_NO_NEW_PRIVS<br/>seccomp filter (§7.3)
    V->>S: view.chan: ready
    Note over S: queued requests for the view are dispatched
    V--xI: exit (crash, or kill on RETIRE)
    I->>S: ctl.chan: EXITED {view, pid, status}
    deactivate V
```

If anything in a stage fails, the whole container exits non-zero. A
half-started sandbox is worse than a crash loop that the platform can see.

---

## 5. Proxies and environment variable swapping

Configuration comes **only** from environment variables. Before stage 1, init
rewrites the environment that the application will see. The rewrite is
driven by the `[proxy.*]` sections of the manifest (§6).

### 5.1 Proxies

A **proxy** is a named broker for one external service. It declares:
* the **protocol** it speaks (`postgresql` for now);
* the **input** variables it owns: the platform's real configuration and
  credentials, which only the proxy gets to see;
* the **output** variables the app sees instead: templates that point at the
  proxy's local endpoint.

```toml
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }
# No output table: DATABASE_URL is rewritten in place, DATABASE_CA_CERT disappears.

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }
[proxy.analytics.output]                # the app wants split variables for this one
ANALYTICS_DB_HOST = "{{ socket_dir }}"
ANALYTICS_DB_PORT = "{{ port }}"
ANALYTICS_DB_NAME = "{{ database }}"
ANALYTICS_DB_USER = "{{ user }}"
```

With DO's environment, the app ends up seeing:

```
platform: DATABASE_URL=postgresql://doadmin:s3cr3t@db-xyz.db.ondigitalocean.com:25060/defaultdb?sslmode=require
          DATABASE_CA_CERT=-----BEGIN CERTIFICATE-----…
          ANALYTICS_URL=postgresql://reader:t0ps3cr3t@db-abc.db.ondigitalocean.com:25060/analytics?sslmode=require
app:      DATABASE_URL=postgresql://blastit@%2Fproc%2Fself%2Fcwd:5433/defaultdb
          ANALYTICS_DB_HOST=/proc/self/cwd
          ANALYTICS_DB_PORT=5432
          ANALYTICS_DB_NAME=analytics
          ANALYTICS_DB_USER=blastit
```

**Rules:**
* **Inputs.** `input` maps the protocol's input slots to environment variable
  names. Every input variable is removed from the app's environment unless an
  output of the same name redefines it. A missing input variable aborts the
  boot.
* **Outputs.** `output` maps variable names to templates. Without an `output`
  table, the protocol's default applies: each input variable is replaced by
  the equivalent local value under the *same name*. That's the
  `DATABASE_URL` → `DATABASE_URL` case, which needs no configuration.
* **Collisions.** An output may not overwrite a variable it doesn't own (one
  that isn't one of its own inputs), and two proxies may not produce the same
  variable. Either aborts the boot instead of silently clobbering
  configuration.
* **Grants refer to proxies by name.** Routes and groups grant
  `[route.db]`, `[group.session.analytics]`, and so on (§6). What a grant may
  contain is defined by the protocol.

**Templates** are deliberately tiny. `{{ field }}` inserts a field and
`{{ field | urlencode }}` percent-encodes it; there is no logic, no loops and
no access to the environment. Templates are rendered once, in stage 0. An
unknown field or filter aborts the boot.

**Secrets can't be templated.** The fields a protocol offers to templates
describe only the *local* endpoint plus a few non-secret facts. Passwords,
upstream hosts and CA material never reach a template, so no output can leak
them.

### 5.2 Protocol `postgresql`

Served by dbproxy (§2.2). One listener per (route, proxy) for routes that
have a grant on that proxy. Each `postgresql` proxy gets its own local port:
`5432 + k`, *k* being the proxy's index among the `postgresql` proxies sorted
by name (so the order of `[proxy.*]` sections doesn't matter).

| Input slot | Content | Notes |
|---|---|---|
| `url` | libpq URI (`postgres://` or `postgresql://`) | the usual case, DO's `${db.DATABASE_URL}` |
| `host`, `port`, `user`, `password`, `database`, `sslmode` | individual parameters | an alternative to `url` (DO's split bindables); setting both forms is an error |
| `ca_cert` | PEM text | optional upstream CA, DO's `${db.CA_CERT}` |

| Template field | Value |
|---|---|
| `url` | `postgresql://{{ user }}@{{ socket_dir \| urlencode }}:{{ port }}/{{ database }}` |
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

**Grant contents:** `queries` (§6) and `transaction_control`.

A percent-encoded host is how libpq and `dj-database-url` write a Unix socket
directory. libpq connects to `<host>/.s.PGSQL.<port>`, so the **port selects
the proxy** and `/proc/self/cwd` selects the view. The rendered values are
identical in every process. That matters because frameworks read their
database settings once, in init, before the fork. No output contains a
password, because dbproxy authenticates by socket identity.

### 5.3 The rest of the environment

1. **Strip.** All `BLASTIT_*` variables, every proxy input, and anything
   listed in `BLASTIT_ENV_STRIP` (for example `PGPASSWORD`, or DO bindables
   you injected but don't route through a proxy) are removed.
2. **Inject.** The process-safety defaults below are added, unless the
   variable is already set:

   | Variable | Value | Why |
   |---|---|---|
   | `OPENBLAS_NUM_THREADS`, `OMP_NUM_THREADS`, `MKL_NUM_THREADS`, `NUMEXPR_NUM_THREADS`, `VECLIB_MAXIMUM_THREADS` | `1` | thread pools don't survive `fork` (§9.3) |
   | `PYTHONDONTWRITEBYTECODE` | `1` | views can't write to the code tree |
   | `LC_ALL` | `C.UTF-8` | no locale files needed |
   | `HOME`, `TMPDIR` | replaced per view | private scratch directory |

Proxies are the general pattern for every future capability: **replace the
credential with a local endpoint served by a broker that checks the
manifest**. A new protocol (Redis, S3-style object storage, outbound HTTP
APIs) adds a protocol definition: input slots, template fields, default
outputs, grant contents and a broker process. The manifest syntax doesn't
change. Secrets that the app itself must hold, such as Django's
`SECRET_KEY`, can't be swapped and stay visible to every view.

---

## 6. The manifest

The manifest is a file in the image (`BLASTIT_MANIFEST`, default
`/app/blastit.toml`). It is versioned with the code, because it describes the
code. dbproxy reads it in stage 0; init and the scheduler read it in stage 1.

It is **blastit's own routing table**, independent of the framework. Besides
the proxies (`[proxy.*]`, §5), it has two kinds of entries.

* **Routes** (`[[route]]`) are sandboxes: one process, one uid each. A route
  says which requests go to it (path regexes, optionally methods), what it
  may do, whether it may open outbound connections, and whether it is
  preforked. Its grants can be written **directly in the route**
  (`[route.<database>]`, the usual case), taken from groups, or both.
* **Groups** are optional named sets of the same grants, for statements that
  several routes share. A group grants nothing until it is attached: to some
  routes (`groups = […]` in the route), or to all of them (`groups = […]` at
  the top level).

A grant, inline or in a group, is per proxy (§5): for a `postgresql` proxy,
the SQL statements allowed and whether transaction control is allowed.

The app is never asked anything. It receives ordinary WSGI requests and routes
them itself, exactly as it would behind any other WSGI server.

```toml
version = 1
groups = ["session"]                # attached to every route (top-level keys come before any [table])

[proxy.db]                          # see §5 for proxies and their variables
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

# Connection setup, plus what middleware does on every request.
[group.session.db]                  # grant on proxy "db" (§5)
transaction_control = true          # BEGIN/COMMIT/ROLLBACK, SAVEPOINT/RELEASE sp_*
queries = [
  "SET TIME ZONE 'UTC'",
  'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21',
]

[group.catalog.db]
queries = [
  'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21',
]

[[route]]
name = "product-detail"
path = ['/products/[0-9]+/']        # full match against PATH_INFO
methods = ["GET", "HEAD"]           # optional; default: any method
groups = ["catalog"]                # shared with checkout; nothing else needed here
prefork = true                      # hot route: fork at startup, skip first-hit fork latency

[[route]]
name = "checkout"
path = ['/checkout/.*']
groups = ["catalog"]
egress = true                       # may open TCP connections (payment API); see §7.3
[route.db]                          # inline grants, only for this route
queries = [
  'INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id"',
]

[[route]]
name = "admin"                      # staff-only back-office: its SQL is too varied to list
path = ['/admin/.*']
[route.db]
unrestricted = true                 # any statement on "db"; flagged at boot

[[route]]
name = "about"
path = ['/about/']
[route.db]                          # inline only: no group involved
queries = ['SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1 LIMIT 21']

[[route]]
name = "fallback"                   # catch-all: the app renders its own 404s
path = ['.*']                       # without it, unmatched paths get blastit's own 404
                                    # only the global groups; add grants if error pages need the DB
```

* **Effective grants.** A route's allowlist for a database is the union of
  its inline queries, its groups' queries and the global groups' queries, for
  that database. `transaction_control` is on if any of them turns it on;
  `unrestricted` is on if any of them turns it on, but a *group* that turns it
  on aborts the boot (below), so in practice it is only ever inline. A route
  with no grant for a database gets no listener for it at all (§2.2).
* **`unrestricted = true`** in a route's own grant (`[route.<db>]`) means
  exactly what it says: that route may run any statement on that proxy,
  transaction control included. It is still a sandbox — the listener, the uid
  check, the other proxies' grants and the credential barrier all stay — it
  simply has nothing left to check on this one, so dbproxy logs no statement
  for it. It is accepted **inline on a route only**: a group can be attached
  to every route at once, which would turn the whole manifest into one
  sandbox, so `unrestricted` in a group is a boot error, and every
  unrestricted (route, proxy) is reported as a `manifest.warning` at boot.
  Its user is a staff-only back-office (Django's admin) whose SQL is too
  varied to list, where the guarantee that remains is that the process still
  never sees the credentials.
* **Validation at boot.** An unknown group, a duplicate route name, an
  invalid regex or `unrestricted` in a group aborts the boot. A group that is
  attached nowhere, and a route that is unrestricted on a proxy, are reported
  as warning events.
* **Routing.**
  * Routes are tried in manifest order, and the first one whose `path` (and
    `methods`, if set) matches wins.
  * A regex must match the whole of `PATH_INFO`, which is the percent-decoded
    path the app will see. They use Rust `regex` syntax, which guarantees
    linear-time matching, so a hostile URL can't stall the scheduler.
  * `BLASTIT_SCRIPT_NAME` strips a mount prefix first, as WSGI does.
* **Getting the routing wrong is safe, if annoying.** A misrouted request runs
  in a process whose allowlist doesn't fit its queries, so they're denied
  (§2.5), and every denial is logged (§8.1).
* **uid assignment** is `UID_BASE + 1000 + index of the route in the
  manifest`. Every process derives it from the same file, and init also hands
  the table to dbproxy. uids only live as long as the container, so reordering
  the manifest between deploys doesn't matter.
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
* **Writing the manifest is out of scope for blastit itself.** Every denied
  statement is logged with its route, database and normalised SQL (§8.1), so
  future tooling can build or extend the manifest from those logs, for
  example after a test-suite run. Each pass may only reveal the first denial
  of each request, because a denied statement usually aborts the request.

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
| import-time app code | root, before any sandboxing | — **out of scope**. Imports must run before the fork for COW sharing |

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
* **rlimits.** `RLIMIT_NPROC` per view uid, plus `RLIMIT_NOFILE` and
  `RLIMIT_CORE=0`. It counts threads as well as processes, summed over every
  process of the uid, on both Linux and DO's gVisor, so it is a hard backstop
  under the scheduler budget. With a limit of 16, Linux allowed 15 new
  threads and gVisor 16: gVisor doesn't count the main thread, so leave one
  unit of margin.
* **`PR_SET_PDEATHSIG(SIGKILL)`.** No process outlives init.

### 7.3 seccomp

Unless `BLASTIT_SECCOMP=0`, each child installs a BPF filter after dropping
privileges. This needs only `PR_SET_NO_NEW_PRIVS`, no capability — which is
what makes it work on DO, where the spike measured `socket(AF_INET)` and
`socket(AF_INET6)` going to `EPERM` while `AF_UNIX` kept working. Because of
that it is the default rather than an option, and the `boot` event says
whether the filters are on.

The filter is a **refusal list**: the default action is allow, and what it
refuses it refuses with `EPERM`, so a syscall it says nothing about cannot be
what breaks a request. It is installed while the child is still
single-threaded — in the zygote before a view serves anything, and in http,
scheduler and dbproxy before their runtime exists — because it cannot be
removed and applies to every thread, present or future.

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
  * **views:** `AF_UNIX` only. A view whose manifest entry has
    `egress = true` may also use `AF_INET`/`AF_INET6`. Every other family —
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
because each request thread can hold one upstream connection.

Memory decides how many view processes can be alive at once. Under DO's
gVisor, every forked process costs a floor of **about 14–18 MiB**, against
about 1 MiB on Linux. On top of that, each 2 MiB chunk of shared memory it
writes to gets copied (§9.2). So:

* `BLASTIT_VIEW_SPAWN=lazy` (the default) means only views that are actually
  hit cost memory. Views marked `prefork = true` in the manifest are forked at
  startup, which removes the first-hit fork latency (12–60 ms on DO).
* `BLASTIT_MAX_VIEW_PROCESSES` caps the number of live view processes. When a
  new one is needed, the least-recently-used idle one is retired. The default
  is derived at startup as `(MemTotal − usage after preload − reserve) /
  BLASTIT_VIEW_MEM_ESTIMATE`. On DO, `MemTotal` *is* the instance size and
  the cgroup v1 `memory.usage_in_bytes` is accurate.
* The scheduler also watches aggregate usage, which is the only reliable
  number (`VmRSS` doesn't move under gVisor). Above
  `BLASTIT_MEM_HIGH_WATERMARK`, it retires idle view processes oldest-first.

### 8.1 Logs

stdout is blastit's only output, and it is an **event stream: one event per
line**. Events cover two things: what blastit decided (routing, denials,
process lifecycle) and what the app printed. blastit doesn't analyse the
stream. Future, out-of-scope tooling can parse it to find missing grants or
routing drift.

**Formats.** `BLASTIT_LOG_FORMAT=json` (the default, for the server) prints one
JSON object per line. `pretty` prints the same events as coloured, aligned,
human-readable lines, for running blastit by hand. Both carry the same fields.

```json
{"ts":"2026-10-25T14:02:11.384Z","level":"info","event":"request","component":"scheduler","request_id":"01JB9…","method":"GET","path":"/products/42/","route":"product-detail","rule":0,"status":200,"outcome":"ok","queued_ms":0.1,"duration_ms":18.4,"bytes_out":5120,"remote_addr":"203.0.113.7"}
{"ts":"2026-10-25T14:02:12.007Z","level":"warn","event":"sql.denied","component":"dbproxy","route":"product-detail","pid":57,"proxy":"db","kind":"parse","sqlstate":"42501","in_transaction":false,"sql":"SELECT \"shop_product\".\"price\" FROM \"shop_product\" WHERE \"shop_product\".\"id\" = $1"}
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
| `request` | scheduler | info (warn for 5xx) | `request_id`, `method`, `path`, `route` (null if unmatched), `rule` (index of the matching `path` regex), `status`, `outcome` (`ok`, `unmatched`, `queue_full`, `queue_timeout`, `timeout`, `view_died`, `client_gone`), `queued_ms`, `duration_ms`, `bytes_in`, `bytes_out`, `remote_addr` | once per request, at the end; this is the routing decision |
| `sql.denied` | dbproxy | warn | `route`, `pid` (view process), `proxy`, `kind` (`query`, `parse`, `function_call`, `copy`, …), `sql` (normalised), `sqlstate`, `in_transaction` | a statement was rejected |
| `pg.refused` | dbproxy | warn | `route`, `proxy`, `reason` (`credentials_mismatch`, `too_many_connections`, `upstream_unreachable`, …) | a connection to the proxy was refused |
| `http.rejected` | http | info | `remote_addr`, `status` (400, 413, 431, 501…), `reason` | a request was refused before reaching the scheduler |
| `process.spawned` / `process.exited` / `process.retired` | init, scheduler | info (warn for crashes) | `route`, `pid`, `uid`, `fork_ms`, `status` or `signal`, `reason` (`idle`, `max_requests`, `stuck_threads`, `memory_pressure`) | view process lifecycle |
| `app.output` | init | info (stdout) / warn (stderr) | `stream`, `line`, `part`, `parts` | the app or a library printed something |
| `boot`, `ready`, `shutdown`, `manifest.warning` | init | info / warn | stage, counts, durations, the rendered output variable *names* per proxy (never values), the warning text (for example an unattached group) | lifecycle of the whole server |

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
| `BLASTIT_MANIFEST` | `/app/blastit.toml` | manifest path |
| `BLASTIT_SCRIPT_NAME` | — | mount prefix stripped before routing, passed as `SCRIPT_NAME` |
| `BLASTIT_LOG_FORMAT` | `json` | `json` (one event per line) \| `pretty` (coloured, for humans) |
| `BLASTIT_LOG_LEVEL` | `info` | minimum level of events printed |
| `BLASTIT_UID_BASE` | `20000` | base of the uid layout |
| `BLASTIT_MAX_WORKERS` | `16` | global number of live request threads |
| `BLASTIT_VIEW_MAX_WORKERS` | `= MAX_WORKERS` | per-view thread cap (also sets `RLIMIT_NPROC`) |
| `BLASTIT_QUEUE_SIZE` / `BLASTIT_QUEUE_TIMEOUT` | `256` / `10s` | admission queue |
| `BLASTIT_REQUEST_TIMEOUT` | `30s` | time until the client gets a `504` |
| `BLASTIT_STUCK_THREADS` | `4` | overdue threads before a view process is recycled |
| `BLASTIT_VIEW_SPAWN` | `lazy` | `lazy` \| `eager` (`prefork = true` in the manifest overrides per view) |
| `BLASTIT_VIEW_IDLE_TIMEOUT` / `BLASTIT_VIEW_MAX_REQUESTS` | `600s` / `0` (off) | view process recycling |
| `BLASTIT_MAX_VIEW_PROCESSES` | derived from memory | live view process cap (LRU retirement) |
| `BLASTIT_VIEW_MEM_ESTIMATE` | `32 MiB` | expected cost per view process, used for the cap |
| `BLASTIT_MEM_HIGH_WATERMARK` | `85%` | of `MemTotal`; above it, idle view processes are retired |
| `BLASTIT_MAX_HEADER_BYTES` / `BLASTIT_MAX_BODY_BYTES` | `64 KiB` / `100 MiB` | HTTP limits |
| `BLASTIT_HEADER_TIMEOUT` / `BLASTIT_KEEPALIVE_TIMEOUT` | `10s` / `75s` | HTTP timeouts (keep-alive above the LB's) |
| `BLASTIT_TRUST_FORWARDED` | `1` | trust `X-Forwarded-*` from the LB |
| `BLASTIT_HEALTH_PATH` | `/_blastit/health` | answered by the scheduler |
| `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` | `= MAX_WORKERS` | upstream connection cap of proxy `<NAME>` (upper-cased) |
| `BLASTIT_ENV_STRIP` | — | extra variables hidden from the app |
| `BLASTIT_SECCOMP` | `1` | the §7.3 filters (verified on DO) |
| `BLASTIT_GRACEFUL_TIMEOUT` | `25s` | drain time on `SIGTERM` |
| `BLASTIT_WARMUP` | — | optional `module:callable` run before the fork |

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

### 9.2 Memory

5. **Preload-then-fork is still the big memory win, but gVisor raises the
   cost of each process.** On Linux, 100 forked workers of a Django +
   scientific-stack parent (≈173 MiB) cost 575 MiB of PSS in total
   (experiment 2). On DO, each forked process costs a floor of about
   14–18 MiB however small or large the parent is. That covers its own
   stack, malloc arenas and interpreter state.
6. **gVisor breaks COW in chunks of about 2 MiB.** After a fork, touching
   0.1% of the objects in a 67 MiB Python heap, at random, made each child
   copy **82 MiB** (on Linux: about 3 MiB). Touching 10% contiguously cost
   only about 8 MiB. Python refcounts write to every object a request reads,
   so without countermeasures each view process would end up copying most of
   the preloaded heap.
7. **Immortalizing the preloaded heap fixes it.** Before `gc.freeze()`, init
   walks every reachable object (`gc.get_objects()` plus `gc.get_referents()`)
   and sets the low 32 bits of `ob_refcnt` to `0xFFFFFFFF`, which is CPython
   3.12/3.13's immortal marker on 64-bit (PEP 683). Incref and decref then
   never write to those objects. Measured on DO:

   | per-child cost on DO | plain | immortalized |
   |---|---:|---:|
   | touch nothing | 17.8 MiB | 17.8 MiB |
   | touch 0.1% / 1% of objects at random | 87 / 89 MiB | **18.8 / 18.8 MiB** |
   | touch 100% | 94 MiB | 26 MiB |
   | call 30 warmed-up functions × 100 | 21 MiB | 22 MiB |

   Immortalizing 1.6 M objects took 1.8 s on DO. What remains is real writes:
   in-place mutation of shared dicts and lists, caches (URL resolver,
   templates, `lru_cache`), and bytecode specialization of code that wasn't
   warmed up. That's why `BLASTIT_WARMUP` exists. The marker depends on the
   CPython version, so blastit refuses to start on a version it doesn't know.
   Immortal objects are never freed, which is fine for a heap that lives as
   long as the container.
8. **Measure memory on aggregates only.** gVisor has no `smaps_rollup`, and
   `VmRSS` doesn't change after allocations. On DO, `/proc/meminfo` reports
   the instance (`MemTotal` = 1 GiB), and cgroup v1
   `memory/memory.usage_in_bytes` is accurate. (Local `runsc` showed the
   host's `meminfo`; DO doesn't.) `os.cpu_count()` returns 8 on a 1 vCPU
   instance, so never size anything from it.

### 9.3 Python and fork

9. **Threads and `fork` don't mix.** BLAS/OMP thread pools started before a
   fork don't exist in the child, and the child deadlocks: exactly the KMeans
   views hung with multi-threaded BLAS, on Linux and under gVisor alike.
   Hence the single-threaded-at-fork invariant and the forced
   `*_NUM_THREADS=1`.
10. **Fork and GIL ordering with pyo3.** `PyOS_AfterFork_Child()` must run
    *while holding the GIL*; otherwise CPython aborts. init's stage 1 is the
    only thread and holds the GIL for its whole supervision loop, so it forks
    between `PyOS_BeforeFork()` and `PyOS_AfterFork_Parent()`, and the child
    inherits the GIL, calls `PyOS_AfterFork_Child()`, and then only calls
    `blastit_view::serve`, which releases it for its I/O loop.
11. **Embedded CPython can't find the distro's `site-packages` by itself.**
    Append them to `sys.path` before the first import.

### 9.4 Latency and robustness

12. **gVisor makes syscalls and forks expensive.** Syscall-heavy requests take
    about twice as long (27 → 50 ms in the PoC). A fork takes 12–60 ms on DO,
    against 0.5–7 ms on Linux. blastit adds two internal hops each way
    (http → scheduler → view), and the DB adds one more. Frames are large and
    batched, and hot views are forked at startup.
13. **A receive timeout on every worker channel** turned a deadlocked worker
    into a single `502` instead of a hung server. It is generalised here as
    the scheduler's timeouts and recycling.

---

## 10. Platform constraints (DigitalOcean App Platform)

All of these were measured on a live App Platform service unless they're
marked *(docs)*.

| Constraint | Consequence for blastit |
|---|---|
| gVisor (reports kernel `4.4.0`); some syscalls missing (`pidfd_open` → `ENOSYS`) | AF_UNIX only internally; no `pidfd`, `io_uring` or namespaces; check every syscall we use against gVisor's compatibility table |
| Root, PID 1, Docker-default capabilities minus `NET_RAW` | enough: `SETUID`, `SETGID`, `CHOWN`, `FOWNER`. The image must **not** set a non-root `USER` |
| No `CAP_SYS_ADMIN` / `CAP_NET_ADMIN` | no mount or user namespaces, no cgroups of our own, no per-uid firewall. Isolation is uids + file modes + rlimits + seccomp |
| `SO_PEERCRED` broken; `/proc/<pid>/environ` readable across uids | per-view pg listeners + `SCM_CREDENTIALS`; `execve` barrier; dbproxy exec'd with an empty env |
| COW in ~2 MiB chunks; ~14–18 MiB floor per process | immortalized heap, lazy spawning, LRU cap on live view processes |
| HTTP/1.1 from the edge, TLS terminated there *(docs)* | http speaks plaintext HTTP/1.1 only |
| `PORT` set by the platform (`8080` by default) | bound by init |
| Root filesystem is a writable 9p overlay; `/tmp` and `/dev/shm` are tmpfs (count as RAM); 4 GiB, ephemeral, no volumes *(docs)* | runtime dirs under `/run/blastit` (disk, not RAM); recreated on every start |
| Forwarded log lines ≤ 2048 bytes *(docs)* | init re-chunks every log line |
| amd64 only *(docs)* | single Rust binary built in the same image as `libpython` |
| `MemTotal` = instance size; cgroup v1 memory usage available; `cpu_count()` overstates | memory cap derived at startup; never size from the CPU count |
| Managed PG connection limits (about 20 on small plans) *(docs)* | `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` |
| Kubernetes underneath (`KUBERNETES_*` variables in the env) | stripped from the app env like everything else not needed |

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
* **Immortalization across CPython versions.** The low-32-bit marker is
  verified on 3.12. 3.13 should behave the same; 3.14 and the free-threaded
  build use a different refcount layout and need their own handling.
* **pregresql#1** (§2.2): review and merge. Then check
  `message_credentials` under DO's gVisor with the real crate; the spike only
  checked `SCM_CREDENTIALS` from Python.
* **SQL variants.** `IN ($1, $2, …)` lists of varying length, Django's dynamic
  savepoint names, and named cursors (`DECLARE "_django_curs_…"`) all break
  exact matching. We will probably need a fingerprint-style normaliser
  (collapse `IN` lists, wildcard identifier suffixes) instead of exact text.
* **Parameter constraints.** Should the manifest ever constrain parameter
  values (for example "`$1` must equal the session's user id")? Out of scope
  for v1.
* **Keeping the routing table in sync with the app.** The two route
  independently, so they can drift. blastit only logs the evidence
  (`request` and `sql.denied` events); turning it into manifest changes is
  left to future tooling.
