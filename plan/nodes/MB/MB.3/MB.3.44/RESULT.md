# RESULT MB.3.44 — the deployment-gate seam must not report a proceeding manifest as a refusal

STATUS: done
ATTEMPT: 1/3
BASE: `304c5b1`

## Summary

Two reviewer items, both in `crates/blastit/src/stage0.rs`:

1. **`deployment_boot_check` now returns `Ok(())` for `PhpBoot::Absent`.** A
   deployment whose manifest declares no `[route.php]` route has nothing to
   authorise and boots; the seam reported that *proceeding* manifest as an `Err`,
   which a caller can only read as a refusal — the exact confusion the seam exists
   to prevent. `Err` is now produced by `PhpBoot::Refused` alone:

   ```rust
   match php_boot_in(&manifest, &search, None, Some(&deployment)) {
       // Nothing to authorise is not a refusal: a manifest with no PHP route
       // boots (`MB.3.44` — reporting this as an error let a caller read a
       // *proceeding* manifest as a refused one, the confusion this seam
       // exists to prevent).
       PhpBoot::Proceed(_) | PhpBoot::Absent => Ok(()),
       PhpBoot::Refused(message) => Err(message),
   }
   ```

   The doc contract now says that in words (`Ok(())` = "the gate proceeds" —
   `Proceed` *and* no PHP route; `Err` = a refusal), so contract and code agree.
2. **`missing_claim`'s rationale corrected, behaviour untouched.** `fs::metadata`
   follows symlinks, so a **dangling symlink** *is* reported as missing — the
   parenthesis claiming "never a broken symlink" was false. It now says what the
   call actually does, and why reporting it as missing is right (the target is the
   file the boot would read; "cannot be inspected" would name the link instead of
   the absent file) and what stays with the provenance sentence (`EACCES`, a
   symlink loop).

Both arms of (1) are pinned by a new unit test
(`stage0::php_boot_gate::the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair`:
no `[route.php]` → `Ok`; a `0777` pair → §4.1's sentence; the same pair at `0755`
→ `Ok`), the protected witness gained one `assert_eq!` for the `Absent` arm, and
(2) is pinned by a dangling-symlink case inside
`a_missing_entry_or_static_root_names_the_missing_path`.

