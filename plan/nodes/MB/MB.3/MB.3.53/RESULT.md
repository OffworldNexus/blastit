# RESULT — MB.3.53

`[route.ingress] body = "raw"` without `allow_raw = true` is now a **boot refusal
on the production path** (stage 0 and stage 1's re-read), while `dev` keeps its
warning and `check` keeps its `raw_ingress` finding. `ADR-0011` records the
decision (superseding `ADR-0006` decision 3) and cites
`docs/design/learn-single-pass.md` §7.2 / `R16` for the analysis.

STATUS: done

## Summary

**The fail-open, closed.** Before this change `allow_raw` was read only by the
`check` finding: `plan_body` returned `StreamPassthrough` for `BodyCodec::Raw`
regardless (`crates/blastit-ingress/src/lib.rs:284`), and nothing at boot
consulted the key, so a production manifest with `body = "raw"` booted, served,
and streamed unvalidated bodies to the app's own parser. The gate the docs
described gated nothing.

**What changed**

1. `Manifest::raw_ingress_refusals` (`crates/blastit-manifest/src/validate.rs`) —
   one sentence per route declaring `body = "raw"` without `allow_raw = true`, in
   route order, naming **the route, the table and the key**. An empty result is
   the gate being open; callers refuse on `!is_empty()`. One home, so the two
   checkpoints cannot drift.
2. `raw_ingress_gate` (`crates/blastit/src/stage0.rs`) — the production rule,
   called at `run()`'s serve path (`stage0.rs:1904`, after `app_rule`, before
   `check_static_roots` and `bind_view_sockets`) and **not** from `run_dev`, which
   keeps `ManifestWarning::RawIngress` (the independent warning loop at
   `stage0.rs:2279`).
3. Stage 1's re-read (`crates/blastit/src/stage1.rs:275`) — the same gate, gated
   on `dev.is_none()`, immediately after its own `parse_and_validate`. `config`
   carries the manifest **path** and no digest of what stage 0 approved, so a file
   swapped between the stages is the real TOCTOU this closes.
4. `check` untouched: the `raw_ingress` finding is `audit_v1.rs`'s and still
   fires (`critical_check_engine` fixtures still produce it).
5. Docs: `docs/reference/manifest.md` (`raw` row + the
   production/dev/check paragraph and an `allow_raw = true` example),
   `docs/security.md` (the gate sentence), `ADR-0011` (new), `ADR-0006` status
   note + `docs/decisions/README.md` index. `ADR-0006`'s body text is kept as
   written — only its status line names the supersession.

**Message** (one line, both stages, `fail` splits lines so several routes are
several events):

```
route "upload": body = "raw" hands the request body to the application unvalidated, and the
production path refuses it without the acknowledgement; add `allow_raw = true` to that
[route.ingress] table, or declare the codec the route takes (json, form, none) (ADR-0011)
```

**Which stream each refusal uses** (measured, and it is *not* what the node
brief assumed — worth pinning because a test that greps the wrong descriptor
reads as "the message is missing"):

| boot | emitter | JSON (production default) | pretty |
|---|---|---|---|
| stage 0 `run()` | `Emitter::stdout(config.log_format)` | **stdout**, `{"event":"app.output","stream":"stderr","line":…}` | **stdout**, plain text (`ERROR app.output  stderr  …`) |
| stage 1 | `Emitter::stdout(...)` from the config document | **stdout**, same `app.output` shape | **stdout**, plain text |
| `blastit dev` pre-banner failures | `startup` emitter on fd 2 | — | raw text on **stderr** |

`Emitter` writes **one descriptor in both formats** and records the originating
stream as a *field*; only `dev`'s pre-banner refusals are literally on stderr
(which is what `critical_static_cli` asserts with `stderr.contains`). So the
suite asserts the stage-1 refusal on **stdout in JSON** and on **stdout in
pretty**, and the stage-0 transcript below was captured with both fds separated.

**How stage 0 was proved** — the honest part. Unprivileged, a stage-0 gate that
lives after the manifest parse is unreachable: `run()`'s first act is
`blastit_sys::caps::detect()`, which refuses an unprivileged boot with the
`setcap` recipe before `./blastit.toml` is even opened
(`critical_dev_and_fail_closed::unprivileged_production_has_no_single_uid_fallback`
pins that). That is why every production stage-0 gate in this tree (`app_rule`,
`check_static_roots`, `php_boot_gate`) is measured through `dev` or a library
seam. So:

* the **rule** is a `cargo test` unit test at its stage-0 call site
  (`stage0::raw_ingress_gate_tests`), and it reddens with the gate removed;
* the **process** is a real production stage-1 boot (`--stage=1 --config-fd`,
  no `--dev-root`) in the protected critical suite — exit `1`, no listener;
* stage 0's **own process** was run once by hand as root in a container (below),
  which is the only environment where that gate is reachable. It is not in
  `make check`: the critical tier may not use Docker or root.

## Evidence

### 1. Stage-1 refusal + re-read + accepted twin + dev warning (protected test)

```console
$ cargo test -p blastit --locked --test critical_raw_ingress
running 3 tests
test the_pretty_refusal_is_plain_text_naming_the_same_route ... ok
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... ok
test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo test -p blastit --features python --locked --test critical_raw_ingress
running 5 tests
test the_pretty_refusal_is_plain_text_naming_the_same_route ... ok
test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... ok
test allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns ... ok
test dev_serves_the_unacknowledged_manifest_that_production_refuses ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

What each case pins:

* `production_stage_one_refuses_raw_ingress_without_allow_raw` — real production
  stage 1: exit `1`; the `app.output` sentence on **stdout** names `route
  "upload"`, `body = "raw"`, `[route.ingress]` and `allow_raw = true`; stderr is
  empty; no `ready` event; nothing accepts a connection on the port.
* `production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it` —
  the configuration document is built for the **acknowledged** manifest, asserted
  to carry the path and **not** `allow_raw`, and only then is the file rewritten
  without it: the same stage 1 refuses. That is the edit landing between the
  stages.
* `the_pretty_refusal_is_plain_text_naming_the_same_route` — the same refusal in
  `BLASTIT_LOG_FORMAT=pretty`, asserted on the *other* spelling (plain text, not
  JSON, still the emitter's descriptor).
* `allow_raw_true_serves_the_body_byte_for_byte_and_dev_still_warns` — the
  accepted twin: `{"a":1,"a":2}\r\n\0\xff\xfe trailing bytes \x01 not json`
  posts and comes back **identical** (duplicate key, NUL, invalid UTF-8, trailing
  bytes — all of which `json` would refuse), and `dev` still logs
  `passes the request body through unvalidated`.
* `dev_serves_the_unacknowledged_manifest_that_production_refuses` — the same
  file with no acknowledgement: `dev` boots, serves, warns. The gate is the
  deployment path's, not a ban on the codec.

### 2. Stage 0 as a real production process (manual, root, not in `make check`)

The binary is the `--features python` build; the container is a stock
`python:3.14-slim-trixie` with the host's `libpython3.13`/`libexpat`/`libz`
mounted (the embedded interpreter is then unusable, which is irrelevant: the gate
is upstream of it). Manifest: `app = "app:app"`, one `POST /upload` route with
`[route.ingress] body = "raw"` and no `allow_raw`.

```console
$ docker run --rm ... python:3.14-slim-trixie /usr/local/bin/blastit-new; echo "exit=$?"
{"ts":"…","event":"boot","component":"init","stage":"stage0","privilege":"root",…}
{"ts":"…","event":"boot","component":"init","stage":"config","privilege":"root",…}
{"ts":"…","level":"error","event":"app.output","component":"init","stream":"stderr","line":"route \"upload\": body = \"raw\" hands the request body to the application unvalidated, and the production path refuses it without the acknowledgement; add `allow_raw = true` to that [route.ingress] table, or declare the codec the route takes (json, form, none) (ADR-0011)","part":1,"parts":1}
exit=1
```

No `ready`, no later event: the boot stops at the gate, which sits before
`check_static_roots` and `bind_view_sockets`, so nothing is bound. The same run
with `allow_raw = true` gets **past** the gate (it prints the `manifest.warning`
and proceeds to the interpreter stage), and with `BLASTIT_LOG_FORMAT=pretty` the
sentence is plain text — on stdout, per the table above.

### 3. The witness (item 5)

Mutation: the refusal removed in its one home — `validate.rs`'s predicate
`(policy.body == BodyCodec::Raw && !policy.allow_raw)` → `… && false && …`. Files
copied out first, then restored, then `git diff --stat` compared.

```console
### WITH THE REFUSAL REMOVED
$ cargo test -p blastit-manifest --locked --lib validate
failures:
    validate::tests::allow_raw_false_is_refused
    validate::tests::every_unacknowledged_route_is_named_in_route_order
    validate::tests::raw_ingress_without_allow_raw_is_the_only_refused_shape
    validate::tests::the_boot_warning_still_fires_for_an_acknowledged_route
