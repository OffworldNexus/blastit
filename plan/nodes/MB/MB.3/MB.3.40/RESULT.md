# RESULT MB.3.40 — Production provenance gate for libphp/shim/manifest_dir (R7 + B2-R5)

STATUS: done
ATTEMPT: 1/3
BASE: `99557d1`

## What was implemented

`docs/testing/AUDIT-MB.3.13.md` gap **G6** ("the production provenance half of `R7`
is neither implemented nor tested") is closed. Design `R7`/`B2-R5` and `ADR-0010`
§3 now have code and a witness.

### `crates/blastit/src/host.rs` (new public half — the decision)

The primitives existed (`plain_is_protected` `:431`, `has_file_caps` `:315`) but
had no consumer beyond the plain executable (`:273`). Added:

| Item | What it decides |
|---|---|
| `pub fn dynamic_search_paths(path)` | the object's `DT_RPATH`/`DT_RUNPATH` entries, read from its **own dynamic section**: section-header table → `SHT_DYNAMIC` → `d_tag` 15/29 → the string table its `sh_link` names. `seek`/`read`, every offset bounds-checked, **zero `dlopen`, zero `mmap`, zero new `unsafe`, zero new dependency**. ELF32 and ELF64. A file it cannot read is an `Err`, never "no search path". |
| `pub struct Claim { what, path }` | one path plus the words a refusal names it with. |
| `pub struct Provenance { init_uid, objects, trees }` with `check_objects()` / `check_trees()` | the deployment authorisation: `facts_of` + `protected` (owner is root/init's uid, no group/other write bit on it or on **any** directory above it), plus — for objects only — `has_file_caps` is false/readable and no writable `DT_RPATH`/`DT_RUNPATH` entry (`$ORIGIN`/`${ORIGIN}` expanded, every other token, a relative path and the empty element refused because the loader would resolve them against the worker's `cwd`). |

### `crates/blastit/src/stage0.rs` (the gate)

* `php_boot_gate` builds a `Deployment { init_uid, manifest_dir }` **only for
  `PhpShimScope::Deployment`**. `dev`/`check` pass `None`, and the reason is a code
  comment on the call site (not only a test): the shim a developer builds lives in
  `target/php-sapi` inside a checkout their own uid owns and writes, so "no other
  uid can replace it" cannot hold there and would refuse every development boot
  while protecting nothing.
* `php_boot_in(.., deployment)` runs discovery, then — before `PhpBoot::Proceed` —
  `deployment_refusal`: the **pair** (`the NTS libphp`, `the embedded shim`) through
  `check_objects`, then the **tree** (`blastit.toml`'s directory — every PHP worker's
  `cwd`/`include_path` —, each `[route.php] entry`, each `[route.static] root`)
  through `check_trees`.
* Sentence shapes: the pair keeps `ADR-0010` §4.1's shape via `present_refusal` —
  `route "app": [route.php] entry "public/index.php": … could be replaced by another
  uid: … chown it to root …; this host is (PHP 8.4, x86_64-gnu), whose embedded shim
  is libblastit_php84-x86_64-gnu.so; install the PHP embed package …; BLASTIT_LIBPHP=…`.
  The tree gets its own sentence (a package cannot fix it): `route "docs":
  [route.static] root "site" (/…/site) could be replaced by another uid: … chown it to
  root (or to init's uid, 1000), remove the group and other write bits (chmod 0644 for
  a file, 0755 for a directory), and do the same to every directory above it`.
* `php_boot` (the unit-test seam) is now `#[cfg(test)]` and is `php_boot_in(.., None)`.
* The `(st_dev, st_ino)` record and stage 1's pin are untouched: this is an
  authorisation **in front of** them, and it runs before `bind_view_sockets`.

## Acceptance

### 1. `cargo test -p blastit --locked --test critical_php_embed`

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 14 tests
test a_world_writable_manifest_root_refuses_a_production_boot ... ok
test a_non_library_blastit_libphp_refuses_before_anything_loads ... ok
test a_dev_boot_of_a_world_writable_tree_still_serves ... ok
test a_runpath_into_a_writable_directory_refuses_a_production_boot ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test a_world_writable_shim_directory_refuses_a_production_boot ... ok
test the_same_tree_owned_by_init_is_accepted ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... ok
test a_probe_that_hangs_is_killed_and_refuses_the_boot ... ok
test a_libphp_replaced_between_the_stages_refuses_the_boot ... ok
test a_shim_for_another_minor_refuses_the_boot_before_the_listener ... ok
test a_zts_libphp_refuses_before_anything_loads ... ok
test no_libphp_is_mapped_above_the_view_worker ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
```

**14, not the 12 in the node's sample block**: the suite had 9 tests, and this node
adds the 4 named legs plus the `dev` twin — 9 + 5 = 14. Nothing was removed, renamed
or weakened (`git diff` on the file shows exactly two `-` lines, both mechanical: the
`use std::os::unix::fs::MetadataExt;` line became `{MetadataExt, PermissionsExt}` and
the `-lc` argument moved into the new `compile_linked`). No `#[ignore]`, no skip.

Runs `4` and `1` threads, both green (serial is the CI form):
`--test-threads=1` → `test result: ok. 14 passed`.

### 2. Red-run mutation controls (one per check)

Each mutation was applied to the rule, its leg run alone, and the file restored from
a saved copy; the restored tree is green again (see §1).

**M1 — the tree check removed (`check_trees` stops calling `cannot_be_replaced`):**

```console
$ cargo test -p blastit --locked --test critical_php_embed a_world_writable_manifest_root_refuses_a_production_boot -- --exact
thread 'a_world_writable_manifest_root_refuses_a_production_boot' panicked at crates/blastit/tests/critical_php_embed.rs:2175:10:
a world-writable manifest directory must refuse a deployment boot: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out
```

**M2 — the pair's ownership check removed (`check_objects` drops
`cannot_be_replaced`):**

