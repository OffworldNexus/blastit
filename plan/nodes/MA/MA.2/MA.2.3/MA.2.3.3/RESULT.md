# Result MA.2.3.3 — Leaf 10: unprivileged dev supervisor, execve barrier and `dev --learn`

Status: **done** (attempt 5).

## Summary

`blastit dev` is now a real, genuinely unprivileged five-role supervisor instead of a seam that
refused to start:

- `cli::run_dev` calls `stage0::run_dev`, which reads `Config::for_dev()` (never `BLASTIT_PRESET`,
  `pretty` default), prints the exact 3-line banner (§1.2), emits `stage0` `boot` with
  `"privilege":"unprivileged-dev"`, and refuses a non-loopback `--host` without
  `BLASTIT_DEV_EXPOSE=1` (exit 1, empty stdout) or with a loud `WARN` naming `<host>:<port>` and
  the euid.
- `ExecutionMode::Dev` runtime: ephemeral `0700`
  `$XDG_RUNTIME_DIR/blastit-dev-<pid>-<nonce>` (verified directory owned by the caller, not a
  symlink) or `/tmp/blastit-dev-<uid>-<pid>-<nonce>`, with `<root>/views/<idx>/` `0700` per route
  and `.s.PGSQL.<port>` `0600`/`SO_PASSCRED` only for granted proxies (all declared proxies under
  `--learn`). Children get `umask 077`, `RLIMIT_NOFILE`, `RLIMIT_CORE=0`, `PR_SET_PDEATHSIG`,
  `PR_SET_NO_NEW_PRIVS` and real per-role seccomp honouring `egress`; no `setresuid`/`setgroups`
  and no per-UID `RLIMIT_NPROC` lowering. The stage-0 `execve` passes only the app environment, so
  `[proxy.*]` secrets, every `BLASTIT_*` and every `BLASTIT_ENV_STRIP` name are gone; the root is
  removed on a clean `SIGTERM`/`SIGINT` and on a component death.
- `dev --learn`: dbproxy is forked in `ListenerMode::Learn` with `obs_fd` + `check_ctl_fd`; stage 1
  drains `WireSqlObs` in its poll loop, attributes statements to the route whose view is alive, and
  on clean shutdown writes routes/exact `queries` through `blastit_manifest::edit::{apply_edits,
  write_atomic}`. Refused statements are printed (redacted) and turn the exit code into `1`.
  `413` and declared `[route.ingress]` codecs remain enforced by the scheduler.
- Protected suite `crates/blastit/tests/critical_dev_and_fail_closed.rs` (7 tests, zero skips) plus
  `crates/blastit/tests/common/mock_pg.rs` (`MockPgServer` on `127.0.0.1:0`). Every positive and
  negative twin in the NODE's acceptance list is covered; seccomp assertions accept `EPERM|ENOSYS`
  and never skip.

## Evidence

### 1. `cargo build --locked -p blastit-dbproxy`

```console
$ cargo build --locked -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
```

### 2. `cargo test -p blastit --test critical_dev_and_fail_closed --locked`

Run **without** `PYO3_PYTHON`/`LD_LIBRARY_PATH`/`PYTHONHOME` in the environment, to prove the
acceptance command stands on its own (`build.rs` RUNPATH):

```console
$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_dev_and_fail_closed --locked
running 7 tests
test unprivileged_production_has_no_single_uid_fallback ... ok
test one_route_banner_singularises ... ok
test a_non_loopback_host_needs_the_acknowledgement ... ok
test an_egress_false_view_cannot_open_inet ... ok
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test only_a_granted_route_gets_a_pg_socket ... ok
test learn_writes_routes_and_queries_on_sigterm ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.25s
```

### 3. `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check`

```console
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
...
guard: OK (8 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 36 changed file(s)
  no change to a protected critical test
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.43s
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.16s
cargo test --workspace --all-targets --locked
    ... test result: ok. 219 passed; 0 failed; 0 ignored; ...   (blastit lib)
cargo test -p blastit --test critical_config_and_presets
    test result: ok. 11 passed; 0 failed; 0 ignored; ...
cargo test -p blastit --test critical_dev_and_fail_closed
    test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
cargo build --locked -p blastit-py -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.65s
python -m unittest discover -s python/tests/critical
    Ran 28 tests in 0.167s
    OK
uvx --offline ruff check python tools      All checks passed!
uvx --offline ruff format --check python tools   40 files already formatted
uvx --offline zensical build --strict      No issues found / Build finished in 0.19s
uv run --no-project tools/plan.py validate plan OK
EXIT=0
```

Every other workspace critical suite in the same run passed (`blastit-dbproxy` 9,
`blastit-http` 5, `blastit-ingress` 55, `blastit-manifest` 17 + 18, `blastit-scheduler` 9,
`blastit-sqlpolicy` 25).

### Acceptance-twin coverage (`# Acceptance` bullets → tests)

