# MB.56.3.2.1 — `e2e_dev.rs` part 1: Python WSGI multi-view, host rules, venv activation

## What changed

`crates/blastit/tests/e2e_dev.rs` holds the Python WSGI half of
`TESTING_PLAN.md` §2 and §4 against the real `CARGO_BIN_EXE_blastit`, with the
shared harness at the top of the file (`Project` temp root under
`/tmp/opencode`, `free_port()` from `28080+`, `Dev` boot collecting both streams,
deadline-based `wait_ready`/`wait_event`, `run_dev` bounded refusals) so
`MB.56.3.2.2` (PHP) and `.3` (static/reload/learn/shutdown) append fixtures and
tests below it. Every boot runs under `env_clear()` with only
`LD_LIBRARY_PATH`/`HOME`/`TMPDIR`/`PATH` and the `BLASTIT_*` a case names;
`BLASTIT_MEM_HIGH_WATERMARK=100` keeps a loaded host from turning a WSGI story
into the documented memory refusal.

Three review fixes changed the harness since the first pass:

- **one boot at a time** (`boot_lock`, whose slot `Dev::stop` releases): eight
  parallel boots starve the timing-sensitive reload suites that share this
  `cargo test` run, and one at a time is also what keeps the parallel leg's clock
  honest;
- **`EADDRINUSE` is retried, never blamed** (`PORT_ATTEMPTS`, both in `Dev::wait`
  and in `run_dev`): a port taken between `free_port()`'s probe and the child's
  `bind` relaunches with a fresh port, the rule
  `critical_dev_and_fail_closed.rs` already follows (`MB.21.4`);
- **one copy of the shared client and calibration**: `tests/common/views.rs`
  (`WIDTH`, `CONTROL_FLOOR`, `CONTROL_MARGIN`, `Response`, `request`, `Client`,
  `burst`, `assert_200`) is included with `#[path]` by `e2e_dev.rs` **and**
  `critical_parallel_views.rs`, whose PHP legs now read the same numbers. The
  absolute wall-clock ceiling (`PARALLEL_BUDGET = 450 ms`) is gone from both: the
  self-calibrating control margin (and, for WSGI, the app's own `t0` span) already
  prove the promise, and the ceiling only capped the host's speed.

The shared PostgreSQL peer is `tests/common/pg_upstream.rs`, also included with
`#[path]`, so the database legs are two real processes over a real v3 wire.

New focused suites (both justified in their crate docs, both mapped to
`TESTING_PLAN.md` §6/§2):

- `crates/blastit-sys/tests/pin_identity.rs` — the live descriptor-identity rule
  (`pin.rs:62-63` plus the `fstat` comparison `stage1/adopt.rs:316-328` makes): a
  replaced path is refused with the documented sentence (both inodes, the fix) and
  a real `dup2` of another file under the pinned descriptor reports the new inode.
- `crates/blastit-py-sapi/tests/discovery_sentences.rs` — the two `Serve`
  discovery sentences no unprivileged boot can reach (`is not an absolute path`,
  `is not an executable file`) with an accepted twin.

`crates/blastit-py-sapi/src/discovery/mod.rs` now owns the `libpython` search list
(`libpython_directories`, `system_libpython_directories`, `libpython_file_names`,
`system_has_libpython`), and `e2e_dev.rs` asks it instead of copying the order, so
a host that ships **both** supported `libpython`s skips that leg loudly instead of
panicking. No behaviour changed: `libpython()` walks the same list.

Deleted (absorbed): `critical_python_shim.rs`, `python_activation_gate.rs`,
`embed_venv_sys_path.rs`, the WSGI half of `critical_parallel_views.rs` (its
`CONTENTION_SPAN`, `WSGI_MANIFEST`, `WSGI_APP`, `wsgi_answer`,
`assert_wsgi_burst`, `serialized_then_parallel_wsgi`, and the helpers now shared
through `common/views.rs`; the PHP legs stay), and the `[[test]]
critical_python_shim required-features = ["python"]` stanza in
`crates/blastit/Cargo.toml` — a `[[test]]` naming a deleted file makes cargo
refuse the manifest.

Docs corrected (second pass): `docs/reference/log-events.md`'s `runtime`
`environment` cell now says what the binary emits (the environment actually
serving); `docs/reference/settings.md` documents `BLASTIT_PYTHON`; and
`docs/troubleshooting.md` has rows for the missing shared `libpython`, an unusable
`BLASTIT_PYTHON`, an unsupported minor and a free-threaded build.

No production logic changed.

## Old file → new test map

| Deleted case | New owner |
|---|---|
| `critical_python_shim::the_one_binary_serves_wsgi_under_both_supported_minors` | `e2e_dev::a_multi_view_wsgi_project_runs_only_its_trait_grants_on_both_minors` (both legs: 3.13 **and** 3.14, asserted to run, no `served >= 1` slack) |
| `python_activation_gate::a_python_boot_with_the_same_unreadable_interpreter_is_refused` | `e2e_dev::an_unusable_blastit_python_names_the_variable_the_value_and_the_fix` (an **absolute** path, the shape the deleted suite used, **and** a relative one; verbatim sentence naming the variable, the value and the absolute `.venv` fix) |
| `embed_venv_sys_path::a_module_that_exists_only_in_the_project_venv_is_served` | `e2e_dev::a_module_that_only_the_project_venv_provides_is_imported` (module exists **only** in `.venv`'s site-packages) |
| `critical_parallel_views::serialized_then_parallel_wsgi` | `e2e_dev::a_threaded_wsgi_route_serves_four_requests_at_once` (control + measured leg in one function, `BLASTIT_MAX_WORKERS=1` control, app `t0` span ≤ 100 ms, one process via `process.spawned`; no absolute millisecond ceiling) |
| `critical_python_shim::stage_1_refuses_a_python_descriptor_whose_identity_moved` | `blastit-sys/tests/pin_identity.rs` (see `## The restored descriptor witness`) |

New cases the inventory of `MB.56.2.1`/`MB.56.2.2` demanded and nothing covered
end-to-end before:

| Case | Test |
|---|---|
| §2: shared `trait` grants across views, parameterized `$1` table/column read **and** write, on 3.13 + 3.14 | `a_multi_view_wsgi_project_runs_only_its_trait_grants_on_both_minors` (two views, neither with a grant of its own; prepared statements only) |
| §2/§6: an ungranted statement and an ungranted column refused **in each view** with the documented denial line; a denied statement never reaches the upstream; the proxy authenticates with the operator's own user | same test (`sql.denied  <view>  db  parse  SELECT … = $?` for `read` **and** `write`, `42501` in the request's error, no literal value in any denial line, upstream statement log) |
| §2/§6: **zero** database credentials in a worker's environment | same test (each view's whole `os.environ`: no password, no `real-user`, no `BLASTIT_*`) |
| §2 `docs/local.md`: a denial fails one request and leaves **every other view untouched** | same test (the denied view still serves its granted statement, the sibling still answers `200`) |
| §2: CPython 3.12 refused verbatim (`… is not supported for WSGI views (CPython 3.13 and 3.14 are supported…)`) | `a_wsgi_boot_refuses_cpython_3_12_verbatim` |
| §2: a free-threaded build refused by name | `a_free_threaded_interpreter_is_refused_by_name` |
| §2: no shared `libpython` — the sentence names the minor and **every** remedy (apt/apk/dnf/uv); skips loudly on a host that ships both supported minors | `a_prefix_without_a_shared_libpython_is_told_every_remedy` |
| §4: `$VIRTUAL_ENV` that is not a venv is skipped (never a refusal) and the `runtime` line names the venv actually served | `a_stale_activated_venv_is_skipped_and_the_boot_names_what_it_serves` |
| §2: the two `Serve`-only `BLASTIT_PYTHON` sentences (`is not an absolute path`, `is not an executable file`) plus an accepted twin | `blastit-py-sapi/tests/discovery_sentences.rs` |

