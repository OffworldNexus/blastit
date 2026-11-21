# RESULT — MB.3.54

The four MINORs from MB.3.53's review are closed: the three "nothing was bound"
assertions now assert the **ordered** signal (`ready`, absent) instead of a port
probe taken after the child exited, the JSON refusal case asserts the parsed
event's `stream` field, the stage-0 gate's call site is now witnessed by the
**compiler** (the bind takes a proof only the gate can mint), and the three pages
that described `raw` as a warning now say the production boot refuses.

STATUS: done

## Summary

**1. `!listening(port)` → no `ready` (`critical_raw_ingress.rs`).** The old
assertion could not fail: it ran after `finish()` had already reaped the child,
so a boot that bound and *then* exited also freed the port. All three refusal
cases now assert `!announced_ready(&stdout)`, where `announced_ready` parses the
`app.output`/`ready` events in JSON **and** matches the event-name column in
pretty (`… INFO  ready       …`). `ready` is emitted by init only after
`bind_tcp` and the forks (`stage1.rs`'s `emit_ready`), so it is the boot
answering for itself — the same rule `MB.3.48` established for
`critical_php_embed`'s `Boot::await_boot`. `listening()` is deleted (it had no
other caller). The predicate is not one-sided: `Dev::start` now waits for
`announced_ready` **instead of** `ready || listening(port)`, so the two serving
twins fail if the signal stops being emitted.

**2. `stream` on the parsed event.** The JSON case now finds the `app.output`
event whose `line` names `allow_raw = true` and asserts `event["stream"] ==
"stderr"` and `event["level"] == "error"`. `app_output_events` was split out of
`app_output` for it, so the joined text the other assertions read and the field
assertions are the same parse.

**3. The stage-0 call site has a witness that runs in CI — the type system.**
`ServeGates` (`stage0.rs`): `ServeGates::clear(&manifest)` is the only
constructor and it *is* `raw_ingress_gate`, and `bind_view_sockets` now takes
`&ServeGates`. So `run()` cannot reach a bound view socket without the gate
having run first in this process, and deleting the call site — or moving it after
the bind — is a **compile error**, not a silent fail-open. This is the smallest
seam that fits the constraint: stage 0's first act is
`blastit_sys::caps::detect`, which refuses an unprivileged boot before the
manifest is even read, so no unprivileged `cargo test` can stand where the
refusal lives. It is `pub(crate)`, not `#[doc(hidden)] pub` — strictly stronger,
since nothing outside the crate can hold or mint the proof, and it never becomes
public API surface. The refusal half is unit-tested beside `raw_ingress_gate`'s
tests (`the_serve_path_proof_is_withheld_from_a_refused_manifest`: refused
manifest → `Err`, acknowledged → `Ok`).

**4. Docs.** `docs/frameworks/index.md` §4 (the `raw` sentence, which still said
"the boot warns either way" and presented `allow_raw` only as what silences
`check`), `docs/ci.md` (`raw_ingress` row + the "Production enforces the same
declaration" paragraph) and `docs/troubleshooting.md` (`raw_ingress` row plus a
new row in "It does not start" for the boot refusal itself) now agree with
`docs/reference/manifest.md` and `ADR-0011`.

**What a privileged run is still the only witness for** (release record): a real
production stage-0 boot — the process that prints the refusal and stops before
`bind_view_sockets` — still needs a root/container run (`make e2e`'s Docker
"boot refusals" scenario, MB.3.53 follow-up 4); everything else about the gate,
including that no view socket can be bound before it, now runs in `make check`.

## Evidence

### 1. The suites (both builds)

```console
$ cargo test -p blastit --locked --test critical_raw_ingress
running 3 tests
test the_pretty_refusal_is_plain_text_naming_the_same_route ... ok
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... ok
test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo test -p blastit --features python --locked --test critical_raw_ingress
running 5 tests
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... ok
test the_pretty_refusal_is_plain_text_naming_the_same_route ... ok
test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
test dev_serves_the_unacknowledged_manifest_that_production_refuses ... ok
test allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s

$ cargo test -p blastit --locked --lib raw_ingress_gate
running 4 tests
test stage0::raw_ingress_gate_tests::every_unacknowledged_route_is_named_in_route_order ... ok
test stage0::raw_ingress_gate_tests::the_serve_path_proof_is_withheld_from_a_refused_manifest ... ok
test stage0::raw_ingress_gate_tests::an_unacknowledged_raw_route_is_refused_and_an_acknowledged_one_is_not ... ok
test stage0::raw_ingress_gate_tests::every_other_codec_and_no_ingress_at_all_pass_the_gate ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 196 filtered out; finished in 0.01s
```

