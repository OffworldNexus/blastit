# Security model

What blastit claims, and what it does not. The argument is in
[`ARCHITECTURE.md` §7](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#7-security-model).

## Threat model

**In scope: compromise at request time** — RCE in a view, SQL injection, a
deserialisation bug, a vulnerable dependency reachable from a route. The question
is what the attacker is holding when that happens.

**Out of scope, deliberately:**

* code the app runs **at import time** — it runs in stage 1, as init, before any
  view exists, so there is no per-route sandbox yet;
* **other secrets** in the environment — an API key the app must hold cannot be
  swapped, and every route can read it. `BLASTIT_ENV_STRIP` hides what you name;
  a proxy is the general answer, and only `postgresql` has one;
* TLS, HTTP/2, WebSockets and `Upgrade` — a load balancer terminates TLS, and
  blastit answers `501`;
* per-uid **egress filtering** — needs `CAP_NET_ADMIN`; the available lever is a
  seccomp filter on `socket()`.
* constraining **parameter values** — a grant says which statement, not which row;
* an attacker who already has root in the container, and the app's own behaviour.

## What a compromise gets

| If this is compromised… | …the attacker gets | …and cannot |
|---|---|---|
| **a view** (request-time RCE) | that route's allowlisted statements; its scratch directory; **every file its uid can open** — the code tree, the virtualenv, `/tmp`; all the CPU and memory the container has; egress only with `egress = true` | run any other statement; read the real database credentials; `ptrace` another process; connect to another route's socket or channel; hold more threads than its `RLIMIT_NPROC`; take another route's worker slot |
| **http** | the raw bytes of in-flight HTTP traffic | reach the database; run Python; spawn processes |
| **the scheduler** | it can send any request to any view; every file beneath each declared static `root` — dotfiles and symlinks included | read the real database credentials; reach another route's socket or a view's memory; go beyond what its uid and a socket-less seccomp role allow |
| **dbproxy** | the real database credentials | — it runs no app code, is not dumpable, and is kept small |
| **init** | everything | — it runs only blastit code after stage 1's imports |

Two lines in that row are worth reading twice: a view has no CPU or memory limit
of its own, and it can read any file its uid can open. Cap the container for
that; `BLASTIT_MAX_WORKERS` bounds the threads.

The sandbox comes from **uid privileges, not code isolation**: all the app's code
is mapped into every view, so a view can run another route's code — only with its
own allowlist. That is why a misrouted request is safe: it lands in a process
whose allowlist does not fit, and its queries are denied.

## What the sandboxed kernel does not do

Every process has its own uid, `gid == uid`, no supplementary groups: different
uids cannot signal, `ptrace` or read each other's
`/proc/<pid>/{mem,io,cwd,root}`. Two things differ from bare Linux:

* **another uid *can* read** `/proc/<pid>/{environ,cmdline,maps,auxv,stat,status}`
  and list `/proc/<pid>/fd`. So no secret is in any sandboxed process's
  `environ` or `argv`, and a view seeing another's file descriptors and memory
  layout is accepted rather than fought;
* **`SO_PEERCRED` returns the listener's own credentials.** That is why identity
  is the [listener plus `SCM_CREDENTIALS`](how-it-works.md#identity-not-passwords):
  socket file permissions *are* enforced at `connect()`, and forging the
  credentials fails with `EPERM`.

## The mechanisms

* **The credential barrier.** No process *after* stage 0 holds a real secret in
  its `environ` or `argv`: stage 0 keeps it only until its own `execve`, which
  replaces that environment before any app or broker process exists. The broker
  is `execve`'d with an empty environment and receives the secret over an
  inherited pipe. A `fork` alone would not do it: a forked child keeps showing
  its parent's initial environment block.
* **Capabilities.** Every child clears every capability set it holds and sets
  `PR_SET_NO_NEW_PRIVS`, and the drop is checked, not assumed. That flag is also
  what makes the [capable copy](deploy/compose-traefik.md#rootless) harmless to a
  child: the kernel refuses an `execve` of a file whose capabilities cannot be
  honoured.
* **seccomp.** Each child installs a BPF refusal list after dropping privileges:
  the default is allow, and what it refuses it refuses with `EPERM`. Every role
  refuses `ptrace`, `bpf`, `keyctl`, `perf_event_open`, `io_uring`, the
  module-loading calls, `mount` and the new mount API, `setns` and `unshare` —
  the calls that read or steer another process, load kernel code or move between
  namespaces. Views may use `AF_UNIX` only, plus `AF_INET`/`AF_INET6` with
  `egress = true`; http and the scheduler may not call `socket()` at all.
  `egress = true` is any IPv4/IPv6 address and port, cloud metadata included:
  there is no allowlist until the HTTP proxy milestone.
* **Filesystem and limits.** The code tree is read-only to views; each view gets a
  private `0700` scratch directory, `umask 077` and that directory as `HOME` and
  `TMPDIR`; `RLIMIT_NPROC` (counting threads) is a hard backstop *under* the
  scheduler's budget, with `RLIMIT_NOFILE` and `RLIMIT_CORE=0`;
  `PR_SET_PDEATHSIG(SIGKILL)` means no process outlives init.
* **Ingress bodies.** A route declares how a body is handled: `json` and `form`
  decode and re-encode it before the app sees it, `none` rejects a body, and
  `raw` passes the bytes unchanged. A body to a route with no `[route.ingress]`
  is a `413` at runtime; `blastit check` reports a body route that declares
  nothing, and a `raw` route the manifest does not acknowledge. 0.1.1
  has no multipart codec and no media or PDF decoding: an upload is `raw`, and
  the app's parser sees the original bytes, polyglots included.

## Static files

A `[route.static]` route serves files from one declared `root`, answered by the
scheduler as its own uid (`UID_BASE+2`) — no new process, socket or uid.

| What arrives | Answer |
|---|---|
| `..` (plain or percent-encoded), NUL and other control bytes, an absolute or backslash path, invalid UTF-8, a malformed percent escape, a segment over 255 bytes, a path over 4096 bytes | `400` |
| a dotfile such as `.env` or `.git`, a symlink, a FIFO, socket or device, an unreadable or oversize file — and, on a route with no `spa` and no `app`, any path that is no file | `404` |

`/.well-known/` is the one dotfile exception. A refusal is final: never the app,
never `index.html`, whatever the method, so a traversal probe cannot be answered
with the SPA shell. The `404`-vs-fallback distinction reveals that *something*
exists at a hidden path — that is the policy, and the canned body never varies.

The boundary is structural, not a string prefix. One directory descriptor per
root is opened at boot; every candidate is
`openat2(dirfd, path, RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)`,
classified by `fstat`, and read-opened only when it is a regular file whose
`(st_dev, st_ino)` still matches — a swapped path or a device node is never read.
A bind mount under `root` is followed: the boundary is the path namespace, not
one filesystem. One percent-decode, no dot-segment resolution, no Unicode
normalization.

[ADR-0007](https://github.com/OffworldNexus/blastit/blob/develop/docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md)
holds the decision and its trade-offs.

## The client address

`REMOTE_ADDR` and `wsgi.url_scheme` come from one resolver, which believes a
header only from a peer in `BLASTIT_TRUSTED_PROXIES`. Both presets use `*`: every
peer is trusted. `compose-traefik` reads the **rightmost** `X-Forwarded-For`
entry — the one the hop closest to Blastit added; `digitalocean-paas` reads
`do-connecting-ip`, which the platform's edge overwrites. The full rule is in
[client address](reference/settings.md#client-address).

## Dev mode is not a sandbox

`blastit dev` shares one uid between every role. The process topology, the
credential barrier and the grants are real, so a misconfiguration is caught
locally, but there is no uid boundary to make a compromise stay in its route. The
banner says so, the `boot` event reports `"privilege": "unprivileged-dev"`, and
production `blastit` **fails closed**: without root, the capability set or
seccomp it exits instead of serving.

## Check it yourself

The end-to-end tests drive a Django app over HTTP, run the same raw SQL from
routes with different grants, and assert on blastit's log: the denials name the
right route, and the password appears nowhere in it.

```console
$ uv run e2e/run.py                  # in Docker, as root
$ uv run e2e/rootless/run.py --local # the rootless path, checking /proc too
```