| NODE acceptance item | Test |
|---|---|
| 5 roles non-root, `"privilege":"unprivileged-dev"` | `dev_boots_five_unprivileged_roles_and_strips_secrets` (≥4 children, each `euid`, none root when the suite is not root; boot line `privilege=unprivileged-dev`) |
| exact banner `1 route` / `2 routes` / `EXPOSED` | `...five_roles` (2 routes), `one_route_banner_singularises` (1 route), `a_non_loopback_host_needs_the_acknowledgement` (`… · uid <euid> (EXPOSED, no auth)`) |
| strips `DATABASE_URL` secret, `BLASTIT_*`, `BLASTIT_ENV_STRIP` | `...five_roles` (`/env` payload has no `real-secret`, no `BLASTIT_SECRET_MARKER`, no `STRIP_ME`) |
| `dev --learn` writes routes/queries on `SIGTERM` | `learn_writes_routes_and_queries_on_sigterm` (proposed `extra` + `/extra/[0-9]+`, `SELECT 1` under `sql`, none under the learn-fallback route `home`, `learn: 1 routes, 1 queries added`, exit 0) |
| unprivileged production exits 1, zero single-UID fallback | `unprivileged_production_has_no_single_uid_fallback` (exit 1, names `blastit dev`, no `unprivileged-dev`, no `ready`) |
| ungranted route socket `ENOENT` | `only_a_granted_route_gets_a_pg_socket` (`errno == ENOENT`, route 1 has no `.s.PGSQL.5432`, granted socket `0600`, dirs `0700`) |
| `egress = false` view `EPERM\|ENOSYS`, `egress = true` allowed | `an_egress_false_view_cannot_open_inet` (accepted errno set only `EPERM\|ENOSYS`, plus the positive twin) |
| `--host 0.0.0.0` without expose: exit 1, empty stdout | `a_non_loopback_host_needs_the_acknowledgement` |
| seccomp preflight, never skip | real view filters exercised; accepted errno set `EPERM\|ENOSYS`; `grep` for skip constructs is empty and `make check`'s `critical-skip-check`/`critical_guard` pass |

## Files

