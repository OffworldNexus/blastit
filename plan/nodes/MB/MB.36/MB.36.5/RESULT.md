# Result MB.36.5

`blastit-view-php` exists and owns every PHP internal; `blastit-view` is transport
only; `WorkerRuntime`/`runtime.rs` are gone; `stage0.rs` lost 1500 lines of PHP and
kept its 27 `php_boot_gate` cases, retargeted at the crate's public API.

## Move 1 — `crates/blastit-view/src/php.rs` (2559) → `crates/blastit-view-php/src/worker.rs` (2798)

`PhpEmbedRuntime`, `PhpRuntimeConfig`, `serve_forever`, `is_serveable_entry`.
Per `ADR-0016` decision 2 (design §8.4):

- **`PinnedEntryRuntime` deleted**, a recorded deviation from `MB.36.5`'s Notes.
  The entry pin is now an opaque (private) `pin: EntryPin` field of
  `PhpRuntimeConfig`, built by `PhpRuntimeConfig::new(binding, script_name, pin)` —
  no `Option`, no `Default`, not readable back out. `serve` re-checks it before the
  body is read and before `libphp` opens `SCRIPT_FILENAME`, answering an empty `500`
  with the sentence on stderr (the adapter's behaviour, now the runtime's own).
- `PhpRuntimeConfig` keeps 7 pub fields + the private pin; the two in-file cases
  that built one by struct literal use a `config(...)` fixture that pins the file
  the way a boot does (assertions unchanged).
- `serve_forever(chan, runtime: PhpEmbedRuntime, limits)` (R3) — concrete, no
  `Arc<dyn …>`. It delegates to a private `serve_loop(chan, limits, serve, recycle)`
  generic over two closures, so the loop's 22 unit cases still drive it with
  stand-ins that need no `libphp`. There is **no** replacement trait anywhere; the
  stand-ins are this module's `#[cfg(test)]` `StandIn` (2 methods, not public).
- `RequestIo`, `RequestEnd` and the `flush_response` contract moved here from
  `runtime.rs` (design §4.2.3's split): this crate is the only implementer.
- The file's `PhpLoadedReport` was **not** carried over: the loaded-file report is
  the transport's `blastit_view::loaded::LoadedReport` (design §4.2.1 says the PHP
  copy is deleted), and `load_reporting`/`PhpFileReport` take that type.

## Move 2 — `crates/blastit/src/php_view.rs` (1851) → `pinning.rs` + `probe.rs` + `spawn.rs`

- `pinning.rs` (1166): `PhpBinding` (+ `route`, `uid`, `script_name`, `privileges`,
  `filter`), `EntryPin`, `boot_bindings`, `open_pinned`, `pin_entry`,
  `verify_entry`, `await_test_swap`; new value types `PhpSlot` and `PhpBootLimits`
  (design §4.4 D2: no `Config`, no `RouteInfo`).
- `probe.rs` (433): `PhpProbeBoot { slots, bindings, limits, libphp, timeout,
  protocol }`, `probe_bindings(probe, forker) -> Result<Option<PhpProbeReport>,
  String>`, `probe_route`, `probe_child`, `load_and_shut_down`, `await_probe`,
  `kill_probe`, `join_group`, the pipe helpers.
- `spawn.rs` (267): `PhpViewSpawn { ctx: SpawnContext, binding, protocol, loaded }`,
  `fork_php_view(spawn, forker)`, `child_main`, `serve`, `set_view_environment`,
  `VIEW_PROCESS_MARGIN`/`VIEW_NOFILE`.
