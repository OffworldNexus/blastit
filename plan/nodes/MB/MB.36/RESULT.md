# Result MB.36

## Summary

The view layer is four crates over one wire. `blastit-view` is transport only
(`wire`, `flow`, `loaded`'s codec, `error`, `config`, `sync`, `stream`); each view
kind is its own crate — `blastit-view-static` (the responder plus the one
request-target decoder), `blastit-view-python` (the WSGI environ/input/request/
response, the serve loop, the immortal preload and the zygote fork) and
`blastit-view-php` (the worker, discovery, pinning, probe and spawn) — composed by
Cargo features, with `blastit` back to being an orchestrator. The `WorkerRuntime`
trait, `runtime.rs` and `PinnedEntryRuntime` are gone; `own_process_group` is gone
and its `setpgid` lives inside `blastit-sys::child::enter`, before the filter.

Seven leaves, each reviewed and committed on `release/0.2.0`:

| Node | Commit | What landed |
|---|---|---|
| `MB.36.2` | `016b592` | `blastit-view-static`; scheduler keeps dispatch behind a default `static` feature and refuses a mount at boot when it is off (that branch is *executed* by `static_feature_off`, not compiled) |
| `MB.36.3` | `2910123` | `blastit-view-python`; `blastit-view` loses `pyo3` from its normal **and** dev tree and its `default = ["python"]` |
| `MB.36.4` | `df6a5bf` | `immortal`/`zygote` move behind the `SpawnContext`/`Forker` seam; `child.rs` → `blastit-sys`; the boot-mode decision pinned by `view_child_plans` (production → per-route `DropTo` + `Seccomp`, `dev` → `Shared` + `Seccomp`, only `BLASTIT_SECCOMP=0` → `Off`) |
| `MB.36.5` | `b42a6c9` | `blastit-view-php`; `runtime.rs` + trait deleted; `stage0.rs` 6 566 → 5 066 |
| `MB.36.6` | `6977bd0` | the feature matrix; `make rs-no-default` (build **and** the refusal test); the archive selects `php,static` and a positive guard refuses a dropped `static`; the living spec corrected |
| `MB.36.7` | `6f722ea` | wheel + Docker + rootless proofs, the DX-invariance replay, `AUDIT-MB.36.md` |
| `MB.36.11` | `a51b40e` | `critical_artifacts`: the wheel's `DT_NEEDED`/`RUNPATH` set and the deleted trait names |

## Evidence

Every `# Acceptance` item, run on this tree at `a51b40e`:

```console
$ test -d crates/blastit-view-python && test -d crates/blastit-view-php && \
    test -d crates/blastit-view-static && echo "three view crates exist"
three view crates exist
$ test ! -e crates/blastit-view/src/runtime.rs && echo "runtime.rs deleted"
runtime.rs deleted
$ test -z "$(grep -rn 'pyo3' crates/blastit-view/Cargo.toml)" && echo "transport crate links no interpreter"
transport crate links no interpreter
$ test -z "$(grep -rn 'blastit_php_sapi' crates/blastit-view/src)" && echo "transport crate knows no SAPI"
transport crate knows no SAPI
$ grep -q 'ADR-0016' docs/decisions/README.md && echo "ADR indexed"
ADR indexed
$ uv run --no-project tools/critical_guard.py --base 4aa39d9 \
    --node MB.36.2 --node MB.36.3 --node MB.36.4 --node MB.36.5
critical-guard: OK: all 14 protected change(s) are authorized by node(s) MB.36.2, MB.36.3, MB.36.4, MB.36.5
$ make rs-no-python
default build: 0 pyo3 dependencies (no libpython linked)
$ make check
plan OK                                          # EXIT=0; 204 "test result: ok", 0 FAILED
$ make e2e
PASS: 0 failure(s)                               # EXIT=0; 411 "ok" lines, 0 FAIL lines
```

- **`--base 4aa39d9`, not `develop`**: `develop` is not an ancestor of this branch
  (merge-base `2c05139`), so a `--base develop` list cannot answer "did this epic
  move a protected test without authorisation". `4aa39d9` is the epic's parent and
  the front of its own range; §4.5 records why the release-wide list belongs to the
  release PR.
- **The four crate suites** run inside `make check` (204 green result lines):
  `blastit-view` 43, `blastit-view-static` 20/18/6, `blastit-view-python` 59 lib +
  30/10/4/12/7/1 plus `critical_loaded_files` 2, `blastit-view-php` 29 + 1 ignored
  (`php_native`, by design), and the protected CLI tier at 12/9/28/7/23.
- **No cycle**: `cargo tree -p blastit-view-php -e normal` and
  `-p blastit-view-static -e normal` contain no `blastit` package.
- The artifacts (wheel `01dce9e1…`, archive `496b5360…`, the container legs, the
  24-capture empty invariance diff, the mutation ratios) are transcribed with their
  commands in `docs/testing/AUDIT-MB.36.md` §4–§6 and `MB.36.7/RESULT.md`.

## Deviations

1. **`RequestIo`, `RequestEnd` and `flush_response` survive** in
   `blastit-view-php/src/worker.rs` instead of being deleted as ADR-0016 decision 2
   ("the worker trait is deleted, not re-homed") and design §4.2.1 require. This
   crate is their only implementer and its worker needs a write contract, so keeping
   them is defensible — but it **is** a deviation from R3, and the record said
   otherwise for a while: `MB.36.5/RESULT.md` cited "design §4.2.3's split", which
   authorises no such move. Corrected in that file (deviation 10), in the design
   (§4.2.1/§4.2.3/§7), and guarded by `critical_artifacts`, which refuses the three
   names everywhere except those files and pins the exemption positively.
2. **Deferred coverage.** The audit's `MB.36.8`/`.9`/`.10` were **not** acceptance
   claims of this epic, so they were re-filed as `MB.26.25`/`.26`/`.27` and
   `superseded` here; `MB.36.11` stayed because R12 is in this epic's own title.
   Precedent, recorded in the epic `LOG.md`: **an epic may close with deferred
   coverage, but never with a deferred claim it made.**
3. **Baseline for the invariance replay** is `4aa39d9` rather than `develop`, for the
   reason above; the replay is head-vs-head controlled as well (24 captures, 0 differ).

## Files

Added: `crates/blastit-view-static/**`, `crates/blastit-view-python/**`,
`crates/blastit-view-php/**`, `crates/blastit-sys/src/child.rs`,
`crates/blastit-sys/tests/critical_child_sequence.rs`,
`crates/blastit-view/src/{path,spawn,stream}.rs`,
`crates/blastit-scheduler/tests/static_feature_off.rs`,
`crates/blastit/tests/{critical_artifacts,dx_capture,dx_invariance}.rs`,
`e2e/dx_replay.py`, `docs/testing/AUDIT-MB.36.md`, `docs/decisions/ADR-0016-*`.
Deleted: `crates/blastit-view/src/{runtime,php,environ,input,request,response}.rs`,
`crates/blastit/src/{php_view,immortal,zygote}.rs`, `crates/blastit-view/src/path.rs`,
`crates/blastit-scheduler/src/{static_asset,static_response,rewrite}.rs`.

## Follow-ups

Filed as `MB.26.25` (a protected suite for the `view.chan` transport), `.26` (run
each view campaign against the tests that cover it), `.27` (the `blastit-view-static`
survivors), `.28` (the `loaded.rs` survivors), `.29` (the static open flags). Known
and out of this epic's paths: `scripts/build-wheel.sh`'s stale feature comment,
`docs/design/backend-runtimes.md` §7.1's two absent test-file names, and the two
`Cargo.toml` comments that still say `blastit-view` defaults `python`.
