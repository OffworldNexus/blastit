# ADR-0016: View Crates Over the Wire: `blastit-view` as Transport and One Crate per View Kind

- **Status:** Proposed
- **Date:** 2026-11-17
- **Node:** MB.36.1. Supersedes, in part only: (a) [ADR-0009](ADR-0009-runtime-providers-and-capability-gating.md)
  Decision 1's `WorkerRuntime`/`Upstream` **trait** requirement, Decision 2's dead
  `Upstream::capacity()` parenthetical, and Decision 1's "each child into `setpgid(0,0)`"
  sentence, which the tree implements only for the PHP probe (the worker forks are killed by
  pid); (b) Decision 5's distribution *command spelling* and wheel feature list, and the same
  command in ADR-0010 §4. Every other decision of ADR-0009
  (manifest minimalism, discovery and pinning, sandbox bijection, CPU fair queuing), all of
  [ADR-0010](ADR-0010-php-embedded-via-libphp-embed.md) except that command, and all of
  [ADR-0014](ADR-0014-dev-fanotify-live-reload.md) stand, except that the dev report socket's
  *carrier* moves from `ViewConfig` to the spawn struct (Consequences).
- **Owner:** designer
- **Analysis:** `docs/design/view-crates-architecture.md` (module map, feature matrix, seam
  decisions, the relocation table and four fresh-context reviews)

## Context

`0.2.0` grew three unrelated things inside `crates/blastit-view`: the `view.chan` transport
(`wire`, `flow`, `loaded`), the whole CPython worker (`serve`, `environ`, `input`, `request`,
`response`), and the whole embedded-PHP worker (`php.rs`, 2 558 lines). It also kept a
`WorkerRuntime` trait (`runtime.rs`, 1 088 lines) whose only two implementations were the two
halves of that same crate, and it defaulted `python = true`, a trap beside the real leak path
(`blastit-py`'s `blastit = { features = ["python"] }` under a `--workspace` build). The forks
lived in the composition root (`zygote.rs` 281 lines, `php_view.rs` 1 726 lines) and PHP
discovery lived in a 6 320-line `stage0.rs`. The interface between a view and the rest of
Blastit is not a trait: it is the `view.chan` frame vocabulary plus the process that speaks it.
A trait invented beside a wire that already exists is a crate boundary that was never drawn.

## Decision

1. **One crate per view kind, composed by Cargo features.** `blastit-view` keeps only the
   transport (`wire`, `flow`, `stream`, `loaded`, `error`, `config`, `sync`), the single
   request-target path decoder (`path`) and the shared spawn context (`spawn`); it has zero
   `pyo3` (including `[dev-dependencies]`) and zero `blastit-php-sapi`, and no `runtime.rs`.
   `ViewConfig` is trimmed to `route_name` + `frame_limits`; the WSGI-only `script_name` and
   `max_threads` move to `blastit-view-python`'s `WsgiConfig`. `blastit-view-python` owns the
   WSGI worker, its pool, `loaded`, `immortal` and the zygote; `blastit-view-php` owns the PHP
   worker, `discovery`, `pinning` and `probe`; `blastit-view-static` owns static resolution and
   response as a **library** the scheduler calls, with no process, no `ctl.chan` slot and no
   seccomp role. `blastit` is the composition root.
2. **The worker trait is deleted, not re-homed.** `WorkerRuntime`, `RequestIo`, `RequestEnd` and
   `flush_response` stop existing; `OutputStream`/`OUTPUT_LINE_CAP` move to
   `blastit-view::stream`, and `serve_forever` takes the concrete `PhpEmbedRuntime`. The
   transport's `Flow`, `Credit` and `WINDOW_BATCH` become `pub` (the workers live in other
   crates); `Credit::grant` is documented as applying a `WINDOW` frame the scheduler sent, and
   the scheduler's own window accounting is what bounds a view. The
   `entrypoint php` pin becomes an **opaque `EntryPin`** (no `Option`, no `Default`, private
   fields) that `PhpRuntimeConfig::new` requires, verified at load and before every request.
   This deliberately overrules `php_view.rs:503-510`'s "not in `PhpRuntimeConfig`" note: the
   wrapper that note justified (`PinnedEntryRuntime`) cannot survive the trait's deletion, and a
   required value is the fail-closed replacement for a wrapper.
