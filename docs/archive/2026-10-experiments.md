# Architecture & Findings

> **Archived.** This is the original write-up of the three feasibility
> experiments (COW across forked uids, the `pywsgi` PoC, gVisor). The findings
> that still shape the design have been carried into
> [`/ARCHITECTURE.md`](../../ARCHITECTURE.md); this file is kept verbatim for the
> raw numbers and the reproduction recipes.

Three experiments, all built and validated on this host (Ubuntu 26.04, Linux
7.0.0-38-generic, 24 vCPU, 124 GiB RAM), then re-validated under gVisor:

1. **`blastit`** — a root process allocates 1 GiB, forks 100 children, each drops
   to a different uid, and we measure whether the RAM stays Copy-on-Write.
2. **`pywsgi`** — a Rust HTTP server that embeds CPython, preloads a Django app
   plus numpy/pandas/scipy/scikit-learn, then forks one worker per view, each
   under a different uid, and dispatches requests over WSGI.
3. **gVisor** — the second experiment re-run inside a user-space-kernel sandbox.

The code has been removed; this document is the durable output. Section 9
contains everything needed to rebuild any of it.

---

## 1. Headline results

| | Experiment 1 (COW) | Experiment 2 (pywsgi) | Experiment 3 (pywsgi on gVisor) |
|---|---|---|---|
| Isolation | QEMU micro-VM, initramfs | QEMU micro-VM, ext4 disk | gVisor (`runsc`) sandbox |
| Guest | 4 GiB / 4 vCPU | 6 GiB / 4 vCPU | host kernel, sentry |
| Workload | 1 GiB buffer, 100 forked children | Django + numpy/pandas/scipy/sklearn, 100 views, 100 workers | same as Experiment 2 |
| Distinct uids | 100 | 100 | 100 |
| Outcome | COW holds | **100/100 views pass** | **100/100 views pass** |
| Naive `sum(VmRSS)` | 100.04 GiB | 12.7 GiB | 16.3 GiB |
| Real `sum(Pss)` | **0.99 GiB** | **575 MiB** | **n/a (unsupported)** |
| Overshoot | 100.7× | 22.1× | — |
| Wall clock | ~7 s | ~8 s (0.85 s preload) | ~15 s (1.11 s preload) |

**The one-sentence result:** forking a preloaded runtime to serve many users is
cheap and safe *only if* the numerical stacks are single-threaded and the sandbox
grants the capabilities and files it needs.

---

## 2. Experiment 1 — `blastit`: COW across 100 forked users

### Design

A single statically-linked Rust binary is `/init` (PID 1) inside a QEMU
micro-VM booted from the host kernel plus a hand-built initramfs. There is no
shell, no busybox, no dynamic linker — the binary mounts `/dev`, `/proc`, `/sys`,
wires stdio to the console, and does everything itself.

```
PID 1 (root)
  │  mmap(MAP_PRIVATE|MAP_ANONYMOUS, 1 GiB)      one anonymous private mapping
  │  write page_index + MAGIC at every page start (so pages are resident AND verifiable)
  ├── fork × 100
  │      child i: setgroups(0,NULL); setgid(uid); setuid(uid)      uid = 20000+i
  │               verify getuid(); read+checksum all 262144 pages over a pipe
  │               signal readiness, then pause()
  └── measure via /proc/<pid>/status, /proc/<pid>/smaps_rollup, /proc/meminfo
```

Two experiments run back-to-back on the same buffer:

* **A — read-only:** children only read. COW must hold.
* **B — COW forced to split:** each child dirties 16 MiB spread across the buffer.

### Results

Guest: 4 GiB RAM, kernel 7.0.0-38-generic, buffer 1 GiB / 262144 pages,
100 children, uids 20000..20099 (none in `/etc/passwd`).

