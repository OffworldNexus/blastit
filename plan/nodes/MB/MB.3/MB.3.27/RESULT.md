# Result MB.3.27

## Status

`STATUS: done`

## What changed

Two shapes, chosen per file by whether the file *mixes* WSGI-bound and
capability-agnostic cases (measured, not guessed):

| file | scope | why |
|---|---|---|
| `critical_dev_and_fail_closed.rs` | **crate-level** `#![cfg(feature = "python")]` | 16 of 19 cases fail without the interpreter; the whole file is one harness (`APP` fixture, `Dev`, `MockPgServer`) around `blastit dev` serving a WSGI app. Gating the 16 cases individually left **19 `dead_code` warnings** (measured: `READY_TIMEOUT`, `EVENT_TIMEOUT`, `EXIT_TIMEOUT`, `PORT_ATTEMPTS`, `Dev`, `LaunchFailure`, `Response`, `copy_runtime_env`, …) — silencing those would need ~20 more `cfg` attributes on harness items, i.e. a much larger diff than the node allows. The 3 capability-agnostic survivors (lost-port classification, watermark reading, unprivileged-production fail-closed) stay covered in every configuration that runs an interpreter, which is all of `make check`/CI. |
| `critical_config_and_presets.rs` | **per-test** `#[cfg(feature = "python")]` on `an_empty_directory_names_the_missing_manifest_before_the_app` | only that one case asserts the WSGI `BLASTIT_APP` sentence; the other 11 (CLI grammar, CWD discovery, preset precedence, seccomp gate) are capability-agnostic and keep running in the standalone build — no dead-code fallout. |
| `critical_static_cli.rs` | **per-test** `#[cfg(feature = "python")]` on `a_mixed_tree_without_an_app_still_names_blastit_app` | same: one WSGI-bound case out of 12. The other 11 exercise the all-static surface, which is precisely what the standalone build carries. |

Diff is `2 0` (added/deleted) in each file: one comment naming `MB.17` + one
`cfg`. No assertion added, removed, relaxed, reordered; no `#[ignore]`; no
helper moved; no other file touched.

## Acceptance

### `cargo test -p blastit --locked` — exit 0, no warnings

```
running 164 tests ... test result: ok. 164 passed; 0 failed
     Running tests/critical_config_and_presets.rs
running 11 tests ... test result: ok. 11 passed; 0 failed
     Running tests/critical_dev_and_fail_closed.rs
running 0 tests ... test result: ok. 0 passed; 0 failed
     Running tests/critical_static_cli.rs
running 11 tests ... test result: ok. 11 passed; 0 failed
     Running tests/no_python_build.rs
running 3 tests ... test result: ok. 3 passed; 0 failed
running 31 tests ... test result: ok. 31 passed; 0 failed
exit=0
```

### `cargo test -p blastit --locked --features python` (3 suites) — exit 0

```
Running tests/critical_config_and_presets.rs
running 12 tests ... test result: ok. 12 passed; 0 failed
Running tests/critical_dev_and_fail_closed.rs
running 19 tests ... test result: ok. 19 passed; 0 failed
Running tests/critical_static_cli.rs
running 12 tests ... test result: ok. 12 passed; 0 failed
exit=0
```

Before/after assertion coverage is identical — recorded **before** the edit, on
the same branch at HEAD `8e657ca`:

```
before: config 12 passed | dev_and_fail_closed 19 passed | static_cli 12 passed
after : config 12 passed | dev_and_fail_closed 19 passed | static_cli 12 passed
```

(For the record, the pre-fix default build failed exactly as the node says:
16 failures in `critical_dev_and_fail_closed`, 1 in `critical_config_and_presets`,
1 in `critical_static_cli`, each on the stage-0 sentence
"this build has no Python support (built without feature \"python\")").)

### `make check` — exit 0

```
guard: OK (29 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 17 changed file(s)   [report only, no --node]
cargo fmt --all -- --check            -> clean
cargo clippy --workspace --all-targets --locked --features python -- -D warnings -> Finished
cargo clippy -p blastit --all-targets --locked -- -D warnings (standalone)      -> Finished
cargo test --workspace --all-targets --locked --features python                 -> all ok
cargo test --workspace --doc --locked --features python                         -> all ok
cargo test -p blastit --features python --test critical_{check_engine,config_and_presets,dev_and_fail_closed,static_cli} -> all ok
cargo test -p blastit --locked --test no_python_build                           -> 3 passed
default build: 0 pyo3 dependencies (no libpython linked)
plan OK
exit=0
```

### `uv run --no-project tools/critical_guard.py --base 8e657ca --node MB.3.27` — exit 0

```
critical-guard: 8e657ca..working tree, 17 changed file(s)
node MB.3.27: paths ['crates/blastit/tests/critical_dev_and_fail_closed.rs', 'crates/blastit/tests/critical_config_and_presets.rs', 'crates/blastit/tests/critical_static_cli.rs', 'crates/blastit/tests/no_python_build.rs']
  M  crates/blastit/tests/critical_config_and_presets.rs  [authorized by crates/blastit/tests/critical_config_and_presets.rs]
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [authorized by crates/blastit/tests/critical_dev_and_fail_closed.rs]
  M  crates/blastit/tests/critical_static_cli.rs  [authorized by crates/blastit/tests/critical_static_cli.rs]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.3.27
exit=0
```

### `git diff -- <the three files>`

```diff
+// `MB.17`: this case asserts the WSGI `BLASTIT_APP` sentence, which the standalone stage-0 gate replaces.
+#[cfg(feature = "python")]
 #[test]
 fn an_empty_directory_names_the_missing_manifest_before_the_app() {

+// `MB.17`: every case boots `blastit dev` on a WSGI app — only with feature = "python" (ADR-0010 §4).
+#![cfg(feature = "python")]

+// `MB.17`: the `BLASTIT_APP` WSGI sentence is a python-build contract; the standalone gate speaks first.
+#[cfg(feature = "python")]
 #[test]
 fn a_mixed_tree_without_an_app_still_names_blastit_app() {
```

`git diff --numstat`: `2 0` / `2 0` / `2 0` (insertions only, nothing deleted).

## Follow-ups

- Mid-session a stray edit to `critical_config_and_presets.rs:17` appeared (an
  alphabetical import reorder of `blastit_events`, which `cargo fmt --check`
  rejects) and was reverted. It was not mine and the worktree is shared/dirty
  with other nodes' work — worth knowing that anything running `cargo fmt` in
  this tree can collide (see `docs/knowledge/LESSONS.md` if it recurs).
- `docs/knowledge/LESSONS.md` candidate: Rust integration tests gated with
  `#[cfg]` turn every helper they used into `dead_code`, so a per-case `cfg`
  split is only viable when the harness is shared with the surviving cases.