### 2. RED 1 — the boot announces `ready` and then exits (items 1)

Mutation (temporary, `crates/blastit/src/stage1.rs`, restored below): the stage-1
gate emits `ready` before it refuses — the "bound, announced service, then
exited" shape, which `exit 1` + a free port cannot distinguish from a correct
refusal. The *old* port probe was also left on the line above the new assertion
for this one run, so the paste shows which of the two fires: the run fails at the
new assertion, i.e. the old one passed.

```console
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... FAILED
test the_pretty_refusal_is_plain_text_naming_the_same_route ... FAILED
test production_stage_one_refuses_raw_ingress_without_allow_raw ... FAILED
test allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns ... ok
test dev_serves_the_unacknowledged_manifest_that_production_refuses ... ok

---- production_stage_one_refuses_raw_ingress_without_allow_raw stdout ----
panicked at critical_raw_ingress.rs:372:
a refused boot never announces `ready` — it is emitted only after the bind:
{"ts":"…","level":"info","event":"ready","component":"init","stage":"serving","routes":1,"workers":16}
{"ts":"…","level":"error","event":"app.output","component":"init","stream":"stderr","line":"route \"upload\": body = \"raw\" … (ADR-0011)"}

---- the_pretty_refusal_is_plain_text_naming_the_same_route stdout ----
panicked at critical_raw_ingress.rs:409:
a refused boot never announces `ready`, in either format:
07:36:00.823 INFO  ready       serving  routes=1  workers=16
07:36:00.828 ERROR app.output  stderr  route "upload": body = "raw" …

---- production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it stdout ----
panicked at critical_raw_ingress.rs:455:
the edited manifest never reaches a serving boot on 42841:
{"ts":"…","level":"info","event":"ready",…}

test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out
```

Both halves of `announced_ready` are exercised: JSON (`"event":"ready"`) and
pretty (`INFO  ready`). The two serving twins stayed green — the mutation only
moved the announcement, it did not stop a boot from serving.

### 3. RED 2 — the flipped `stream` field (item 2)

Mutation (temporary, `crates/blastit/src/logs.rs`, restored below):
`logs::fail`'s `Stream::Stderr` → `Stream::Stdout`. Exactly one case reddens, on
the new assertion — `stderr.is_empty()` still holds, which is the hole the node
found:

```console
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... ok
test the_pretty_refusal_is_plain_text_naming_the_same_route ... ok
test production_stage_one_refuses_raw_ingress_without_allow_raw ... FAILED
test allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns ... ok
test dev_serves_the_unacknowledged_manifest_that_production_refuses ... ok

---- production_stage_one_refuses_raw_ingress_without_allow_raw stdout ----
panicked at critical_raw_ingress.rs:358:
assertion `left == right` failed: the refusal records the stream it came from as a field:
{…,"event":"app.output","level":"error","stream":"stdout",…}
  left: String("stdout")
 right: "stderr"
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

And the same mutation at the *level* field (`.with_level(Level::Warn)`), for the
second field this node added:

```console
panicked at critical_raw_ingress.rs:354:
assertion `left == right` failed: a refusal is an error-level event:
{…,"level":"warn",…,"stream":"stderr",…}
  left: String("warn")
 right: "error"
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

### 4. RED 3 — the stage-0 call site cannot be deleted (item 3)

Deleting the gate statement from `run()` (`stage0.rs`), which is what used to
leave every test green:

```console
$ cargo check -p blastit --locked --lib
error[E0425]: cannot find value `serve_gates` in this scope
    --> crates/blastit/src/stage0.rs:1997:81
     |
1997 |     let listeners = bind_view_sockets(&emitter, &manifest, &prepared, &layout, &serve_gates);
     |                                                                                 ^^^^^^^^^^^ not found in this scope
error: could not compile `blastit` (lib) due to 1 previous error
```

