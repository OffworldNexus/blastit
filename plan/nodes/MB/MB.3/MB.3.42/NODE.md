---
id: MB.3.42
title: "The replaced-file premise in php_view's pin test must not depend on the allocator (red smoke (root) on PR #64)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/src/php_view.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T02:35"
updated: "2026-11-15T02:55"
---

# Goal

Fix the one red test in the required privileged CI gate (`smoke (root)`, PR #64 run
`43663059443`): `php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name` panics at
`crates/blastit/src/php_view.rs:1041:14` (`expect_err`, 207 passed / 1 failed) because
`boot_bindings` returned `Ok(Some(PhpBinding { .. }))` — the `(st_dev, st_ino)` pin saw the
same identity after the test "replaced" the file.

# Context

- The test replaces the recorded object with `remove_file()` + `fs::write()`
  (`php_view.rs:1037-1038`, and the shim half at `:1056-1057`). "unlink then create yields a
  new inode" is **not** a guarantee: POSIX only guarantees distinct inode numbers among files
  that are *simultaneously* live, so a filesystem may hand the freed inode straight back.
  Measured here: tmpfs 0/40 and XFS 0/40 reuses (so it passes locally, unprivileged); on the
  runner's filesystem as uid 0 it reused, so the first `smoke (root)` run that ever executed
  this test (`72e6d1f`, `MB.3.13.3`) is red while the unprivileged `test` job is green.
- This is **not** the `critical_static_cli` class ("root can list anything"): the assertion is
  a file *identity* comparison, not a permission refusal, so it is valid for every uid and
  belongs to no unprivileged tier. Do not exclude the binary, do not skip as root, do not
  move the test.
- The protected suite already uses the sound construction: `critical_php_embed.rs`'s
  `Swap { from, to }` renames a second, pre-existing file over the target.

# Acceptance

```console
$ cargo test -p blastit --locked --features python php_view::tests
$ make check
```
- Both halves (`libphp` and the `shim`) replace the recorded object by: writing the new
  content to a **side path in the same directory** while the recorded file is still
  allocated, then `rename()` over the recorded path. Distinctness is then structural, not
  allocator-dependent: the replacement's inode exists while the recorded one is live, so the
  two cannot be equal.
- Before calling `bindings()`, each half asserts the path's `(st_dev, st_ino)` differs from
  the one in `boot.config.php` (`dev`/`ino`, `shim_dev`/`shim_ino`), so a fixture that ever
  stopped replacing the file fails as "the fixture did not replace the file", never as a false
  accusation of the implementation.
- The doc comment says why the premise is permission-independent and why unlink+create is not
  a valid way to get a new inode (so nobody "simplifies" it back).
- The two assertions keep their current teeth: `message.contains("libphp")` /
  `contains("shim")`, `contains("not the file stage 0 inspected")`, and each half must not
  blame the other object.
- Existing assertions are never weakened, nothing is `#[ignore]`d or skipped, and the file is
  not a protected suite (no `critical_guard` involvement).

# Notes

- Scope is this one unit test. `crates/blastit/tests/critical_php_embed.rs` is being edited by
  `MB.3.40` and `MB.3.37` in parallel-serial order — do not touch it.
- The root phase's witness is CI itself (a local run cannot reproduce the reuse); say so in
  `RESULT.md` rather than claiming a local green as proof of the premise.
