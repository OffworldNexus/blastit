---
status: Review
owner: critic
node: MB.36.1
lens: implementability
round: 11
subject: docs/design/view-crates-architecture.md, docs/decisions/ADR-0016-view-crates-over-wire.md
---

# Implementability review — `MB.36` view crates, round 11 (sign-off pass)

Judged against the design (1 009 lines), ADR-0016 (138), and `MB.36.2`…`MB.36.7` as they exist in
`plan/nodes/MB/MB.36/`: each leaf's real `paths`, its real `# Acceptance`, plus §8.0's
P1/P2/P4/P5 amendments (P4 is included because MB.36.6's own acceptance is one of the gates).
Criterion: can one cheap implementer reach the stated end state in one pass, and is the leaf
green **and correct** there?

**Round 10's two MAJORs are closed, verified against the tree, not just the prose.**

- **F1 (`is_protected_object`) — closed.** §4.2.3 (260) and §4.4 D2 (495–499) now name the
  public whole decision: "`is_protected_object` is `host::Provenance::check_objects()` … never a
  hand-rolled per-path test, because the RUNPATH half is private to `host`", and "`blastit`
  supplies the first over **`host::Provenance::check_objects()`**". That call is writable from
  `stage0.rs`: `pub struct Provenance` with `pub init_uid`/`objects`/`trees`
  (`host.rs:1013–1024`), `pub fn check_objects` (`:1031`), `pub fn check_trees` (`:1084`),
  `Claim::new` (`:990`). The private half the old text asked a cheap implementer to re-derive
  (`no_writable_search_path`, `:1115`, `$ORIGIN` + nearest-existing, *refuses* unresolvable) is
  now reachable only through `check_objects`, which is the point. The faithful body is one line
  per closure — `Provenance { init_uid, objects: vec![Claim::new("", p.to_path_buf())], trees:
  vec![] }.check_objects()` — and it keeps the sentence assertions: `deployment_refusal`
  (`stage0.rs:1302–1330`) already builds the two object claims and calls `check_objects`, and
  every kept assertion is `message.contains(...)` (`:5690–5712` "the NTS libphp" / "the embedded
  shim" / "could be replaced by another uid" / "chown" / "(PHP 8.4, "; `:5807–5836` "the
  directory holding the Blastfile" / `route "app": entrypoint php …` / `static "/assets": root
  …` / `!contains("install the PHP embed package")`), so discovery naming the claim before
  `present_refusal` satisfies them all. No assertion is exact-string, so the double-naming
  residual is cosmetic.
- **F2 (`PhpProbeBoot`) — closed.** §4.4 D2 (486–487) now reads `PhpProbeBoot { slots, bindings,
  limits: Rlimits, libphp: Option<&PhpRuntime>, timeout }`, and every value the probe reads today
  has a carrier: `limits` replaces `php_view.rs:643`'s `nproc` from `cfg.view_worker_cap()`;
  `libphp` replaces `:685`'s `cfg.php … .version` in `php_probe_refusal`; `PhpSlot.filter`
  replaces `:720`'s `cfg.seccomp`; `PhpSlot.privileges` replaces `:712`'s `drop_privileges`;
  `PhpSlot.egress` replaces `:616`/`:631`; `PhpBinding` route+uid replaces `info.name`/`info.uid`
  (`:610`, `:712`). Nothing the probe needs is left to a root type.

**Verdict first: PASS.** Zero BLOCKER, zero MAJOR, five MINOR (three are one-line prose fixes).
Each leaf's real `paths` plus P1/P2/P4/P5 legalises every file its moves create, edit or delete,
and each leaf's acceptance is reachable in its end state — including the two that bite only after
the split (`.3`'s served loop importing `OutputStream` before `stream.rs` exists, and `.5`'s
deletion of `RequestIo`/`RequestEnd`).

## Findings

### M1 — MINOR — ADR-0016 decision 4's field lists still omit the two values that closed F2 (and the per-slot sandbox)

ADR-0016 (76–77):

> `PhpSearch::new`, `PhpSlot` (route, dir, egress, script name), `PhpProbeBoot` (slots, bindings,
> timeout) and two per-path provenance callbacks (object and tree) — instead of `Config`.

§4.4 D2 (485–487) carries `PhpSlot { …, privileges: Privileges, filter: Filter }` and
`PhpProbeBoot { …, limits: Rlimits, libphp: Option<&PhpRuntime>, … }`. The design is the analysis
and the leaf reads "design of record" first, so this is not a gate; but the ADR is the decision
record and its two parentheticals are *exactly* the ones a cheap implementer would copy into the
seam — the same copy that silently drops `RLIMIT_NPROC`/`RLIMIT_CORE` and the refusal tuple.
Fix: add `privileges`/`filter`, `limits`/`libphp`, and `PhpBinding`'s route+uid to lines 76–77.

### M2 — MINOR — `PhpProbeBoot.limits`' construction rule is stated only for `SpawnContext`; an unconditional `Some(cap)` breaks `blastit dev`

451 gives the rule once: "`nproc = view_worker_cap() + margin` when `drop_privileges`, `nofile =
1024`, `core_zero = true`". Today the probe applies it conditionally
(`php_view.rs:643`: `probe.drop_privileges.then(|| …)`), because `RLIMIT_NPROC` is per-UID and in
`dev` the developer's whole process tree counts. At 486–487 `limits` is a plain required field
with no note, so an implementer building it in `stage1`/`supervise` can reasonably write
`Some(view_worker_cap() + margin)` unconditionally — a real `blastit dev` behaviour change (R14)
that no `make check` leg names. Fix: one clause at 486–487: "built as 451 says, with `nproc`
present only under `Privileges::DropTo`".

### M3 — MINOR — `ForkProtocol`'s "disagrees" direction is still unstated, and the wrong reading is reachable from `.5`'s acceptance

D1 (421–427): `fork_for(protocol, interpreter_live)` "refuses a protocol that disagrees with the
image". The view crate chooses the protocol and cannot know whether the wheel's interpreter is
live; the root supplies the truth. Only `NoInterpreter`-while-live is unsafe (it skips
`PyOS_BeforeFork` and the two `AfterFork` halves, `forking.rs:48–83`); `InterpreterLive`-while-
absent is a no-op (`:57–58`). A symmetric refusal makes `.5`'s default `cargo test -p blastit`
(php + static, no python) fail the PHP probe fork at boot. Both hard-coded values fail one of the
two configurations the leaf must run, so the correct resolution is reachable — but one wasted
attempt is the price of the missing clause. Fix: "`InterpreterLive` is always accepted;
`NoInterpreter` while an interpreter is live is the refusal". (Round 10 already logged this as
polish, not a finding; it stays polish.)

### M4 — MINOR — `PhpSlot` carries no `uid`, yet `PhpBinding` is said to "carry its slot's route + uid"

469–476 gives `PhpSlot` `route`, `dir`, `egress`, `script_name`, `privileges`, `filter`; 490 says
"`PhpBinding` carries its slot's route + uid so the probe can name what it refuses". The uid
exists only inside `Privileges::DropTo(u)`; under `Shared` there is none, and after D1 the probe
drops through `ChildPlan.privileges`, not `binding.uid`. `PhpBinding` today has neither field
(`php_view.rs:95–127`), so the implementer must invent the `Shared` case. Harmless but
one-sentence-ambiguous: say `uid: Option<u32>` (unused under `Shared`) or drop the field.

### M5 — MINOR — §4.1's `root --> blastit-view-static` edge disagrees with the feature line, and `MB.36.5`'s Context says a third thing

§4.2.5 (303) and §4.3 (322) agree: `static = ["blastit-scheduler/static"]`, with `blastit`
depending on the scheduler `default-features = false`. §4.1's DAG (181–184) draws `root --> stv`
directly, and `MB.36.5`'s Context (not among P5's items) still says
`static = ["dep:blastit-view-static"]`. The implementation follows §4.3/§4.2.5; the DAG arrow and
the node line are the residues. One spelling is inert (`default-features = false` plus forwarding
is the one that keeps `--no-default-features` honest).

## Per-leaf judgement

| Leaf | `paths` + P1/P2/P4/P5 cover the design? | One pass? | Findings |
|---|---|---|---|
| `MB.36.2` | yes. P1 holds `scheduler/src/{error,static_asset,static_response}.rs`, `tests/common/static_fixture.rs`, `critical_rewrite.rs`, and `blastit-view/src/{path,lib}.rs` + `Cargo.toml`; the root manifest needs no edit (`Cargo.toml:3`, `members = ["crates/*"]`); `blastit/Cargo.toml` gains `default = ["php","static"]` here so `critical_static_cli` is green at the leaf's end state; the three `default-features = false` edges (846–848) are what stop the still-default `python` feature from unifying `pyo3` into `-p blastit`. `grep -rn 'openat2\|ResolveFlags' crates/blastit-scheduler/src` is already empty outside `static_asset.rs`, so the acceptance gate is satisfiable. | yes | — |
| `MB.36.3` | yes. P1 grants `src/zygote.rs` + `tests/critical_loaded_files.rs`; the served loop can import `OutputStream`/`OUTPUT_LINE_CAP` from the crate root (`lib.rs:194` re-exports both; `stream.rs` is not needed until `.5`), so the split needs no new public module here; `stage1.rs` (`LoadedTracker::install`) is in `paths`; `dev_reload.rs` needs no edit because `loaded::{decode, Batch, MAX_BATCH_BYTES, Reporter}` stay in the transport (checked imports). P5(b) replaces the one acceptance line (`-p blastit --test critical_loaded_files`) that the leaf's own move deletes. | yes | — |
| `MB.36.4` | yes. `crates/blastit-sys/**` covers `child.rs`'s new home, the `set_env`/`setpgid` helpers, the `fchdir` change and the new protected suite; `child.rs` imports only `std`, `blastit_sys` and `rustix` (`child.rs:10–19`), so it moves with no cycle and `rustix` already has `process` (`blastit-sys/Cargo.toml`); `SignalMask`/`Rlimits` are `Copy` (`signal.rs:15`, `privs.rs:151`), so `SpawnContext` can hold them; `forking.rs` keeps `fork` and only adds `fork_for`, so `php_view.rs` compiles untouched with it out of `paths`. | yes | M3 |
| `MB.36.5` | yes on `paths` (P1 adds `config.rs`, `supervise.rs`, `check.rs`, `tests/common/**`, `critical_parallel_views.rs`; `host.rs` is correctly absent — `Provenance`'s public API is all the closures need) and yes on the seam. `RequestIo`/`RequestEnd`/`WorkerRuntime` are used only by `php.rs` and `lib.rs:194` (grep: WSGI's `serve`/`request.rs` never name them), so their deletion touches no file outside this leaf's `paths`; `php.rs` has **zero** `unsafe` and `php_view.rs`'s six sites (`:763`, `:776`, `:825`, `:843`, `:1132`, `:1161`) are precisely the six §5's "Unsafe policy" replaces, so `#![forbid(unsafe_code)]` on the new crate is reachable. | yes | M1, M2, M4 |
| `MB.36.6` | yes as amended. P1 grants ADR-0010 + `docs/decisions/README.md` + `backend-runtimes.md` (P2); P4 narrows the `WorkerRuntime` grep to `ARCHITECTURE.md` + `crates/` (whose only hit, `ARCHITECTURE.md:476`, is in §4, i.e. in the section list §8.5 already assigns); P5(i) adds `make rs-no-default`; ADR-0009's status line and the README row already name ADR-0016 (checked in-tree), so no extra glob. | yes | M5 |
| `MB.36.7` | yes. `crates/blastit/tests/**` covers `dx_invariance.rs`/`dx_capture.rs` (P5(e) adds the capture command and `make check`); §8.6 names `PKG=blastit-view-python` where the node's `# Acceptance` lists three packages — a recorded difference, not a gate (the leaf runs §8.6). | yes | — |

## Checked and not findings

- **`.3` is not missing `blastit-view/src/stream.rs`.** `OutputStream`/`OUTPUT_LINE_CAP` are
  re-exported at `blastit-view`'s root (`lib.rs:194`), so the moved `serve.rs` imports
  `blastit_view::{OutputStream, OUTPUT_LINE_CAP}` until `.5` relocates them to `stream.rs`; R3's
  oracle (`grep -q 'pub enum OutputStream' …/stream.rs`) is a final-state oracle.
- **`.2`'s dependency list is right.** `static_asset.rs` imports only `blastit_manifest` and
  `rustix` (`:34–36`); `static_response.rs`/`rewrite.rs` import only `crate::static_asset`. No
  `blastit-frame` edge is needed, so §4.2.4's "`blastit-view`, `blastit-manifest`, `rustix`" is
  exactly the set a cheap implementer needs.
- **`.2` needs no protected-CLI path.** `crates/blastit/tests/critical_static_cli.rs` carries no
  `include_str!` (the four `stage0.rs` readers are all PHP suites), so nothing in it breaks when
  the static modules leave the scheduler.
- **The moved view tests keep R1.** `tests/{serve,body,concurrency,environ,flow,gil}.rs` and
  `tests/support/mod.rs` import only `std`, `blastit_frame`, `blastit_view` and `pyo3` — no
  scheduler, so `cargo tree --edges normal,build,dev` stays clean for `blastit-view-python`.
- **The two code-level `Deployment` sites stay in the root.** `stage0.rs:1179`/`:1277` build the
  closures from `current_uid()`; the five test literals (`:5677`, `:5782`, `:5801`, `:5879`,
  `:6057`) do the same, and discovery keeps the claim names — so "the leaf adapts imports; it
  does not rewrite assertions" holds for the 27-case module even though `Deployment` gains two
  fields.

## Verdict

Round 10's blockers are genuinely gone: the object-check body is now the public
`Provenance::check_objects` (writable, and the RUNPATH refusal cannot be lost), and
`PhpProbeBoot` carries `limits`/`libphp` so `RLIMIT_NPROC`, `RLIMIT_CORE` and the `(PHP 8.4, …)`
tuple survive the probe. Every leaf's `paths` + P1/P2/P4/P5 legalises its work, and nothing in
the leaf order leaves a half-moved crate on the branch (`MB.36.5`'s 5a/5b option is recorded).
The remaining items are prose: ADR-0016's decision-4 field lists (M1), the probe's `limits`
construction clause (M2), the `ForkProtocol` direction (M3), `PhpBinding`'s `Shared` uid (M4) and
the `static`-feature spelling in the DAG/node (M5). None blocks a one-pass implementation.

VERDICT: PASS