```console
$ cargo test -p blastit --locked --test critical_php_embed a_world_writable_shim_directory_refuses_a_production_boot -- --exact
thread 'a_world_writable_shim_directory_refuses_a_production_boot' panicked at crates/blastit/tests/critical_php_embed.rs:2225:6:
a shim in a world-writable directory must refuse a deployment boot: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out
```

**M3 — the `DT_RPATH`/`DT_RUNPATH` check removed (`check_objects` drops
`no_writable_search_path`):**

```console
$ cargo test -p blastit --locked --test critical_php_embed a_runpath_into_a_writable_directory_refuses_a_production_boot -- --exact
thread 'a_runpath_into_a_writable_directory_refuses_a_production_boot' panicked at crates/blastit/tests/critical_php_embed.rs:2314:14:
a writable search path must refuse a deployment boot: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out
```

**M4 (extra) — the gate's *call site* removed** (`php_boot_in` stops calling
`deployment_refusal`), which the protected file cannot see because an unprivileged
process cannot reach the `Production` gate at all (see "which half is unit-level"):

```console
$ cargo test -p blastit --locked --lib stage0::php_boot_gate
test stage0::php_boot_gate::a_deployment_boot_refuses_a_pair_a_route_uid_could_replace ... FAILED
test stage0::php_boot_gate::a_development_boot_does_not_apply_the_production_rule ... FAILED
test stage0::php_boot_gate::a_deployment_boot_refuses_a_tree_a_route_uid_could_write ... FAILED
test result: FAILED. 13 passed; 3 failed; 0 ignored; 0 measured; 177 filtered out
```

### 3. The `dev` twin — the rule is `Production`-scoped (end to end)

```console
$ cargo test -p blastit --locked --test critical_php_embed a_dev_boot_of_a_world_writable_tree_still_serves -- --exact --nocapture
`blastit dev` served a 0777 tree the deployment rule refuses: HTTP/1.1 502 Bad Gateway
test a_dev_boot_of_a_world_writable_tree_still_serves ... ok
```