Changed (all inside the NODE's `paths`):

- `crates/blastit/src/cli.rs` — `run_dev` calls the supervisor; `--dev-root/--dev-host/--dev-obs-fd/--dev-ctl-fd`; stage-1 options.
- `crates/blastit/src/lib.rs` — registers `mod dev_learn`.
- `crates/blastit/src/stage0.rs` — `run_dev`, `dev_view_sockets`, `fork_dbproxy_dev`, `exec_stage1_dev`, `dev_fail`; production refusal sentence now names `blastit dev`.
- `crates/blastit/src/stage1.rs` — dev root/host adoption, `Option<u32>` privilege drop, dev bind address, `learn_mode`, dev route table, `Learn` adoption.
- `crates/blastit/src/supervise.rs` — `obs_fd` in the poll set, request-event capture, `push_routed`/`pop`, root cleanup and `--learn` write in `finish`.
- `crates/blastit/src/zygote.rs` — dev view directory, `drop_privileges: bool`, `RLIMIT_NPROC` only when dropping.
- `crates/blastit/src/dev_learn.rs` (new) — `DevRoot`, banner/exposure, `Dev`/`Learn`, `LearnRun`, atomic manifest write.
- `crates/blastit/tests/common/{mod,mock_pg}.rs` (new) — `MockPgServer` on `127.0.0.1:0`.
- `crates/blastit/tests/critical_dev_and_fail_closed.rs` (new, protected) — 7 tests, no skips.

## Follow-ups

- **Full `R27`/`R28` unrouted-SQL attribution** belongs to the `check`/audit leaves: `dev --learn`
  proposes routes from unrouted *requests* (the scheduler's `request` event) and deliberately does
  **not** write statements the learn-fallback view served (they belong to no declared route); the
  exact unrouted→route→query mapping lands with `plan_learn` in Leaf 11/12.
- `crates/blastit/src/check.rs` and `python/blastit/pytest_plugin.py`'s non-root `blastit dev` path
  remain the later leaves' work (not in this node's paths).
- `Makefile` and `python/**` changes visible in `git status` belong to a sibling node, not this
  one; `make check` passed with them in place.

## Review

Fresh-context review at base `15931c6`, own runs: `cargo build -p blastit-dbproxy` OK;
`cargo test --test critical_dev_and_fail_closed` 7/7; `cargo test --test
critical_config_and_presets` 11/11; `make check` exit 0; `make test-critical` exit 0;
`critical_guard.py --base 15931c6 --node MA.2.3.3` exit 0; `guard.py`/`plan.py validate` OK.
JSON-mode probe confirmed `"privilege":"unprivileged-dev"` and no `"port"` field.

1. MAJOR `crates/blastit/src/stage1.rs:216-228,436` — the adopted `Dev` root is only removed
   in `Supervisor::finish` and `component_exit`; every post-adopt `fail()`/`python_fail()`
   path (port in use, route table, memory, app import) `_exit(1)` with no cleanup. Stage 0
   also bypasses `fail_dev!` via `pipe_ok` (`stage0.rs:700,905`). Reproduced:
   `blastit dev --port <occupied>` → exit 1 and `/tmp/blastit-dev-1000-672555-…` left
   behind. R20 says "cleaned up on exit". Fix: thread `Option<&Dev>` into `fail`/`python_fail`
   and remove the root on every fatal exit; add a critical twin (occupied port, root gone).
2. MINOR `crates/blastit/src/dev_learn.rs:522` — `Some(obs.proxy.as_str()) == learn_route`
   compares the *proxy* name to the fallback *route* name; drop it or compare the observation's
   route (line 528 already does the intended check).
3. MINOR `crates/blastit/src/{stage0.rs:796,933,zygote.rs:85}` — new
   `#[allow(clippy::too_many_arguments)]` with no rationale comment; add one or group args.
4. MINOR `crates/blastit/tests/critical_dev_and_fail_closed.rs` — R22's "never a port field"
   and R12/R14-under-`--learn` are unasserted; add a `"port"`-key absence check over the
   boot/ready JSON lines and a `413`/malformed-JSON case in the `--learn` test.

## Attempt 3

All four review findings are resolved cleanly inside `MA.2.3.3`'s `paths`:

1. **MAJOR (`R20` runtime root cleanup across every fatal exit):**
   - **Root cause:** `libc::_exit(1)` skips Rust destructors. Stage 1 imported `crate::logs::fail` directly and exited via `fail`/`python_fail` without removing the adopted `Dev` root; `Supervisor` in `supervise.rs` also called `crate::logs::fail` without cleaning `self.dev`; and stage 0's `run_dev`/`fork_dbproxy_dev`/`exec_stage1_dev` escaped cleanup via `pipe_ok`/`fork_ok`/`cstring`.
   - **Fix:**
     - `stage1.rs`: adopts `options.dev_root` at the very start of `stage1::run` (before `read_config`), drops `crate::logs::fail` from imports so no call site can bypass cleanup, defines local `fail(emitter, dev: Option<&Dev>, message)` and `python_fail(py, emitter, dev: Option<&Dev>, ...)`, and returns `io::Result<Pid>` from `fork_http`/`fork_scheduler`.
     - `supervise.rs`: drops `crate::logs::fail` from imports, cleans `parts.dev` if `Supervisor::new`'s `fcntl_setfl` fails, and routes every fatal supervisor exit through `Supervisor::fail(&self, ...)` (`kill_all()` + `dev.cleanup()` + `logs::fail`).
     - `stage0.rs`: replaces every post-`DevRoot::create` parent `pipe_ok`/`fork_ok`/`cstring`/`reexec_argv` call with `pipe_dev`/`fail_with_root`/`fail_dev_root`/`cstring_dev`/`reexec_argv_dev` (never touching `root` from inside `Fork::Child`, where the waiting parent owns cleanup).
   - **Critical twin:** added `fatal_startup_removes_the_ephemeral_dev_root` in `crates/blastit/tests/critical_dev_and_fail_closed.rs`, exercising both an occupied `--port` (`stage1::fail` after stage 0 `boot`) and a missing `BLASTIT_APP` module (`stage1::python_fail`), asserting `exit 1` and zero `/tmp/blastit-dev-<uid>-<pid>-*` directories left behind for each `pid`.
2. **MINOR (`dev_learn.rs:522` proxy-vs-route comparison):**
   - Deleted the erroneous `if !self.unrouted.is_empty() && Some(obs.proxy.as_str()) == learn_route` branch (leaving the `ObsTarget::Routed { route }` comparison right below it) and pinned the regression in `dev_learn::tests::a_proxy_named_like_the_fallback_route_keeps_its_routed_grants`.
3. **MINOR (`#[allow(clippy::too_many_arguments)]` without rationale):**
   - Eliminated all four `#[allow(clippy::too_many_arguments)]` attributes introduced in `stage0.rs` and `zygote.rs` by grouping cohesive parameters into `DevBoot<'a>` (`stage0.rs`) and `ViewSpawn<'a>` (`zygote.rs`), and added an explicit rationale comment on `stage1::fork_scheduler`.
4. **MINOR (`R22` JSON no-`port`-key + `R12`/`R14` enforcement under `--learn`):**
   - Added `json_mode_emits_privilege_and_never_a_port_key` verifying that under `BLASTIT_LOG_FORMAT=json` (with `--host 0.0.0.0` + `BLASTIT_DEV_EXPOSE=1`), every stdout line is a JSON object, `stage0` `boot` carries `"privilege":"unprivileged-dev"`, `preload` `boot` and `ready` are emitted, and zero JSON objects (recursively) carry a `"port"` key.
   - Extended `learn_writes_routes_and_queries_on_sigterm` with `POST /sql` (`413` on an undeclared body, never synthesising `[route.ingress]`) and `POST /json` (`400` on malformed JSON on a declared `body = "json"` route), while the same `--learn` run still writes the proposed route and query cleanly (`exit 0`).
- **Institutional lesson note (for `docs/knowledge/LESSONS.md`, out of `MA.2.3.3` `paths`):** Whenever a process uses `libc::_exit` (to avoid `atexit`/CPython GIL deadlocks) while owning ephemeral filesystem artefacts (`DevRoot`/`Dev`), RAII `Drop` will never run; do not import a raw `_exit` helper (`logs::fail`) into modules that hold the resource — shadow it with a module-local `fail(..., Option<&Dev>, ...)` so the compiler enforces cleanup at every fatal call site, and never run cleanup inside a `Fork::Child` branch while the parent still owns the directory.

### Finding 1 Reproduction (Before vs After)

Reproduced with an occupied `127.0.0.1` TCP listener and `XDG_RUNTIME_DIR` unset, checking `/tmp/blastit-dev-<uid>-<pid>-*` bounded to the spawned `blastit` PID:

```text
BEFORE: pid=677910 exit=1 leaked=['/tmp/blastit-dev-1000-677910-b58aff86172df1eb']
stdout: blastit dev · single-uid simulation · 127.0.0.1:46031
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
15:23:33.474 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
15:23:33.483 ERROR app.output  stderr  binding 127.0.0.1:46031: Address already in use (os error 98)

AFTER: pid=684809 exit=1 leaked=[]
stdout: blastit dev · single-uid simulation · 127.0.0.1:33697
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
15:39:07.508 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
15:39:07.517 ERROR app.output  stderr  binding 127.0.0.1:33697: Address already in use (os error 98)
```

### Fresh Acceptance & Check Evidence (Attempt 3)

```console
$ cargo build --locked -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_dev_and_fail_closed --locked
running 9 tests
test unprivileged_production_has_no_single_uid_fallback ... ok
test fatal_startup_removes_the_ephemeral_dev_root ... ok
test one_route_banner_singularises ... ok
test a_non_loopback_host_needs_the_acknowledgement ... ok
test an_egress_false_view_cannot_open_inet ... ok
test json_mode_emits_privilege_and_never_a_port_key ... ok
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test only_a_granted_route_gets_a_pg_socket ... ok
test learn_writes_routes_and_queries_on_sigterm ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.25s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_config_and_presets --locked
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
...
protected critical tests hold no skip construct
cargo test -p blastit --test critical_config_and_presets    ... 11 passed; 0 failed
cargo test -p blastit --test critical_dev_and_fail_closed   ... 9 passed; 0 failed
cargo build --locked -p blastit-py -p blastit-dbproxy       ... Finished
python -m unittest discover -s python/tests/critical        ... Ran 28 tests in 0.352s / OK
uvx --offline ruff check python tools                       ... All checks passed!
uvx --offline ruff format --check python tools              ... 40 files already formatted
uvx --offline zensical build --strict                       ... No issues found / Build finished in 0.21s
uv run --no-project tools/plan.py validate                  ... plan OK

$ uv run --no-project tools/critical_guard.py --base d291078 --node MA.2.3.3
critical-guard: d291078..working tree, 13 changed file(s)
node MA.2.3.3: paths ['crates/blastit/src/lib.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/stage0.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/src/child.rs', 'crates/blastit/src/zygote.rs', 'crates/blastit/src/supervise.rs', 'crates/blastit/src/dev_learn.rs', 'crates/blastit/tests/**']
  no change to a protected critical test
```

## Re-review 3

Fresh-context re-review at base `d291078`. The four `## Review` findings all reproduce as fixed:
occupied `--port` (exit `1`, no `/tmp/blastit-dev-1000-<pid>-*`) and a bad `BLASTIT_APP` (exit
`1`, no root) both clean up; `dev_learn.rs:526` now compares route to route (pinned by
`a_proxy_named_like_the_fallback_route_keeps_its_routed_grants`); one `#[allow(too_many_arguments)]`
left (`stage1.rs:688`) with a rationale comment, the rest replaced by `DevBoot`/`ViewSpawn`; and the
JSON `port`-key absence plus `--learn` `413`/`400` twins are asserted. My own runs: `cargo build
--locked -p blastit-dbproxy` OK; 9/9 `critical_dev_and_fail_closed`; 11/11
`critical_config_and_presets`; `make check` exit `0`; `critical_guard.py --base d291078 --node
MA.2.3.3` / `guard.py` / `plan.py validate` exit `0`. `blastit dev --host 0.0.0.0` without the
acknowledgement: exit `1`, `0` stdout bytes; unprivileged production `blastit`: exit `1`, no
single-UID fallback.

1. **MAJOR `crates/blastit/src/supervise.rs:511,635` — `dev --learn` writes one route's queries
   onto another route.** `push_routed` runs on every `SPAWN` and `pop` only when that view exits,
   while `blastit-dbproxy/src/hooks.rs:337` attributes from that one process-global LIFO stack.
   Two live views therefore attribute every later statement to the newest route. Reproduced by hand
   (manifest: `a` `['/a/.*']`, `b` `['/b']`, no grants; `dev --learn`; `GET /a/1`, `GET /b`,
   `GET /a/2` where only `/a/2` runs `SELECT 333`):

   ```text
   request GET /a/1 → a  200 | GET /b → b  200 | GET /a/2 → a  200
   learn: 0 routes, 3 queries added
   [[route]] name = "a" ... [route.db] queries = ['SELECT 111']
   [[route]] name = "b" ... [route.db] queries = ['SELECT 222', 'SELECT 333']
   ```

   `SELECT 333` was executed by route `a` only, yet it is granted to `b` (and missing from `a`),
   contradicting `R27` and this suite's own claim that a statement "belongs to the route that ran
   it". Fix: attribute from the listener in `hooks::observe` (`ListenerSpec.route` is exact in
   `dev`, only `"*"` listeners need the control stack), drop the spawn-order
   `push_routed`/`pop`, and add a twin where route `a` queries **after** route `b`'s view is live.
