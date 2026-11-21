# Result MB.3.29

STATUS: done

## What changed

`crates/blastit-php-sapi/src/toolchain.rs`

- `ToolchainError::Spawn { path, detail }`: the kernel refused to start a
  candidate, so the crate cannot tell whether it is usable. Its `Display` names
  the candidate, the OS reason, and the fix (`retry`, or point `PHP_CONFIG` at a
  `php-config` that runs) — never "no php-config found".
- `from_exe` now classifies a `Command::output()` failure instead of mapping
  every one to `NotExecutable`: `ENOENT` (vanished between `is_file` and
  `exec`), `EACCES`/`EPERM` (present, not executable), `ENOEXEC` (an
  executable text file with no `#!`/ELF header) and `ELOOP` (a symlink loop)
  stay `NotExecutable`; everything else becomes `Spawn`. `Spawn` therefore
  keeps only transient or unexplained failures, which is what makes its
  "retry" advice honest. (`ENOEXEC`/`ELOOP` have no stable `ErrorKind`, so
  their numbers are named constants.)
- `discover_with` remembers a `Spawn` failure (from `PHP_CONFIG` and from the
  `PATH` walk) and returns it when no toolchain is found: it can no longer
  swallow "could not ask" into `NotFound`. Precedence stays
  `UnsupportedVersion`, then `IncludeDir`, then `Spawn`, then `NotFound`.
- `run` retries only transient spawn failures (`EAGAIN`/`WouldBlock`,
  `ETXTBSY`/`ExecutableFileBusy`, `Interrupted`): the first try plus three
  attempts, 5 ms apart, then the typed `Spawn`. Non-zero exit and non-UTF-8
  output are unchanged (`Failed` -> `NotExecutable`/`IncludeDir` exactly as
  before). `NotFound`, `NotExecutable`, `UnsupportedVersion` and `IncludeDir`
  sentences are byte-for-byte unchanged.

`crates/blastit-php-sapi/tests/critical_shim_contract.rs` (authorized by this
node)

- `FakeConfig` names its directory `pid-counter-nanos` from a process-wide
  `AtomicU64`, and removes any stale directory with that unique name before
  creating it. Two tests can no longer share a directory, so one `Drop` cannot
  delete the other's script mid-run.
- New twin `a_spawn_failure_is_not_reported_as_not_found`: holds the fake
  `php-config` open for writing so `exec` deterministically fails `ETXTBSY`,
  asserts `ToolchainError::Spawn` naming the candidate, asserts the message
  offers `PHP_CONFIG` and is not the `NotFound` sentence, then releases the
  writer and asserts the same path is discovered as a usable PHP 8.4
  toolchain (the allowed half).
- New twin `a_text_file_named_php_config_is_not_a_spawn_failure`: an
  executable text file with no `#!` fails `exec` with `ENOEXEC`; it must be
  `NotExecutable`/`NotFound` and never `Spawn` with retry advice. The allowed
  half rewrites the same path as a real `php-config` and discovers it.
  (`FakeConfig` gained `write_script`, used by both the constructor and the
  twin; the `0o755` mode survives the in-place rewrite.)

## Root cause

Two independent defects produced the same symptom:

1. **Directory collision.** `FakeConfig::new` keyed its tempdir on
   `pid + SystemTime::now().as_nanos()`. Two tests built in the same
   nanosecond got the same directory; the first `Drop` deleted it while the
   other test was still using its script, so the script was gone (or replaced)
   between `is_file()` and `exec()` — `ENOENT`, then `NotExecutable`, then
   `NotFound`.
2. **Spawn failures were indistinguishable from "not there".**
   `from_exe` mapped *every* `Command::output()` error to `NotExecutable`, and
   `discover_with` discarded `NotExecutable` (`Err(_) => {}`). A transient
   `EAGAIN` (fork under load) or `ETXTBSY` therefore read as "no toolchain
   installed", which is exactly `expected UnsupportedVersion, got NotFound`.

The `ETXTBSY` half is subtle and process-wide: the fd table is shared by all
threads, so a child forked by *any* test while the fake script is open for
writing inherits the write handle. Close-on-exec only releases it when that
child reaches `exec`, so the inode's write reference can outlive the closing
thread by one fork window. A single immediate retry is not enough; the bounded
retry needs a scheduling break (5 ms) between attempts. Verified with a
standalone 16-thread reproducer: ~10 000/12 800 post-close `ETXTBSY` failures
with no delay, 0 with a 1 ms break.