| Metric (over all 100 children) | A: read-only | B: 16 MiB dirtied each |
|---|---:|---:|
| `sum(VmRSS)` | 100.04 GiB | 100.04 GiB |
| `sum(Pss)` | **0.99 GiB** | 2.56 GiB |
| `sum(Private_Dirty)` | 2.7 MiB | **1602.7 MiB** |
| `sum(Shared_Dirty)` | 100.03 GiB | 98.48 GiB |
| guest `AnonPages` | 1026.8 MiB | 2626.9 MiB |
| `MemAvailable` delta (the fork, not the buffer) | **218.8 MiB** | 1.72 GiB |
| RSS/PSS overshoot | 100.7× | 39.1× |

Cost of allocating and touching the 1 GiB buffer itself: **1.01 GiB**
(`MemAvailable` delta measured before/after the allocation).

### Findings

1. **A uid does not need to exist in `/etc/passwd`.** `setuid()` to an arbitrary
   uid requires only `CAP_SETUID`, which root has. All 100 children ran as
   `20000..=20099` with no entries anywhere. (A login or `getpwuid()` would need
   the entry; the bare syscall does not.)
2. **COW survives the fork.** One 1 GiB allocation served 101 processes. The
   guest never used more than ~1 GiB of anonymous memory for it.
3. **`setuid()` does not break COW.** Changing credentials touches the task's
   credential struct, not the data pages.
4. **Only a write splits a page, and the accounting proves it.** Dirtying
   100 × 16 MiB = 1600 MiB moved `Private_Dirty` to 1602.7 MiB while
   `Shared_Dirty` fell by the same amount — a page-for-page fingerprint of COW.
5. **`fork()` is not free even when everything is shared.** 100 children cost
   ~219 MiB, almost entirely page tables: each child needs its own PTEs for the
   shared 1 GiB. Data sharing is free; bookkeeping is not.

### Why the measurements are trustworthy

* `MAP_PRIVATE|MAP_ANONYMOUS` plus a write to every page guarantees the pages are
  genuinely resident — otherwise COW would be unobservable.
* Children checksum all 262144 pages, so their read is real, not a lazy mapping.
* The guest has no swap and no other workload, so `MemAvailable` is clean.

---

## 3. Experiment 2 — `pywsgi`: Rust + embedded CPython + Django over WSGI

### Design

A Rust HTTP server embeds CPython (`pyo3` 0.30), loads Django and the heavy
scientific stack **once**, and then forks one worker per view — the
"preload then fork" pattern, applied to a realistic payload.

```
QEMU micro-VM (KVM, 6 GiB) — kernel + ext4 disk built from a Docker image
  PID 1 = pywsgi (root), started with init=/opt/blastit/pywsgi
    ├── Python::attach → import wsgi_bridge
    │      django.setup()  +  numpy, pandas, scipy, sklearn  +  the 100 views
    │      warmup(): run every view once, so nothing lazy happens after the fork
    ├── bind 127.0.0.1:8080
    ├── fork × 100   ── worker i: setgid/setuid(20000+i), serves /view/i
    └── fork × 1     ── test client: GET /view/0 … /view/99, one by one

  parent parses just enough HTTP to route /view/N → worker N (per-worker socketpair)
  worker builds the WSGI environ, calls Django's WSGIHandler in-process, returns
  the complete raw HTTP response
```

* **100 views** are generated from ten numerical families (matmul, FFT, pandas
  groupby, `scipy.linalg.det`, `scipy.special.gamma`, `scipy.stats.norm.cdf`,
  LinearRegression, KMeans, PCA, pandas rolling/ewm), parameterised by `i`.
* Every view asserts `os.getuid() == 20000 + i` and returns HTTP 500 otherwise,
  so a routing or credentials mistake cannot pass silently.
* The Rust↔Python boundary is deliberately tiny: Rust passes
  `(method, path, headers, body)` to one Python function that builds `environ`,
  supplies `start_response`, calls Django, and returns raw response bytes.

### Results

Python 3.11.2, Django 5.2.18, numpy 2.4.6, pandas 3.1.0, scipy 1.17.1,
scikit-learn 1.9.1, Rust 1.99.1.