2. **MINOR `crates/blastit/src/cli.rs:563-570` — the `--dev-*` flags are accepted by the production
   grammar.** `blastit --stage=1 --dev-root=… --dev-host=…` (no `dev` subcommand) adopts dev mode:
   no `setresuid`, `check_view_paths` skipped, and the `privilege: "unprivileged-dev"` marker lives
   only in stage 0, so the run is silent. It needs internal flags typed deliberately, so this is not
   a fallback, but stage 1 should also stamp `privilege` when it adopts a dev root.

## Attempt 4

Both `## Re-review 3` findings are resolved cleanly inside `MA.2.3.3`'s extended `paths`:

1. **MAJOR (`R27` `dev --learn` per-route statement attribution across concurrent live views):**
   - **Root cause:** `blastit dev` binds a dedicated per-route socket `<root>/views/<index>/.s.PGSQL.<port>` carrying `ListenerSpec.route = <route name>`, yet `DbproxyQuery::observe` (`crates/blastit-dbproxy/src/hooks.rs`) unconditionally attributed observations from `Learning::current_target()` — a single process-global LIFO stack designed for `blastit check`'s shared wildcard (`"*"`) socket. Meanwhile, `Supervisor` (`crates/blastit/src/supervise.rs`) called `push_routed` on view `SPAWN` and `pop` only on view exit. Because views remain alive across requests, spawning view `b` pushed `"b"` above `"a"`, attributing every subsequent query from view `a`'s private socket onto route `b`.
   - **Fix:**
     - `crates/blastit-dbproxy/src/hooks.rs` (`DbproxyQuery::observe`) and `crates/blastit-dbproxy/src/learn.rs` docs: when `listener.spec.route != WILDCARD_ROUTE` (`"*"`), attribute directly to `ObsTarget::Routed { route: listener.spec.route.clone() }` (matching `Shared::allowlist_for`); reserve `self.shared.learning().current_target()` exclusively for `"*"` wildcard listeners. `Enforce` mode and `critical_learn_and_redaction.rs` remain untouched.
     - `crates/blastit/src/{supervise,dev_learn,stage0}.rs`: removed the spawn/exit `push_routed`/`pop` calls from `Supervisor` and the unused control-line helpers/socketpair creation, while keeping `--dev-ctl-fd` internal-flag typing in `cli.rs`, `stage1.rs`, and `Learn::adopt`.
   - **Tests:** added unit test `hooks::tests::per_route_learning_listeners_attribute_from_the_listener_not_the_stack` in `crates/blastit-dbproxy/src/hooks.rs` and protected critical e2e twin `learn_attributes_queries_per_route_across_concurrent_live_views` in `crates/blastit/tests/critical_dev_and_fail_closed.rs` (verifying both route `a`'s and route `b`'s view PIDs are distinct and simultaneously alive, then issuing `/a/2` (`SELECT 333`) on warm view `a` and asserting parsed `blastit.toml` grants `["SELECT 111", "SELECT 333"]` to `a` and `["SELECT 222"]` to `b`).