- **`own_process_group` deleted**, not moved: its `setpgid(0, 0)` is inside
  `blastit_sys::child::enter` (D1), before `install_filter`; a verbatim move would
  have run it after the `EmbeddedView` filter and got `EPERM`. `join_group` stays
  with the probe (as the parent's half of the same race).
- `emit_runtime` (a `blastit_events` event) stays in the root — see Deviations.
- The crate is `#![forbid(unsafe_code)]`: the probe's wait/kill/setpgid are now
  `rustix` calls (`waitpid`/`WaitOptions::NOHANG`, `kill_process_group`, `setpgid`),
  the child's `UnixStream` comes from `From<OwnedFd>` and its `HOME`/`TMPDIR` from
  `blastit_sys::child::set_env`. No `unsafe` anywhere (`R17`).

## Move 3 — `stage0.rs`'s PHP code → `discovery.rs` (1901)

`LIBPHP_CANDIDATES`, `LibphpProbe`, `LIBPHP_PROBES`, `PhpShimScope`, `PhpSearch`,
`ZendAbi`, `PhpRefusal`, `PhpBoot`, `PhpSearch::new`, `deployment_refusal`, the
ELF walker, `discover_php`, `resolve`, `first_existing`,
`installed_unsupported`, `version_of`, `path_versions`, `choose_version`,
`shim_name`, `target_tuple`, `abi_tuple`, `php_refusal_message`,
`present_refusal`, `missing_claim`, `php_trees`, `first_php_route`,
`php_probe_refusal`, plus `PhpVersion`/`PhpRuntime` from `config.rs`.

`stage0.rs`: **6566 → 5066 lines (−1500)**. What stayed:

- `php_boot_gate(config, manifest, scope)` — the `Config` half: maps `libphp`,
  `php_shim_dir`, scope into `PhpSearch::new`, and supplies the **two** provenance
  callbacks (`object_is_protected` over `host::Provenance::check_objects()`, which
  keeps the file-capability and `DT_RPATH`/`DT_RUNPATH` halves; `tree_is_protected`
  over `check_trees()`), so `check.rs` keeps compiling unchanged — it calls the
  gate, which still exists in every build. `host.rs` did **not** stay unchanged:
  its `pub use crate::stage0::deployment_boot_check` gained
  `#[cfg(feature = "php")]`, because the seam's body needs the crate that owns
  PHP. A `php`-less build therefore exposes no gate at all, and the refusal it
  owes a manifest is `stage0::no_php_support`, called by `run`, `run_dev` and
  `check.rs`.
- `deployment_boot_check(...)` (the protected suite's seam) and the `#[cfg(test)]`
  `php_boot`.
- The 27-case `php_boot_gate` module: unchanged assertions, `use super::*` now
  finds the crate's names, and a `HostChecks` fixture supplies the two callbacks
  over `crate::host` — the same functions production uses, so a no-op closure
  cannot silently invert the deployment cases.

`Deployment` carries `is_protected_object` **and** `is_protected_tree` as
`&dyn Fn(&Path) -> Result<(), String>`; no `blastit::host` type crosses (`php_trees`
returns `(what, path)` pairs).

## Move 4 — `runtime.rs` deleted, `blastit-view` is transport only

`crates/blastit-view/src/runtime.rs` deleted with the trait and its two cases
(`the_seam_is_object_safe…`, `a_request_writes_head_then_body…` — named in design
§4.2.1). `OutputStream`/`OUTPUT_LINE_CAP` were already in `stream.rs`.
`Cargo.toml` is now `blastit-frame` + `blastit-sys` + `thiserror`, `default = []`,
no `php`/`python` feature, no `pyo3` (not even a dev-dep), no `blastit-php-sapi`.
`Credit::try_take` lost its `cfg(any(feature = "php", test))`. The
`WorkerRuntime::recycle` mention in `blastit-scheduler/src/scheduler.rs` names the
PHP worker's own `recycle` and `crates/blastit-view-php/src/worker.rs`.

## Move 5 — tests

- `crates/blastit-view/tests/php_native.rs` → `crates/blastit-view-php/tests/php_native.rs`
  (261 lines, still `#[ignore]`, still 1 case): rebuilt on a real `PhpBinding`, so
  it now exercises the pin's accepted path and the `Fd` load together.
- `critical_php_{backend,embed,parallel_views}.rs` and
  `tests/common/libphp_probes.rs`: **named edits only** — the four `include_str!`
  paths point at `crates/blastit-view-php/src/discovery.rs` — plus the one added
  leg (below). No assertion changed.
- **The added pin leg** (60 → 61 CLI cases, `critical_php_embed` 24 → 25 of its
  own): `the_worker_cannot_be_built_without_the_entry_pin` — `R6`'s no-pin
  constructor proof, asserted on the crate's source (private non-`Option` field,
  no `Default`, `new(…, pin)` required, `serve` re-checks, no `entry_pin: Option`),
  needs no host PHP and runs in every `make check`.

Counts (`--list`): `critical_php_embed` **28** (25 own + 3 helper cases),
`critical_php_backend` **9** (6 + 3), `critical_parallel_views` **6** by default
and **7** with `--features python` (3 helper cases + 4 own, one of which is
`python`-gated and untouched; `git diff` on that file is exactly its one
`include_str!` line).

## Deviations (recorded, not silent)

1. **`PinnedEntryRuntime` deleted** instead of kept as an adapter — `MB.36.5`'s
   Notes are superseded by `ADR-0016` decision 2 and design §8.4's first
   paragraph. The pin is a required value of the runtime; the per-request refusal
   behaviour is identical (proved by `critical_php_embed`'s inode legs, green).
2. **`probe_bindings` returns the probe report** (`PhpProbeReport`) instead of
   emitting `EventKind::Runtime`. The event is the composition root's: a view crate
   has no `Emitter` and may not build events, so `stage1::emit_php_runtime` emits
   the same line from the returned `(version, ini)`. Same event, same point in the
   boot, asserted by `critical_php_embed`'s boot lines.
3. **`PhpSlot` carries `uid`** (design §4.4 D2's field list omits it): the probe
   child needs the route's uid for its refusal message and `PhpBinding` must carry
   it (D2 says so), so it travels on the slot that builds the binding.
4. **`PhpViewSpawn`/`PhpProbeBoot` carry a `ForkProtocol`**: the fork is injected
   (`Forker`), and `fork_for` *refuses* a protocol that disagrees with the image, so
   the caller must say which image it is. Stage 1/supervise pass
   `InterpreterLive`/`NoInterpreter` from `hosted`/`interpreter_live`.
5. **`set_view_environment` uses `blastit_sys::child::set_env`** (the safe
   `setenv`), which refuses a process with more than one thread. A forked view
   child is one thread, so production is exact; a libtest binary is always two, so
   the re-exec'd pinning case asserts both halves without pretending: the parent
   **seeds** the child's `HOME`/`TMPDIR` (`Command::env`) and the child proves the
   sequence leaves them exactly as handed — the values `set_view_environment`
   writes, read back by the process that will serve — and the child pins
   `set_env`'s threaded *refusal* sentence. The accepted **write** path is the real
   boot's (`critical_php_embed` serves PHP routes whose worker could only reach
   `READY` with `HOME`/`TMPDIR` set) and `MB.36.7`'s e2e PHP scenario re-proves it
   against a shipped artifact.
6. **`#[cfg(feature = "php")]` out of necessity**: `Config::php`, its JSON arms,
   `PhpVersion`/`PhpRuntime` (moved to `discovery`), `php_boot_gate`,
   `deployment_boot_check`, `emit_runtime`'s PHP arm, and a new
   `stage0::no_php_support(manifest)` refusal used by `run`, `run_dev` and
   `check.rs`, so `--no-default-features` compiles and refuses `entrypoint php`
   with one sentence (`R13`; `MB.36.6` owns the `rs-no-default` guard).
7. **`PhpLoadedReport` deleted** (design §4.2.1): the loaded-file report is
   `blastit_view::loaded::LoadedReport` for every backend, so `PhpViewSpawn.loaded`
   is that type, and `supervise`'s crate-local `ViewReportEnd` and both
   field-by-field conversions are gone — `type ViewReport =
   Option<blastit_view::LoadedReport>` is handed to either fork verbatim.
8. **`crates/blastit/tests/critical_php_embed.rs` gained `#![cfg(feature =
   "php")]`**: it drives `entrypoint php` end to end and calls
   `blastit::host::deployment_boot_check`, which exists only with that feature
   (see deviation 6). No assertion changed, and under the default features
   (`php` + `static`) the whole tier still runs: **28 listed cases** (25 of its
   own + the 3 `#[path]`-included `libphp_probes` cases), i.e. the 60 → 61 CLI
   count is unchanged.
9. `discovery`'s public surface is larger than the design's nine names: the kept
   fixtures also need `EM_X86_64`, `EM_AARCH64`, `SHT_DYNSYM`, `SHT_STRTAB`,
   `decode_x86_64`, `decode_aarch64`, `library_dirs`, `target_tuple`,
   `php_refusal_message` — imported, never respelled, so a fixture cannot drift
   from the table it feeds.
10. **`RequestIo`, `RequestEnd` and `flush_response` moved to
    `blastit-view-php/src/worker.rs` instead of being deleted** (errata, added by
    the lead during the `MB.36.11` review). `ADR-0016` decision 2's title is
    "**The worker trait is deleted, not re-homed.**", and design §4.2.1 says this
    leaf deletes those three along with the trait: the tree did not. They survive
    because this crate is their only implementer and its worker still needs a
    write contract, but that is a **recorded deviation from R3**, not design
    §4.2.3's split — the citation this file originally carried was wrong, and
    §4.2.1/§4.2.3/§7 of the design have since been corrected to say so.
    `crates/blastit/tests/critical_artifacts.rs` (`MB.36.11`) is the guard: it
    refuses the three names everywhere except those files and pins the exemption
    positively, so a stale one goes red.

## Acceptance (command + tail)

```console
$ cargo test -p blastit-view --locked
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
exit=0

$ cargo test -p blastit-view-php --locked
test result: ok. 29 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit=0

$ cargo test -p blastit-view-python --locked
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.05s
exit=0

$ cargo test -p blastit --locked
test result: ok. 242 passed; 0 failed; …  (every suite ok; 21 `test result: ok` lines, 0 FAILED)
exit=0

$ cargo test -p blastit --features python --locked
21 × `test result: ok`, 0 × `FAILED`/`error`
exit=0

$ cargo test -p blastit --test critical_php_embed --locked
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.46s
exit=0            # 25 own cases (grep -cE '^#\[(tokio::)?test\]' = 25) + the 3
                  # `#[path]`-included `libphp_probes` cases = 28 listed

$ cargo test -p blastit --test critical_php_backend --locked
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
exit=0

$ cargo test -p blastit --test critical_parallel_views --locked
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.74s
exit=0

$ make critical-guard NODE=MB.36.5
critical-guard: HEAD..working tree, 27 changed file(s)
  M  crates/blastit/tests/critical_parallel_views.rs  [authorized by …]
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by …]
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by …]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.36.5
exit=0