3. **The view child sequence becomes one function with three callers.**
   `blastit_sys::child::enter` takes a `ChildPlan` (`Privileges::{DropTo,Shared}`,
   `Filter::{Seccomp,Off}`, pgroup, mask, `ChildIo`, name) and runs `ChildIo::prepare →
   setpgid(0,0) when asked → drop_privileges → harden_child → install_filter →
   set_dumpable(false) → saved signal mask (when present)` in that order and returns before any
   load. The "production always drops and filters" rule cannot live in the child — after the
   `execve` barrier there is no mode and no `BLASTIT_*` to read — so stage 1 builds `DropTo` +
   `Seccomp` for a production boot, `Shared` + `Seccomp` for `dev`/`check` (they are filtered
   today), and `Off` only for an explicit `BLASTIT_SECCOMP=0`; a census
   asserts no view crate names the latter two. The crate then sets `HOME`/`TMPDIR`, loads, serves, and
   `READY` stays `serve`'s first write. Only the PHP probe passes `pgroup: true`; the two worker
   forks are not group leaders and the supervisor keeps killing them by pid. The forks move to the view
   crates and take the fork syscall by injection (`Forker` + a `ForkProtocol` enum), so CPython's
   three-hook protocol keeps one implementation in `blastit/src/forking.rs`, which refuses a
   protocol that disagrees with the image instead of forking bare.