| | Thread-pinned BLAS | Multi-threaded BLAS |
|---|---:|---:|
| Views passing | **100 / 100** | 90 / 100 |
| Distinct worker uids | 100 | 100 |
| Test wall clock | 2.80 s (~27 ms/view) | 107 s (10 views × 10 s timeout) |
| Failing views | — | `7, 17, …, 97` (**`i % 10 == 7`**, KMeans) |
| Preload | 0.85 s | 0.85 s |
| Parent RSS after preload | 173 MiB | 173 MiB |
| `sum(worker VmRSS)` | 12.7 GiB | — |
| `sum(worker Pss)` | **575 MiB** | — |
| Overshoot | 22.1× | — |

### Findings

1. **Preload-then-fork pays off enormously.** The loaded interpreter plus the
   whole scientific stack is ~173 MiB, and 100 workers cost 575 MiB in total
   (`sum(Pss)`) while naive accounting reports 12.7 GiB. Same lesson as
   Experiment 1, now with a real Python payload.
2. **It holds together — if the numerical backends are single-threaded.** With
   `OPENBLAS_NUM_THREADS=1` (and OMP/MKL/NumExpr equivalents) all 100 views pass
   in 2.8 s.
3. **Otherwise it blows up, very specifically.** With multi-threaded BLAS exactly
   10 of 100 views hang forever: the KMeans ones. OpenBLAS creates a thread pool
   during the parent's warmup; the forked child does not inherit those threads
   and spins waiting for them. The other BLAS-heavy families survived, which is
   why the failure looks arbitrary until it is mapped back to `i % 10`.
   This is *not* specific to KMeans — it is any operation whose BLAS call is big
   enough to trigger a thread pool.
4. **The server survives a deadlocked worker.** A receive timeout on every worker
   channel plus a per-request client timeout turns a stuck child into a `502` for
   that one view, so the run still finishes with a clean report instead of
   hanging.

---

## 4. Experiment 3 — the same PoC under gVisor

`runsc release-20261019.0`, standalone (no Docker), from an OCI bundle built out
of the same Docker image. Standalone was chosen deliberately so the host's Docker
daemon — which had 16 live containers — did not have to be reconfigured.

| | QEMU VM | gVisor |
|---|---:|---:|
| Views passing | 100 / 100 | **100 / 100** |
| Distinct worker uids | 100 | 100 |
| Preload | 0.85 s | 1.11 s |
| 100-view test | 2.80 s (~27 ms/view) | 5.09 s (~50 ms/view) |
| Parent RSS after preload | 173 MiB | 267 MiB |
| `sum(worker Pss)` | 575 MiB | **0 MiB (unsupported)** |
| `/proc/meminfo` | guest's | **the host's** |
| `uname -r` inside | 7.0.0-38-generic | `4.19.0-gvisor` |

Inside the sentry, embedded CPython, Django, the full scientific stack, 100
forks, 100 `setuid()`s to nonexistent uids, and WSGI dispatch all work.

### Findings

1. **The default OCI spec grants almost no capabilities.** `runsc spec` emits
   only `CAP_AUDIT_WRITE`, `CAP_KILL`, `CAP_NET_BIND_SERVICE`. With that set every
   worker's `setuid(20000+i)` fails with `EPERM` and the views (correctly) report
   the wrong uid. A full capability set including `CAP_SETUID`/`CAP_SETGID` is
   required.

2. **A bare `--network=sandbox` sandbox has no network interfaces at all — not
   even loopback.** Verified independently with a 10-line C `bind()` probe
   compiled inside the sandbox:

   ```
   sandbox:  bind(127.0.0.1:9999) = -1 errno=99 (Cannot assign requested address)
             bind(0.0.0.0:9998)   =  0 ok
             interfaces: 0
   host net: bind(127.0.0.1:9999) =  0 ok
             interfaces: 31
   ```

   So `127.0.0.1` is unusable in the default mode and the sandbox must run with
   `--network=host` (dropping the netns from the spec). The sentry still mediates
   every syscall, the filesystem and process isolation — only the network
   namespace is shared. Under Docker + `--runtime=runsc` this is a non-issue,
   because dockerd creates the netns with loopback already up.

3. **gVisor does not implement `/proc/<pid>/smaps_rollup`.** `sum(Pss)` silently
   becomes `0`, so the copy-on-write bookkeeping is simply unavailable. `VmRSS`
   still works but is even more inflated than on a real kernel (16.3 GiB across
   the workers). Good software degrades gracefully here rather than erroring.

