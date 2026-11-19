# Result MB.42.2.3

## Summary

`crates/blastit/src/cli/handoff.rs` implements `docs/design/universal-distribution.md` §4 exactly:
a global `blastit` silently becomes the project's own `blastit` for `dev`, `check`, `fmt` and `lsp`,
and never for a privileged or self-describing shape.

- **Scope.** `cli::main` funnels the four `Subcommand::Run` arms through one helper
  (`after_handoff`), so `Serve` (bare `blastit`, `--preset`, `--stage=1`, `--role=*`,
  `--internal-json`, `--help`/`help`, `--version`) cannot reach it, and a subcommand's own
  `--help` never does either (`Subcommand::Help` returns before the funnel).
- **Bypass.** `BLASTIT_HANDOFF_DONE` non-empty, or `BLASTIT_NO_LOCAL_HANDOFF` set and neither
  `""` nor `"0"`.
- **Candidate order from `cwd`.** `$VIRTUAL_ENV/bin/blastit` when `VIRTUAL_ENV` is non-empty
  (explicit — no `./.venv` fallback), else `<cwd>/.venv/bin/blastit`; then
  `<cwd>/vendor/bin/blastit`; then `<cwd>/node_modules/.bin/blastit`.
- **Zero-TOCTOU verification.** `canonicalize` names the binary; the parent is opened
  `O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW`, the binary is opened **relative to it**
  (`openat(parent_fd, name, O_RDONLY|O_CLOEXEC|O_NOFOLLOW)`), and every check reads `fstat` of
  those two descriptors: regular file, `mode & 0o111`, `uid ∈ {geteuid(), 0}`,
  `mode & 0o022 == 0` on the binary, its parent and every ancestor to `/` (a root-owned sticky
  directory is allowed when its immediate child is owned by `geteuid()` or `0`). A candidate whose
  `(st_dev, st_ino)` is `/proc/self/exe`'s stops probing (no loop).
- **Skip vs exit.** An auto-discovered candidate that fails is skipped with one `stderr` line and
  probing continues; `exit 1` (with the sentence naming `BLASTIT_NO_LOCAL_HANDOFF=1`) is reserved
  for the `$VIRTUAL_ENV` the user named. A valid handoff prints **zero** bytes.
- **Exec.** `BLASTIT_HANDOFF_DONE=1` is added to the inherited environment; `\x7fELF` →
  `execveat(bin_fd, "", argv, envp, AT_EMPTY_PATH)`, `#!` → `execve(canonical_path, …)` with
  `parent_fd` + `bin_fd` alive across the call. The second exec is the only path-based one, by
  `binfmt_script`'s requirement, and `argv[0]` is the project's own script so `$0`/`__file__`
  stay correct.

`crates/blastit/tests/critical_handoff.rs` is new (no existing `critical_*.rs` touched):
12 cases over real fixture trees, driven through the real binary.

## Evidence

### The suite (real binary, real trees)

```console
$ cargo test -p blastit --locked --test critical_handoff
running 12 tests
test a_binary_owned_by_another_uid_is_refused ... ok
test the_exec_never_reopens_the_verified_path ... ok
test an_elf_candidate_runs_from_the_verified_descriptor ... ok
test a_candidate_that_is_this_image_does_not_hand_off ... ok
test an_unsafe_activated_venv_is_refused_with_the_fix ... ok
test an_activated_venv_replaces_the_project_venv ... ok
test a_group_or_world_writable_binary_is_skipped_and_the_next_one_runs ... ok
test a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not ... ok
test every_auto_discovered_location_hands_off ... ok
test the_four_commands_hand_off_with_argv_and_environment ... ok
test the_two_switches_control_the_handoff ... ok
test the_production_shapes_never_hand_off ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### Clippy, both feature shapes (the `--no-default-features` leg stays green)

```console
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.47s

$ cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.20s
```

### Critical-test guard

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.42.2.3
critical-guard: HEAD..working tree, 7 changed file(s)
node MB.42.2.3: paths ['crates/**']
  no change to a protected critical test
```

### `strace`: the verified descriptor is the file that runs

`#!` leg (one `execve`, no path-based open between the check and the exec):

