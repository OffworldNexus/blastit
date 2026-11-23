# RESULT — MB.67.6 (test fixture: honour TMPDIR, stay under SUN_LEN)

STATUS: done

## Change

`crates/blastit/tests/common/project.rs` only.

- `scratch_root()` now returns `std::env::temp_dir().join(format!("blastit-e2e-{}", pid))`
  (honours `TMPDIR`, as `cargo mutants` already relies on) instead of the hardcoded
  `/tmp/opencode`, and asserts the root is `<= 48` bytes so
  `blastit-check-<uid>-<pid>-<rand>-0/pg/.s.PGSQL.5432` keeps the deepest socket under
  `SUN_LEN` (108, NUL included).
- `Project::new` joins the per-project `tag` under that per-run root (was
  `blastit-e2e-dev-<pid>-<tag>`); `Drop` cleanup, pid+tag uniqueness and
  `Project::{dir,path,write}` are unchanged. No `src/**`, no refusal message touched.

Deepest socket path before: `/tmp/opencode/blastit-e2e-dev-3166866-learn-write/tmp/blastit-check-1000-3166872-196716785-0/pg/.s.PGSQL.5432` (109 bytes).
After: `/tmp/blastit-e2e-<pid>/learn-write/tmp/blastit-check-...-->` ~96 bytes.

## Evidence

### Before the fix — exact failing command

```
$ cargo test -p blastit --test e2e_check_fmt learn_writes_one_grant_without_widening_a_view
test learn_writes_one_grant_without_widening_a_view ... FAILED
thread 'learn_writes_one_grant_without_widening_a_view' (3166867) panicked at crates/blastit/tests/e2e_check_fmt.rs:642:5:
assertion `left == right` failed: blastit check: /tmp/opencode/blastit-e2e-dev-3166866-learn-write/tmp/blastit-check-1000-3166872-196716785-0/pg/.s.PGSQL.5432: path must be shorter than SUN_LEN
  left: Some(2)
 right: Some(0)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 23 filtered out
```

### After the fix

```
$ cargo test -p blastit --test e2e_check_fmt learn_writes_one_grant_without_widening_a_view
test learn_writes_one_grant_without_widening_a_view ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.90s

$ cargo test -p blastit --test e2e_check_fmt
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.97s
```

### `make fast-check` tail

```
test a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not ... ok
test help_lists_the_commands_and_presets_but_never_the_internal_flags ... ok
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.87s

sh scripts/test_install.sh
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

## Follow-ups

- `crates/blastit/tests/e2e_cli.rs` has its own `temp_dir()`-based roots (already TMPDIR-aware);
  no change needed, but its `blastit-e2e-cli-<pid>-<tag>` names are within the same budget.