$ cargo tree -p blastit-view -e normal
blastit-view v0.2.0
├── blastit-frame v0.2.0          (→ thiserror)
├── blastit-sys v0.2.0            (→ libc, rustix, thiserror)
└── thiserror v2.0.21
exit=0            # no pyo3, no blastit-php-sapi, no blastit, no scheduler

$ cargo tree -p blastit-view-php -e normal
blastit-view-php v0.2.0
├── blastit-frame v0.2.0
├── blastit-manifest v0.2.0
├── blastit-php-sapi v0.2.0       # the ABI + the one `dlopen`; no link-time libphp
├── blastit-sys v0.2.0
├── blastit-view v0.2.0
├── rustix v1.1.5
├── serde_json v1.0.154
└── thiserror v2.0.21
exit=0            # no pyo3, no `blastit`, no `blastit-scheduler`

$ cargo check -p blastit --no-default-features --features php --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.43s
exit=0            # the `php`-only corner

$ cargo check -p blastit --no-default-features --all-targets --locked
warning: `blastit` (lib) generated 1 warning        # stage1.rs's `let mut routes`
warning: `blastit` (lib test) generated 1 warning (1 duplicate)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
exit=0            # passes: the suite's `#![cfg(feature = "php")]` is deviation 8

$ cargo check -p blastit-view-php --release --locked
    Finished `release` profile [optimized] target(s) in 0.43s