2. **MINOR (`cli.rs` / `stage1.rs` `--dev-*` defence in depth and stage-1 `privilege` stamping):**
   - `crates/blastit/src/stage1.rs` (`preload_lifecycle`): whenever stage 1 adopts a `dev` root (`dev == true`), stage 1's `preload` `boot` event stamps both `"privilege": "unprivileged-dev"` and `"build": env!("CARGO_PKG_VERSION")` (overriding any `config.privilege`), so even a direct `blastit --stage=1 --dev-root=…` invocation unmistakably stamps `unprivileged-dev` before `ready`. Pinned in `stage1::tests::preload_stamps_unprivileged_dev_and_build_when_adopting_a_dev_root` and asserted in `critical_dev_and_fail_closed::json_mode_emits_privilege_and_never_a_port_key`.
   - `crates/blastit/src/cli.rs` (`parse`): kept `--dev-root/--dev-host/--dev-obs-fd/--dev-ctl-fd` typing intact for `--stage=1` while rejecting any `--dev-*` flag outside `--stage=1` or without `--dev-root`.
- **Institutional lesson note (for `docs/knowledge/LESSONS.md`, outside `MA.2.3.3` `paths`):** In `blastit-dbproxy`, `Learning`'s `push_routed`/`pop` LIFO stack is strictly for single-socket wildcard (`route == "*"`) test harnesses where requests run serially over a shared socket; whenever per-route sockets exist (`blastit dev`), views stay warm concurrently so attribution must derive from `listener.spec.route` — never from view spawn/exit lifecycle hooks.

