# Consistency review — `docs/design/backend-runtimes.md` (MB.12), round 2

- **Node:** MB.12
- **Lens:** `consistency`
- **Targets:** `docs/design/backend-runtimes.md`, `ADR-0009`, `ADR-0010`, `ADR-0008`, `docs/decisions/README.md`, `docs/design/backend-runtimes.resolve-owner.md`
- **Verdict:** **PASS** — 0 `BLOCKER`, 0 `MAJOR`, 7 `MINOR`

Round 1's `B1`, `M1`–`M7` and `m1`–`m9` are all resolved on the text. The residual items below are naming/status polish; none blocks `lead` acceptance or an implementer.

## Round-1 findings — verified resolved

| Round-1 | Status | Evidence in the current text |
|---|---|---|
| **B1** (`MB.3.21`/`.22` collision) | **Resolved** | §4.1: "proposed sibling leaves **`MB.3.23`** … and **`MB.3.24`** … verified free via `plan.py show` so closed `MB.3.11` and existing `MB.3.20`–`MB.3.22` stay untouched"; §8.1 note states the "no node id … unless `plan.py show` resolves it" convention; `MB.3.25`/`MB.3.26` allocated in §5.1, §8.2 row 9, `ADR-0009` Consequences, `ADR-0010` §5; `MB.3.5.4` marked `done` in §8.1/§8.2/`ADR-0010` §5. |
| **M1** (`<slot>`) | **Resolved** | P1 defines `<slot>` = "`<uid>` … in `Production` (`/run/blastit/views/<uid>`, `config::view_dir` …) and `<route_index>` in `Dev`/`Check` (`<root>/views/<index>`, `dev_learn.rs:126`)"; `ADR-0009` §3 repeats it; R1/R16/§5.2/§4.6 all use `<slot>`. |
| **M2** (`Option<SpawnSpec>`, `Node`/`Jvm`) | **Resolved** | §4.3: "`Option<SpawnSpec>` per route: `Some(SpawnSpec)` for `ProviderKind::Embedded` and `ProviderKind::Managed`; `None` for `ProviderKind::None`"; `WorkerBinding` has `Wsgi`, `Php`, `Node`, `Jvm`; `ADR-0009` Decision 1 says `WorkerBinding::{Node, Jvm}`. |
| **M3** (`PYTHONHOME` mode) | **Resolved** | `ADR-0009` §2: "`PYTHONHOME` (on `Install::Wheel`)"; R7 "interpreter prefixes (`Install::Wheel`)"; §5.2 "`PYTHONHOME` (`Install::Wheel`)". |
| **M4** (`BLASTIT_ALLOW_EGRESS` owner) | **Resolved** | R16: "implemented in `config.rs` by proposed `MB.3.26` and documented in `docs/reference/settings.md` by `MB.3.10`"; §5.2 "added in `MB.3.26` & `MB.3.10`"; `ADR-0009` §3; §7.1 (R3/R16 row) and §9.2 item 2. |
| **M5** (`workers = 1..=64`) | **Resolved** | P4 "grammar bound `1..=64` at parse time"; R3 "`1..=64` manifest parse bound in `crates/blastit-manifest/src/parse.rs`"; `ADR-0009` §2 "(`workers` parse bound `1..=64`)"; `ADR-0010` §1.2. |
| **M6** (`docs_fences.rs` on `MB.3.20`) | **Resolved** | §8.2 (b) "append `crates/blastit-manifest/tests/docs_fences.rs` to `MB.3.20`'s `paths` (`M6-C1`)"; §7.1 "`MB.3.20` (after `lead` adds `docs_fences.rs` to its `paths`)"; `ADR-0010` §5. |
| **M7** (lens split) | **Resolved** | Frontmatter comment and §9 both cite `MB.12/NODE.md` Acceptance #5 and name `MB.3.10`/`MB.3.20`/`MB.3.18` for `dx` + `ai-ergonomics`. |
| **m1** (`ADR-0008` scoping) | **Resolved** (one gap → m14) | `ADR-0008` status line: "Decision 1 … remains active as amended by ADR-0009 (ignoring historical mentions of `blastit-fastcgi` / `FastCgiPolicy` / `PoolSpawn` / `Ctl::Pool*`…)"; `docs/decisions/README.md` row updated. |
| **m2** (`OutputStream`) | **Resolved** | §4.1: "`OutputStream` in `blastit-view/src/runtime.rs`" + `pub enum OutputStream { Stdout, Stderr }`. |
| **m3** (`sql_obs.jsonl`) | **Resolved** | R22 "serialized in `sql_obs.jsonl` / `WireSqlObs`"; §7.1 (R20–R22 row). |
| **m4** (`docs/security.md` anchors) | **Resolved** except one residual → m10 | §3.1 note uses `#what-the-sandboxed-kernel-does-not-do`; §5.1/§5.2 use `#what-a-compromise-gets`. |
| **m5** (`seccomp.rs:895`) | **Resolved** | R14 and §7.1 (R14 row) both say `seccomp.rs:895` / "`widest == 54` at `seccomp.rs:895`". |
| **m6** (`env` unknown key) | **Resolved** | R3, `ADR-0009` §2, `ADR-0010` §1.2 all list `env` among rejected keys. |
| **m7** (`MB.3.8` sync scope) | **Resolved** | §8.1: "Owned by `MB.3.20` … + `MB.3.10` … + `MB.3.8` (`VISION.md`, `ARCHITECTURE.md`, `docs/security.md` sync)". |
| **m8** (`MB.3.5.4` done, `provider_ceiling`) | **Resolved** | §8.1/§8.2/`ADR-0010` §5 say `MB.3.5.4` (`done`); P4, R4 and `ADR-0009` §2 all use `provider_ceiling >= route_allocation`. |
| **m9** (`"runtimes"` in R20) | **Resolved** | R20: "Structured `boot` events emit a `"runtimes"` array …"; §7.1 (R20–R22 row) matches. |

