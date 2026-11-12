# RESULT MA.2.3.2 — Leaf 9: CLI grammar, CWD manifest discovery, presets and seccomp gate

Status: **done** (attempt 2/3)

## What changed

### `crates/blastit-events`
- `Lifecycle` gains `build: Option<String>`, `Lifecycle::with_build(...)` and
  `Lifecycle::build()` (`R23`).
- `json.rs` writes `"build"` only when it is set and decodes it back; `pretty.rs`
  renders `build=<version>`; the size-budget slot list can cut it like any other
  string.
- `crates/blastit-events/tests/lifecycle_build.rs` (new) proves JSON/pretty
  round-trips, absence on `stage0`/`config`/`ready`/`shutdown`, absence of any
  `port` field, and the `privilege: "unprivileged-dev"` spelling (`R22`).

### `crates/blastit/src/config.rs`
- `./blastit.toml` is the manifest when `BLASTIT_MANIFEST` is unset; **zero**
  upward search; a set-but-empty `BLASTIT_MANIFEST` fails validation (`R6`, `R9`).
- `app`/`preload` fall back to the manifest's top-level values; `var_os`
  presence is what decides (`BLASTIT_APP=""` and `BLASTIT_MANIFEST=""` fail;
  `BLASTIT_PRELOAD=""` is `[]`) (`R7`, `R9`).
- `Config::from_env_with_format(preset, default_log_format, policy)` supplies the
  per-command default — `json` for `blastit`, `pretty` for `dev`/`check` — and
  takes the preset policy explicitly (`PresetPolicy::{Env, IgnoreEnv}`); an
  explicit `BLASTIT_LOG_FORMAT` still wins (`R10`). `Config::for_dev()` and
  `Config::for_check()` are the `IgnoreEnv` constructors (`ADR-0005`).
- `Privilege::lifecycle` attaches `build = env!("CARGO_PKG_VERSION")` for the
  `preload` lifecycle only, so the real stage-1 preload `boot` carries it without
  touching `stage1.rs` (`R23`).
- `BLASTIT_SECCOMP=0` fails (`BLASTIT_SECCOMP: … preset …`) when any preset is
  active, and appends `Warning::SeccompDisabled` otherwise (`R18`).
- Preset precedence remains `built-in < preset < manifest < set env < CLI`;
  `PresetChoice` continues to report `supplied`/`overridden` via `var_os`.

### `crates/blastit/src/cli.rs`
- `Command::{Serve, Dev, Check}` and the normative dispatch order: `dev` first,
  then `check`/`test`, then the unchanged stage-0/stage-1/`--role=dbproxy` loop.
- `dev`: `--port N`/`--port=N` (`1..=65535`, default `8080` or `PORT`),
  `--host ADDR`/`--host=ADDR` (default `127.0.0.1`), `--learn`, `-h/--help`;
  `--manifest`, `--preset`, bad ports/IPs and unknown words exit `1`, stderr
  only, stdout empty. `BLASTIT_PRESET` is never read (`R2`).
- `check`/`test`: split on `--` first, reject forwarded
  `--blastit-manifest`/`--blastit-live-server`; `--json`, `--learn`,
  `--dry-run` (only with `--learn`), help; `--manifest`, `--preset`, unknown
  flags and bad combinations exit `2`, stderr only, stdout empty (`R3`).
- Top-level `--help` lists `blastit`, `blastit dev`, `blastit check` and every
  preset; there is no `--version` (`R1`).
- Explicit seams: `run_dev` (MA.2.3.3) and `run_check` (MA.2.3.5) print a
  clearly-marked "not wired yet" message and return `2`; no stub module or
  supervisor was created in another node's paths.

### `crates/blastit/src/lib.rs`
- `pub mod config;` so the protected integration test can call the real
  `Config::from_env*` instead of re-implementing discovery.

### `crates/blastit/tests/critical_config_and_presets.rs` (new, protected)
- CWD discovery with no upward search.
- Set-but-empty `BLASTIT_APP`/`BLASTIT_MANIFEST` failures and
  `BLASTIT_PRELOAD=""` -> `[]`.
- Both presets' four values, `preset_overridden` including an empty override.
- `build == CARGO_PKG_VERSION` on the preload lifecycle only.
- `--json`/`--pretty` default and explicit `BLASTIT_LOG_FORMAT`.
- `BLASTIT_SECCOMP=0`: preset abort vs. `Warning::SeccompDisabled`.
- Table-driven `cli_matrix` over every required rejection, asserting exact exit
  code, stderr text and `stdout == b""`, plus the three help screens and
  `--version` refusal.