0 warnings        # red before the fix: `warning: unused import: Instant`

$ make rs-no-python
ok   …/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
env -u PYO3_PYTHON -u PYTHONHOME cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.13s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.26s
default build: 0 pyo3 dependencies (no libpython linked)
exit=0

$ make check
…
zed-check: OK (59 properties checked)
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0            # 0 × `test result: FAILED`, 0 × `error`            # rs-fmt, rs-lint (-D warnings), rs-doc (-D warnings), rs-test,
                  # test-critical, py-lint, docs-*, rs-no-python, seam-check, …, plan-validate
```

Design §4.4 D1's `Shared`/`Off` census (the grep is the design's, verbatim):

```console
$ grep -rn -e 'Privileges::Shared' -e 'Filter::Off' \
    crates/blastit/src crates/blastit-view/src crates/blastit-view-*/src
crates/blastit/src/stage1.rs:2341:                Privileges::Shared
crates/blastit/src/stage1.rs:2353:                Filter::Off
```

Both sites are `stage1.rs`'s `view_child_plans`, and there is **no** mention in
any view crate: the one `Shared` arm is the `dev` boot (`check` boots no stage 1
and forks no view — the design counts two *modes* through this single line), and
the one `Off` is the explicit `BLASTIT_SECCOMP=0`. This leaf holds that shape: its
earlier drafts added a doc mention of the shared variant in
`pinning.rs` and a forwarding `Filter::Off => Filter::Off` arm in `stage1.rs`, both
removed (the arm is now `other => other`, which is the same decision forwarded
without a second spelling).

NODE.md's greps:

```console
$ test ! -e crates/blastit-view/src/runtime.rs && echo "trait file deleted"
trait file deleted
$ test -z "$(grep -rn 'WorkerRuntime' --include=*.rs crates/)" && echo "no WorkerRuntime anywhere"
no WorkerRuntime anywhere
$ test -z "$(grep -n 'pyo3\|blastit-php-sapi' crates/blastit-view/Cargo.toml)" && echo "transport crate is language-free"
transport crate is language-free
$ test ! -e crates/blastit-view/src/php.rs && test ! -e crates/blastit/src/php_view.rs && echo "PHP left"
PHP left
$ test -z "$(grep -nE '^(pub(\(crate\))? )?(fn|struct|enum|const) (LIBPHP_PROBES|PhpSearch|php_boot_in|discover_php)\b' crates/blastit/src/stage0.rs)" && echo "discovery left stage0"
discovery left stage0
$ test -z "$(grep -rn 'blastit_view::php' crates/)" && echo "no old module path"
no old module path
```

Line counts and the case oracles:

```console
$ wc -l crates/blastit/src/stage0.rs
 5066 crates/blastit/src/stage0.rs          # 6566 → 5066 (−1500)
