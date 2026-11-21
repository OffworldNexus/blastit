# RESULT — MB.24.4

Status: **done** (all acceptance commands run and recorded; `make check`/`make e2e` left to
`lead`). Worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, branch
`release/0.2.0`, base `9feb291`, attempt 1/3. Nothing committed; no plan status touched; no
existing `tests/critical_*` file edited; no new dependency, no `Cargo.toml`/`Cargo.lock` change,
no knob, no flag, no manifest key, no second PHP transport.

## Summary

* `crates/blastit/src/stage1.rs`: the `routes` binding is now `mut`, and one `#[cfg(feature =
  "php")]` loop sits **after** `php_view::boot_bindings` returns and **before** `probe_bindings`
  borrows `&routes`. For every slot whose binding is `Some`, it writes
  `per_process_cap = Some(1)` + `process_pool = true` — the same datum `spawn_now` reads, so the
  shape is the *binding's*, not the manifest-wide `php().is_some()` predicate. It runs after the
  three-arm `routes` match (dev, dev `--learn` with its slot-0 fallback, production), so all three
  arms are covered by index alignment. No other line moved.
* `crates/blastit/tests/critical_parallel_views.rs`: **new** protected suite (a new file is always
  allowed) with its own dev-boot helper (own port, `env_clear`, JSON events, `get`, barrier
  `burst`), a host probe copied from `critical_php_embed` (with `MB.3.62`'s drift test against
  `stage0.rs`), and the four legs the node names:
  `serialized_then_parallel_wsgi`, `serialized_then_parallel_php`,
  `the_cap_holds_and_the_pool_shrinks`, and the no-pair refusal branch inside each PHP leg
  (`refused_without_a_host`). Neither branch skips: the no-pair branch asserts the documented
  refusal first and only then returns, and no test here is marked ignored. The WSGI app stamps its own
  per-request `t0` (design §7) and the leg asserts the four stamps span `<= 100 ms`, and that a
  lazy WSGI boot forks nothing while the burst forks exactly one process (`ADR-0015` decision 9).
* `docs/decisions/ADR-0010-...md` §1 item 2: the stale `ViewSet` "`1..=workers`" sentence and the
  deleted `workers` cap are replaced with today's data model (`per_process_cap = Some(1)` +
  `process_pool = true`, one worker per concurrent request, bounded by `BLASTIT_MAX_WORKERS` and
  the memory-derived live-process cap, idle extras retiring on `BLASTIT_VIEW_IDLE_TIMEOUT`;
  `BLASTIT_VIEW_MAX_WORKERS` sizes one process and is not a per-route lever), post-fork
  `php_module_startup` unchanged, the `[route.fastcgi]`/`listen`/`socket`/`port`/`env` rejection
  kept, `ADR-0015` and `ADR-0012` §7 cited.
* `Makefile`: `PHP_SUITE_ARGS` (and the three comment/help lines that counted the suites) now carry
  `--test critical_parallel_views`.

## Measurements, in the order the design asks for them

All wall times are one process's own clock around `GET`s released together behind a `Barrier`, on
this host (x86_64-gnu, PHP 8.4.27, real pair: `/usr/lib/libphp8.4.so` +
`target/php-sapi/libblastit_php84-x86_64-gnu.so`). Fixtures: a PHP page and a WSGI app that each
sleep 200 ms and echo a distinct per-request marker (the PHP page also echoes `getmypid()`).

1. **Pre-change measurement, before the cap was wired** (HEAD `9feb291`, `target/debug/blastit`
   built at that commit, `/tmp/pv` fixture, four concurrent `GET /m/N` to one `entrypoint php`
   route, `BLASTIT_MAX_WORKERS=8`, `prefork`): **810.8 ms** and, warm, **809.5 ms**, all four `200`,
   all four served by the single pid `3527353`. That is the serialized floor the change is measured
   against.
2. **WSGI control, then parallel** (`serialized_then_parallel_wsgi`, same function):
   control `BLASTIT_MAX_WORKERS=1`, four requests one at a time = **821.7 ms**
   (`>= 800 ms` asserted); parallel on a fresh boot = **209.0 ms** over **1** spawned process
   (`< 450 ms` and `<= control - 400 ms` asserted), four `200`s, four markers.
3. **PHP control, warm-up burst, measured window** (`serialized_then_parallel_php`):
   control `BLASTIT_MAX_WORKERS=1` = **824.2 ms** (one pid serves all four, asserted);
   fresh boot (`prefork`, `BLASTIT_MAX_WORKERS=4`): warm-up burst **251.9 ms** over four distinct
   pids **[3591821, 3591846, 3591847, 3591848]** (four `200`s, four markers, four pids asserted —
   its duration includes the three new workers' `dlopen` + `php_module_startup` and is *not* part
   of the 450 ms);
   measured window **206.7 ms** over the **same** four pids, **4** `process.spawned` before and
   after (unchanged, asserted), i.e. no fork inside the clock.