```console
$ strace -f -e trace=execve,execveat,openat … target/debug/blastit dev   # cwd has .venv/bin/blastit
7: openat(3, "blastit", O_RDONLY|O_LARGEFILE|O_NOFOLLOW|O_CLOEXEC) = 4
8: execve("/home/remy/blastit-handoff-tests/manual/.venv/bin/blastit", ["/home/remy/blastit-handoff-tests"..., "dev"], …) = 0
$ cat marker
args=dev done=1
```

ELF leg (`execveat` of the very descriptor opened for the checks):

```console
$ strace -f -e trace=execve,execveat,openat … target/debug/blastit dev   # .venv/bin/blastit = /bin/true
7: openat(3, "blastit", O_RDONLY|O_LARGEFILE|O_NOFOLLOW|O_CLOEXEC) = 4
8: execveat(4, "", ["/home/remy/blastit-handoff-tests"..., "dev"], …, AT_EMPTY_PATH) = 0
```

### Size

```console
$ wc -l crates/blastit/src/cli/handoff.rs crates/blastit/tests/critical_handoff.rs
  410 crates/blastit/src/cli/handoff.rs
  547 crates/blastit/tests/critical_handoff.rs
```

### The leaf gate

`make fast-check` is the session's last command; its tail is quoted in the reply (nothing was
edited after it).

## Follow-ups

- **The wrong-uid case needs root to build its fixture.** An unprivileged process cannot create a
  file owned by another uid, so `a_binary_owned_by_another_uid_is_refused` builds the fixture with
  `chown` and prints `skipped: creating another uid's file needs root` otherwise; a root runner
  exercises it. Making it unconditional would need a user namespace the test does not own.
