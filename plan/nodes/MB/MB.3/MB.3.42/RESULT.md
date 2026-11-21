# Result MB.3.42

STATUS: done

## Summary

`php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name` no longer depends on the
allocator. Both halves (libphp and shim) now replace the recorded object the only structural
way there is:

1. write the new content to a side path (`.{name}.replacement`) in the **same directory**
   while the recorded file is still allocated, then `std::fs::rename()` it over the recorded
   path — the replacement's inode exists while the recorded one is live, so by POSIX the two
   numbers cannot be equal;
2. assert the path's `(st_dev, st_ino)` differs from `boot.config.php`'s record
   (`dev`/`ino`, `shim_dev`/`shim_ino`) **before** `bindings()`, panicking with
   "the fixture did not replace the file … so this test would assert nothing" — a degenerate
   fixture now fails as a fixture bug, never as a false accusation of `open_pinned`.

The test's doc comment says why the premise is a file *identity* (hence permission-independent
and valid unprivileged as well as as root) and why `remove_file()` + `fs::write()` is not a
valid way to obtain a new inode, so it is not "simplified" back. Every pre-existing assertion
is unchanged and equally strong: `contains("libphp")` / `contains("shim")`,
`contains("not the file stage 0 inspected")`, and each half must not blame the other object.
No `#[ignore]`, no skip, no early return, no euid branch, no workflow edit; the root-phase
two-suite exclusion list in `.github/workflows/ci.yml` is untouched.

## Evidence

### 1. The node's acceptance test — green

```console
$ cargo test -p blastit --locked --features python php_view::tests
test php_view::tests::the_child_sequence_keeps_a_pinned_descriptor ... ignored, re-exec'd as a single-threaded child by the test above
test php_view::tests::a_pinned_descriptor_survives_the_child_sequence ... ok
test php_view::tests::a_missing_record_is_refused_by_route ... ok
test php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name ... ok
test php_view::tests::a_php_slot_gets_one_binding_and_a_wsgi_slot_none ... ok
test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 213 filtered out; finished in 0.01s
```

(The one `ignored` is the pre-existing re-exec'd child of
`a_pinned_descriptor_survives_the_child_sequence`, not a skip of this test.)

### 2. The whole lib suite, so nothing adjacent moved

```console
$ cargo test -p blastit --locked --features python --lib
test result: ok. 216 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.36s
```

### 3. The fixture guard actually bites (negative check, then reverted)

Temporarily replacing the `fs::write` + `fs::rename` pair with a no-op and re-running the
test produced exactly the intended failure — it blames the fixture, not the implementation:

```console
$ cargo test -p blastit --locked --features python php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name
thread 'php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name' (3415985) panicked at crates/blastit/src/php_view.rs:1018:9:
assertion `left != right` failed: the fixture did not replace the file /tmp/blastit-php-binding-3415984-replaced/libphp8.4.so: it still has stage 0's (st_dev, st_ino) (73, 9697559), so this test would assert nothing
  left: (73, 9697559)
 right: (73, 9697559)
```

The no-op was reverted; `git diff crates/blastit/src/php_view.rs` shows only the change
described above, and the test is green again (evidence 1).

### 4. Format and lints

```console
$ cargo fmt --all
$ cargo fmt --all -- --check
FMT CLEAN

$ cargo clippy -p blastit --lib --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.22s

$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
error: this function has too many arguments (8/7)   # crates/blastit/src/host.rs:1893 (write_section)
error: could not compile `blastit` (lib test) due to 1 previous error
```

`--all-targets` could not be green in this worktree **at the time**, for a reason outside
this node: the single diagnostic was `clippy::too_many_arguments` in the **test module of
`crates/blastit/src/host.rs`**, which was sibling node MB.3.40's in-flight work (816 added
lines, 0 from me). It was out of this node's `paths`. Filtered evidence that this change was
clean:

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings 2>&1 | grep -c "php_view.rs"
0
$ ... | grep -E "^(error|warning)(\[|:)" | sort -u
error: could not compile `blastit` (lib test) due to 1 previous error
error: this function has too many arguments (8/7)
```

Zero diagnostics mention `php_view.rs`; the lib-only (non-test) clippy was fully green.

**Correction (`MB.3.43`, after MB.3.40 landed)**: that diagnostic was fixed by MB.3.40
(its `write_section` helper now takes a `SectionHeader` struct), so the workspace-wide
gate this node could not run is green on the merged tree, recorded here rather than left
as "red":

```console
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.38s
exit=0
```

### 5. `make check` — not run by this node's owner (run green afterwards)

Not run: sibling node MB.3.40 was mid-flight in this same worktree, and two concurrent
`make check` runs race on the shared wheel / `uv` cache. The lead runs the full gate serially
before committing. Running it here would have risked corrupting the sibling's build artefacts.

**Correction (`MB.3.43`)**: the gate has now been run on the merged tree and is green —
`make check` exits `0` with `cargo test --workspace --all-targets --locked --features python`
green, including `critical_php_backend` (5 passed) and `critical_php_embed` (19 passed).
MB.3.43's `RESULT.md` holds the command and its output tail; this node's contribution
(`php_view`'s side path + `rename`) is covered by it and by the suites above.

### 6. Honest limits of this evidence (important)

A local green does **not** prove the premise. This host's filesystems do not hand a freed
inode number back (the node measured tmpfs 0/40 and XFS 0/40 reuses), so the *old*
`remove_file()` + `fs::write()` fixture also passed here — which is exactly why the defect
only ever showed up in the privileged `smoke (root)` job. What changed is not "green locally"
but the construction: the replacement inode is now provably distinct from a live recorded one
before `bindings()` is called, so allocation behaviour can no longer decide the outcome.
The witness for the original failure is CI: `smoke (root)` on PR #64 must go green (one run,
not a statistical claim).

## Files

- `crates/blastit/src/php_view.rs` — only file changed by this node:
  - new test helpers `identity()` and `replace_by_rename(path, recorded, contents)` (side path
    + `rename` + the fixture-identity assertion);
  - `a_replaced_libphp_or_shim_is_refused_by_name` rewritten to use them for both halves and
    its doc comment rewritten (file identity vs. permission, and why unlink+create is not a
    valid new inode). Assertions and their teeth are unchanged.

## Follow-ups

1. **MB.3.40 (owner of the file): `crates/blastit/src/host.rs:1893` `write_section` has 8
   arguments**, so `cargo clippy -p blastit --all-targets -- -D warnings` fails with
   `clippy::too_many_arguments`. It blocks the shared gate for everyone in this worktree.
2. `cargo fmt --all` was run as the node's acceptance requires, workspace-wide. `host.rs`,
   `stage0.rs` and `critical_php_embed.rs` were already modified by sibling nodes when I
   started; if any of them were not rustfmt-clean, `cargo fmt --all` normalised them too.
   `cargo fmt --all -- --check` is now clean, but the lead should expect whitespace-only
   context in those files that is not authored by MB.3.40/MB.3.37.
3. Proposed `docs/knowledge/LESSONS.md` line (not added: that path is outside this node's
   `paths`, so I left it to the lead):
   "POSIX guarantees distinct inode numbers only among *simultaneously live* files, so
   `unlink()` + `create()` may hand the freed number straight back — a test that 'replaces' a
   file to move its identity must create a side path and `rename()` it over the target, and
   assert the recorded `(st_dev, st_ino)` actually moved. tmpfs/XFS did not reuse inodes in
   40/40 tries locally, while the privileged CI runner did, so this class of bug is invisible
   unprivileged."