4. **The cap and the shrink** (`the_cap_holds_and_the_pool_shrinks`,
   `BLASTIT_MAX_WORKERS=4`, cold route): 8 concurrent = **472.3 ms** over exactly four pids
   **[3592031, 3592032, 3592033, 3592034]**, `process.spawned` exactly those four, every marker
   once; then every burst pid retires with reason `idle` and the follow-up request got the fresh
   pid **3592062**.

### `docs/design/parallel-views.md` §7's `600 ms` floor is unreachable on a fast host

§7 puts the cap leg's window at `600 ms < wall < 1000 ms`, pricing four PHP startups into a cold
8-wide burst. Measured here, a fork + `dlopen` + `php_module_startup` is **~30 ms** (the four-wide
warm-up burst is 251.9 ms of which 200 ms is the sleep), so the window is **444–482 ms** over six
runs (481.5, 447.6, 457.1, 458.8, 444.2, 472.3) and `600 ms` cannot be met by an honest
implementation on this host. The leg therefore asserts the **causal** two-wave floor
(`2 × 200 ms`, i.e. `wall >= 400 ms`, plus `< 1000 ms`), and the host-independent oracles carry the
rest: exactly four pids for eight requests (eight parallel requests would need eight processes,
because `per_process_cap = 1`) and `process.spawned` equal to that pid set. Both are asserted and
both go red when the wiring is deleted (mutation below). Correcting §7's constant is outside this
node's paths — recorded as a follow-up.

## Evidence — every `# Acceptance` command