- Every env/CWD-touching test holds one global `Mutex`.

## Acceptance evidence

```console
$ cargo test -p blastit --test critical_config_and_presets --locked
...
running 9 tests
test a_boot_event_round_trips_the_build_and_never_a_port ... ok
test a_set_but_empty_variable_wins_over_the_manifest ... ok
test a_set_variable_overrides_a_preset_even_when_empty ... ok
test both_presets_supply_their_four_values ... ok
test cwd_manifest_is_discovered_with_no_upward_search ... ok
test log_format_defaults_to_json_for_serve_and_pretty_for_dev_and_check ... ok
test seccomp_zero_aborts_with_a_preset_and_warns_without_one ... ok
test top_level_help_lists_the_three_commands_and_the_presets ... ok
test cli_matrix_rejects_each_bad_command_line ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

```console
$ cargo test -p blastit-events --locked
...
     Running unittests src/lib.rs (target/debug/deps/blastit_events-9743111e27c1251a)
test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
     Running tests/lifecycle_build.rs (target/debug/deps/lifecycle_build-045bdc53b36c64ab)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ make check
guard: OK (4 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 35 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
... cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.31s
... cargo doc --workspace --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.43s
... cargo test --workspace --all-targets --locked
test result: ok. 132 passed; 0 failed; 1 ignored; ...
test result: ok. 9 passed; 0 failed; 0 ignored; ...   (critical_config_and_presets)
test result: ok. 31 passed; 0 failed; 0 ignored; ...  (smoke)
...
cargo test -p blastit --test critical_config_and_presets
running 9 tests ... test result: ok. 9 passed; 0 failed
cargo build --locked -p blastit-py -p blastit-dbproxy
python -m unittest discover -s python/tests/critical
Ran 19 tests in 0.141s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
40 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups (out of scope here)

1. **MA.2.3.3** must wire `run_dev` to the unprivileged supervisor and construct
   its configuration with `Config::for_dev()` (`PresetPolicy::IgnoreEnv`,
   `LogFormat::Pretty`, no preset): `BLASTIT_PRESET` must stay unread, so an
   exported shell preset cannot flip dev's log format or trip the seccomp gate.
2. **MA.2.3.5** must replace the `run_check` seam with `check::run` and use
   `Config::for_check()` (the same policy: `IgnoreEnv`, `Pretty`).
   `Config::from_env_with_format(preset, format, policy)` is the shared core;
   `from_env` is production's `PresetPolicy::Env` entry point, and neither
   `dev` nor `check` may call it (or pass `Env` to the core).
3. `Config::from_env` now reads `./blastit.toml` for the `app`/`preload`
   fallback; stage 0 still re-reads and validates the same file afterwards.
   A later leaf may collapse that into one read if the double read becomes
   measurable.
4. `pub mod config` is needed by the protected integration test; if the crate's
   public surface is later trimmed, replace it with a narrowly-scoped accessor.
5. `run_dev`/`run_check` placeholders deliberately return `2` with a message
   naming the leaf that fills them; no `check.rs` or supervisor code was added.
6. The production `BLASTIT_SECCOMP=0`-with-a-preset abort is asserted
   in-process (`ConfigError` naming the variable and the preset) because a
   process-level form is unreachable from an unprivileged suite: stage 0
   refuses on init's capability sets *before* it reads any configuration
   (`blastit_sys::caps::detect`, §4/D6). On a root/capable host the real
   binary reaches the gate and exits `1` naming `BLASTIT_SECCOMP`; the
   protected suite keeps the in-process message assertion and, in the same
   test, asserts the real process never reaches `ready`.

## Review

Goldfish re-run at base `aa401fd`: `cargo test -p blastit --test critical_config_and_presets --locked` -> 9 passed;
`cargo test -p blastit-events --locked` -> 75 + 5 + 1 doc passed; `make check` -> OK (first run hit the sibling
`target/python-critical` staging race, two later runs green); `critical_guard.py --base aa401fd --node MA.2.3.2` ->
"no change to a protected critical test"; `guard.py` -> OK; `plan.py validate` -> plan OK. `check.rs` absent,
`stage0/stage1/supervise/zygote/child` untouched, no event carries a port. Verdict: **FAIL**.

1. **[MAJOR] `crates/blastit/src/cli.rs:258-266` (also `:356`, `RESULT.md` follow-up 1).** The shipped help/reject text
   says "`blastit dev` ignores BLASTIT_PRESET", and the follow-up tells MA.2.3.3/5 to call
   `Config::from_env_with_format(None, …)`. That call goes through `preset_choice(None)` (`config.rs:1974-1995`), which
   reads `BLASTIT_PRESET`: the crate's own `an_explicit_variable_overrides_the_preset` (`config.rs:3601-3620`) asserts
   `Config::from_env(None)` applies `BLASTIT_PRESET=compose-traefik`. An exported shell preset would therefore set
   `config.preset`, replace dev's `pretty` default with the preset's `json`, and — with `BLASTIT_SECCOMP=0` — abort
   `blastit dev` through the new gate (`config.rs:1194-1201`), contradicting ADR-0005. Fix: make the preset policy an
   explicit parameter (`PresetPolicy::{Env, IgnoreEnv}`) so dev/check can ignore `BLASTIT_PRESET`, and add the missing
   R2 negative twin (`BLASTIT_PRESET=compose-traefik` + dev/check -> `config.preset == None`, `log_format == Pretty`).
2. **[MINOR] `crates/blastit/tests/critical_config_and_presets.rs:98-105`.** `run_cli` uses `.env_clear()`, so no CLI
   case can exercise env-driven behaviour: the node's "`BLASTIT_SECCOMP=0` with a preset exits `1`" is only asserted as
   an in-process `ConfigError` (`:278`), and `PORT`/`BLASTIT_PRESET` are untestable. Fix: add
   `run_cli_env(&[(&str,&str)])` and assert `status == 1` with `stderr` containing `BLASTIT_SECCOMP` (stdout is *not*
   empty on a config abort — `stage0::fail_early` flushes the stage-0 boot stream there).
3. **[MINOR] `crates/blastit/src/config.rs:2614`.** The unit-test `env()` guard clears neither `BLASTIT_MANIFEST` nor
   CWD, so `Config::from_env` now reads `./blastit.toml` from whatever directory `cargo test` was invoked in; a stray
   manifest there silently changes dozens of unit tests' `app`/`preload`. Fix: point `BLASTIT_MANIFEST` at a
   non-existent fixture inside the guard. (The protected suite does isolate correctly.)
4. **[MINOR] `crates/blastit/tests/critical_config_and_presets.rs:447`, `cli.rs:370-403`.** `stdout.contains("blastit")`
   is trivially true; and `--port 0`, `--port=9000`, `--host=127.0.0.1`, `check --learn --dry-run` have no grammar
   twin (the stubs return `2`, so subprocess positives are impossible). Fix: add `#[cfg(test)]` unit cases:
   `parse_dev`/`parse_check` asserting `DevOptions`/`CheckOptions` and the `--port 0` rejection; drop the
   redundant `"blastit"` assertion.

## Attempt 2

All four findings fixed; every `# Acceptance` command re-run green. No assertion weakened, no
file outside the node's paths touched.

### 1. [MAJOR] Preset policy is now explicit (`ADR-0005`, R2/R3)

- `config.rs` gains `pub enum PresetPolicy { Env, IgnoreEnv }`, and
  `Config::from_env_with_format(preset, default_log_format, policy)` now **requires** it, so no
  caller can read `BLASTIT_PRESET` by accident.
- `Config::from_env` stays the production entry point (`PresetPolicy::Env`) for stage 0; new
  `Config::for_dev()` and `Config::for_check()` construct with `PresetPolicy::IgnoreEnv` and
  `LogFormat::Pretty`. `preset_choice(flag, policy)` never calls `variable("BLASTIT_PRESET")`
  under `IgnoreEnv`, so a padded, empty, unknown or non-UTF-8 value cannot reach `dev`/`check`.
- The protected suite's R2/R3 negative twin `dev_and_check_ignore_an_exported_preset`: with
  `BLASTIT_PRESET=compose-traefik` **and** `BLASTIT_SECCOMP=0` exported, `for_dev()`/`for_check()`
  yield `preset == None`, `log_format == Pretty`, `seccomp == false` plus
  `Warning::SeccompDisabled` and no `PresetFlagWins`; the same environment through
  `Config::from_env(None)` is the documented abort naming `BLASTIT_SECCOMP` and the preset. With
  `BLASTIT_PRESET=compose-trafik` (unknown) the dev/check configs remain `Ok`, while production
  refuses it naming `BLASTIT_PRESET`.
- `RESULT.md` follow-ups 1-2 now name the new entry points: MA.2.3.3 calls `Config::for_dev()`,
  MA.2.3.5 calls `Config::for_check()`, and neither may call `from_env*` (the `Env` policy).

### 2. [MINOR] Real-process env assertions

- `Sandbox::run_cli_env(&[(&str, &str)], args)` added; `run_cli` delegates to it with `&[]`.
- New `an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause` asserts the real
  binary's exit `1` and stderr naming `PORT` and the value for `PORT=70000 blastit dev --learn`,
  and that `BLASTIT_PRESET=compose-traefik` + `BLASTIT_SECCOMP=0` exits `1` and never reaches
  `ready`; when that boot reaches the configuration it must name `BLASTIT_SECCOMP`.
- **Why the process-level `BLASTIT_SECCOMP` message is conditional:** stage 0 refuses on init's
  capability sets (`blastit_sys::caps::detect`) *before* it reads any configuration, and writes its
  events to **stdout** (JSON), not stderr. Probe, unprivileged (this suite's environment):
  `BLASTIT_SECCOMP=0 BLASTIT_PRESET=compose-traefik target/debug/blastit` -> exit `1`, stderr
  `0` bytes, stdout ending in the rootless-capability recipe. The same command under
  `unshare -Ur` (a boot that can hold init's capabilities) exits `1` with

  ```
  BLASTIT_SECCOMP: must not be 0 while preset `compose-traefik` is active: remove BLASTIT_SECCOMP or set BLASTIT_SECCOMP=1
  ```

  on that same stdout stream. The exact message is therefore asserted in-process
  (`seccomp_zero_aborts_with_a_preset_and_warns_without_one`, which also checks
  `BLASTIT_SECCOMP` and `preset` in the text), and the process test pins the exit code
  and the never-serving invariant for both hosts. Recorded as follow-up 6.

### 3. [MINOR] `config.rs` unit-test guard

- `env()` now returns an `EnvGuard` that (a) holds the suite mutex and (b) points
  `BLASTIT_MANIFEST` at `crates/blastit/tests/fixtures/absent-blastit.toml` (never created),
  restoring the caller's value on drop. Discovery is deterministic whatever directory
  `cargo test` starts in.
- Evidence: `printf 'version = 1\napp = "stray:application"\npreload = ["stray"]\n' >
  crates/blastit/blastit.toml && cargo test -p blastit --lib --locked` -> `134 passed`,
  then the stray file was removed.

### 4. [MINOR] Grammar twins

- `cli.rs` `#[cfg(test)]`: `the_dev_grammar_is_exactly_the_documented_one` asserts `--port N` and
  `--port=N` (`DevOptions`), `--host=127.0.0.1` and `--host ::1`, `--learn`, the last-value-wins
  rule, and the `--port 0` / `65536` / `soon` rejections;
  `the_check_grammar_is_exactly_the_documented_one` asserts `check --learn --dry-run`, `--json`
  with forwarded `-q tests`, plain `check`, `--dry-run` alone, a forwarded `--dry-run` (still
  forwarded) and the `--blastit-live-server` refusal.
- The trivially-true `stdout.contains("blastit")` is gone: the help test now asserts
  `blastit dev`, `blastit check` and `blastit [--preset NAME]`.

### Acceptance evidence (attempt 2)

```console
$ cargo test -p blastit --test critical_config_and_presets --locked
running 11 tests
test a_boot_event_round_trips_the_build_and_never_a_port ... ok
test a_set_but_empty_variable_wins_over_the_manifest ... ok
test both_presets_supply_their_four_values ... ok
test an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause ... ok
test cwd_manifest_is_discovered_with_no_upward_search ... ok
test cli_matrix_rejects_each_bad_command_line ... ok
test log_format_defaults_to_json_for_serve_and_pretty_for_dev_and_check ... ok
test dev_and_check_ignore_an_exported_preset ... ok
test top_level_help_lists_the_three_commands_and_the_presets ... ok
test seccomp_zero_aborts_with_a_preset_and_warns_without_one ... ok
test a_set_variable_overrides_a_preset_even_when_empty ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```

```console
$ cargo test -p blastit-events --locked
     Running unittests src/lib.rs (target/debug/deps/blastit_events-9743111e27c1251a)
test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
     Running tests/lifecycle_build.rs (target/debug/deps/lifecycle_build-045bdc53b36c64ab)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ make check
guard: OK (6 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 35 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
... cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
... RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Generated .../target/doc/blastit/index.html and 12 other files
... cargo test --workspace --all-targets --locked
test result: ok. 134 passed; 0 failed; 1 ignored; ...      (blastit lib, incl. the new grammar tests)
test result: ok. 11 passed; 0 failed; 0 ignored; ...       (critical_config_and_presets)
test result: ok. 31 passed; 0 failed; 0 ignored; ...       (smoke)
test result: ok. 75 passed; 0 failed; ...                  (blastit-events)
test result: ok. 5 passed; ...                             (lifecycle_build)
...
cargo test -p blastit --test critical_config_and_presets
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
cargo build --locked -p blastit-py -p blastit-dbproxy
python -m unittest discover -s python/tests/critical
Ran 24 tests in 0.913s
OK
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
40 files already formatted
uvx --offline zensical build --strict
No issues found
uv run --no-project tools/plan.py validate
plan OK
```


## Re-review 2

Fresh-context goldfish re-review at base `aa401fd`. Verdict: **PASS**.

- Re-ran: `cargo test -p blastit --test critical_config_and_presets --locked` -> 11 passed;
  `cargo test -p blastit-events --locked` -> 75 + 5 + 1 doc passed; `make check` -> exit 0
  (fmt/clippy/doc/tests/python/ruff/zensical/plan all green); `critical_guard.py --base aa401fd
  --node MA.2.3.2` -> "no change to a protected critical test"; `guard.py` -> OK;
  `plan.py validate` -> plan OK.
- Findings 1-4 re-reproduced from disk, not from the write-up:
  1. Preset policy genuinely fixed. Independent compile-against-the-crate probe in a manifest-free
     cwd with `BLASTIT_PRESET=compose-traefik`: `for_dev()` -> `preset=None log=Pretty
     seccomp=true`; with `BLASTIT_SECCOMP=0` -> `preset=None log=Pretty seccomp=false
     warnings=[SeccompDisabled]` (no abort); `from_env(None)` -> `preset=Some("compose-traefik")
     log=Json`, and the same env aborts naming `BLASTIT_SECCOMP`/the preset; an unknown
     `compose-trafik` is ignored by `for_dev()` and refused by `from_env`. Production stage 0 still
     calls `Config::from_env` (stage0.rs:408) under `PresetPolicy::Env`, and
     `from_env_with_format` requires the policy, so no caller can omit it.
  2. Real-process twin exists (`run_cli_env`): `PORT=70000 blastit dev --learn` -> exit 1, empty
     stdout, stderr names `PORT` and `70000`; `BLASTIT_PRESET`+`BLASTIT_SECCOMP=0` -> exit 1, no
     `ready`. The ordering caveat is genuine: `blastit_sys::caps::detect()` (stage0.rs:352) returns
     `Missing` on this unprivileged host and `fail_early` (stage0.rs:365) exits before
     `Config::from_env` (stage0.rs:408); reproduced: exit 1, 0 bytes stderr, stdout names the
     missing capabilities. Accepted.
  3. `config.rs` unit guard pins `BLASTIT_MANIFEST` to an absent fixture; `cargo test -p blastit
     --lib` with a hostile outer `BLASTIT_MANIFEST` -> 134 passed; no unit test calls
     `Config::from_env` without the guard, and no unit test depends on the `./blastit.toml` default.
  4. Grammar twins exist (`--port 0`/`65536`/`soon`, `--port=`, `--host=`, `--host ::1`,
     `check --learn --dry-run`, forwarded `--`), and the trivial `stdout.contains("blastit")` is gone.
- Scope: `crates/blastit/src/check.rs` absent; stage0/stage1/supervise/zygote/child untouched; no
  event field carries a port; `--version` still exits 1; no `#[ignore]`/weakened assertion added.
- New MINOR (non-blocking, next leaf or a micro-fix):
  1. `crates/blastit/tests/critical_config_and_presets.rs:547` — `if combined.contains(r#""stage":"config""#)`
     can never be true: `config_lifecycle` is emitted only *after* `Config::from_env` succeeds
     (stage0.rs:420), and `fail` writes bare `app.output` lines. On a capable host this process test
     would still pass with the seccomp gate removed. Fix: assert the abort text unconditionally once
     the capability advice is absent, or drop the branch and rely on the in-process assertion.
  2. `crates/blastit/src/cli.rs:253,346` — `--manifest=x` / `--preset=x` fall through to
     "unknown argument `--manifest=x`", while the space forms give the actionable "put
     `blastit.toml` in the working directory" text. Fix: `strip_prefix("--manifest=")` /
     `strip_prefix("--preset=")` before the unknown-argument arm. Also `cli.rs:382` doc reads
     "when neither `--port` nor its environment spelling is absent" (double negative).