### Reproduction (Before vs After)

#### 1. Multi-view `dev --learn` route attribution (`GET /a/1` → `SELECT 111`, `GET /b` → `SELECT 222`, `GET /a/2` → `SELECT 333`)

```text
BEFORE:
15:59:51.029 INFO  request     GET /a/1 → a  200  4.1ms
15:59:51.033 INFO  request     GET /b → b  200  3.3ms
15:59:51.035 INFO  request     GET /a/2 → a  200  1.2ms
learn: 0 routes, 3 queries added
[[route]] name = "a"  path = ['/a/.*']  [route.db] queries = ['SELECT 111']
[[route]] name = "b"  path = ['/b']     [route.db] queries = ['SELECT 222', 'SELECT 333']

AFTER:
16:14:56.595 INFO  request     GET /a/1 → a  200  4.1ms
16:14:56.598 INFO  request     GET /b → b  200  2.5ms
16:14:56.600 INFO  request     GET /a/2 → a  200  1.2ms
learn: 0 routes, 3 queries added
[[route]] name = "a"  path = ['/a/.*']  [route.db] queries = ['SELECT 111', 'SELECT 333']
[[route]] name = "b"  path = ['/b']     [route.db] queries = ['SELECT 222']
```

#### 2. Stage-1 `preload` `boot` `privilege` stamp (`BLASTIT_LOG_FORMAT=json`)

```text
BEFORE:
event=boot stage=stage0  privilege=unprivileged-dev build=None
event=boot stage=preload privilege=None             build=None
event=ready stage=serving privilege=None            build=None

AFTER:
event=boot stage=stage0  privilege=unprivileged-dev build=None
event=boot stage=preload privilege=unprivileged-dev build=0.1.0
event=ready stage=serving privilege=None            build=None
```

### Fresh Acceptance & Check Evidence (Attempt 4)

```console
$ cargo build --locked -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_dev_and_fail_closed --locked
running 10 tests
test unprivileged_production_has_no_single_uid_fallback ... ok
test fatal_startup_removes_the_ephemeral_dev_root ... ok
test one_route_banner_singularises ... ok
test a_non_loopback_host_needs_the_acknowledgement ... ok
test an_egress_false_view_cannot_open_inet ... ok
test json_mode_emits_privilege_and_never_a_port_key ... ok
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test only_a_granted_route_gets_a_pg_socket ... ok
test learn_writes_routes_and_queries_on_sigterm ... ok
test learn_attributes_queries_per_route_across_concurrent_live_views ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s

$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
running 9 tests
test a_root_listener_without_the_opt_in_is_rejected ... ok
test a_malformed_control_line_is_answered_and_closed ... ok
test an_oversized_control_line_is_refused_without_buffering_it ... ok
test enforce_still_answers_42501_and_redacts_every_literal ... ok
test an_unrouted_request_carries_its_own_target ... ok
test set_db_switches_the_database_the_next_session_requests ... ok
test a_listener_for_another_uid_refuses_the_client_before_the_upstream ... ok
test learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route ... ok
test a_stalled_control_line_times_out_instead_of_hanging ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.02s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_config_and_presets --locked
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
...
protected critical tests hold no skip construct
cargo test -p blastit-dbproxy --test critical_learn_and_redaction ... 9 passed; 0 failed
cargo test -p blastit --test critical_config_and_presets          ... 11 passed; 0 failed
cargo test -p blastit --test critical_dev_and_fail_closed         ... 10 passed; 0 failed
cargo build --locked -p blastit-py -p blastit-dbproxy             ... Finished
python -m unittest discover -s python/tests/critical              ... Ran 28 tests in 0.394s / OK
uvx --offline ruff check python tools                             ... All checks passed!
uvx --offline ruff format --check python tools                    ... 40 files already formatted
uvx --offline zensical build --strict                             ... No issues found / Build finished in 0.18s
uv run --no-project tools/plan.py validate                        ... plan OK

$ uv run --no-project tools/critical_guard.py --base d291078 --node MA.2.3.3
critical-guard: d291078..working tree, 15 changed file(s)
node MA.2.3.3: paths ['crates/blastit/src/lib.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/stage0.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/src/child.rs', 'crates/blastit/src/zygote.rs', 'crates/blastit/src/supervise.rs', 'crates/blastit/src/dev_learn.rs', 'crates/blastit/tests/**', 'crates/blastit-dbproxy/src/**', 'crates/blastit-dbproxy/tests/**']
  no change to a protected critical test
```

