# RESULT MB.3.43 — the red `php-sapi` job, and the MB.3.40/37 review MINORs

STATUS: done
ATTEMPT: 1/3
BASE: `4749882`

## Summary

**(A) the release blocker.** `protected_root` in
`crates/blastit/tests/critical_php_embed.rs` never found a candidate inside the
`php-sapi` container, because the loop `continue`d past every rejection *without
recording it*, so the panic printed `(tried )`. Three changes, all in the test's own
fixture discovery:

1. the workspace's own tree is now a candidate — runtime `current_dir()/target/tmp`
   **and** compile-time `env!("CARGO_MANIFEST_DIR")/../../target/tmp` (the image
   bind-mounts the checkout at that same absolute path, as the runner's uid, with
   root-owned `0755` parents), alongside `CARGO_TARGET_TMPDIR`, `$HOME` and
   `/opt`/`/usr/local`/`/srv`;
2. every rejected candidate records a reason — not a directory, `create_dir_all`'s
   errno, the probe's errno, or `host::protected`'s own sentence — so the panic can
   never print an empty list again;
3. `protected_root` returns a `ProtectedRoot` guard (`Deref<Target = Path>` +
   `Drop`), so each fixture is removed on the pass **and** the panic path. The four
   legs lost their trailing `remove_dir_all` lines; nothing else about them changed.

No `#[ignore]`, no skip, no host-tier demotion, no workflow or Makefile edit.
`the_same_tree_owned_by_init_is_accepted` still runs everywhere.

**(B) MB.3.40's MINORs.**

* b1 — new protected leg `the_deployment_gate_is_invoked_behind_its_sentence` runs
  `stage0::php_boot_in` with a `Deployment` (via a new seam, below) and requires
  **Refused** for a `0777` tree, **Refused** for a `0777` shim directory and
  **Proceed** for the same fixture at `0755`.
* b2 — `stage0::protected_scratch` and `host::safe_dir` no longer return
  `Option`: they panic with the reason, and the capability predicate is injected
  into `Provenance::check_objects` (`check_objects_with`) the way `placement` takes
  `caps_of`, so `a_capable_object_is_refused` is exercised unprivileged.
* b3 — `deployment_refusal` asks `missing_claim` before each tree claim's provenance,
  so a PHP manifest naming a missing `entry` (or `[route.static] root`) is refused
  with a sentence that says **missing** and names the path.

**(C) MB.3.37's MINORs** (same protected file): the shim leg's doc no longer claims
the miscompare leaves every assertion green; `BLOCKING_PHP` became `blocking_php()`
built from `BLOCK_MS`; the two probe twins share `assert_probe_containment`.

## The one new export, and why it is `#[doc(hidden)]`

`stage0` is a **private** module (`mod stage0;` in `lib.rs`), so
`tests/critical_php_embed.rs` cannot name `php_boot_in`. A protected witness for the
*invocation* therefore needs one reachable item:

* `crates/blastit/src/stage0.rs`: `pub fn deployment_boot_check(manifest_source,
  manifest_dir, libphp, shim_dir, uid_base) -> Result<(), String>` — parses the
  manifest, builds the same `PhpSearch`/`Deployment` a boot builds (`init_uid` is
  `current_uid()`, exactly as `php_boot_gate` sets it), calls **`php_boot_in`** and
  maps `Proceed` → `Ok(())`, `Refused(message)` → `Err(message)`.
* `crates/blastit/src/host.rs`: `#[doc(hidden)] pub use
  crate::stage0::deployment_boot_check;` — `host` is the public module that already
  owns `Provenance`, the decision this applies, and it is the only public module this
  node's `paths` allow (`lib.rs` is not one).

It is `#[doc(hidden)]` because it is not API: a `dev`/`check` boot never passes a
`Deployment`, and an unprivileged process cannot reach a `Production` boot at all, so
the protected suite is the only possible caller. `RUSTDOCFLAGS="-D warnings" cargo doc`
is clean (the doc links no private item).

## Evidence

### (A) `make rs-test-php-image` — before, then after

Before (`4749882`, reproduced locally at the start of this session):

```console
$ make rs-test-php-image
running 18 tests
thread 'a_world_writable_manifest_root_refuses_a_production_boot' (25) panicked at crates/blastit/tests/critical_php_embed.rs:2672:5:
no directory on this host is one uid's alone (tried ): a deployment boot could not run on this host either, so the accepted twin has nothing to accept
…
failures:
    a_runpath_into_a_writable_directory_refuses_a_production_boot
    a_world_writable_manifest_root_refuses_a_production_boot
    a_world_writable_shim_directory_refuses_a_production_boot
    the_same_tree_owned_by_init_is_accepted

test result: FAILED. 14 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s
make: *** [Makefile:553: rs-test-php-image] Error 101
```