- **This workspace's `target/tmp` cannot host §4's fixtures.** `/home/remy/offworld-nexus/blastit`
  is `0775`, so the suite's own probe finds `CARGO_TARGET_TMPDIR` refused and falls through to
  `$HOME/blastit-handoff-tests`. Worth a `docs/knowledge/LESSONS.md` bullet ("a fixture for §4 must
  live under a tree whose ancestors are not group-writable; `target/tmp` here is not one"), which
  this node cannot write (`docs/**` is out of scope).
- **`handoff`'s exec-failure warning reuses the "skipping unsafe local binary" sentence.** It only
  fires when a verified file cannot be started (wrong ELF interpreter, `ENOMEM`), which the design
  does not give its own wording for; if §4 grows a third sentence, this is the call site.

## Progress (run 2)

`review-arch` confirmed the security core (scope exclusion, the single
`execveat(4,"",…,AT_EMPTY_PATH)` on the verified fd, the permissions algebra, the
fail-closed/skip split) and the ordering was **not** rewritten. Everything below
is the consolidated fix list, plus the two MAJOR defects.

### MAJOR — a pip-installed `blastit` never handed off

`python/blastit/__main__.py` and `python/blastit/_blastit.py` set
`BLASTIT_HANDOFF_DONE=1` unconditionally before exec'ing the bundled binary, so a
`PATH` `blastit` that *is* the wheel's console script could never pick up
`.venv/bin/blastit`, `vendor/bin/blastit` or `node_modules/.bin/blastit` — and the
set was redundant, because `handoff::environment()` already adds `DONE` on the
`execve`. Both setters are gone; the binary is the only setter, and both
docstrings now say why.

Proof through the real console-script path (`__main__` → `_blastit.main` →
binary; `BLASTIT_BIN` stands in for the wheel's `blastit/_bin/blastit`), in a
project whose `./.venv/bin/blastit` writes a marker:

```console
$ cd <project>   # has .venv/bin/blastit
$ env -i PATH=/usr/bin:/bin HOME=$HOME \
    BLASTIT_BIN=<checkout>/target/debug/blastit PYTHONPATH=<checkout>/python \
    uv run --no-project python -m blastit dev
exit=0
$ cat marker
args=dev
done=1
pwd=/home/remy/blastit-handoff-tests/pip-proof
```

The venv binary ran (`done=1` is the binary's own handoff, not the Python
side's), and nothing was printed. Removing the pre-set is what makes the first
line possible at all: with it, the binary skipped the probe and the marker never
appeared.

### MAJOR — two tests that could not fail

- `a_binary_owned_by_another_uid_is_refused` now prints `SKIP …` with `eprintln!`
  (libtest captures `println!`, so the old form reported a pass with zero
  assertions) and, when `CI` is set and the caller is not root, **fails** with a
  sentence saying the rule must not go untested. A root runner remains its real
  home.
- `the_production_shapes_never_hand_off` gained the shapes a grammar change would
  re-open: `["dev","--stage=1"]`, `["test","--stage=1"]`, `["serve","dev"]`,
  `["--role=dbproxy","dev"]`, `["--stage=1","dev"]`, `["dev","--role=dbproxy"]`,
  `["dev","--preset=x"]`, `["--internal-json","dev"]` — each asserting the
  marker was **not** written.

### BLOCKER — the skip line read as a security incident

Both sites now print (before → after, on an ordinary `umask 002` tree):

```text
blastit check: skipping unsafe local binary /p/vendor/bin/blastit (it is mode 0775, so another user could have written it)
blastit check: skipping local binary /p/vendor/bin/blastit (it is mode 0775, so another user could have written it); continuing with the next candidate or the global binary
```

and the ancestor reason is now `{} is mode {:04o}, so another user could replace
the binary`.

### MAJOR — a broken shim was reported as "unsafe" with a bare errno

An `exec_verified` failure has its own sentence:
`blastit {name}: cannot run local binary {} (exec failed: {reason}); continuing
with the next candidate or the global binary` — covered by a new case whose
fixture is a batch file at `.venv/bin/blastit` (the one untested branch).

### MINORs

1. `name()` now takes the word from `argv[0]`, so `blastit test` reports
   `blastit test:` — new test `the_message_names_the_command_the_user_typed`.
2. The four `cannot open`/`cannot stat` messages use `: {err}`, not `({err})`.
3. `to use global blastit` → `to use the global blastit`.
4. `cli.rs`'s composition-root inventory now names `VIRTUAL_ENV`,
   `BLASTIT_HANDOFF_DONE` and `BLASTIT_NO_LOCAL_HANDOFF` beside `PORT` and
   `BLASTIT_PRESET`.
5. `handoff.rs`'s header now says holding `bin_fd` pins the **inode** and does not
   make `execve(path)` resolve to that descriptor — `check_tree` is what prevents
   substitution, and a refactor must keep the rule, not "keep the fd".
6. The no-op `let _open_across_the_call = (&verified.parent, &verified.bin);` is
   gone; the kept-open `Verified::parent` field carries its own doc + `allow(dead_code)`.
7. The third hand-rolled dir open is gone: `blastit_sys::open_dir_nofollow` (new,
   `crates/blastit-sys/src/fd.rs`) is now used by `handoff.rs`, `dev_learn.rs`'s
   `verify_private_dir` and `sock.rs`'s `bind_unix_listener`.
8. The env table gained `("BLASTIT_HANDOFF_DONE", "", true)`; the non-ELF/non-`#!`
   branch has its own test.

### Evidence

```console
$ cargo test -p blastit --locked --test critical_handoff
running 14 tests
test a_binary_owned_by_another_uid_is_refused ... ok
test a_file_that_is_neither_an_elf_image_nor_a_script_is_not_run ... ok
test an_activated_venv_replaces_the_project_venv ... ok
test an_unsafe_activated_venv_is_refused_with_the_fix ... ok
test an_elf_candidate_runs_from_the_verified_descriptor ... ok
test a_candidate_that_is_this_image_does_not_hand_off ... ok
test the_exec_never_reopens_the_verified_path ... ok
test a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not ... ok
test a_group_or_world_writable_binary_is_skipped_and_the_next_one_runs ... ok
test every_auto_discovered_location_hands_off ... ok
test the_message_names_the_command_the_user_typed ... ok
test the_four_commands_hand_off_with_argv_and_environment ... ok
test the_two_switches_control_the_handoff ... ok
test the_production_shapes_never_hand_off ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.30s
$ cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.10s
$ cargo clippy -p blastit-sys --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.31s
$ uvx --offline ruff check python/blastit/__main__.py python/blastit/_blastit.py
All checks passed!
$ wc -l crates/blastit/src/cli/handoff.rs
  414 crates/blastit/src/cli/handoff.rs
```

`make fast-check` is the session's last command; its tail is quoted in the reply
and nothing is edited after it.

### Follow-ups (run 2)

- `run_check`'s own refusal still says `blastit check: …` when it is reached as
  `blastit test` (seen in the alias test's stderr). That sentence belongs to
  `check`, not to the handoff, and renaming it is a `run.rs`/`check` decision, not
  this node's.
- The wrong-uid leg still needs a root runner for its only real assertion; the
  `SKIP`+`CI` guard makes the gap visible instead of silent.