4. **`/proc/meminfo` reports the host's memory**, so `MemAvailable` (122 GiB) is
   meaningless for measuring the sandbox and the "physical used" delta is a no-op.

5. **The fork hazard is a property of the payload, not the sandbox.** With
   multi-threaded BLAS the *same* views deadlock inside gVisor (18 ok / 2 failed
   in a 20-view run; views 7 and 17). gVisor neither causes nor cures it.

6. **Cost.** Syscall interception roughly doubles per-request latency
   (~27 ms → ~50 ms) and slows preload by ~1.3×.

7. **gVisor's release layout changed in 2026-07.** Releases are now a
   `gvisor.tar.zstd` containing `runsc`, `containerd-shim-runsc-v1` and a
   `gvisor-bin/` sidecar directory; the old bare `.../latest/x86_64/runsc` 404s.

---

## 5. Cross-cutting findings

These are the reusable conclusions, independent of which experiment produced them.

1. **`sum(VmRSS)` across processes is a lie; `sum(Pss)` is the truth.** `VmRSS`
   counts a shared page once per mapping, so 100 processes sharing 1 GiB report
   100 GiB. `Pss` divides a page by its mapcount. Cross-check with the
   `/proc/meminfo` `MemAvailable` delta. Overshoot factors observed: 100.7×,
   22.1×, 12.1×.
2. **COW is preserved by `fork` and by `setuid`; only writes split pages.** A
   write moves a page from `Shared_Dirty` to `Private_Dirty` one for one.
3. **`fork` has a fixed cost even when sharing everything** — page tables, roughly
   2 MiB per GiB of shared address space per process.
4. **Any arbitrary uid is available to root via `setuid()` alone**, `/etc/passwd`
   notwithstanding.
5. **Preloading a heavy runtime before forking is the single biggest memory win**,
   and it is what makes "one process per user" affordable.
6. **Threads and fork do not mix.** Thread pools (OpenBLAS, OMP, MKL) created
   before a fork are not present in the child; the child then deadlocks. Pin them
   to one thread (`OPENBLAS_NUM_THREADS`, `OMP_NUM_THREADS`, `MKL_NUM_THREADS`,
   `NUMEXPR_NUM_THREADS`, `VECLIB_MAXIMUM_THREADS`) whenever you preload-then-fork.
7. **Make the workload self-verifying.** Checksums over every touched page (Exp 1)
   and `os.getuid()` assertions per view (Exp 2) turned "looks like it works" into
   "provably worked" — and made the one real failure (KMeans) impossible to miss.
8. **Design for reduced sandboxes.** Restrictive sandboxes take away
   capabilities, `/proc` files and networking. Read the failure instead of
   assuming the application is wrong: `EPERM` on `setuid` is a capability
   problem, `EADDRNOTAVAIL` on a loopback bind is a missing interface.

---

## 6. Engineering recipes and non-obvious gotchas

A checklist distilled from everything that actually went wrong.

**Booting a VM with no initramfs.** If the kernel has `VIRTIO_BLK`, `EXT4_FS` and
`DEVTMPFS` built in (`=y`, not `=m`), you can boot a disk rootfs directly:

```
qemu-system-x86_64 \
  -kernel /boot/vmlinuz-$(uname -r) \
  -drive file=disk.img,if=virtio,format=raw \
  -append "root=/dev/vda rootfstype=ext4 rw console=ttyS0 init=/opt/app panic=1 loglevel=4" \
  -m 6144 -smp 4 -display none -serial stdio -monitor none -no-reboot -enable-kvm -cpu host
```

`panic=1` plus `-no-reboot` is the safety net: if PID 1 exits, the kernel panics
and QEMU exits rather than spinning.

**Ubuntu ships `/boot/vmlinuz-*` as mode 600 root:root**, so `-kernel` fails as
a normal user. Copy it somewhere readable first (`sudo cp`, `sudo chmod 0644`).

**Building a bootable ext4 image without mounting it:**

