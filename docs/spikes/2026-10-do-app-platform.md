# Spike — blastit primitives on DigitalOcean App Platform

**Date:** 2026-10-25 · **Code:**
[`Xowap/blastit-do-spike`](https://github.com/Xowap/blastit-do-spike) (public,
stdlib-only Python probe) · **Raw report:**
[`2026-10-do-app-platform.report.json`](2026-10-do-app-platform.report.json)

**Target:** an App Platform service `blastit-do-spike` on the `delos` team,
region `fra`, `apps-s-1vcpu-1gb`, built from the repo's `Dockerfile`
(`python:3.12-slim`, no `USER`, so it runs as root). The probe runs once at
boot and serves its JSON report on `$PORT`: `/` for the summary, `/full` for
everything, `/rerun` to run it again. Every probe runs in a forked child,
usually after `setuid()` to an arbitrary uid.

The baseline is the same image under Docker/runc on the dev host (Linux
7.0.0-38). The design consequences are applied in
[`ARCHITECTURE.md`](../../ARCHITECTURE.md) §2.2, §4, §7, §8, §9 and §10.

## Results

| # | Question | Linux (runc) | DO App Platform (gVisor) | Verdict |
|---|---|---|---|---|
| S1 | Root with `CAP_SETUID`/`CAP_SETGID`? | yes | **yes**: PID 1, uid 0, Docker defaults minus `NET_RAW`; `setuid` drops every capability, and `setuid(0)` afterwards → `EPERM` | ✅ |
| S2 | `SO_PEERCRED` = connecting uid? | yes | **no**: returns the listener's own pid/uid (root or not, stream or seqpacket); the client side sees itself too | ❌ |
| S2b | `SO_PASSCRED` → `SCM_CREDENTIALS`? | correct | **correct**, automatic (sender sends nothing special); forged uid 0 → `EPERM` | ✅ |
| S2c | Unix socket file permissions enforced at `connect()`? | yes | **yes**: `0600` socket and `0700` directory both → `EACCES` for other uids | ✅ |
| S2d | `connect("/proc/self/cwd/.s.PGSQL.5432")`? | works | **works** | ✅ |
| S3 | App-installed seccomp enforced? | yes | **yes**: `AF_INET`/`AF_INET6` → `EPERM`, `AF_UNIX` ok | ✅ |
| S4 | Usable uid range | all tested | **all tested**, up to 2³²−2; `uid_map` = `0 0 4294967295` | ✅ |
| S5 | `RLIMIT_NPROC` per uid, counts threads? | yes (limit 16 → 15 threads) | **yes** (limit 16 → 16 threads: main thread not counted); siblings of the same uid count | ✅ |
| — | Cross-uid `kill`, `ptrace`, `/proc/<pid>/mem` | denied | denied | ✅ |
| — | Cross-uid `/proc/<pid>/environ` | denied | **readable**, `PR_SET_DUMPABLE(0)` doesn't help; so are `cmdline`, `maps`, `auxv`, `stat`, `status`, the `fd/` list and `readlink`s; `/proc/1/environ` too | ❌ |
| — | `execve` clears `/proc/<pid>/environ`? | — | **yes** (`SECRET` visible before, gone after) | ✅ |
| — | Forked child's `environ` | initial block | initial block (changes to `os.environ` don't show) | — |
| — | `SCM_RIGHTS` across uids, `SOCK_SEQPACKET`, `execve("/proc/self/exe")`, `PDEATHSIG`, subreaper, `chown` | all ok | all ok (seqpacket: 200 KiB ok, 1 MiB `EMSGSIZE`) | ✅ |
| — | `pidfd_open` | ok | `ENOSYS` | ⚠️ |
| — | Loopback | yes | yes (`lo`, `eth0`) | ✅ |
| S6 | `fork()` time | 0.5–7 ms | **12–60 ms** (up to ~300 ms with 20 children of a 180 MiB parent) | ⚠️ |
| S6 | Per-child floor (parent 0 / 64 / 256 MiB, child reads only) | ~0.1–0.5 MiB | **≈13.5 MiB**, whatever the parent size | ⚠️ |
| S6 | COW cost, 67 MiB heap, child touches 0.1% / 1% of objects at random | 3 / 15 MiB | **82–89 MiB** (the whole heap) | ❌ |
| S6 | …same, heap immortalized first | 1.1 / 1.2 MiB | **18.8 / 18.8 MiB** (= the floor) | ✅ |
| — | Memory observability | cgroup v2 `memory.current` | no `smaps_rollup`; `VmRSS` doesn't move; `MemTotal` = 1 GiB (the instance); cgroup v1 `memory.usage_in_bytes` accurate | — |
| — | `os.cpu_count()` on a 1 vCPU plan | 24 (host) | 8 | ⚠️ |