After:

```console
$ make rs-test-php-image
docker run blastit-php-sapi-e2e:local …/critical_php_backend-3b9e55e09c71c369 --nocapture
running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s

docker run blastit-php-sapi-e2e:local …/critical_php_embed-0b784173b4fc9736 --nocapture
running 19 tests
refused (production provenance, pair half): the embedded shim (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/tmp/blastit-php-provenance-1-shim-2/shims/libblastit_php84-x86_64-gnu.so) could be replaced by another uid: …
the deployment gate refused the 0777 tree and the 0777 shim directory, and proceeded once both were 0755
test the_deployment_gate_is_invoked_behind_its_sentence ... ok
accepted at 0755/0644 owned by init's uid; refused once one directory is 0777
test the_same_tree_owned_by_init_is_accepted ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s
exit=0
```

**19, not the 18 the node's witness names**: this node adds b1's leg to the 18
MB.3.37 left (14 from MB.3.40 + 4 from MB.3.37), and nothing was removed or
filtered. The fixture path in the log is `target/tmp/…`, i.e. the new candidate is
what the container used — `$HOME` there is `/root` and the system directories are
root-owned, exactly as diagnosed.

### `cargo test -p blastit --locked --test critical_php_embed`

```console
$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s
$ … -- --test-threads=1     # twice, the serialized CI form
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.65s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.63s
$ ls -d /tmp/blastit-php-embed-* | wc -l
0
$ ls -d /home/remy/blastit-php-provenance-* | wc -l
0
```

(The `$HOME` count is **0** where MB.3.37 measured 10 stale `0777` directories from
panicking runs: every fixture is now removed by a guard, including on the panic
paths exercised by the red runs below.)

### `cargo test -p blastit --locked`

```console
$ cargo test -p blastit --locked
   Running unittests src/lib.rs …
test result: ok. 193 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
   Running tests/critical_php_backend.rs …
test result: ok. 5 passed; 0 failed
   Running tests/critical_php_embed.rs …
test result: ok. 19 passed; 0 failed
   … (every other binary green)
```

### Red runs (each mutation applied, run, then restored; the tree is green again above)

**R1 — b1, the gate's call site removed** from `php_boot_in` (the
`deployment_refusal` invocation replaced by `let _ = (deployment, deployment_refusal);`):

```console
$ cargo test -p blastit --locked --test critical_php_embed the_deployment_gate_is_invoked_behind_its_sentence -- --exact
thread 'the_deployment_gate_is_invoked_behind_its_sentence' panicked at crates/blastit/tests/critical_php_embed.rs:3084:34:
a deployment boot must run the tree authorisation: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out

$ cargo test -p blastit --locked --test critical_php_embed     # whole suite, still mutated
test a_runpath_into_a_writable_directory_refuses_a_production_boot ... ok
test a_world_writable_manifest_root_refuses_a_production_boot ... ok
test a_world_writable_shim_directory_refuses_a_production_boot ... ok
test the_deployment_gate_is_invoked_behind_its_sentence ... FAILED
test the_same_tree_owned_by_init_is_accepted ... ok
test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out

$ cp /tmp/opencode/mb343/stage0.rs.orig crates/blastit/src/stage0.rs   # restored
$ cargo test -p blastit --locked --test critical_php_embed the_deployment_gate_is_invoked_behind_its_sentence -- --exact
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out
```

The four `Provenance`-driving legs staying green under that mutation is exactly the
reviewer's finding; the new leg is the one that reddens.

**R2 — b2a, `protected_scratch` and the "assert nothing" body.** The old
`Option<Scratch>` shape (restored from `4749882`) on a host with no protected
directory (`HOME=/tmp`, the only candidate it knew) printed `skipped` and passed:

```console
$ HOME=/tmp RUSTUP_HOME=/home/remy/.rustup CARGO_HOME=/home/remy/.cargo cargo test -p blastit --locked --lib stage0::php_boot_gate::a_deployment_boot_refuses_a_pair -- --nocapture
skipped: no directory only its owner can write here (BLASTIT_EXEC=/tmp/blastit-php-gate-protected-…/probe could be replaced by another uid: a directory above it is group- or other-writable …)
test stage0::php_boot_gate::a_deployment_boot_refuses_a_pair_a_route_uid_could_replace ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 192 filtered out
```

The same command on this node's code — one test, **not** a vacuous pass:

```console
$ HOME=/tmp RUSTUP_HOME=… CARGO_HOME=… cargo test -p blastit --locked --lib stage0::php_boot_gate::a_deployment_boot_refuses_a_pair -- --nocapture
thread '…' panicked at crates/blastit/src/stage0.rs:
no directory on this host is one uid's alone (tried [/…/target/tmp/blastit-php-gate-protected-…: a directory above it is group- or other-writable (…); /tmp/blastit-php-gate-protected-…: a directory above it is group- or other-writable (…); /opt/…: create_dir_all failed (Permission denied (os error 13)); /usr/local/…: create_dir_all failed (Permission denied (os error 13)); /srv/…: create_dir_all failed (Permission denied (os error 13))]), so a deployment boot could not run on this host either and these tests would assert nothing
test result: FAILED. 0 passed; 1 failed
```

(`HOME=/tmp` needs `RUSTUP_HOME`/`CARGO_HOME` pinned or rustup refuses to pick a
toolchain — recorded in `LESSONS.md`.)

**R3 — b2b, the capability refusal.** Old `host.rs` (root-only guard) as a non-root:

```console
$ cargo test -p blastit --locked --lib host::tests::a_capable_object_is_refused -- --nocapture
skipped: setting security.capability needs CAP_SETFCAP
test host::tests::a_capable_object_is_refused ... ok
```

This node's code with the injected predicate ignored
(`match caps_of(&claim.path)` → `match Ok::<bool, io::Error>(false)`):

```console
$ cargo test -p blastit --locked --lib host::tests::a_capable_object_is_refused -- --exact
thread 'host::tests::a_capable_object_is_refused' panicked at crates/blastit/src/host.rs:2106:14:
a capable object must be refused: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 193 filtered out
```

**R4 — b3, the missing-path sentence.** New unit test
`stage0::php_boot_gate::a_missing_entry_or_static_root_names_the_missing_path`
asserts the refusal contains the claim (`route "docs": [route.static] root "site"`),
its absolute path, and `no such file or directory`, and does **not** contain
`cannot be inspected`; it then accepts the same manifest once both paths exist.
Order control: the pre-existing `a_deployment_boot_refuses_a_tree_a_route_uid_could_write`
failed at first with the missing-path sentence for its `0777` `manifest_dir` case
(`…public/index.php) is missing …`), which is why `missing_claim` is asked **per
claim, in `php_trees` order** — the first refusal still wins exactly as before, and
the leg is green again. The sentence now reads:

```text
route "app": [route.php] entry "public/index.php" (/…/public/index.php) is missing (no such file or directory): the manifest names a path this boot cannot read, so a deployment would hand its workers nothing; create it, or correct the path the manifest names
```

**R5 — c2, `BLOCK_MS` cannot lie.** The fixture is now
`usleep(BLOCK_MS * 1000)`. Changing the constant alone **moves** the leg (it stays
green), and a fixture that stops following it goes red:

```console
$ sed -i 's/const BLOCK_MS: u64 = 200;/const BLOCK_MS: u64 = 400;/' …   # constant only
blocked 400 ms in usleep: status 200  cpu_ns=137886  wall_ns=416185447  cpu/wall=1/3018  answered in 419.972895ms
test result: ok. 1 passed; 0 failed

$ … and the fixture hardcoded back to `200_000` (BLOCK_MS still 400)
thread '…' panicked at crates/blastit/tests/critical_php_embed.rs:2471:5:
the 400 ms block is not in the 215645927 ns wall time this leg asserts against: {"cpu_ns":88188,"cost_ns":201008720,"status":200,…}
test result: FAILED. 0 passed; 1 failed
```

MB.3.37's own control still holds — a shim rebuilt to report the wall clock as
`cpu_ns` (`CLOCK_PROCESS_CPUTIME_ID` → `CLOCK_MONOTONIC`, built in
`/tmp/opencode/mb343/mut-shim`, repository untouched) still reddens the leg:

```console
$ BLASTIT_PHP_SHIM_DIR=/tmp/opencode/mb343/mut-shim cargo test … -- a_blocking_script_is_charged_cpu_time_not_wall_time --exact --nocapture
`cpu_ns` (200239031 ns) must be under a 4th of the wall time (214918604 ns) for a 200 ms `usleep`: …
test result: FAILED. 0 passed; 1 failed
$ cargo test … -- a_blocking_script_… --exact --nocapture        # restored (real shim)
blocked 200 ms in usleep: status 200  cpu_ns=96203  wall_ns=215742344  cpu/wall=1/2242  answered in 217.95391ms
test result: ok. 1 passed; 0 failed
```

**R6 — c3, the shared containment helper is still an oracle, in both twins.** With
both halves of the probe's process-group split removed (`own_process_group` and
`join_group` no-ops), **both** legs fail through `assert_probe_containment`:

```console
$ cargo test … -- an_egress_route_lets_the_probe_open_a_socket -- --exact
assertion `left == right` failed: the probe leads its own process group, which is what `kill(-pgid)` names: … probe pid=3728257 ppid=3728254 pgid=3727751 uid=1000 euid=1000 inet=0
  left: 3727751
 right: 3728257
test result: FAILED. 0 passed; 1 failed

$ cargo test … -- a_probe_that_hangs_is_killed_and_refuses_the_boot -- --exact
  left: 3728300
 right: 3728369
test result: FAILED. 0 passed; 1 failed
```

and MB.3.37's `egress` control is unchanged (hardcoded `egress = false`: the
`egress = true` leg reddens, the denied twin stays green):

```console
$ … probe_bindings: `let egress = false;`
test result: FAILED. 0 passed; 1 failed   # an_egress_route_lets_the_probe_open_a_socket (left: 1)
test result: ok. 1 passed; 0 failed       # a_probe_that_hangs_is_killed_and_refuses_the_boot
```

Every mutation above was restored from a saved copy; the final `git status --short
crates/` lists only the three files this node owns (`php_view.rs` untouched).

### `make check` — exit 0

```console
$ make check
…
critical-guard: HEAD..working tree, 8 changed file(s)
protected critical tests hold no skip construct
cargo test -p blastit --features python --test critical_php_backend   → test result: ok. 5 passed; 0 failed
cargo test -p blastit --features python --test critical_php_embed     → test result: ok. 19 passed; 0 failed
…
No issues found                       # docs-check
cargo clippy -p blastit --all-targets --locked -- -D warnings  (standalone)   Finished
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings   Finished
default build: 0 pyo3 dependencies (no libpython linked)
plan OK
exit=0
```

```console
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.38s
exit=0
```

### `tools/critical_guard.py`

```console
$ uv run --no-project tools/critical_guard.py --base 4749882 --node MB.3.43
critical-guard: 4749882..working tree, 8 changed file(s)
node MB.3.43: paths ['crates/blastit/src/stage0.rs', 'crates/blastit/src/host.rs', 'crates/blastit/tests/critical_php_embed.rs', 'plan/nodes/MB/MB.3/MB.3.40/RESULT.md', 'plan/nodes/MB/MB.3/MB.3.42/RESULT.md']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.43
exit=0
```

## Files

* `crates/blastit/tests/critical_php_embed.rs` — `protected_root` (candidates +
  reasons) and the new `ProtectedRoot` guard; `the_deployment_gate_is_invoked_behind_its_sentence`;
  `blocking_php()`; `assert_probe_containment`; the shim leg's corrected doc;
  `BTreeMap` import. No assertion deleted or weakened; the two probe legs keep 6
  containment assertions each (now one call to the same 6).
* `crates/blastit/src/stage0.rs` — `deployment_boot_check` (the seam),
  `missing_claim` + the per-claim loop in `deployment_refusal`, `protected_scratch`
  as a panicking candidate list, and the new `a_missing_entry_or_static_root_names_the_missing_path` test.
* `crates/blastit/src/host.rs` — `check_objects_with` (injected capability
  predicate), `a_capable_object_is_refused` rewritten unprivileged, `safe_dir` →
  `Result`, `#[doc(hidden)] pub use crate::stage0::deployment_boot_check`.
* `plan/nodes/MB/MB.3/MB.3.40/RESULT.md` — the two false sentences corrected (the
  "single `unsafe`" claim, and "which half is unit-level" — the accepted twin never
  boots either); M4's "the protected file cannot see this" now says what is true.
* `plan/nodes/MB/MB.3/MB.3.42/RESULT.md` — clippy and `make check` evidence
  corrected: both red/skipped items are green on the merged tree.
* `docs/knowledge/LESSONS.md` — one line on the container fixture discovery (and
  the `HOME=/tmp` + rustup trap).
* No `Cargo.toml`/`Cargo.lock` change, no new dependency, no non-test `unsafe`.

## Follow-ups

1. `stage0::deployment_boot_check` is a `#[doc(hidden)]` public item because
   `lib.rs` is outside this node's `paths`. If a later node opens a proper
   `#[cfg(test)]`-only facade or a `test_support` module, move the seam there and
   drop the `host` re-export.
2. `critical_php_embed::protected_root`'s `target/tmp` candidate is refused on this host in-process
   (`/home/remy/offworld-nexus/blastit` is `0775`, so the gate is right) and used
   inside the container (root-owned `0755` parents); the `$HOME` candidate covers the
   in-process case. If the repo root is ever `0755` in CI, both candidates agree.
3. `a_real_capability_on_the_shared_interpreter_is_refused_under_every_name` still
   early-returns when not root: it is about the *real* `security.capability`
   attribute (needs `CAP_SETFCAP`) and is outside this node's findings; a future
   node could split its sentence half into `placement`'s injected `caps_of` the same
   way b2 did.