And the proof is not handed out unconditionally — `ServeGates::clear` mutated to
skip the gate (`let _ = manifest; Ok(..)`; temporary, restored):

```console
$ cargo test -p blastit --locked --lib the_serve_path_proof
---- stage0::raw_ingress_gate_tests::the_serve_path_proof_is_withheld_from_a_refused_manifest stdout ----
panicked at crates/blastit/src/stage0.rs:5271:
an unacknowledged raw route must not hand out the proof the bind needs
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 199 filtered out
```

### 5. Restore

```console
$ git status --porcelain
 M crates/blastit/src/stage0.rs
 M crates/blastit/tests/critical_raw_ingress.rs
 M plan/BUDGET.md            # the lead's, not mine
 M plan/JOURNAL.md
 M plan/nodes/MB/MB.3/MB.3.53/LOG.md
 M plan/nodes/MB/MB.3/MB.3.8/LOG.md
?? plan/nodes/MB/MB.3/MB.3.54/
$ diff before.stat after.stat && echo DIFF_STAT_UNCHANGED
DIFF_STAT_UNCHANGED
```

No test edit, no `#[ignore]`, no weakened assertion survived: the only protected
file change is the three strengthened cases (plus the helpers they share).

### 6. Acceptance commands

```console
$ make check
MAKE_CHECK_EXIT=0
  # guard: OK (2 staged/untracked file(s) checked)
  # critical-guard: report only, `--base HEAD` (the node-scoped enforced run is §7)
  # rs-fmt / rs-lint / rs-doc / rs-test / test-critical / py-lint / docs-unit
  # zensical build --strict: No issues found
  # docs-test: every fence PASS (incl. docs/security.md fence 1)
  # rs-no-python: default build: 0 pyo3 dependencies (no libpython linked)
  # plan OK
  # 0 `test result: FAILED`

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s
DOCS_CHECK_EXIT=0

$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.43s
CLIPPY_EXIT=0

$ uv run --no-project tools/critical_guard.py --base 86f885b --node MB.3.54
critical-guard: 86f885b..working tree, 9 changed file(s)
node MB.3.54: paths [... 'crates/blastit/tests/critical_raw_ingress.rs' ...]
  M  crates/blastit/tests/critical_raw_ingress.rs  [authorized by crates/blastit/tests/critical_raw_ingress.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.54
GUARD_EXIT=0

$ make critical-skip-check
protected critical tests hold no skip construct
SKIP_EXIT=0
```

## Files

| Path | What |
|---|---|
| `crates/blastit/tests/critical_raw_ingress.rs` | `events`/`app_output_events`/`announced_ready` helpers; the three refusal cases assert no `ready` instead of a post-exit port probe; the JSON case asserts the parsed event's `stream` and `level`; `Dev::start` waits on `announced_ready` only; `listening()` deleted |
| `crates/blastit/src/stage0.rs` | `ServeGates` (the bind's proof) + `ServeGates::clear`; `run()` mints it where the gate was; `bind_view_sockets` takes `&ServeGates`; one unit test |
| `docs/frameworks/index.md` | §4 Ingress: `raw` is refused at a production boot without `allow_raw = true`; dev warns, check reports |
| `docs/ci.md` | `raw_ingress` row + the "Production enforces the same declaration" paragraph |
| `docs/troubleshooting.md` | `raw_ingress` row; new "It does not start" row for the boot refusal |

## Follow-ups

1. **The `e2e/` "boot refusals" scenario (MB.3.53 follow-up 4) is still not
   landed.** This node took the seam instead because that harness is Docker and
   builds a release wheel (`make e2e`), so it could not be run here, and an
   unrun harness is not evidence. It remains the home for a *real* production
   stage-0 boot per refusal class (exit code + which stream, and the two-stage
   TOCTOU on a live tree).
2. **Unchanged from MB.3.53:** `make check`'s own `critical-guard` invocation is
   report-only (`--base HEAD`, no `--node`), so it prints
   `[VIOLATION: not covered by any node path]` for any critical-test edit. The
   enforced run is the node-scoped one in §6; if the intent is that
   `make check` blocks, it needs `BASE=<merge-base> NODE=<id>` wired from the
   plan.
3. **`docs/design/ma-lean-dx.md` R11/R16** still describe the warning-only
   behaviour (a historical design record, noted by MB.3.53 too).