The same fixture is refused by the deployment rule inside the same test
(`trees_refusal(..).is_err()`), and its directory is asserted `0777` before the boot,
so the leg cannot pass on a fixture that was never world-writable. The `502` is the
stub shim (its `blastit_php_request` returns `-1`); what is proved is that `dev`
**bound and answered**, which a `Production`-scoped rule must allow. Removing the
scope split makes this leg fail.

### 4. `make check`

```console
$ make check
guard: OK (3 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 8 changed file(s)
  M  crates/blastit/tests/critical_php_embed.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
…
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

The `VIOLATION … report only` line is `make check`'s **own** invocation
(`tools/critical_guard.py --base HEAD`, no `--node`): with `HEAD` as the base every
protected edit is un-attributable, so it can only report. The authoritative run is
§6, which names the node. Because `make check` type-checks nothing of this message,
note what it did run: `cargo test -p blastit --features python --test
critical_php_embed` → `14 passed`, and the standalone/`-p blastit` clippy pass that
checks the artifact keeps no `libpython`.

### 5. `cargo clippy --workspace --all-targets --locked --features python -- -D warnings`

```console
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.11s
exit=0
```

No new dependency (`git diff Cargo.toml Cargo.lock` is empty) and no new `unsafe`:
the dynamic-section walk is `File::seek`/`read_exact` only; the single `unsafe` in
`host.rs` is the pre-existing `getxattr` in `has_file_caps`. (A sibling's clippy run
caught `write_section`'s 8 arguments at `host.rs:1893`; that test helper now takes a
`SectionHeader` struct — `rust-standards` §3 — instead of an `#[allow]`.)

### 6. `tools/critical_guard.py --base 99557d1 --node MB.3.40`

```console
$ uv run --no-project tools/critical_guard.py --base 99557d1 --node MB.3.40
critical-guard: 99557d1..working tree, 8 changed file(s)
node MB.3.40: paths ['crates/blastit/src/stage0.rs', 'crates/blastit/src/host.rs', 'crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.40
exit=0
```

## Which half is end to end, and which is unit-level

Stated plainly, because the node asks for it:

* **End to end**: `a_dev_boot_of_a_world_writable_tree_still_serves` — a real
  `blastit dev` process, a real listener, a real HTTP answer, on the `0777` fixture
  the other legs refuse. That is what proves the rule is `Production`-scoped, which is
  half of the node's claim.
* **Unit-level, by necessity**: every *refusal* leg. An unprivileged process cannot
  reach the `Production` half of stage 0 at all — `blastit_sys::caps::detect` fails and
  `fail_early` exits **before** `Config::from_env` (measured; `LESSONS.md` `MB.1.6`),
  and running the gate as root is not something `make check` may depend on. So the
  four named legs drive `blastit::host::Provenance` — the exact function
  `stage0::deployment_refusal` calls, in the exact order it calls it — and the
  *sentence and the call site* are pinned by `stage0::php_boot_gate`'s
  `a_deployment_boot_refuses_a_pair_a_route_uid_could_replace`,
  `…_tree_a_route_uid_could_write` and `a_development_boot_does_not_apply_the_production_rule`.
  That is why M4 above exists: it is the control for the half the protected file
  cannot see. **"No listener"** for these refusals is therefore argued, not observed by
  a new leg: `php_boot_in` returns `Refused` from the same `php_boot_gate` call site
  (`stage0.rs`, before `bind_view_sockets`) that the suite's existing stage-0 refusal
  legs (`a_zts_libphp_refuses_before_anything_loads`,
  `a_non_library_blastit_libphp_refuses_before_anything_loads`) already prove leaves
  nothing bound and nothing on stdout.

## Did the node's scope guard hold?

