# `backend-runtimes` — Goldfish Review (Round 2, lens `implementability`)

- **Node:** MB.12
- **Lens:** `implementability` (cheap-model buildability: crate seams, trait signatures, leaf sizing, licenses)
- **Targets:** `docs/design/backend-runtimes.md`, `ADR-0009`, `ADR-0010`, `backend-runtimes.resolve-owner.md`
- **Read in repo:** `crates/blastit-php-sapi/{c/include/blastit_php_abi_v1.h,c/blastit_php_sapi.c,src/{abi,toolchain,request}.rs,build.rs}`, `crates/blastit-view/{Cargo.toml,src/lib.rs}`, `crates/blastit/{Cargo.toml,src/{supervise,visibility,config}.rs}`, `crates/blastit-sys/src/{seccomp,signal,proc}.rs`, `crates/blastit-frame/src/{ctl,frame}.rs`, `Makefile` (`php-sapi`), `plan/nodes/MB/MB.{3,9}/**`, `docs/VISION.md`
- **Verdict:** **FAIL** — 0 `BLOCKER`, 4 `MAJOR`, 8 `MINOR`

Round 1's two `BLOCKER`s are genuinely closed (flat ABI surface now matches the header; `MB.3.11.1` exists as a dedicated leaf). What remains are four seams a cheap implementer cannot resolve without guessing, all in text the doc presents as exact.

---

## Verification matrix (the two questions this round asks)

| Check | Result |
|---|---|
| §4.4 function surface vs shipped header | **PASS** — `blastit_php_abi_version`, `blastit_php_minor`, `blastit_php_worker_init`, `blastit_php_worker_shutdown`, `blastit_php_request` match exactly; `BLASTIT_PHP_ABI_VERSION 1u`, `BLASTIT_PHP_ABI_MAJOR 8`, `OUTCOME_OK/BAILOUT` match; `blastit_php_minor()` really returns `PHP_MAJOR*100+PHP_MINOR` (`c:410`), so `802/803/804` is right. |
| §4.4 two-step `dlopen` + `:14–18` cite | **PASS** — header lines 14–18 are exactly the `RTLD_NOW\|RTLD_GLOBAL` (libphp) → `RTLD_NOW\|RTLD_LOCAL` (shim) rule; `DT_NEEDED`-free claim matches `Makefile:406` (links nothing). |
| §4.4 *other* shipped claims (`size ==`, `#if ZEND_MODULE_API_NO`, hardening flags) | **FAIL** → `MAJOR-3` |
| `MB.3.11.1` exists / owns `runtime.rs` / `#![forbid(unsafe_code)]` / `unsafe` in `blastit-php-sapi` | **PASS** (leaf to be created by `lead`; `blastit-view/src/lib.rs:126` already `#![forbid(unsafe_code)]`; `abi.rs` header states it is the only `unsafe`-allowed module) |
| `init`-owned supervision, `SpawnSpec`, sole `waitid`/`waitpid` | **PARTIAL** → `MINOR-2` |
| Acyclic DAG | **PASS** — `blastit-php-sapi` has zero dependencies (`Cargo.toml` comment: "`cargo tree -p blastit-php-sapi` must stay empty"), so `blastit-view → optional blastit-php-sapi` cannot cycle; `blastit-backend` does not exist yet and nothing depends on it in reverse. |
| `CostSource` derivation | **FAIL** → `MAJOR-2`, `MINOR-1` |
| Apache-2.0 header rationale | **PASS** — `c/blastit_php_sapi.c:4` carries `SPDX-License-Identifier: Apache-2.0`; §4.4 item 4's "no `PHP-3.01` redistribution" argument holds. |
| ≤600-line split leaves | **PARTIAL** → `MAJOR-4`, `MINOR-7` |
| Serialized `Cargo.lock` + explicit `lead` pre-dispatch syncs | **PASS with gaps** — chain nodes `MB.9.1.1` / `MB.9.2.1.1` do exist on disk (`plan/nodes/MB/MB.9/MB.9.1/MB.9.1.1`, `…/MB.9.2.1.1`); `MB.3.19.{1,2}` paths (`seccomp.rs`, `signal.rs`, `child.rs`) all exist. Gaps: `MINOR-6`, `MINOR-8`. |

---

## BLOCKER

None.

---

## MAJOR

### MAJOR-1 — §4.1's `WorkerRuntime` signature names three types that do not exist and cannot be imported under §4.2's DAG