$ cargo test -p blastit --lib -- --list | grep -c '^stage0::php_boot_gate::'
27
$ cargo test -p blastit --lib -- --list | grep -cE '^stage0::(app_rule_executors|raw_ingress_gate_tests)::'
8
$ cargo test -p blastit-view-php --lib -- --list | grep -cE '^worker::tests::'      # 22 → 22
22
$ cargo test -p blastit-view-php --lib -- --list | grep -cE '^pinning::tests::'     # ≥ 7
7
$ cargo test -p blastit-view-php --lib -- --list | grep -cE '^discovery::tests::'   # the moved PhpVersion table
1
$ wc -l crates/blastit-view/src/*.rs | tail -1
 2295 total                                  # was 8935 with the two workers
$ wc -l crates/blastit-view-php/src/*.rs | tail -1
 6636 total → 1901 discovery, 2798 worker, 1166 pinning, 433 probe, 267 spawn, 71 lib
                (+ 261 `tests/php_native.rs`, the moved `#[ignore]` harness)
```

`make check`'s own `critical-guard` line is report-only by design (the Makefile's
`critical-guard` target enforces only with `NODE=`, which is the run above); it
prints `report only: 3 breaking change(s) would fail an enforced run` and does not
fail the target.

## Review round 1 — the seven fixes

1. `pinning.rs`'s `assert_eq!(binding.uid, binding.uid, …)` now compares the
   binding's uid against its slot's (`boot.slots[0]…uid`). Proved red once
   (`slot.uid + 1` → `left: 21000, right: 21001`), then restored with `cp` and
   `cmp` (identical) and re-run green.
2. `critical_php_embed.rs` gained `#![cfg(feature = "php")]` (deviation 8); the
   suite still runs 25 own / 28 listed cases under the default features.
3. `pinning.rs`'s `Instant` import is `#[cfg(debug_assertions)]`; the release
   build of the crate went from `warning: unused import: Instant` to 0 warnings.
4. `PhpLoadedReport` deleted (deviation 7): `PhpViewSpawn.loaded` is
   `blastit_view::loaded::LoadedReport`, `supervise`'s `ViewReportEnd` and both
   conversions are gone.
5. The `check.rs`/`host.rs` sentence above is corrected: `host.rs` gained the
   `cfg`, and `stage0::no_php_support` carries the refusal in a `php`-less build.
6. The census above returns exactly the design's sites again.
7. The re-exec'd pinning child asserts the **accepted** `HOME`/`TMPDIR` shape as
   well: the parent seeds both (`Command::env`), the child proves the sequence
   leaves them exactly as handed, and it keeps the `set_env` threaded-refusal
   case. The accepted *write* path is the real boot's (`critical_php_embed`) and
   `MB.36.7`'s e2e PHP leg re-proves it on a shipped artifact (deviation 5).

## Follow-ups

- `docs/knowledge/LESSONS.md` is outside this node's `paths`; proposed line:
  *"`std::env::set_var` is `unsafe` in edition 2024 and `blastit_sys::child::set_env`
  refuses a process with more than one thread — a libtest binary always has two, so
  a `HOME`/`TMPDIR` success path can only be proved from a real forked view child
  (or end to end), never from a `cargo test` process."*
- `scripts/build-standalone.sh`, `ADR-0009`/`ADR-0010`'s feature spelling,
  `ARCHITECTURE.md` §2/§3/§7.3/§9.3 and `Makefile`'s `rs-no-default` are `MB.36.6`'s
  (`php = ["dep:blastit-view-php"]` is already in place here; the archive command
  still says `--no-default-features --features php`).
- A `--no-default-features --all-targets` build now compiles (it is a gate here),
  with **one** warning left in that corner: `stage1.rs`'s `let mut routes`, which
  only the `php` loop mutates. `MB.36.6` should decide whether `rs-no-default` runs
  with `-D warnings`; the clean fix is a `mark_php_slots(routes, &php) -> Vec<_>`
  helper. (`supervise`'s report-struct warning went away with `ViewReportEnd`,
  deviation 7.)
- **DX polish, one sentence, deliberately not changed here**: an object refusal
  reads `route "app": entrypoint php "public/index.php": the NTS libphp: the file
  (/usr/lib/libphp8.4.so) could be replaced by another uid: …`, because
  `discovery::deployment_refusal` prefixes *what the object is* and the
  composition root's callback names the claim again. The fix is a
  `Claim`-shaped `what` on the callback (a signature change on the seam, i.e.
  `MB.36.6` or a later leaf doing it in one place), not a rewrite of the
  formatter.
- `README.md` and `docs/design/portable-lsp.review-{ai-ergonomics,dx}.md` were
  never touched and never staged: `README.md` was already dirty when this leaf
  started, and the two review files are untracked. The leaf's commit names
  explicit paths.
- `plan/BUDGET.md` shows a modified `updated:` timestamp from the plan tooling
  (not an edit of this leaf).