* `crates/blastit/src/php_view.rs` was **not** touched (the worktree shows it modified
  by sibling `MB.3.42`; `git diff --stat` lists it, `critical_guard` does not attribute
  it to this node, and this node's evidence does not depend on it).
* No leg here asserts that a file's **inode/identity moved**, and none "replaces" a
  file with `remove_file()` + `write()`. Every leg changes either permission bits
  (`0755` ↔ `0777`, `0644` ↔ `0666`) or compiles a *distinct* object with
  `-Wl,--disable-new-dtags,-rpath,…` / `-Wl,-rpath,…` / no `-rpath`. The existing
  `Swap { from, to }` rename-based leg is untouched, and the suite still runs it green
  (§1).
* Files touched: `crates/blastit/src/host.rs`, `crates/blastit/src/stage0.rs`,
  `crates/blastit/tests/critical_php_embed.rs`, plus `docs/knowledge/LESSONS.md`
  (the standing "append one gotcha" instruction) and this file.

## What the gate now refuses, and what that costs an operator (DX)

A deployment boot (root or rootless) refuses when a `[route.php]` exists and any of
these is true, **before any listener binds**:

1. the resolved NTS `libphp` or the selected shim is owned by a uid that is neither
   root nor init's uid, is group/other-writable, sits under a directory that is, or
   carries file capabilities, or names a `DT_RPATH`/`DT_RUNPATH` directory another uid
   can write;
2. `blastit.toml`'s directory, a `[route.php] entry`, or a `[route.static] root` is
   owned by a route uid or is group/other-writable (or sits under a directory that
   is — `/tmp` and `/srv` at `0775` are the usual ones).

The sentence names the path and the `chown`/`chmod`. `blastit dev` and `blastit check`
are unchanged (the same `0777` fixture still serves, §3).

## Follow-ups (outside this node's `paths`)

1. **`docs/troubleshooting.md` needs a row for this refusal.** The table has
   `carries file capabilities` and `is writable by another uid` (the WSGI `sys.path`
   family) but nothing for `could be replaced by another uid`, which is now a boot
   refusal an operator can hit (a checkout bind-mounted at `0775`, a
   docker-compose volume created by the host user). Suggested row: symptom
   `could be replaced by another uid` → "a deployment path belongs to a uid a route
   serves as: `chown -R root:root` it and `chmod 0755` the directories (`0644` the
   files), or keep developing with `blastit dev`". `docs/deploy/compose-traefik.md`
   should also say the mounted `blastit.toml` tree must be root- or init-owned.
2. **`docs/frameworks/php.md` / `docs/security.md`**: the production half of `R7` is
   now real; the security page's "what a compromise gets" list can drop the caveat
   that provenance is design-only (if such a caveat survives) and name the new
   refusals.
3. **The `.ini`/`extension_dir`/`extension=`/`zend_extension=` pre-scan** of
   `ADR-0010` §3 and design `R7` is still unimplemented: it needs the worker's
   effective INI (today `blastit-php-sapi` renders the `ZEND_INI_SYSTEM` set itself and
   the host scan would either duplicate that policy or read files the worker never
   loads). `host::Provenance` is the primitive it would use; a G6-shaped audit entry
   should track it rather than this leaf growing a second INI parser.
4. **`Boot::launch`'s free-port probe** (`critical_php_embed.rs`) picks an ephemeral
   port, drops it, then lets the child bind it: under load a *foreign* listener can
   take the port in between, and a "must refuse" leg then reads "something accepted" as
   "the boot served" (observed once in the pre-existing
   `a_non_library_blastit_libphp_refuses_before_anything_loads`, 1 run in ~12, on a
   host with several agents' cargo runs). `await_boot` also returns before stage 1's
   `ready`, so a *snapshot* of `events()` there is flaky by construction — the new legs
   use `wait_event` and a bounded retry, and `LESSONS.md` records both. Fixing the port
   hand-off properly (bind in the parent, or a retry on `EADDRINUSE`) touches the
   harness for every leg and belongs to `MB.3.39`'s CI-witness work rather than here.
5. **`cargo-mutants` on `crates/blastit`** would be the honest next oracle for
   `dynamic_search_paths`' bounds checks (the `bounded_dynamic`, `dynamic_strtab` and
   `MAX_DYNAMIC_STRTAB` arms have unit tests but no mutant campaign); `make check` is
   the gate here and the manual branch controls in §2 are what this node could afford.