## The restored descriptor witness

The first pass deleted `stage_1_refuses_a_python_descriptor_whose_identity_moved`
and claimed `critical_dev_and_fail_closed` covered its stage-0 side. **That claim
was false**, and it is withdrawn: nothing exercised the identity rule.

The rule now has a witness again, in `crates/blastit-sys/tests/pin_identity.rs`,
in the two shapes a boot actually reaches:

1. **`pin.rs:62-63`** (the pin `stage0/fork_exec.rs` takes for every Python boot):
   the path is replaced between inspection and the pin, and the refusal is the
   documented sentence with both inodes and `restore the file and restart blastit`.
   The descriptor pinned before the swap still names the inspected object, and
   pinning the current object with the current identity is the accepted twin.
2. **`stage1/adopt.rs:316-328`** (the re-check on the descriptor stage 0 handed
   over): a real `dup2` of a second file onto the pinned descriptor number makes
   the live `(st_dev, st_ino)` differ from the record — the boot's `fstat`
   comparison, produced with real descriptors and no configuration document.

This is a §6 story ("tampered application entry files" / fail-closed runtime
integrity) and it is **not reproducible end-to-end**: stage 0 pins and stage 1
re-checks inside one `execve` chain whose only entry point is the internal
`--stage=1 --config-fd` grammar, which is why the deleted test had to hand-build a
`blastit::config::Config` (a test constructing production config types). The
focused suite drives the same rule with real files instead.

## Deliberately not re-proved here (and where they went)

- `critical_python_shim::a_php_only_boot_never_opens_libpython`,
  `python_activation_gate::a_php_only_boot_never_reaches_the_python_discovery`
  → `MB.56.3.2.2` (PHP half, same file).
