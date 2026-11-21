# MB.3.23 — `WorkerRuntime`: the worker-side contract paired with `Upstream`

STATUS: **done**

## What changed

| File | Change |
|---|---|
| `crates/blastit-view/src/runtime.rs` *(new, 466 lines)* | `WorkerRuntime`, `RequestIo`, `OutputStream`, `RequestEnd`, `OUTPUT_LINE_CAP` + docs, a doctest, 2 unit tests |
| `crates/blastit-view/src/lib.rs` *(+4)* | `mod runtime;` and a crate-root re-export, so the seam is `blastit_view::WorkerRuntime` (R10) |

No `Cargo.toml` change, no new dependency, no `docs/**`, no plan frontmatter edit, no commit.

### The surface (from `docs/design/backend-runtimes.md` §4.1, verbatim)

```rust
pub trait WorkerRuntime: Send + Sync + 'static {
    fn kind(&self) -> &'static str;                       // "wsgi" | "php", diagnostics only
    fn max_inflight(&self) -> u32;                        // view_worker_cap | 1
    fn serve(&self, req: ReqHead, io: &mut dyn RequestIo) -> RequestEnd;
    fn recycle(&self) -> bool;
}
pub struct RequestEnd { pub outcome: DoneOutcome, pub cpu_ns: Option<u64> }

pub trait RequestIo {                                     // the view.chan framing, nothing under it
    fn head(&self) -> &ReqHead;
    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn write_head(&mut self, head: RespHead) -> io::Result<()>;
    fn write_body(&mut self, data: &[u8]) -> io::Result<()>;
    fn output(&mut self, stream: OutputStream, line: &str);
    fn flush_response(&mut self) -> io::Result<()>;
}
```

Types are `blastit_frame::{ReqHead, RespHead, DoneOutcome}` directly — the fix the
`review-implementability` MAJOR-1 asked for, so `blastit-view` needs no `blastit-http`/`blastit-events`
edge and invents no second request vocabulary.

## Prerequisites verified in CODE (not in the plan)

- `crates/blastit-frame/src/lib.rs:83` — `pub use crate::frame::{DoneOutcome, Frame, …, ReqHead, …, RespHead, …}` ✅ all three types exist and are public.
- `crates/blastit-view/src/` — no `runtime.rs` existed, no `WorkerRuntime` anywhere (`grep -rn WorkerRuntime crates/` was empty at base `2299b32`); the seam is genuinely new, not a stub of something else.
- `MB.3.11` in code: `crates/blastit-view/Cargo.toml` has `default = ["python"]`, `python = ["dep:pyo3"]`, and `make check`'s `rs-no-python` leg (clippy + tests with `--no-default-features`) passes with the new module compiled in.
- Transport named with no type: `wire`/`flow` are `pub(crate)` today, so the trait could not secretly depend on `Writer`. `grep -cE "Python<'|UnixStream|pyo3|TcpStream" crates/blastit-view/src/runtime.rs` → **0**. (The word "socket" appears 4× in prose about *client* sockets; no Rust type token.)

## Acceptance

```console
$ cargo test -p blastit-view --locked
A1_EXIT:0
running 56 tests
test runtime::tests::the_seam_is_object_safe_and_its_result_can_cross_a_thread ... ok
test runtime::tests::a_request_writes_head_then_body_then_closes_before_reporting ... ok
test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
…
   Doc-tests blastit_view
test crates/blastit-view/src/runtime.rs - runtime::WorkerRuntime (line 95) ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo test -p blastit --locked
A2_EXIT:0
244 tests passed across 11 targets, 0 failed

$ make check
A3_EXIT:0
guard: OK (1 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 4 changed file(s)
  no change to a protected critical test
cargo clippy --workspace --all-targets --locked --features python -- -D warnings   → Finished
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings   → Finished
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

113 suites, 2547 tests, `grep -cE 'test result: FAILED|^error(\[|:)'` → **0**.

### The seam surface + object safety

```console
$ grep -rn 'WorkerRuntime' crates/ | head
crates/blastit-view/src/lib.rs:174:pub use crate::runtime::{OUTPUT_LINE_CAP, OutputStream, RequestEnd, RequestIo, WorkerRuntime};
crates/blastit-view/src/runtime.rs:176:pub trait WorkerRuntime: Send + Sync + 'static {
crates/blastit-view/src/runtime.rs:430:        let runtime: Option<Box<dyn WorkerRuntime + Send + Sync>> = None;
crates/blastit-view/src/runtime.rs:443:        let runtime: Box<dyn WorkerRuntime + Send> = Box::new(Echo);
```

Object safety is asserted at the NODE's exact shape, `Box<dyn WorkerRuntime + Send>`, in
`runtime::tests::the_seam_is_object_safe_and_its_result_can_cross_a_thread`, together with
`assert_send::<RequestEnd>()` and `assert_send::<Box<dyn WorkerRuntime + Send + Sync>>()`.

### Outside-crate implementation is proved, not asserted

The doctest on `WorkerRuntime` is compiled and **run** as its own crate by `cargo test`; it defines
`Echo` + a recording `RequestIo` and dispatches through `Box<dyn WorkerRuntime + Send>`. Mutation
check that it bites (then restored, `grep -c 'fn execute'` → 0):

```console
$ sed -i 's/fn serve(/fn execute(/' crates/blastit-view/src/runtime.rs && cargo test -p blastit-view --locked --doc
error[E0407]: method `serve` is not a member of trait `WorkerRuntime`
error[E0046]: not all trait items implemented, missing: `execute`
error[E0599]: no method named `serve` found for struct `Box<(dyn WorkerRuntime + Send + 'static)>`
test result: FAILED. 0 passed; 1 failed
```

E0407/E0046/E0599 are the *external crate* errors: the doctest can see only public items and must
implement every one of them, so a sealed or private-heavy trait cannot pass it.

## Deviation — one NODE acceptance line contradicts the design of record

`NODE.md` says "its returned future is `Send`". §4.1's contract has **no future**: `serve` is
`fn serve(&self, …) -> RequestEnd`, blocking on the transport's request thread by construction
(`libphp` is not re-entrant; the GIL is not a future), and `blastit-view` has no async runtime to
put one on. I implemented the design of record and satisfied the *intent* — a `Send + Sync + 'static`
runtime and a `Send` `RequestEnd`, which is what makes the `spawn_blocking` task movable — and
asserted it in the unit test. Adding a boxed future or a tokio dependency to match the stale line
would have contradicted §4.1 and broken the §4.2 DAG. `MB.3.24` should confirm this reading before
building the loop on it.

## Follow-ups

- `MB.3.24` owns `RequestIo`'s implementation over `wire`/`flow` and the `RESP_END`/`DONE` writing
  from `RequestEnd`; the trait deliberately does neither. The sequencing is documented on `serve`
  and in the module's ASCII diagram so the two leaves agree on who sends `DONE` (the caller — it is
  the side that knows the request id and that must send on panic).
- `OUTPUT_LINE_CAP` (2048) is exported so the WSGI and PHP sinks cannot drift on the documented cap
  §4.1 gives `output`.
- `wire`/`flow` visibility was **not** changed: the trait names no writer, and both future
  implementations live in this crate, so `pub(crate)` is enough. `MB.3.24` may revisit.
- Pre-existing `plan/` modifications in the worktree (`JOURNAL.md`, this node's `LOG.md`/`NODE.md`)
  are the dispatch harness's own `plan.py` writes (status `ready → active`, attempt 1/3), not a
  second writer. No other file changed during this session.