## A DO-injected secret, hidden from view uids

**Question:** DO delivers configuration, secrets included, as environment
variables of the container's PID 1, and gVisor lets any uid read any
process's `/proc/<pid>/environ`. Can blastit still keep a secret away from
the views?

**Setup:** `SPIKE_SECRET` = `BLASTITSECRET-<32 random hex>`, declared in the app
spec as `type: SECRET, scope: RUN_TIME`. DO stores it encrypted (`EV[…]`), and
the value never appears in the repo. Deployment `71142c83` ran commit
`a06fcba`. Report:
[`2026-10-do-secret-barrier.report.json`](2026-10-do-secret-barrier.report.json),
which contains only hashes.

1. **Stage 0 (PID 1, uid 0, the platform's env):** reads the secret.
   * It forks a **negative control** (uid 20500) *without* exec. That child's
     `environ` still holds the secret.
   * It fork + `execve`s a **holder** (uid 20503) with `env = {}` and sends it
     the secret over a pipe. The holder stands in for dbproxy.
   * It then `execve`s itself without the variable, passing only the sha256.
2. **Stage 1 (still PID 1):** a scanner forked as **uid 20510** (a view), and
   one as root, search for `BLASTITSECRET-[0-9a-f]{32}` in:
   * their own memory, inherited from stage-1 PID 1, read via
     `/proc/self/mem`, with a planted fake canary as a positive control;
   * the `environ` and `cmdline` of every process;
   * every other process's `/proc/<pid>/mem`;
   * every file under `/etc /run /var/run /tmp /root /app /home /dev/shm
     /var/tmp`.

| Check | Result |
|---|---|
| DO injected our value into PID 1's env | ✅ stage-0 sha256 `e73cf2f6…` = local sha256 of the value we set |
| Holder (empty env) has the secret | ✅ its sha256 matches; its `environ` = `LC_CTYPE` only |
| PID 1 after `execve`: `os.environ` / `/proc/1/environ` | ✅ variable absent from both |
| View uid scans its own memory (42.6 MB inherited from PID 1) | ✅ nothing found; the canary *was* found, so the scan works |
| View uid scans every `environ` + `cmdline` | ✅ found **only** in the control process, so the scan works and gVisor really does expose `environ` |
| View uid reads other processes' `mem` | denied for pids 1, 3, 4 |
| View uid scans 413 files | ✅ nothing found |
| Root scanner (forked from stage 1) | same: only the control process |
| Secret in DO runtime logs / in the report | ✅ 0 occurrences |

**Verdict: yes.** A secret that DO injects through the app spec reaches the
process that needs it and nothing else, *provided* every process that
inherited the original environment block is `execve`'d away. The negative
control shows what happens otherwise: a process forked without exec from the
PID 1 that received the env leaks the secret to every uid under gVisor.

## Notes on the method

* **Memory.** Per-child cost is the change in cgroup v1
  `memory.usage_in_bytes` (and `MemAvailable`, which agreed on DO) while all
  children are alive, divided by the number of children. The children drop to
  distinct uids before touching memory.
* **COW granularity.** The 2 MiB figure is inferred. Sparse touches copy the
  whole heap, while 10% contiguous touches (≈6.7 MiB of objects) cost ≈8 MiB
  above the floor. This fits gVisor breaking COW in huge-page-sized units.
* **Immortalization.** Walk `gc.get_objects()` plus `gc.get_referents()` from
  every object, set the low 32 bits of `ob_refcnt` to `0xFFFFFFFF` (CPython
  3.12, 64-bit little-endian), *then* `gc.freeze()`. Order matters:
  `gc.get_objects()` skips frozen objects. On DO, 1.6 M objects took 1.8 s.
  Function calls on warmed-up code stayed at the floor.

## Cleanup

The service is still running so the probe can be re-run (`/rerun`). To remove
it:

```bash
doctl --context delos apps delete d68f26db-0fae-47bb-8eae-1c9fa17aa354
gh repo delete Xowap/blastit-do-spike
```