## MINOR (Round 2)

- **m10 — one numbered `docs/security.md` anchor survives.** §3.3 "All Backends" row: "Cannot hide non-proxied `.env` files … (`docs/security.md` §1)". Every other reference now uses the unnumbered anchors (`#what-a-compromise-gets`, `#what-the-sandboxed-kernel-does-not-do`). Replace `§1` with the anchor used in §5.1/§5.2 (or add the `.env` subsection anchor).
- **m11 — `MB.3.24` dependency edge stated two ways.** §4.1: "`MB.3.24` … `depends_on: [MB.3.23]`"; §8.2 (a): "create proposed sibling leaves **`MB.3.23`** & **`MB.3.24`** (`depends_on: [MB.3.11]` …)". `lead` creating both from §8.2 alone would drop the `MB.3.24 → MB.3.23` serialization edge and let `WsgiRuntime` be written before the trait exists. Make §8.2 read "`MB.3.23` `depends_on: [MB.3.11]`; `MB.3.24` `depends_on: [MB.3.23]`".
- **m12 — `BLASTIT_MAX_VIEW_PROCESSES` is a dangling env name.** §4.2: "`ViewUpstream::ensure_ready` asks `init` (`Ctl::Spawn`) to fork another PHP worker … (respecting `BLASTIT_MAX_VIEW_PROCESSES` and `BLASTIT_MEM_HIGH_WATERMARK`)". P4, R4, `ADR-0009` §2 and `ADR-0008` §Amends use only `BLASTIT_MAX_WORKERS` (global) and `BLASTIT_VIEW_MAX_WORKERS` (per-route cap, `view_worker_cap()`). Either rename it to `BLASTIT_VIEW_MAX_WORKERS`/`view_worker_cap()` or state that it is a distinct new knob with an owner leaf.
- **m13 — `ceiling` names two different quantities.** §4.3 `SpawnSpec { ceiling: u32, // >= allocation }` is the provider concurrency capacity (P4's `provider_ceiling`), while R19/§4.5/`ADR-0009` §4 use `ceiling = observed_wall_ns.saturating_mul(max_threads)` (CPU nanoseconds). R20's "derived slot ceilings" inherits the ambiguity. Rename the `SpawnSpec` field to `capacity` (matching `Upstream::capacity().max_concurrent`) and leave `ceiling` to CPU accounting, or qualify both (`provider_ceiling`, `cpu_ceiling_ns`).
- **m14 — `ADR-0008` Decision 1's "`SessionPort` is WSGI-only" is not covered by the scoping note.** Decision 1 still states "`SessionPort` is WSGI-only", while §4.2 uses "`SessionPort::open(route)` selects an idle worker in `ViewSet`" for both WSGI and PHP. The status-line note enumerates only `blastit-fastcgi` / `FastCgiPolicy` / `PoolSpawn` / `Ctl::Pool*`. Add `SessionPort` (and its WSGI-only sentence) to the ignored-historical-mentions list, or amend it to "`SessionPort` serves every `ViewSet` backend".
- **m15 — `MB.3.20` status differs between targets.** `ADR-0010` §5 row: "`MB.3.10`, `MB.3.20` (`proposed`)"; design §8.2 row 2: "`MB.3.20`, `MB.3.21`, `MB.3.22`, `MB.3.10` | `proposed`/`ready`". Pick the on-disk value once and use it in both.
- **m16 — `ADR-0007` status vs §8.1's "shipped".** `docs/decisions/README.md` lists ADR-0007 as `Proposed` (node `MB.2.2`), while §8.1 says "**MB.2 (`0.2.0` shipped)** … **Engine holds**" and only the `ProviderKind::None` port is pending (`MB.9.2`). Either mark ADR-0007 `Accepted` (its static-serving decision shipped) or state in §8.1 that the ADR stays `Proposed` only because the `StaticUpstream` port has not landed.

## Checked and *not* a defect (pre-empting the obvious false positive)

- **`blastit-scheduler` does not list `blastit-view` in its crate edges** (§4.2) even though `ViewSet`/`ViewUpstream` are defined there and `WorkerRuntime` lives in `blastit-view/src/runtime.rs`. This is coherent, not missing: R9/R10 and §4.2 make `view.chan` `RawFrame` the transport, so the scheduler holds framed sessions and per-worker `max_inflight` state, never a `WorkerRuntime` trait object. No edge is required.
- **`<route_index>` vs `<index>`** in P1's definition is intentional (`<slot>` = uid in prod, route index in dev/check) and matches R22's `<CheckRoot>/dev/views/<index>`; no action.
- **`ADR-0008` §Amends / Decision 6 pool prose** is explicitly bracketed as pre-amendment and superseded by `ADR-0010`; the status line and §8.1 row both say so.