- `critical_python_shim::a_static_only_boot_never_opens_libpython`,
  `python_activation_gate::a_static_only_boot_never_opens_libpython`
  → `MB.56.3.2.3` (static half, same file).
- `critical_python_shim::a_dev_cycle_keeps_serving_through_the_pinned_descriptors`
  → `MB.56.3.2.3` (live reload is that slice's story).
- `python_activation_gate`'s "production boot ignores an activated venv": only a
  `serve` boot can show it and an unprivileged `serve` is refused **before**
  `Config::from_env` (`MB.1.6` lesson), so no E2E leg exists without root. The two
  `Serve` discovery sentences are asserted by
  `blastit-py-sapi/tests/discovery_sentences.rs` instead.
- An activated `$VIRTUAL_ENV` that *is* a venv **wins** over `<manifest>/.venv`
  (design §2.2, `dev`/`check` only), so "skipped" above is read exactly as the
  code reads it: `$VIRTUAL_ENV` is consulted only when `pyvenv.cfg` makes it a
  venv, and the boot's `runtime` line is the on-record statement of which
  environment it serves. A test asserting the opposite would be asserting a rule
  the design does not have.
- `crates/blastit-py-sapi` had **zero** tests before this slice (its inline
  `#[cfg(test)]` modules went in `MB.56.2.2`); the first pass's claim that the
  discovery sentences "remain pinned" there was wrong. `discovery_sentences.rs`
  is that pin now.

## LOC

The node's soft target was ≲ 800 lines for `e2e_dev.rs`. It is 1 504: about 430
are the harness `.2`/`.3` reuse (no test in it), ~300 the multi-view fixture (a
dependency-free prepared-statement PostgreSQL client in the WSGI app plus its
manifest and assertions), and the rest the four verbatim refusal sentences, the
two venv rules and the parallel proof. The client, the burst and the calibration
now live once, in `tests/common/views.rs` (126 lines), shared with the PHP legs.
Trimming further would mean dropping coverage the node requires, so the number is
reported rather than met.

## Evidence

```
$ cargo test -p blastit --test e2e_dev
running 8 tests … test result: ok. 8 passed; 0 failed; 0 ignored

$ cargo test -p blastit-sys --test pin_identity
running 2 tests
test a_descriptor_swapped_under_the_pin_reports_the_new_inode ... ok
test a_path_replaced_after_inspection_is_refused_with_both_inodes ... ok
test result: ok. 2 passed; 0 failed; 0 ignored

$ cargo test -p blastit-py-sapi --test discovery_sentences
running 2 tests … test result: ok. 2 passed; 0 failed; 0 ignored

$ cargo test -p blastit --test critical_parallel_views --features python
running 6 tests … test result: ok. 6 passed; 0 failed; 0 ignored   # the PHP legs, on the shared client

$ cargo clippy -p blastit --all-targets --features python -- -D warnings          # 0 warnings
$ cargo clippy -p blastit --all-targets --no-default-features -- -D warnings      # 0 warnings
$ cargo clippy -p blastit-sys --all-targets -- -D warnings                        # 0 warnings
$ cargo clippy -p blastit-py-sapi --all-targets -- -D warnings                    # 0 warnings

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.45s

$ make fast-check
guard: OK (5 staged/untracked file(s) checked)
cargo fmt --all -- --check                     # 0 diffs
workspace clippy --features python -D warnings # 0 warnings
cargo doc (RUSTDOCFLAGS=-D warnings)           # 0 warnings
75 × `test result: ok`; 0 × FAILED; 0 × `warning:`; 0 × `error`
ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 …; no RPATH/RUNPATH
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
exit 0

## Follow-ups

- `MB.56.3.2.2`/`.3` append below the harness; the harness has no
  request-with-body helper yet (PHP POSTs and `--learn` probes will want
  `request(method, path, body)`), and `views::Client::request` is the place.
- `critical_parallel_views.rs` now imports `common/views.rs`; the next slice that
  owns its PHP legs should move its remaining `Boot` harness onto the same
  one-copy rule (its port picker still binds an ephemeral port without the
  `EADDRINUSE` retry that `e2e_dev.rs` has).
- `e2e/venv-smoke/wsgi.py:17` still names `critical_python_shim` in a comment
  (outside this node's paths); it is prose only.
- `docs/design/parallel-views.md` mentions `serialized_then_parallel_wsgi`,
  which is now `a_threaded_wsgi_route_serves_four_requests_at_once` (design docs
  are historical records; no code reads the name).
- `make fast-check` once saw `critical_dev_reload::a_queue_overflow_reloads_every_view_once_and_logs_one_line`
  fail under full-workspace parallelism; it passes alone and on later full runs,
  and this file now serializes its boots, which removes the contention it added.
