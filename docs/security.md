# Security model

What blastit is claiming, what it is not, and what holds each claim up. This
page is the summary; the
[specification](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#7-security-model)
is the argument, and the
[spike reports](https://github.com/OffworldNexus/blastit/tree/develop/docs/spikes)
are the measurements.

## Threat model

**In scope: compromise at request time.** Remote code execution in a view, SQL
injection, a deserialisation bug, a vulnerable dependency reachable from a
route. The question blastit answers is: *when that happens, what is the
attacker holding?*

**Out of scope, deliberately:**

| Not covered | Why |
|---|---|
| code the app runs **at import time** | imports run before the sandboxes exist, and they have to: sharing the preloaded heap copy-on-write is the memory story. Installed as a wheel, this includes the virtualenv's `.pth` files and `sitecustomize`, which run with the real credentials still in the environment |
| **other secrets** in the environment | an API key for a third-party service that the app itself must hold cannot be swapped, and stays visible to every route. `BLASTIT_ENV_STRIP` hides what you name; proxies are the general answer, and only `postgresql` has one so far |
| TLS, HTTP/2, WebSockets, `Upgrade` | a load balancer terminates TLS. `Upgrade` is answered `501` |
| per-uid **network egress** filtering | that needs `CAP_NET_ADMIN`, which the target platform does not grant. The best available is a seccomp filter on `socket()` |
| constraining **parameter values** | out of scope for v1: a grant says which statement, not which row |
| an attacker who already has root in the container | init is root. Nothing here is a defence against the platform |
| the app behaving | blastit does not look inside your app. A route that reads `SECRET_KEY` and renders it is doing that on its own |

## What a compromise buys

| If this is compromised… | …the attacker gets | …and cannot |
|---|---|---|
| **a view** (request-time RCE) | that view's allowlisted statements; its scratch directory; egress, only if the route has `egress = true`; other processes' `environ`/`cmdline`/`maps`, which hold no secrets | run any other statement; read the real database credentials; read or `ptrace` another process's memory; connect to another view's database socket; reach another view's channel; hold more threads than its `RLIMIT_NPROC` |
| **http** | the raw bytes of in-flight HTTP traffic | reach the database (its uid is in no listener's table); run Python; spawn processes |
| **the scheduler** | it can send any request to any view | do anything a view could not already be made to do by an HTTP request |
| **dbproxy** | the real database credentials | — this is the crown jewel. It runs no application code, is not dumpable, and is the smallest Rust component in the tree |
| **init** | everything | — it runs only blastit code after stage 1's imports, and handles no external input |

The sandbox comes from **uid privileges, not from code isolation**: all app code
is mapped into every view process, and a view can run another view's code — only
with its own allowlist. That is also why a routing mistake is safe: a misrouted
request lands in a process whose allowlist does not fit, its queries are denied,
and it never gains a privilege.

## The mechanisms

### Distinct uids, and what the sandboxed kernel does *not* do

Every process has its own uid, `gid == uid`, and an empty supplementary group
list. Different uids cannot signal each other, `ptrace` each other, or read each
other's `/proc/<pid>/{mem,io,cwd,root}`. Views never hold `CAP_SYS_PTRACE`.

On App Platform's gVisor, the
[spike](https://github.com/OffworldNexus/blastit/blob/develop/docs/spikes/2026-10-do-app-platform.md)
measured two things that differ from bare Linux, and the design accommodates
both:

* **another uid *can* read** `/proc/<pid>/{environ,cmdline,maps,auxv,stat,status}`
  and list `/proc/<pid>/fd`. `PR_SET_DUMPABLE(0)` changes the file's owner but not
  its `0444` mode. So nothing secret is ever in `environ` or `argv`, and the
  fact that a view can see another view's file descriptors and memory layout is
  accepted rather than fought;
* **`SO_PEERCRED` on an accepted Unix socket returns the listener's own
  credentials** — it cannot identify a client. That is why [identity comes from
  the listener](how-it-works.md#identity-not-passwords) plus `SCM_CREDENTIALS`.
  File permissions on Unix sockets *are* enforced at `connect()`, and forging a
  uid in `SCM_CREDENTIALS` fails with `EPERM`.

### The credential barrier

Two rules, and both are structural rather than conventional:

1. **no process may ever have a real secret in its `environ` or `argv`;**
2. **the process that holds them is `execve`'d with an empty environment**, and
   receives them over an inherited pipe.

The barrier is an `execve`: stage 0 reads the real credentials, hands them to
dbproxy over a pipe, and replaces its own address space with stage 1's, whose
environment has the credentials removed and the proxies' local endpoints in their
place. `fork` alone would not do it: a forked child keeps showing its parent's
*initial* environment block, which is what `/proc/<pid>/environ` reports.

The spike proved the scheme end to end with a real DigitalOcean `SECRET`: after
the barrier, a view uid found it neither in any `environ`/`cmdline`, nor in its
own inherited memory, nor on disk — while a deliberately un-`exec`'d control
child leaked it. The same check runs in CI as part of the end-to-end test, which
scans the log for the database password as a test rather than as a convention.

### Capabilities, and `no_new_privs`

Every child **clears every capability set it holds** and sets
`PR_SET_NO_NEW_PRIVS`, in the order the kernel requires: ambient first, then
inheritable, then the bounding set while `cap_setpcap` is still effective, then
`setgroups`/`setresgid`/`setresuid`, then permitted and effective. The drop is
checked, not assumed — a child whose sets are not empty does not start.

That matters most in rootless mode. A rootless init's uid never was 0, so the
kernel does not clear its capabilities on `setresuid`: a child that dropped
nothing would keep `cap_setuid`, which is a way back to any uid in the container.
And `no_new_privs` is what makes a capable file harmless to a child: the kernel
refuses an `execve` of a file whose capabilities cannot be honoured
(`EPERM`).

The capability has to be on a **dedicated copy** of the file init starts from,
owned by init's uid, mode `0500`, because a file with file capabilities turns any
process of that uid that can run it into init. The
[rootless recipe](infra/compose-traefik.md#rootless) has the details, including
the boot checks that refuse the two deployments that cannot work.

### seccomp

Unless `BLASTIT_SECCOMP=0`, each child installs a BPF filter **after** dropping
privileges. It needs only `PR_SET_NO_NEW_PRIVS`, no capability — which is what
makes it work on the target platform, where the spike measured `socket(AF_INET)`
going to `EPERM` while `AF_UNIX` kept working.

The filter is a **refusal list**: the default action is allow, and what it
refuses it refuses with `EPERM`, so a syscall it says nothing about cannot be
what breaks a request. It is installed while the child is still single-threaded,
because it cannot be removed and applies to every thread, present or future.

* **every role** refuses `ptrace`, `process_vm_readv`/`writev`, `bpf`, `keyctl`,
  `perf_event_open`, `userfaultfd`, `io_uring`, the module-loading calls, `mount`
  and the new mount API, `setns`, `unshare`, `open_by_handle_at`, `pidfd_getfd`,
  `kcmp` and friends — the calls that read or steer another process, load code
  into the kernel, or move between namespaces;
* **views** may use `AF_UNIX` only. A route with `egress = true` may also use
  `AF_INET`/`AF_INET6`;
* **http and the scheduler** may not call `socket()` at all: they only use
  inherited descriptors;
* **dbproxy** may use `AF_UNIX`, `AF_INET` and `AF_INET6`: it connects to the
  real PostgreSQL over TCP;
* **a foreign architecture is fatal.** The filter compares the syscall's
  architecture against the build's, and kills the process when it does not
  match, because a syscall number from another ABI has nothing safe to compare
  it against.

This is the only lever for egress control, and it is what `egress = true` in the
manifest buys.

### Filesystem and limits

* the code tree is read-only to views (owned by root, mode `0755`/`0644`);
* each view gets a private `0700` scratch directory and `umask 077`, which is
  also its `HOME` and `TMPDIR`. `/tmp` stays `1777` but is no longer the default
  scratch directory;
* `RLIMIT_NPROC` per view uid — counting threads as well as processes, summed
  over every process of the uid — plus `RLIMIT_NOFILE` and `RLIMIT_CORE=0`. It
  is a hard backstop *under* the scheduler's worker budget;
* `PR_SET_PDEATHSIG(SIGKILL)`: no process outlives init.

## The client address

`REMOTE_ADDR` and `wsgi.url_scheme` are the app's, and both come from the same
resolver, which believes a header only from a peer in
`BLASTIT_TRUSTED_PROXIES`. Two rules matter to a reviewer:

* **`BLASTIT_TRUSTED_PROXIES=*` trusts every peer**, and both presets use it.
  Each preset says why the assumption holds on its platform — and it is a real
  assumption, not a default: if the port is reachable from somewhere you do not
  control, every peer can forge the client address, and the fair queue's flows
  and your logs key on a value an attacker chose;
* **`*` takes the rightmost entry of the chain**, not the leftmost. That is the
  entry the hop closest to blastit added, and the one thing that hop cannot be
  lying about where the edge appends to a header the client already sent.

The full rule, including why a network list is not the safer default, is in
[client IP and trusted proxies](infra/anything-else.md#client-ip-and-trusted-proxies).

## Evidence

Every claim above is measured somewhere, and each measurement is checked in CI
where it can be. The rows worth chasing:

| Claim | Where it was measured |
|---|---|
| `setuid()` to any uid works without an `/etc/passwd` entry, and does not break copy-on-write | experiment 1; re-checked by the App Platform live test |
| `SO_PEERCRED` is wrong under gVisor; `SCM_CREDENTIALS` is right; socket file permissions are enforced | the App Platform spike |
| `/proc/<pid>/environ` is readable across uids, even with `PR_SET_DUMPABLE(0)` | the App Platform spike — this is *why* the barrier exists |
| a real injected `SECRET` is unreachable from a view uid after the barrier | the App Platform spike, and the end-to-end test's log scan |
| seccomp filters installed by the application are enforced, and `PR_SET_NO_NEW_PRIVS` is too | the App Platform spike |
| `RLIMIT_NPROC` is per uid and counts threads | experiment, and the spike (gVisor leaves out the main thread, so leave one unit of margin) |
| a non-root `--user` process gets **no** capabilities from `--cap-add` | measured on Docker 29.5 (compose v5), locally and on a Droplet |
| a file with capabilities is init's alone, and a child cannot `execve` one | experiment, and the rootless end-to-end test, which checks that a different uid is refused the copy |
| copy-on-write breaks in ~2 MiB chunks, and immortalisation fixes it | experiments 2 and 3; the numbers are in [§9.2](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#92-memory) |
| the App Platform edge **overwrites** `do-connecting-ip` and **appends** to `X-Forwarded-For` | the live probe beside [§10](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#10-platform-constraints-digitalocean-app-platform) |
| what Traefik v3 does with each of eight headers, under three configurations | the measurement beside [§10](https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#docker-compose-behind-traefik) |

## Reviewing it yourself

The end-to-end test is written to be read as an argument rather than as a
smoke test:

* `e2e/scenario.py` drives a small Django app over HTTP: log in, add and delete
  your own notes, log out, and — the interesting part — run the *same* raw SQL
  text from two routes that were granted different statements, and from a third
  with no grants at all. The denials are per route;
* `e2e/check_logs.py` reads blastit's own stdout afterwards and asserts on it:
  the `sql.denied` events name the route that should have been refused, the
  database password appears nowhere in the log, and the `boot` events say which
  mode, which preset and which seccomp setting were in force;
* `e2e/rootless/` runs the same scenario with init **not** as root, and then
  checks `/proc` in the container: every process of a layout uid holds nothing,
  bounding set included, and has `no_new_privs` set;
* `e2e/live.py` runs the same thing on App Platform, with a fresh managed
  PostgreSQL per run — and it is the only place the platform's own behaviour is
  checked against a real binary.

```console
$ uv run e2e/run.py                # under blastit, in Docker
$ uv run e2e/rootless/run.py --local
```

## See also

* [How it works](how-it-works.md) — the same machine, described.
* [The manifest](reference/manifest.md) — what "granted" means exactly, and what
  an exact `queries` entry does *not* check.
* [Log events](reference/log-events.md#what-is-never-logged) — the fields that
  cannot appear in a log line.