> `fn serve(&self, req: RequestHead, io: &mut dyn RequestIo) -> RequestEnd;`
> `pub struct RequestEnd { pub outcome: Outcome, pub cpu_ns: Option<u64> }`
> `… write_head(ReplyHead) …`

`blastit-view` today depends only on `blastit-frame` + optional `pyo3` (`crates/blastit-view/Cargo.toml`), and §4.2 fixes the DAG as `blastit-view → {blastit-frame, optional pyo3, optional blastit-php-sapi}`. Against that:

- `RequestHead` exists only as `blastit_http::RequestHead` (`crates/blastit-http/src/head.rs:72`) — a crate `blastit-view` is forbidden to depend on. The frame type the view actually uses is `blastit_frame::ReqHead` (`lib.rs:165`).
- `ReplyHead` **does not exist anywhere** in the workspace (`grep` over `crates/*/src`).
- `Outcome` exists only as `blastit_events::Outcome` (`crates/blastit-events/src/lib.rs:180`) — again not a `blastit-view` dependency; the view's own vocabulary is `blastit_frame::DoneOutcome`.

**Failure mode:** `MB.3.11.1` is the first leaf a cheap implementer touches, and its single deliverable is this trait. They will either (a) add `blastit-http`/`blastit-events` edges to `blastit-view`, silently breaking the §4.2 DAG and the "zero I/O / zero event" purity of the view crate, or (b) invent three local types with guessed variants, and `ViewUpstream`/`MB.9.3` then has to map them by hand — exactly the kind of cross-leaf mismatch that costs a whole retry.

**Fix (one paragraph in §4.1):** declare them view-owned in `runtime.rs` —
`pub struct RequestHead { … }` / `pub struct ReplyHead { pub status: u16, pub headers: … }` / `pub enum Outcome { Completed, Errored, Aborted }` — with an explicit note on how each maps to `blastit_frame::{ReqHead, RespHead, DoneOutcome}` and to `ExchangeOutcome`, or state plainly that the trait uses `blastit_frame::ReqHead`/`RespHead`/`DoneOutcome` and delete the three new names.

### MAJOR-2 — `CostSource` has no `Ceiling` variant, but three requirements and a critical test demand `cpu_source: "ceiling"`

> §4.2: "`CostReport { pub cpu_ns: u64, pub source: CostSource }` — `ProcessExact`, `ThreadExact`, `AsyncContext`"
> §4.5: "failing closed to `ceiling` (`cpu_source: "ceiling"`)"
> R19: "fallback **`rlimit_nproc`** … emitting `Warning::ProcUnreadableCeilingFallback`" … "failing closed to `ceiling` (`cpu_source: "ceiling"`)"
> ADR-0009 §4: same, twice.

The enum is enumerated exhaustively (three variants, no `…`), yet the only fail-closed path the design has — `/proc` unreadable under `hidepid=2` — must be *reported* as `"ceiling"`. `critical_fair_cpu.rs` (`R17`–`R19` row) asserts `hidepid=2` fallback charges `rlimit_nproc * wall`; the `request` event's `cpu_source` is the oracle for that test.

**Failure mode:** `MB.9.1.1` writes `CostSource` from §4.2, `MB.9.3` implements the floor, and there is no representable value for the fail-closed case. The cheap implementer either adds a variant the design never names (and the `--json`/log spelling `"ceiling"` is then invented, not specified) or reuses `ProcessExact`, which makes the anti-forgery path indistinguishable from an honest report.

**Fix:** extend §4.2 item 1 to `CostSource { ProcessExact, ThreadExact, AsyncContext, Ceiling, KernelFloor }` with one line each on when it is emitted, and state that `Ceiling` serializes as `"ceiling"` (wire/`--json`/log). Also say which value `source` takes when the kernel floor wins over a valid report (`KernelFloor`), because §4.5's `bounded = reported.min(ceiling).max(kernel_floor)` has two "who won" cases and only one field to express them.

### MAJOR-3 — R11/§4.4 claim three shim verifications that shipped `MB.3.12` does not perform

> §4.4: "builds native ~25 KiB Apache-2.0 shims … against each PHP minor's headers (`#if ZEND_MODULE_API_NO != 20220829 / 20230831 / 20240924 #error`), with `-fstack-protector-strong -D_FORTIFY_SOURCE=3 -Wl,-z,relro,-z,now`"
> R11: "each verifying `abi == 1`, `size == sizeof(...)`, and `#if ZEND_MODULE_API_NO == ...`"

Checked against the shipped artefacts:

1. **No `ZEND_MODULE_API_NO` guard exists.** `grep -rn ZEND_MODULE_API_NO crates/blastit-php-sapi/` returns only doc comments in `src/toolchain.rs:26–30`; neither `c/blastit_php_sapi.c` nor `c/include/blastit_php_abi_v1.h` has the `#error`. A shim compiled against PHP 8.5 headers builds clean and reports `blastit_php_minor() == 805`.
2. **No runtime `size == sizeof(...)` check exists.** The frozen header exposes `blastit_php_abi_version`, `blastit_php_minor`, `blastit_php_worker_init`, `blastit_php_worker_shutdown`, `blastit_php_request` — there is **no** size accessor, so the check R11 mandates is unrepresentable in ABI v1. (Layout *is* pinned, but at compile time: `c/blastit_php_sapi.c:52–61` `_Static_assert`s on size/offsets, mirrored in `src/abi.rs` and `tests/critical_shim_contract.rs`.)
3. **None of the hardening flags are in the build.** `Makefile:406` compiles with `-O2 -D_GNU_SOURCE -fPIC -shared -Wall -Wextra -Werror -Wl,-soname,…` only. No `-fstack-protector-strong`, no `-D_FORTIFY_SOURCE=3`, no `-Wl,-z,relro,-z,now`.

**Failure mode:** `MB.3.18` builds and ships the 12-leg shim matrix by trusting §4.4 — so the release artefacts ship without stack protector/RELRO/FORTIFY, and R11's acceptance line (`critical_php_abi.rs` "wrong `abi`/`size` returns `NULL`") has no callable to test. This is the one place where "the doc says it is already shipped" directly suppresses work.

**Fix (pick one per item, all cheap):** (a) add the 3-line `#if ZEND_MODULE_API_NO != …` guard to `c/blastit_php_sapi.c` and the four flags to the `Makefile` `php-sapi` rule, and keep §4.4 as written — or (b) rewrite §4.4/R11 to say "compile-time `_Static_assert` size/offset pins; no runtime size accessor in ABI v1" and drop the `#error`/flag claims. Do not leave the text asserting behaviour the repo lacks.

### MAJOR-4 — `MB.3.13.2` bundles two crates plus tests into one ≤600-line leaf

> Row 7: `MB.3.13.2` — "Safe Rust `dlopen`/`dlsym` host wrapper in `blastit-php-sapi` (`libphp` `RTLD_GLOBAL`, shim `RTLD_LOCAL`) + `PhpEmbedRuntime` (`WorkerRuntime`) in `blastit-view`" — paths `crates/blastit-php-sapi/src/**`, `crates/blastit-view/{Cargo.toml,src/**}`, `Cargo.lock`.

Size evidence from the repo this leaf must extend: `blastit-php-sapi/src/request.rs` is already 477 lines (request/INI/server-var types), `blastit-view/src/lib.rs` 1167 and `request.rs` 468 (the loop being trait-ified). The loader alone needs handle ownership, `dlsym` symbol table, `abi_version`/`minor` refusal, a `Mutex`-wrapped worker handle, and `catch_unwind` sink trampolines; `PhpEmbedRuntime` needs `RequestIo`→`blastit_php_request_v1` translation, server-var assembly, sink→frame writes with credits, `recycle()`, and `max_requests`. A realistic diff is 700–950 lines before tests.

**Failure mode:** the implementer hits the 600-line rule mid-leaf, and the doc's own remedy ("split via `plan.py` rather than exceeded") becomes a mid-flight plan mutation by a cheap model — the exact failure the split was meant to prevent.

**Fix:** split row 7 into `MB.3.13.2` (safe loader in `blastit-php-sapi` + its unit tests, no `blastit-view` paths) and `MB.3.13.4` (`PhpEmbedRuntime` + `critical_view_transport.rs` additions in `blastit-view`, depends on `.2`). Keep the `Cargo.lock` ownership on `.2` only.

---

## MINOR