test result: FAILED. 2 passed; 4 failed; 0 ignored; … finished in 0.00s

$ cargo test -p blastit --locked --lib raw_ingress_gate      # the stage-0 rule
thread 'stage0::raw_ingress_gate_tests::every_unacknowledged_route_is_named_in_route_order' panicked:
  called `Result::unwrap_err()` on an `Ok` value: ()
failures:
    stage0::raw_ingress_gate_tests::an_unacknowledged_raw_route_is_refused_and_an_acknowledged_one_is_not
    stage0::raw_ingress_gate_tests::every_unacknowledged_route_is_named_in_route_order
test result: FAILED. 1 passed; 2 failed; … finished in 0.01s

$ cargo test -p blastit --locked --test critical_raw_ingress
test the_pretty_refusal_is_plain_text_naming_the_same_route ... FAILED
test production_stage_one_refuses_raw_ingress_without_allow_raw ... FAILED
test production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it ... FAILED
test result: FAILED. 0 passed; 3 failed; … finished in 0.02s
# the failing process output shows the boot running on to a *different* refusal —
# "this build has no Python support … the route declares no [route.static] block" —
# i.e. the boot served where it used to be gated.

$ docker run … python:3.14-slim-trixie /usr/local/bin/blastit-new    # stage 0, as root
{"level":"warn","event":"manifest.warning","message":"route \"upload\": body = \"raw\" passes the request body through unvalidated"}
Fatal Python error: Failed to import encodings module      # it got past the gate
(no allow_raw refusal line)

### AFTER RESTORE
$ cargo test -p blastit-manifest --locked --lib validate
test result: ok. 6 passed; 0 failed; …
$ cargo test -p blastit --locked --lib raw_ingress_gate
test result: ok. 3 passed; 0 failed; …
$ cargo test -p blastit --locked --test critical_raw_ingress
test result: ok. 3 passed; 0 failed; …
$ docker run … python:3.14-slim-trixie /usr/local/bin/blastit-new
{"level":"error","event":"app.output","line":"route \"upload\": body = \"raw\" hands the request body to the application unvalidated, … (ADR-0011)"}
exit=1
$ diff before.stat after.stat && echo DIFF_STAT_UNCHANGED
DIFF_STAT_UNCHANGED
```

### 4. The three levels, and the halves that must not be deleted

| level | file | command |
|---|---|---|
| policy | `blastit-manifest/src/validate.rs` (4 tests) | `cargo test -p blastit-manifest --locked` |
| stage-0 rule | `blastit/src/stage0.rs` (3 tests) | `cargo test -p blastit --locked` |
| process | `blastit/tests/critical_raw_ingress.rs` (5 tests) | `cargo test -p blastit --features python --locked --test critical_raw_ingress` |

`check`'s half is unchanged and still green (its fixtures are the two
`body = "raw"` blocks in `critical_check_engine.rs`, which must keep producing
`raw_ingress`, plus `python/tests/critical/test_check_contract.py`'s
acknowledged `UPLOAD_MANIFEST`); `dev`'s half is the warning asserted in two
cases above.

### 5. Acceptance commands

```console
$ cargo test -p blastit-manifest --locked
test result: ok. 44 passed … (×15 suites, 0 failed)

$ cargo test -p blastit --locked
test result: ok. 195 passed; 0 failed; 1 ignored   # 1 ignored is pre-existing

$ cargo test -p blastit --features python --locked
test result: ok. 219 passed; 0 failed; 2 ignored   # ×15 suites, 0 failed

$ make check
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0

$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile … (no warnings)

$ uv run --no-project tools/critical_guard.py --base b2d0fa2 --node MB.3.53
critical-guard: b2d0fa2..working tree, 15 changed file(s)
node MB.3.53: paths [...]
  no change to a protected critical test
exit=0