## Re-review 4

Fresh-context re-review at base `d291078` (attempt 4). Own runs: `cargo build --locked -p blastit-dbproxy` OK; `critical_dev_and_fail_closed` 10/10; `critical_learn_and_redaction` 9/9 (file unmodified); `critical_config_and_presets` 11/11; `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check` exit 0; `critical_guard.py --base d291078 --node MA.2.3.3`, `guard.py`, `plan.py validate` exit 0. `hooks.rs:342` attributes non-wildcard listeners from `listener.spec.route` (LIFO stack only for `"*"`), `supervise.rs` no longer pushes/pops, stage 1 stamps `privilege=unprivileged-dev`, `--dev-*` is `--stage=1`-only. Own `dev --learn` reproduction (a `/a/.*`, b `/b`; `GET /a/1`→111, `/b`→222, `/a/2`→333) wrote a `['SELECT 111','SELECT 333']`, b `['SELECT 222']`, and removed the root on SIGTERM.

1. MAJOR `crates/blastit/src/dev_learn.rs:518` — `edits()` skips every observation whose route equals `learn_route`, which is the *first declared route* (`stage1.rs:454`), whenever any unrouted request exists; per-route attribution can no longer tell the learn fallback's statements from that route's own. Reproduced: `GET /a/1` (SELECT 111), `GET /zzz/9` (unrouted, runs no SQL), `GET /b` (SELECT 222), `GET /a/2` (SELECT 333), clean SIGTERM → `learn: 1 routes, 1 queries added`, manifest: route `a` has no `[route.db]`, `b` `queries = ['SELECT 222']`; `a` silently lost 111 and 333 (the same sequence without the unrouted request writes both). Fix: stop keying the skip on a declared route name — give the learn fallback a reserved identity (a fallback route outside `manifest.routes()`, or a restored per-request `ObsTarget::Unrouted` signal) — and add a twin where the first route runs SQL while an unrouted request exists.

## Attempt 5

The `## Re-review 4` MAJOR finding (`R27`) is resolved cleanly inside `MA.2.3.3`'s `paths`:

- **Root cause:** `blastit-scheduler` dispatches unrouted `--learn` fallback requests onto view slot `0` (`Scheduler::learn_route() -> Some(0)`). Previously, `stage0` and `stage1` placed the first declared manifest route at slot `0` too, so both the first declared route's requests and any unrouted fallback requests shared `views/0/.s.PGSQL.<port>` (`ListenerSpec.route == manifest.routes()[0].name()`), and `LearnRun::edits` (`dev_learn.rs:518`) discarded every observation matching `learn_route` whenever `!self.unrouted.is_empty()` — silently erasing all queries of the first declared route even if the unrouted request ran zero SQL.
- **Fix:**
  - `crates/blastit/src/dev_learn.rs`: added `pub const LEARN_FALLBACK_ROUTE: &str = "_learn_fallback"` (cannot collide with any declared `[a-z0-9][a-z0-9_-]*` or inferred route name), removed `learn_route: Option<String>` from `Learn::adopt` and `LearnRun`, and changed `LearnRun::edits` to skip `LEARN_FALLBACK_ROUTE` instead of any declared route name.
  - `crates/blastit/src/stage0.rs` (`run_dev`, `dev_view_sockets`) & `crates/blastit/src/stage1.rs` (`run`, `learn_routing_table`, `Serving`): under `dev --learn`, dedicated view slot `0` (`views/0`, `egress = false`, unsatisfiable routing pattern `"a^"` in `learn_routing_table`) to `LEARN_FALLBACK_ROUTE`, shifting declared routes `0..N` to slots `1..=N` while keeping `banner` and `ready` route counts equal to `manifest.routes().len()`.
  - `crates/blastit/tests/critical_dev_and_fail_closed.rs`: added `first_declared_route_keeps_all_queries_when_unrouted_requests_exist` (plus unit tests in `dev_learn.rs` and `stage1.rs`), verifying route `a` (first declared route) keeps `["SELECT 111", "SELECT 333"]`, route `b` keeps `["SELECT 222"]`, and unrouted `/zzz/9` (no SQL) + `/extra/7` (`SELECT 1` on the fallback view) propose only `["extra", "zzz"]` without dropping `a`'s grants or leaking fallback SQL onto `a`/`b`.
- **Institutional lesson note (for `docs/knowledge/LESSONS.md`, outside `MA.2.3.3` `paths`):** Because `blastit-scheduler` routes `--learn` fallback traffic to view slot `0` (`learn_route() == Some(0)`), never place a declared manifest route on slot `0` in `dev --learn` or filter observations by a declared route name; dedicate slot `0` to a reserved sentinel outside `[a-z0-9][a-z0-9_-]*` (`_learn_fallback` with an unsatisfiable `"a^"` pattern) and shift declared routes to `1..=N`.

### Reproduction (Before vs After)