4. **The spawn seam is a struct, not the root's configuration.** View crates accept
   `blastit_view::spawn::SpawnContext` (route, frame limits, `Rlimits`,
   `Privileges`/`Filter`, `egress`, saved signal mask) plus only their own language's data; each crate owns
   its working directory (PHP's is `PhpBinding.view_tmp`, one value, never two). They depend on
   neither `blastit` nor `blastit-scheduler`: `RouteInfo` is reduced to `name` + `uid` at the fork
   site, `config::Immortalize` becomes `immortal::Setting`, and discovery/boot take values —
   `PhpSearch::new`, `PhpSlot` (route, dir, egress, script name, its own `Privileges`/`Filter`),
   `PhpProbeBoot` (slots, bindings, `Rlimits`, the `libphp` record, timeout) and two per-path
   provenance callbacks (objects through `host::Provenance::check_objects()`, trees through the
   ownership claim) — instead of `Config`. The probe spawn takes the same injected forker as the
   two worker forks.
5. **Discovery is a module of the view crate, and the limitation is recorded.** `discovery` reads
   ELF `.dynsym` tables, never `dlopen`s, and names no SAPI type; `pinning`/`probe`/`worker`/
   `spawn` are the rest. `blastit` links the crate for stage 0 and stage 1, so the loader's object
   code is in init's image — it already is today through `blastit-php-sapi` — and the enforceable
   invariant is "nothing is loaded before the fork", proved by the canary.
6. **Feature wiring and distribution.** `blastit` is `default = ["php", "static"]` (set in the
   leaf that introduces the feature, `MB.36.2`); `blastit-view`'s `default` is `[]`;
   `blastit-py` enables `python`, so the wheel is `python, php, static`; `blastit-scheduler`'s
   `static` is a **default** feature over an optional `blastit-view-static`, and `blastit` depends
   on the scheduler with `default-features = false` and forwards `static`, so the package's own
   tests run the static suites while a `--no-default-features` build refuses a manifest with
   mounts instead of failing to link. The standalone binary (`cargo build -p blastit`, never `--workspace`) keeps no
   `DT_NEEDED` on `libpython` and none on `libphp`; the wheel keeps the python capability and
   still never `libphp`; `cargo build -p blastit --no-default-features` compiles. This amends the
   **command spelling** in ADR-0009 Decision 5 and ADR-0010 §4 from
   `--no-default-features --features php` to `--features php,static`, and their wheel feature list
   from `["python", "php"]` to `["python", "php", "static"]`; with `static` now a feature, the old
   spelling ships an archive that cannot serve a `static` mount. `scripts/build-standalone.sh`
   has five `--no-default-features --features php` sites (the echo, two build lines, the `cargo
   tree` proof and the header comment); all five are updated.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| Keep `WorkerRuntime`/`Upstream` and move code behind it | A trait with only the two implementations you also own hides the concrete capabilities (`max_inflight`, `recycle`) and gives a new backend a contract the wire never asked for; the frames already are the contract. |
| One `blastit-view` with per-language features (status quo) | It is how `php.rs` ended up in the Python crate, `runtime.rs` in the middle, and `libpython` one `--workspace` away from the standalone artifact. |
| Leave both forks in `blastit` | Keeps 611 lines of per-language process code in the root, so `blastit-view-node` would edit the root again — the failure this decision removes. |
| A non-language `blastit-view::child`, or a ninth `blastit-child` crate | `child_exit` needs `libc::_exit` (the transport crate is `#![forbid(unsafe_code)]`), and the primitives it calls already live in `blastit-sys`. |
| A typed "GIL is held" token instead of a protocol enum + mismatch refusal | The token cannot be constructed outside a crate that owns `pyo3`, and putting `pyo3` in `blastit-view` is what the split exists to prevent; the single fork implementation refuses a mismatched protocol and a test pins it. |
| Two crates for PHP (discovery vs worker) | One `-p blastit --features php` build needs both halves, so Cargo cannot keep the loader out of init's image; the invariant that matters is proved by the canary, not by a crate edge. |
| A static worker process | A fork, a uid, a filter and 12–25 MiB per route to replace one `openat2` and a `pread` in the scheduler, and it would need a seccomp role that opens files. |
| A runtime `--backend` flag | A second capability gate with a worse failure mode; Cargo features fail at `execve` and change no user-facing surface. |

## Consequences

- **Positive:** a view kind is a crate (Node in `0.2.2`, JVM in `0.2.3`) with no sibling edit and
  no root edit; the transport crate links no interpreter, so `-p blastit` cannot grow `libpython`
  by inheritance; the PHP worker is no longer compiled into a Python crate; stage 0 shrinks by
  ≈1 800 lines of discovery code (its 1 700-line `php_boot_gate` module and siblings stay, because it drives
  `crate::host`), taking the file to ≈4 520 lines.
- **Trade-offs:** three new workspace members and their `Cargo.lock` churn (serialized in one leaf
  at a time); `MB.36.2`, `MB.36.4` and `MB.36.5` each need a few `paths` globs added before they
  are dispatched (they cannot create or delete files they do not own); and `MB.36.5` is a
  ~5 000-line leaf (one crate boundary) that may need splitting into a/b.
- **Unsafe, stated.** `blastit-view`, `blastit-view-static` and `blastit-view-php` are
  `#![forbid(unsafe_code)]` (the PHP `unsafe` calls become `rustix`/`std` call forms and a
  `blastit_sys::set_env` helper, which needs no `libc` in the view crate); `blastit-view-python` is
  `#![deny(unsafe_code)]` with **one** exception, `immortal`'s raw `PyObject` refcount access.
- **One carrier moves, the mechanism does not.** ADR-0014 Decision 2 says the dev report socket
  is "carried in the worker's `ViewConfig`"; after this split it is a `WsgiSpawn`/`PhpSpawn`
  field added to `ChildIo.keep`, because `ViewConfig` is trimmed to transport data. The
  mechanism ADR-0014 decided — an inherited descriptor, never an environment variable — is
  unchanged, and this ADR is the record of the carrier change (no leaf rewrites ADR-0014).
- **What does not change:** the stage-0 `execve` secret barrier and the absence of any new
  `BLASTIT_*`; `SeccompRole::{View, EmbeddedView}` and the child order (now with the probe's
  `setpgid` inside it); `BLASTIT_LIBPHP`, the `(st_dev, st_ino)` library/shim pins and the
  `(st_dev, st_ino, st_size, st_ctime)` entry pin; `openat2`-only static resolution and the rule
  that `.php`/`.phtml`/`.phar` are never served; the `view.chan` credit window; the dev
  loaded-file report over an inherited fd; and every CLI command, `Blastfile` key and status code.