$ make critical-skip-check
protected critical tests hold no skip construct
```

`make check` also ran `rs-doc` (`-D warnings`), `rs-no-python` (the standalone
build + `cargo test -p blastit --lib` without `python`, and standalone clippy),
`zensical build --strict` (`No issues found`), `docs-unit`/`docs-test` (the
runnable-docs gate: every fence PASS), `py-lint`, and `plan-validate`. No new
dependency; no change to `crates/blastit-ingress/**` (not needed: the runtime
behaviour of an acknowledged route is exactly as before), and none to
`crates/blastit/src/check.rs` (the finding lives in `audit_v1.rs` and needed no
change).

I saw two `Blocking waiting for file lock on build directory` lines from the
concurrent gate in this tree; no red belonged to another node's files.

## Files

| Path | What |
|---|---|
| `crates/blastit-manifest/src/validate.rs` | `Manifest::raw_ingress_refusals` (+4 unit tests: refused/accepted, `allow_raw = false`, route order, warning-vs-gate) |
| `crates/blastit/src/stage0.rs` | `raw_ingress_gate` (the production rule) + its call at `run()`'s serve path + 3 unit tests beside `app_rule`'s |
| `crates/blastit/src/stage1.rs` | the re-read gate after stage 1's own `parse_and_validate`, on `dev.is_none()` |
| `crates/blastit/tests/critical_raw_ingress.rs` | **new protected suite**, 5 tests (3 production stage-1, 2 `dev`) |
| `docs/reference/manifest.md` | `raw` row, the `raw` paragraph (production refuses / dev warns / check reports) and an `allow_raw = true` example |
| `docs/security.md` | ingress bullet: the gate, and that `raw` is the codec that needs one |
| `docs/decisions/ADR-0011-raw-ingress-boot-refusal.md` | **new ADR**: decision, three environments, consequences, six "what would make this wrong" |
| `docs/decisions/ADR-0006-route-ingress-body-gate.md` | status line only: decision 3 superseded by `ADR-0011`; body kept as written |
| `docs/decisions/README.md` | index rows for `ADR-0011` and `ADR-0006`'s new status |

### Presets and examples (`grep -rn 'body = "raw"'`)

Nothing **live** needed the key added, and nothing in the tree is broken by the
gate:

| Where | What it is | Action |
|---|---|---|
| `crates/blastit/tests/critical_check_engine.rs:747,1020` | fixtures that must fail with `raw_ingress` | **kept as is** (the finding's negative twin) |
| `python/tests/critical/test_check_contract.py:690` | already `allow_raw = true` | kept |
| `docs/reference/manifest.md`, `docs/frameworks/{django,index,php}.md`, `docs/ci.md` | examples/rows already say `allow_raw = true` | untouched except `manifest.md` (mine) |
| `docs/design/ma-lean-dx.md`, `docs/decisions/ADR-0006-…` | historical design records | **quoted, not rewritten**; `ADR-0011` says what supersedes what, and `ADR-0006`'s status line points at it |
| `e2e/**` | the demo manifest's routes use `body = "form"`; no `raw` anywhere | nothing to change |
| presets (`config.rs`) | config-only defaults, no manifests | nothing to change |

So: no preset or live example was using `raw` without the key — the only
`body = "raw"`-without-`allow_raw` blocks in the repository are the two fixtures
whose job is to be refused.

## Follow-ups

1. **`docs/ci.md:30` / `docs/troubleshooting.md:37`** (out of this node's paths):
   both still say "`raw` also needs `allow_raw = true` … or `blastit check`
   reports `raw_ingress`". Accurate, but they should add that the **production
   boot refuses** without it — that is the sentence a reader hitting the new
   exit-1 needs. One line each.
2. **`suggest_fix` still authors `allow_raw = true`** (`crates/blastit-ingress/
   src/lib.rs:426-441`, `audit_v1.rs:1025-1053`): recorded in
   `learn-single-pass.md` §13 and out of these paths. The gate now fails closed
   for a manifest the tool *did* fix, but the opt-out is still one an agent never
   presents as a decision. Consider `observe` before `raw`.
3. **The `raw` codec has no body-size cap of its own** — unchanged by this node;
   the acknowledgement buys streaming, not validation, and `docs/reference/
   manifest.md` now says so.
4. **A stage-0 process test cannot exist unprivileged** (caps gate precedes the
   manifest read). The permanent home for a *real* production stage-0 boot is
   `e2e/` (rootless/Docker). This node did not add one: `make e2e` builds a
   release wheel image and cannot run inside `make check`, so it was not run
   here and an unrun harness is not evidence. If a later node adds a "boot
   refusals" e2e scenario (a refused `blastit` per refusal class, asserting exit
   code + stream), this gate should join it.
5. **`docs/design/ma-lean-dx.md`'s R11/R16** still describe the warning-only
   behaviour (a historical design record). `docs/design/learn-single-pass.md`
   §7.2 and `ADR-0011` are the current record; a future docs sweep may add a
   pointer.