```text
BEFORE (`cargo test -p blastit --test critical_dev_and_fail_closed --locked`):
test first_declared_route_keeps_all_queries_when_unrouted_requests_exist ... FAILED
thread 'first_declared_route_keeps_all_queries_when_unrouted_requests_exist' panicked at
crates/blastit/tests/critical_dev_and_fail_closed.rs:1322:10:
first declared route `a` must keep its postgresql grant

AFTER (`cargo test -p blastit --test critical_dev_and_fail_closed --locked`):
test first_declared_route_keeps_all_queries_when_unrouted_requests_exist ... ok
(routes = ["a", "b", "extra", "zzz"]; a.queries = ["SELECT 111", "SELECT 333"];
 b.queries = ["SELECT 222"]; summary = "learn: 2 routes, 3 queries added")
```

### Fresh Acceptance & Check Evidence (Attempt 5)

```console
$ cargo build --locked -p blastit-dbproxy
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_dev_and_fail_closed --locked
running 11 tests
test fatal_startup_removes_the_ephemeral_dev_root ... ok
test unprivileged_production_has_no_single_uid_fallback ... ok
test one_route_banner_singularises ... ok
test a_non_loopback_host_needs_the_acknowledgement ... ok
test an_egress_false_view_cannot_open_inet ... ok
test json_mode_emits_privilege_and_never_a_port_key ... ok
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test only_a_granted_route_gets_a_pg_socket ... ok
test learn_writes_routes_and_queries_on_sigterm ... ok
test learn_attributes_queries_per_route_across_concurrent_live_views ... ok
test first_declared_route_keeps_all_queries_when_unrouted_requests_exist ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s

$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
running 9 tests
test a_root_listener_without_the_opt_in_is_rejected ... ok
test an_oversized_control_line_is_refused_without_buffering_it ... ok
test a_malformed_control_line_is_answered_and_closed ... ok
test enforce_still_answers_42501_and_redacts_every_literal ... ok
test an_unrouted_request_carries_its_own_target ... ok
test set_db_switches_the_database_the_next_session_requests ... ok
test a_listener_for_another_uid_refuses_the_client_before_the_upstream ... ok
test learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route ... ok
test a_stalled_control_line_times_out_instead_of_hanging ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.00s

$ env -u PYO3_PYTHON -u LD_LIBRARY_PATH -u PYTHONHOME \
    cargo test -p blastit --test critical_config_and_presets --locked
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
...
protected critical tests hold no skip construct
cargo test -p blastit-dbproxy --test critical_learn_and_redaction ... 9 passed; 0 failed
cargo test -p blastit --test critical_config_and_presets          ... 11 passed; 0 failed
cargo test -p blastit --test critical_dev_and_fail_closed         ... 11 passed; 0 failed
cargo build --locked -p blastit-py -p blastit-dbproxy             ... Finished
python -m unittest discover -s python/tests/critical              ... Ran 28 tests in 0.533s / OK
uvx --offline ruff check python tools                             ... All checks passed!
uvx --offline ruff format --check python tools                    ... 40 files already formatted
uvx --offline zensical build --strict                             ... No issues found / Build finished in 0.18s
uv run --no-project tools/plan.py validate                        ... plan OK

$ uv run --no-project tools/critical_guard.py --base d291078 --node MA.2.3.3
critical-guard: d291078..working tree, 15 changed file(s)
node MA.2.3.3: paths ['crates/blastit/src/lib.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/stage0.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/src/child.rs', 'crates/blastit/src/zygote.rs', 'crates/blastit/src/supervise.rs', 'crates/blastit/src/dev_learn.rs', 'crates/blastit/tests/**', 'crates/blastit-dbproxy/src/**', 'crates/blastit-dbproxy/tests/**']
  no change to a protected critical test
```

## Re-review 5

Fresh-context re-review at base `d291078` (attempt 5). Own runs: `cargo build --locked -p blastit-dbproxy` OK; `critical_dev_and_fail_closed` 11/11; `critical_learn_and_redaction` 9/9 (file unmodified); `critical_config_and_presets` 11/11; `CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check` exit 0; `critical_guard.py --base d291078 --node MA.2.3.3`, `guard.py`, `plan.py validate` exit 0. `dev_learn.rs:518`-equivalent now skips only `LEARN_FALLBACK_ROUTE`; `stage0::dev_view_sockets`/`stage1::learn_routing_table` put the sentinel on slot 0 with `"a^"` and declared routes on `1..=N`; manifest route names require a leading `[a-z0-9]` (`validate.rs:344`), so no declared or inferred route can collide. The new critical twin exercises the exact `/a/1`→111, `/zzz/9` (no SQL), `/b`→222, `/a/2`→333, `/extra/7`→fallback-SQL sequence and asserts `a == ["SELECT 111","SELECT 333"]`, `b == ["SELECT 222"]`, routes `["a","b","extra","zzz"]`, no fallback SQL leak, exit 0 — it fails pre-fix at the `a` grant `.expect` (the old skip erased the first declared route's grants), so it is not tautological. No findings; verdict PASS.