```bash
truncate -s 3G disk.img
sudo mke2fs -t ext4 -d rootfs_dir -F -q disk.img      # -d populates from a directory
```

**Producing the rootfs from a container image:** `docker create` the image, then
`docker export | sudo tar -xf - -C rootfs`. `docker export` drops `/proc`, `/sys`
and device nodes, so the guest must mount `devtmpfs` (which the kernel also does
automatically when `DEVTMPFS_MOUNT=y`).

**Embedded CPython does not see the distro's `site-packages`.** There is no
`python3` executable to derive a prefix from, so append them explicitly before
importing anything:

```python
import site, os, sys
for p in list(site.getsitepackages()) + [
    f"/usr/local/lib/python{maj}.{min}/dist-packages",
    f"/usr/lib/python{maj}.{min}/dist-packages",
    "/usr/lib/python3/dist-packages",
]:
    if os.path.isdir(p) and p not in sys.path:
        sys.path.append(p)
```

**Fork + GIL ordering (pyo3 / CPython 3.11+).** Load everything inside a
`Python::attach` that then returns, so the fork happens with the GIL released and
the process single-threaded. Each child re-attaches and calls
`PyOS_AfterFork_Child()` **while holding the GIL** — CPython asserts this and
aborts the process otherwise.

**Reading another uid's `/proc/<pid>/smaps_rollup` needs `CAP_SYS_PTRACE`**
(absent from Docker's default capability set). Have each process read its *own*
`/proc/self/smaps_rollup` and report it instead — that always works.

**Environment for a process that will `setuid` to arbitrary uids:**
`HOME=/tmp`, `TMPDIR=/tmp`, `LC_ALL=C`, `PYTHONDONTWRITEBYTECODE=1`, and a
world-writable `/tmp` (1777) plus `/dev/shm` (joblib and friends want it).

**Bringing up loopback with no init system:**

```c
struct ifreq ifr = {0};
strcpy(ifr.ifr_name, "lo");
ioctl(fd, SIOCGIFFLAGS, &ifr);
ifr.ifr_flags |= IFF_UP | IFF_RUNNING;
ioctl(fd, SIOCSIFFLAGS, &ifr);
```

Needed in the VM (there is no init to do it). Not needed under Docker, and it does
not help under gVisor's empty default netns.

**`mke2fs -d` is the only step that needs root; everything else can run
unprivileged.** Likewise `nmcli`-free VM networking is unnecessary if all traffic
is loopback.

**Reproducible Rust→Python linkage:** build the Rust binary *inside* the same
container that becomes the guest rootfs, so it links against that exact
`libpython`. Cross-building against a different distro's `libpython` is not worth
the trouble.

---

## 7. Isolation options compared

| | QEMU micro-VM (initramfs) | QEMU micro-VM (disk rootfs) | gVisor (runsc) |
|---|---|---|---|
| Kernel | host kernel, shared | host kernel, shared | sentry (reports 4.19.0-gvisor) |
| Setup cost | low (one static binary) | medium (Docker + export + mke2fs) | low–medium (OCI bundle) |
| Rootfs | single binary in RAM | full distro on disk | full distro, mediated |
| `/proc` fidelity | full | full | partial (no `smaps_rollup`, host `meminfo`) |
| Networking | needs `lo` up | needs `lo` up | sandbox netns empty → use `--network=host` |
| Capabilities | full root | full root | must be requested explicitly |
| Isolation strength | strong | strong | strong (user-space kernel) |
| Measured overhead | baseline | baseline | ~2× per syscall-heavy request |

Rule of thumb: use the VM when you need kernel fidelity (real `/proc`, real
`meminfo`, real PSS) and gVisor when you want per-sandbox isolation with a
fraction of a VM's footprint — accepting that some `/proc` and networking
surfaces are simply absent.

---

## 8. Measurement methodology

* **Attribute, don't total.** `VmRSS` per process, then divide shared pages by
  mapcount (`Pss`) to get the real physical cost. Always report both and the
  ratio.
* **Cross-check with the kernel's own view.** `/proc/meminfo` `MemAvailable`
  delta and `AnonPages` should agree with `sum(Pss)`; when they disagree you have
  a measurement bug.
* **Make the payload unavoidable.** Touch every page (Exp 1) and warm up every
  code path (Exp 2) *before* measuring, so nothing is lazily faulted later.
* **Bound the blast radius.** A VM sized just below the failure threshold turns
  "what if COW breaks?" into a contained OOM instead of a host incident.
* **Bound the run in time.** Per-request timeouts (client and worker channel) plus
  a hard harness timeout turn a deadlock into a reported failure.
* **Watch for capability-gated measurements.** Cross-uid `/proc` reads need
  `CAP_SYS_PTRACE`; sandboxes may not grant it. Self-report instead.

---

## 9. Reproducing from scratch

**Experiment 1 (COW).** Rust binary, `-C target-feature=+crt-static`, run as
`/init` in an initramfs (`find . | cpio -o -H newc | gzip`) booted with
`-kernel`/`-initrd`/`rdinit=/init`. The program: `mmap` 1 GiB private anonymous,
write a page index into every page, `fork` 100 times, `setgid`/`setuid` to
`20000+i` in each child, checksum every page, report `RSS`/`Pss`/`AnonPages`.

**Experiment 2 (pywsgi).** `Dockerfile` from `debian:bookworm-slim`:

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-dev python3-pip build-essential pkg-config curl ca-certificates
RUN pip3 install --break-system-packages --no-cache-dir \
      django numpy pandas scipy scikit-learn
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
COPY Cargo.toml src ./    && cargo build --release
COPY app /opt/blastit/app
```

Then `docker export` → `mke2fs -d` → boot (section 6). The Rust side uses
`pyo3 = { version = "0.30", features = ["auto-initialize"] }`; the Python side is
one `wsgi_bridge.handle(method, path, headers, body)` that wraps Django's
`WSGIHandler`.

**Experiment 3 (gVisor).**

```bash
curl -fsSL https://gvisor.dev/archive.key \
  | sudo gpg --dearmor -o /usr/share/keyrings/gvisor-archive-keyring.gpg
curl -fsSL -o /tmp/gvisor.tar.zstd \
  "https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/gvisor.tar.zstd"
sudo tar --zstd -xf /tmp/gvisor.tar.zstd -C /usr/local/bin
```

Then build an OCI bundle from the same image (`docker export | sudo tar -xf - -C
bundle/rootfs`), `runsc spec`, and patch the spec before running:

```bash
jq --argjson caps '["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FOWNER","CAP_FSETID",
  "CAP_KILL","CAP_SETGID","CAP_SETUID","CAP_SETPCAP","CAP_NET_BIND_SERVICE",
  "CAP_NET_RAW","CAP_SYS_CHROOT","CAP_MKNOD","CAP_AUDIT_WRITE","CAP_SETFCAP",
  "CAP_SYS_PTRACE"]' \
   --argjson args '["/opt/blastit/pywsgi"]' '
  .process.capabilities = {bounding:$caps, effective:$caps, inheritable:$caps, permitted:$caps}
  | .process.args = $args
  | .process.cwd = "/opt/blastit"
  | .process.terminal = false
  | .root.readonly = false
  | .linux.namespaces = [.linux.namespaces[] | select(.type != "network")]
' config.json > config.new && mv config.new config.json

sudo runsc --root /tmp/runsc-state --network=host run --bundle bundle pywsgi
```

---

## 10. What was removed

This document is the only surviving artifact of the three experiments. Removed:
both Rust projects (COW PoC and pywsgi), the Django app, all Dockerfiles and
scripts, the QEMU disk images and the copied kernel, the console logs, the OCI
bundle and runsc state, the `pywsgi-dev` / `pywsgi-poc` Docker images, the cargo
target volume, and the handful of scratch files these experiments created under
`/tmp/opencode`.

Kept intentionally: the gVisor install (`/usr/local/bin/runsc` and its
`gvisor-bin/` sidecars) and the base `debian:bookworm-slim` image. `/tmp/opencode`
itself was **not** touched as a whole — it is a shared scratch directory holding
unrelated, concurrent work, so only the files belonging to these experiments were
removed. The host's Docker daemon configuration was never modified.