1. **`CostSource` derived from a type the scheduler cannot name.** §4.2: "`ViewUpstream` derives `source` from its `WorkerBinding`". `WorkerBinding` is defined in §4.3 under `crates/blastit/src/**` (the binary), while `ViewUpstream` lives in `blastit-scheduler`, which per the same §4.2 never depends on `blastit`. Reword to "from the route's `kind()` (`"php"`→`ProcessExact`, `"wsgi"`→`ThreadExact`)", or move `WorkerBinding`/`SpawnSpec`/`PinnedFile` into `blastit-backend` and say so in the DAG.
2. **"`supervise.rs` is the **sole** caller of `waitid`/`waitpid`" is false today.** `crates/blastit/src/visibility.rs` calls `libc::waitpid` at 4 sites (`:796`, `:1246`, `:1772`, `:1792`) inside its bounded probe `reap`, and `blastit-sys/src/{proc.rs:204,signal.rs:241}` wrap it. The intent is right (the supervisor is the sole reaper of supervised children), but the invariant as written cannot be asserted by `critical_provider_parity.rs`. Suggested wording: "`supervise.rs` is the sole reaper of supervised children; `visibility::reap` waits only on its own probe child and hands survivors back to `init`." (The code comment at `visibility.rs:773` already says exactly this — the doc should quote it.)
3. **`blastit-view`'s PHP feature is never named.** §4.2 says "optional `blastit-php-sapi`" but not the feature; `crates/blastit/Cargo.toml:28` currently has `php = []`. State `blastit-view` `php = ["dep:blastit-php-sapi"]` and that `blastit`'s `php` must forward `blastit-view/php` (owned by `MB.3.13.3`, which owns `crates/blastit/Cargo.toml`) — otherwise `--no-default-features --features php` builds a binary with no PHP runtime in it and R9/R11's `readelf` test still passes.
4. **§4.4's result-field comment omits `reserved`.** Header `blastit_php_result_v1` is `outcome, status, contained, reserved, cpu_ns, body_bytes` with `_Static_assert(sizeof == 32)`; §4.4 lists five fields. A mirror written from the doc is 4 bytes short.
5. **Stale line cite.** §5.2 says `Warning::SeccompDisabled` at "`config.rs:1212`"; the push is at `crates/blastit/src/config.rs:1230`.
6. **§8.1 contradicts §4.7 on shim count.** §8.1 row 6 says "`12` native PHP shims"; §4.7 and ADR-0010 §4 say "4 proven … Do not claim 12 shims before that evidence exists". Make §8.1 say "up to 12 (4 proven)".
7. **`MB.3.11.1`'s ≤400-line target is optimistic.** The extraction touches `blastit-view/src/lib.rs` (1167 lines: `serve`, `serve_detached`, `serve_one`) and `request.rs` (468). A move-only refactor still diffs both files. Either allow move-only diffs explicitly ("relocated lines do not count") or split `.11.1` (trait + `RequestIo` + `RequestEnd`) from `.11.2` (`WsgiRuntime` extraction + `critical_view_transport.rs`).
8. **§8.2(a) must name the `MB.3.13` brief corrections, not just "split".** The on-disk brief (`plan/nodes/MB/MB.3/MB.3.13/NODE.md`) still says the PHP worker runs under `SeccompRole::View { egress }` and "drives the vtable from `MB.3.11`'s interpreter-free loop", and measures CPU "around `php_execute_script`". All three are wrong post-`ADR-0009/0010` (`EmbeddedView`, `WorkerRuntime` from `MB.3.11.1`, `php_request_startup..php_request_shutdown`). Add to the mandatory sync: "rewrite `MB.3.13.{1,2,3}` briefs: `SeccompRole::EmbeddedView`, `WorkerRuntime` (no vtable), CPU window = startup..shutdown." Left implicit, a split can faithfully copy a `View` role into the PHP worker — a `fork`/`execve` regression against R14.

---

## What is genuinely ready

- `§4.4` function surface, constants, `blastit_php_minor()` semantics, `dlopen` ordering and the `:14–18` citation all match the shipped header byte-for-byte.
- `SpawnSpec`/`WorkerBinding`/`PinnedFile`/`RouteIdentity` are concrete and derivable: `SeccompRole` already derives `Clone, Copy, Debug, PartialEq, Eq` (`seccomp.rs:37`), so `#[derive(PartialEq, Eq)]` on `SpawnSpec` compiles.
- `MAX_PROGRAM = 96` exists (`seccomp.rs:282`), so R14's "exact instruction count" is a real, checkable bound.
- `blastit-php-sapi` has zero dependencies by design, so the `blastit-view → php-sapi` edge cannot create a cycle; Apache-2.0 SPDX header is present.
- The `Cargo.lock` serialization chain names nodes that exist on disk (`MB.9.1.1`, `MB.9.2.1.1`), and every `MB.3.19.{1,2}` path exists.

**Verdict: FAIL** (0 `BLOCKER`, 4 `MAJOR`) — all four `MAJOR`s are one-paragraph doc edits plus one leaf split; none requires redesign.