## Evidence

### RED before the fix

```
$ cargo test -q -p blastit-php-sapi --test critical_shim_contract --locked \
      a_spawn_failure_is_not_reported_as_not_found
running 1 test
a_spawn_failure_is_not_reported_as_not_found --- FAILED
thread '...' panicked at crates/blastit-php-sapi/tests/critical_shim_contract.rs:423:5:
a spawn failure must be its own error, got NotFound
```

Review fix, RED before the `ENOEXEC`/`ELOOP` classification (same test, with
the classification term temporarily removed):

```
---- a_text_file_named_php_config_is_not_a_spawn_failure stdout ----
assertion `left == right` failed: a text file is a bad candidate, not an unanswered one
  left: Spawn { path: ".../php-config8.4", detail: "Exec format error (os error 8)" }
 right: NotFound
```

### Acceptance

```
$ cargo test -p blastit-php-sapi --locked
running 5 tests / test result: ok. 5 passed
running 15 tests / test result: ok. 15 passed
running 2 tests / test result: ok. 0 passed (2 ignored, php_native)
test result: ok. 1 passed (doc tests)
rc=0
```

```
$ for i in $(seq 1 50); do cargo test -q -p blastit-php-sapi \
      --test critical_shim_contract --locked || break; done
passed=50/50                      # before the review fix (14 tests)
passed=20/20                      # after the review fix (15 tests)
tail -2 loop 20: test result: ok. 15 passed; 0 failed; 0 ignored
```

Extra: 20/20 under deliberate fork pressure
(`seq 1 40000 | xargs -P 32 -n 1 true` in the background):

```
under fork load passed=20/20
```

```
$ make check        # twice, on the final tree
make check #1 rc=0   ... uv run --no-project tools/plan.py validate / plan OK
make check #2 rc=0   ... uv run --no-project tools/plan.py validate / plan OK
```

```
$ uv run --no-project tools/critical_guard.py --base f4bfcbe --node MB.3.29
critical-guard: f4bfcbe..working tree, 12 changed file(s)
node MB.3.29: paths ['crates/blastit-php-sapi/src/toolchain.rs',
  'crates/blastit-php-sapi/tests/critical_shim_contract.rs']
  M  crates/blastit-php-sapi/tests/critical_shim_contract.rs  [authorized]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.29
```

`cargo fmt -p blastit-php-sapi -- --check`, `cargo clippy -p
blastit-php-sapi --all-targets --locked -- -D warnings` and
`RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-php-sapi --no-deps --locked`
are clean.

### Reproduction of the pre-fix flake (root-cause check)

Running the same 30-iteration loop in the main checkout — pre-fix code, its
own `12 tests` binary, no MB.3.29 change — under the same fork load reproduced
the original symptom at the base line number:

```
running 12 tests
an_unsupported_minor_is_refused_by_name --- FAILED
panicked at crates/blastit-php-sapi/tests/critical_shim_contract.rs:383:18:
expected UnsupportedVersion, got NotFound
```

Same load, this worktree (fixed): 20/20. (Those main-checkout runs only wrote
build artifacts there; no source file was modified.)

## Follow-ups

- `docs/knowledge/LESSONS.md` candidate (out of this node's `paths`, not
  edited): "A child forked by any thread inherits the whole process fd table;
  close-on-exec only drops the fd when the child reaches `exec`. `ETXTBSY` on a
  just-written script can therefore outlive the writer's `close()` by one fork
  window — a retry needs a real scheduling break (>=1 ms), not an immediate
  second attempt."
- `crates/blastit-php-sapi/build.rs` has its own duplicate probe with the same
  weakness: under fork pressure its `probe()` can miss a working `php-config`
  and cache a "no toolchain" build world (observed as a replayed
  `no PHP development toolchain found` warning). Out of scope here; worth a
  follow-up node to reuse/classify the same way.
- During this session the worktree's test binary was observed being rebuilt by
  a concurrent cargo session; concurrent `cargo test` invocations in one
  worktree can momentarily present a different `cfg(php_toolchain)` world.
  Not caused by this change, but don't run two sessions' cargo loops in one
  worktree when collecting flake evidence.