**1. `cargo test -p blastit --features python --test critical_parallel_views` — PASS** (real pair
found by the suite's own probe; no `BLASTIT_*` in this shell):

```console
$ timeout 500 cargo test -p blastit --features python --test critical_parallel_views
test serialized_then_parallel_wsgi ... ok
test serialized_then_parallel_php ... ok
test the_cap_holds_and_the_pool_shrinks ... ok
test the_probe_table_matches_stage0 ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.13s
```

**2. `BLASTIT_LIBPHP=/usr/lib/libphp8.5.so cargo test -p blastit --features python --test
critical_parallel_views` — PASS.** This host has no PHP 8.5 (`/usr/lib/libphp8.5.so` does not
exist), so the probe rejects the override and the three PHP legs take the refusal branch:

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.5.so timeout 500 cargo test -p blastit --features python \
      --test critical_parallel_views
test serialized_then_parallel_wsgi ... ok
test serialized_then_parallel_php ... ok
test the_cap_holds_and_the_pool_shrinks ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.55s

# --nocapture, one leg:
php parallel: refused (no usable libphp + shim on this host): blastit dev: route "app":
entrypoint php "public/index.php": BLASTIT_LIBPHP (/usr/lib/libphp8.5.so) cannot be opened or is
not a regular file (No such file or directory (os error 2)); install the PHP embed package —
Debian/Ubuntu: apt install libphp-embed, …, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

The same command with the host's real library is the served half (PASS, all seven):

```console
$ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so BLASTIT_PHP_SHIM_DIR=$PWD/target/php-sapi \
      timeout 500 cargo test -p blastit --features python --test critical_parallel_views \
      -- --nocapture --test-threads=2
wsgi: serialized control 821.687944ms (BLASTIT_MAX_WORKERS=1), parallel 208.959771ms over 1 spawned process(es)
php: serialized control 824.197516ms, warm-up burst 251.891301ms over pids [3591821, 3591846, 3591847, 3591848], measured 206.655583ms over the same pids with 4 spawned process(es)
php cap: 8 concurrent took 472.256211ms over pids [3592031, 3592032, 3592033, 3592034]; all retired (idle); the follow-up got pid 3592062
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.29s
```

The no-pair branch was also driven for real on this host by moving `target/php-sapi` aside (the
probe then finds no shim, the boot refuses, the leg passes on the refusal, shim restored):

```console
$ mv target/php-sapi /tmp/php-sapi-hold && cargo test -p blastit --features python \
      --test critical_parallel_views serialized_then_parallel_php -- --nocapture
php parallel: refused (no usable libphp + shim on this host): blastit dev: route "app": entrypoint
php "public/index.php": no shim libblastit_php84-x86_64-gnu.so was found for the NTS libphp
/usr/lib/libphp8.4.so (looked in …/target/debug, …/target/php-sapi); this host is (PHP 8.4,
x86_64-gnu), …; install the PHP embed package — …
test result: ok. 1 passed; 0 failed; … 6 filtered out
```

and it runs **without** `--features python` (the shape `make rs-test-php` / the image use):

```console
$ cargo test -p blastit --test critical_parallel_views
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.25s
```

**3. `cargo test -p blastit --features python --test critical_php_backend --test
critical_php_embed` — PASS** (26/26 embed, 9/9 backend, as `MB.24.3` saw it):

```console
$ timeout 600 cargo test -p blastit --features python --test critical_php_backend \
      --test critical_php_embed
     Running tests/critical_php_backend.rs (…)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
     Running tests/critical_php_embed.rs (…)
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.03s
```

**4. `make rs-test-php`** (the target the Makefile edit changes; builds the shim, runs all three
suites against this host's pair):

```console
$ timeout 900 make rs-test-php
php-sapi: cc -> …/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
BLASTIT_PHP_SHIM_DIR=… cargo test -p blastit --test critical_php_backend --test critical_php_embed --test critical_parallel_views -- --nocapture
     Running tests/critical_parallel_views.rs (…)
test result: ok. 6 passed; … finished in 3.87s
     Running tests/critical_php_backend.rs (…)
test result: ok. 9 passed; … finished in 0.42s
     Running tests/critical_php_embed.rs (…)
test result: ok. 26 passed; … finished in 2.62s
```

**5. Lints** (the node's `cargo fmt`/`clippy -D warnings`; both feature sets, because the WSGI
legs are `#[cfg(feature = "python")]` and `rs-no-python`'s clippy sees the default build):

```console
$ cargo fmt --all -- --check
FMT_CLEAN
$ timeout 900 cargo clippy -p blastit --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.50s
$ timeout 900 cargo clippy -p blastit --all-targets --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.86s
```

**`make check` / `make e2e` — NOT RUN**, by instruction (`lead` runs them).

## Mutation check (the suite's oracle is the wiring, not the fixture)

Deleting the two assignments in `stage1.rs` (loop kept, shape not written), rebuilding, then
restoring:

| Mutation | Witness |
|---|---|
| `route.per_process_cap = Some(1); route.process_pool = true;` removed | `serialized_then_parallel_php` red: *"the warm-up burst is four wide: it must have used four processes, saw [3587737, 3587737, 3587737, 3587737]"* |
| the same | `the_cap_holds_and_the_pool_shrinks` red: *"8 concurrent requests under BLASTIT_MAX_WORKERS=4 must run on exactly 4 processes, saw [3588849 × 8]"* |

The WSGI leg stays green under that mutation (the Python path is unchanged by design, decision 9),
which is why the two files' oracles are separate.

## Polish pass (reviewer `VERDICT: PASS`, five MINOR findings fixed/filed, plus one blocking skip-guard fix)

| # | Finding | What changed |
|---|---|---|
| 1 | The module doc claimed "no early return anywhere", which both PHP legs contradict (`refused_without_a_host` then `return`) | Reworded to "neither branch skips: the no-pair branch asserts the documented refusal first and only then returns", and each of the two `let Some(host) = … else` sites now says the same in one comment. The assertion is unchanged. |
| 2 | The WSGI print faked the spawn count (`spawned_before.len().max(1)` printed "over 1" for a lazy boot that forks nothing) | Prints the real post-burst count, and the leg now **asserts** `ADR-0015` decision 9's shape instead of assuming it: the lazy boot spawns **0** (`spawned_before.is_empty()`) and the four-request burst spawns **exactly 1** process. |
| 3 | Design §7 asks for a per-request `t0` in the WSGI fixture and an assertion that the four `t0`s span `<= 100 ms` | Added: the app stamps `t0 = time.monotonic()` first thing and echoes `wsgi:<marker>:<t0>`; `wsgi_answer` parses it; `assert_wsgi_burst` returns the four stamps; the parallel leg asserts `max - min <= 100 ms` (`CONTENTION_SPAN`) and prints the span. Measured: **0.6 ms**. The fixture is 12 lines. |
| 4 | The `PHP_SUITE_ARGS` comment said "These targets run the same three suites", false for `rs-test-php-image` | Now names `rs-test-php` as the three-suite target and states that `rs-test-php-image` still runs only the two its image compiles (hardcoded in `e2e/php-sapi/Dockerfile`, exactly-2 check), with the third a filed follow-up. |
| 5 | CI's `rs-test-php-image` cannot run the third suite | Out of this node's `paths` (`e2e/`, `.github/` untouched); filed as its own plan node by the dispatcher. |
| 6 | Blocking (`make check` red at `critical-skip-check`): the module doc **quoted** the skip attribute while denying it, and that target greps every protected critical file for the literal construct — the file tripped its own guard | The sentence now makes the same point without the token ("…and no test here is marked ignored"), the only occurrence in the file (and the same wording in this RESULT's Summary). The guard, its pattern and every assertion are untouched; `make critical-skip-check` is green again. |

Final-tree verification (nothing else changed; no `make check`, no commit, no plan mutation):

```console
$ timeout 300 make critical-skip-check
protected critical tests hold no skip construct          # exit 0, nothing on stderr
$ cargo fmt --all -- --check
FMT CLEAN
$ timeout 600 cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
$ export BLASTIT_LIBPHP=/usr/lib/libphp8.4.so BLASTIT_PHP_SHIM_DIR=$PWD/target/php-sapi
$ timeout 900 cargo test -p blastit --features python --test critical_parallel_views
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.52s
```

The `--nocapture` run of the same tree, for the numbers:

```console
$ timeout 900 env BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
      BLASTIT_PHP_SHIM_DIR=$PWD/target/php-sapi \
      cargo test -p blastit --features python --test critical_parallel_views \
      -- --nocapture --test-threads=2
wsgi: serialized control 814.430032ms (BLASTIT_MAX_WORKERS=1), parallel 217.238226ms over 1 spawned process(es), four t0s spanning 0.6 ms
php: serialized control 825.610581ms, warm-up burst 254.970855ms over pids [3636531, 3636536, 3636537, 3636538], measured 207.371221ms over the same pids with 4 spawned process(es)
php cap: 8 concurrent took 420.088708ms over pids [3636593, 3636594, 3636595, 3636596]; all retired (idle); the follow-up got pid 3636608
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.53s
```

The no-env form (probe discovery, 7/7) and the `BLASTIT_LIBPHP=/usr/lib/libphp8.5.so` refusal form
(7/7) were re-run on the same tree and stayed green.

## Files

| File | What |
|---|---|
| `crates/blastit/src/stage1.rs` | `let mut routes`; the `#[cfg(feature = "php")]` loop that writes `Some(1)` + `process_pool` on every bound slot, after `boot_bindings`, before `probe_bindings`; one comment block citing `ADR-0015` decisions 1/9. |
| `crates/blastit/tests/critical_parallel_views.rs` | **New protected suite**: boot helper, host probe (+ drift test), the four legs, fixtures (WSGI app 12 lines with its own `t0`, PHP page 6 lines), the WSGI `t0`-span and one-process assertions. |
| `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` | §1 item 2 reconciled (one worker per concurrent request as boot data; `workers` cap deleted by `ADR-0012` §7; `BLASTIT_VIEW_MAX_WORKERS` sizes one process; `[route.fastcgi]`/`listen`/`socket`/`port`/`env` rejected; cites `ADR-0015`). |
| `Makefile` | `PHP_SUITE_ARGS` gains `--test critical_parallel_views`; the comment/help lines that counted "two" suites updated, and the image target's limit stated rather than implied. |

## Follow-ups (outside this node's `paths`)

1. **The image leg does not build the new suite yet — filed as its own plan node.**
   `e2e/php-sapi/Dockerfile`, `e2e/php-sapi/Dockerfile.two-package` and
   `e2e/php-sapi/run-two-package.sh` hardcode `--test critical_php_backend --test
   critical_php_embed`, and `rs-test-php-image` refuses anything
   but **2** recorded executables, so `PHP_SUITE_ARGS` now covers the local `make rs-test-php` while
   the image build is unchanged (no breakage). Getting the wall-clock leg into the image (what CI
   runs) needs `--test critical_parallel_views` there plus that count raised to 3 — `e2e/` is
   outside every leaf's paths, and the dispatcher has filed the node.
2. **`docs/design/parallel-views.md` §7's `600 ms`** cap-leg floor is unreachable on a host whose
   PHP startup is ~30 ms (measured 444–482 ms, §above); the design should carry the causal
   two-wave floor. `docs/design/` is not this node's to edit.
3. **Real-`ulimit` e2e leg** (`MB.24.3` follow-up 6) still needs `e2e/`: `ulimit -u` =
   `MAX_WORKERS + VIEW_PROCESS_MARGIN - 1`, four concurrent requests, four `process.spawned` with
   distinct pids **or** a graceful refusal with no shutdown.
4. **Documentation deltas still owed** (`MB.24.3` follow-up 7, unchanged):
   `ARCHITECTURE.md` §2.4/§2.6/§8/§8.2, `docs/reference/settings.md:68-69`,
   `crates/blastit-view/src/php.rs:9-14` (still says *"The scheduler forks `1..=workers`"*), and the
   `docs/decisions/README.md` / `docs/design/README.md` index rows.
5. **`tests/common/` is now the shared home for the like-named boot helpers** — this suite is the
   fifth copy the design already prices; extracting them is a refactor nobody scheduled.