The `#[doc(hidden)] pub use` seam stays in `host.rs` (moving it needs `lib.rs`,
out of scope; it remains the recorded follow-up). No new `LESSONS.md` bullet (that
path belongs to MB.3.43's plan entry).

## Evidence

### `cargo test -p blastit --locked`

```console
$ cargo test -p blastit --locked
   Running unittests src/lib.rs …
test result: ok. 194 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
   Running tests/critical_php_backend.rs …
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   Running tests/critical_php_embed.rs …
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   … (every other binary green)
```

(194 lib tests, one more than MB.3.43's 193: the new seam test. `critical_php_embed`
stays 19 — the protected file gained an *assertion*, not a leg.)

### Red run — the `Absent` arm (item 1's oracle)

The pre-`MB.3.44` arm restored (`PhpBoot::Absent => Err("the manifest declares no
[route.php] route".to_string())`), then both the unit test and the protected
witness:

```console
$ cargo test -p blastit --locked --lib stage0::php_boot_gate::the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair -- --exact
thread '…' panicked at crates/blastit/src/stage0.rs:4873:9:
  left: Err("the manifest declares no [route.php] route")
 right: Ok(())
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 194 filtered out

$ cargo test -p blastit --locked --test critical_php_embed the_deployment_gate_is_invoked_behind_its_sentence -- --exact
test the_deployment_gate_is_invoked_behind_its_sentence ... FAILED
thread '…' panicked at crates/blastit/tests/critical_php_embed.rs:3098:5:
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out
```

Restored from `/tmp/opencode/mb344/stage0.rs.orig`; both green again (below).

### Red control — the witness survives the seam change (required)

`deployment_refusal`'s call site deleted from `php_boot_in`
(`let _ = (deployment, deployment_refusal);`), `stage0.rs` copied out first:

```console
$ cargo test -p blastit --locked --test critical_php_embed the_deployment_gate_is_invoked_behind_its_sentence -- --exact
test the_deployment_gate_is_invoked_behind_its_sentence ... FAILED
thread '…' panicked at crates/blastit/tests/critical_php_embed.rs:3107:34:
a deployment boot must run the tree authorisation: ()
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out; finished in 0.27s

$ cargo test -p blastit --locked --test critical_php_embed        # whole suite, still mutated
test a_world_writable_manifest_root_refuses_a_production_boot ... ok
test a_world_writable_shim_directory_refuses_a_production_boot ... ok
test the_deployment_gate_is_invoked_behind_its_sentence ... FAILED
test the_same_tree_owned_by_init_is_accepted ... ok
test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s

$ cp /tmp/opencode/mb344/stage0.rs.orig crates/blastit/src/stage0.rs   # restored
$ git diff --stat > /tmp/opencode/mb344/diffstat.after
$ diff /tmp/opencode/mb344/diffstat.before /tmp/opencode/mb344/diffstat.after
diffstat unchanged
$ cargo test -p blastit --locked --test critical_php_embed the_deployment_gate_is_invoked_behind_its_sentence -- --exact
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out; finished in 0.25s
```

`git diff --stat` before and after the mutation cycle is byte-identical
(`stage0.rs`, `critical_php_embed.rs`, plus the sibling's `plan/JOURNAL.md`), i.e.
the tree is exactly the one described here. The three `Provenance`-driving legs stay
green under the mutation, as in MB.3.43: the seam is the only witness for the call.

### The corrected `missing_claim` behaviour is pinned

New case 3 of `a_missing_entry_or_static_root_names_the_missing_path`:

```console
$ cargo test -p blastit --locked --lib stage0::php_boot_gate -- --nocapture
test stage0::php_boot_gate::a_missing_entry_or_static_root_names_the_missing_path ... ok
test stage0::php_boot_gate::the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair ... ok
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 177 filtered out
```

A dangling symlink (`public/dangling.php` → `public/gone.php`) is refused with
`… public/dangling.php) is missing …` and **not** with "cannot be inspected" —
which is what the corrected doc now claims.

### `make check` — exit 0

```console
$ make check
guard: OK (2 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 3 changed file(s)
protected critical tests hold no skip construct
Documenting blastit v0.2.0 …                       # rs-doc, RUSTDOCFLAGS="-D warnings"
Running tests/critical_php_backend.rs …            # 5 tests, green
Running tests/critical_php_embed.rs …              # 19 tests, green
No issues found                                    # docs-check
clippy      -p blastit{,-view} (standalone)        # green
plan OK
exit=0
```

```console
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.81s
exit=0
```

### `tools/critical_guard.py`

```console
$ uv run --no-project tools/critical_guard.py --base 304c5b1 --node MB.3.44
critical-guard: 304c5b1..working tree, 3 changed file(s)
node MB.3.44: paths ['crates/blastit/src/stage0.rs', 'crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.44
exit=0
```

### The container job, re-run because the protected file changed (not required)

```console
$ make rs-test-php-image
docker run blastit-php-sapi-e2e:local …/critical_php_backend-… --nocapture
running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
docker run blastit-php-sapi-e2e:local …/critical_php_embed-… --nocapture
running 19 tests
the deployment gate proceeded on the manifest with no [route.php], refused the 0777 tree and the 0777 shim directory, and proceeded once both were 0755
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.57s
exit=0
```

## Files

* `crates/blastit/src/stage0.rs` — `deployment_boot_check`'s `Absent` arm and its
  doc contract; `missing_claim`'s rationale; the new
  `the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair` test; a
  dangling-symlink case in `a_missing_entry_or_static_root_names_the_missing_path`.
* `crates/blastit/tests/critical_php_embed.rs` — `STATIC_ONLY_MANIFEST` and one
  `assert_eq!` for the `Absent` arm in
  `the_deployment_gate_is_invoked_behind_its_sentence` (plus that leg's `println!`
  wording). Additions only: no assertion, leg or fixture removed or weakened.
* No `Cargo.toml`/`Cargo.lock` change, no new dependency, no new `unsafe`, no
  change to `host.rs` (the seam's location stays the recorded follow-up).

## Follow-ups

1. The `#[doc(hidden)] pub use crate::stage0::deployment_boot_check` still lives in
   `host.rs`; moving it to a proper `#[cfg(test)]`/`test_support` facade needs
   `lib.rs`, which neither MB.3.43 nor this node owns.
2. `deployment_boot_check` returns `Result<(), String>` and cannot distinguish
   `Proceed` from `Absent` for a caller that wants to know whether PHP was found.
   No caller needs that today (the protected suite asserts the contract: proceed vs
   refuse); an enum would be a public type for one test, so it stayed out.
3. `a_real_capability_on_the_shared_interpreter_is_refused_under_every_name` still
   early-returns when not root (real `security.capability` needs `CAP_SETFCAP`),
   unchanged from MB.3.43's follow-up.
