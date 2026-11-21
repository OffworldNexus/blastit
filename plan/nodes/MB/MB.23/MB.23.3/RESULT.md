# MB.23.3 — `blastit-sys` unprivileged fanotify watcher + the zero-`inotify` guard

## STATUS: done

## Delivered

| File | What |
|---|---|
| `crates/blastit-sys/src/fanotify.rs` (new, 1 577 lines) | The one watcher: `Watcher::{new, mark_dir, read, fd, init_flags, mark_mask, probe_flags}`, `INIT_FLAGS = 0x0c03`, `MARK_MASK`, `READ_BUF`/`ReadBuffer` (16 KiB, `align(8)`), `Event { mask, dir, name }`, `Readout { events, overflow, skipped }`, `HandleKey { fsid, handle_type, handle }`, `parse_records`, `ParseError::ShortBuffer`, `filesystem_type`, and `WatcherError { Unavailable { op, errno }, NoFileHandle { fs_type } }` carried inside the `io::Error` (the crate's `Error` shape). `mark_dir` marks with `FAN_MARK_ADD\|FAN_MARK_ONLYDIR` first (a file is `ENOTDIR`), then probes `name_to_handle_at(AT_HANDLE_FID)` with one `flags = 0` retry on `EINVAL`, keys the table on `(statfs f_fsid, handle_type, handle bytes)`; `read` is a non-blocking drain that maps `EAGAIN`/`EINTR` to an empty readout and `ParseError::ShortBuffer` to `EINVAL`. No `open_by_handle_at`, no raw syscall numbers, no hand-written **wire** structs (the one output-buffer mirror is argued in `## Design deltas` 5), no poll thread, no async. 15 unit tests (layout, mask bits, errno ladder, `NoFileHandle`, magics, handle keys, 8 malformed-record cases, both overflow signals, short buffer, non-UTF-8 name, kernel-version agreement). |
| `crates/blastit-sys/src/lib.rs` | `pub mod fanotify;` + re-export of `Event`, `Readout`, `Watcher`, `WatcherError`. |
| `crates/blastit-sys/tests/critical_fanotify.rs` (new, 810 lines) | The real-kernel protected suite: 4 tests, no skip construct. |
| `scripts/check-no-inotify.sh` (new, 437 lines) | Three checks (`readelf --dyn-syms` + `nm -D` dynamic symbols on every named ELF; `cargo tree -e normal --locked -p blastit`; every tracked **and untracked** source file, including `fanotify_init` and `Watcher::new(` outside the one watcher), one `ok` line on success, and a `--self-test` that plants a call in a scratch copy and in an untracked file, a second constructor call, a symbol in a fake `readelf` dump and a crate in a fake `cargo tree` dump. |
| `Makefile` | `no-inotify` target (builds the binary if missing, then the script + `--self-test`), and the same two lines inside `rs-no-python` right after `check-portable-elf.sh`, so `make check` runs it on `target/debug/blastit`. |
| `docs/knowledge/LESSONS.md` | One entry: the measured host fact (unprivileged `0x0c03` group works on 6.12.117/xfs; `EPERM` for a non-FID group and for Docker's default profile, per research §1.1/§1.7), the `unshare(CLONE_NEWUSER)` empty-`uid_map`/`euid 65534` trap, and `libc`'s private `fsid_t.__val`. |

## Design deltas (deliberate, and why)

1. **`WatcherError` is carried *inside* `io::Result`, not returned instead of it.** Design §4.2 (review finding F6) froze `io::Result` for the whole surface, while `NODE.md` and the dispatch name `WatcherError::Unavailable { op, errno }` / `NoFileHandle { fs_type }`. Both hold: every fallible function returns `io::Result`, and the typed detail travels as the `io::Error`'s inner value (`WatcherError::of(&err)`, `.errno()`, `.errno_name()`), exactly as `blastit_sys::Error` already does. Consequence for `MB.23.5`: read the errno from `WatcherError::of`, because an `io::Error` built that way has no `raw_os_error()` (design §4.2's "the policy names the cause from `err.raw_os_error()`" becomes `WatcherError::of(&err)`).
2. **`parse_records(buf, out)` has no directory table** (its frozen signature), so the events it returns carry an **empty** `Event::dir`; `Watcher::read` is what resolves the handle to the marked path. This keeps the pure seam pure and still lets the malformed-record oracle see every parsed name/mask.
3. **`ReadBuffer` gains `as_mut_slice()`** beside `as_bytes()`, so the suite can prove "the drained descriptor is `EAGAIN`" with a raw `read(2)` into the same buffer type.
4. **The source guard allowlists `crates/blastit-sys/tests/critical_fanotify.rs` for `fanotify_init`** (in addition to `src/fanotify.rs` and `src/dev_reload.rs`): the `FAN_MODIFY` test needs a raw group to prove the mask encoder carries a bit the production mask never asks for. Test crates are linked into no shipped artifact, and check 1 (dynamic symbols) covers the artifacts; the allowlist is literal and documented in the script header.
5. **`HandleStorage` is a hand-written mirror of `libc::file_handle`** — the node's "define **no** structs" line, read literally, is not what the code does, so here is the delta and the argument (the reviewer's finding 5; the unsafe path is deliberately **not** rewritten in this pass). `name_to_handle_at` writes `sizeof(file_handle) + MAX_HANDLE_SZ` bytes at an 8-byte-aligned address, and `libc::file_handle` ends in `f_handle: [c_uchar; 0]`: it is 8 bytes with no payload, so libc's type can be **named** (it is the cast target and the `offset_of!` source) but cannot be the **storage**. The alternatives were (a) an aligned `Vec`/`[u64; N]` buffer plus `ptr.add(offset_of!(file_handle, f_handle))` and `slice::from_raw_parts` to read the handle back — more unsafe, in the hot path, and a place to get the length wrong — or (b) a `#[repr(C, align(8))]` mirror whose payload is a real array, so `bytes()` is a checked slice and the only unsafe left is the one `name_to_handle_at` call itself. The code takes (b). `fanotify::tests::the_measured_layout_is_libcs_layout` pins it field by field (offsets 0/4/8, `size_of::<HandleStorage>() == 8 + HANDLE_MAX`, `align_of == 8`, `size_of::<libc::file_handle>() == 8`), so a libc layout change is a red test rather than a corrupt syscall. A future rewrite that deletes the mirror would allocate `Layout::from_size_align(8 + MAX_HANDLE_SZ, 8)` once, keep it in an owned aligned buffer type, and read the payload through one `// SAFETY:`-documented `from_raw_parts` — the same syscall, one **more** unsafe block, and no layout assertion left to fail. The mirror is therefore the smaller unsafe surface, and it is not a wire struct: the kernel's `fanotify_event_metadata`, `fanotify_event_info_header` and `fanotify_event_info_fid` are all libc's, read with `offset_of!` and the `u16_at`/`u32_at`/`u64_at` helpers, exactly as the node asks.

## Acceptance — commands and their tails

```console
$ cargo test -p blastit-sys --locked fanotify
test fanotify::tests::the_measured_layout_is_libcs_layout ... ok
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 134 filtered out; finished in 0.00s
# (the `critical_fanotify` target runs 0 of 4 tests under this filter, as expected)

$ cargo test -p blastit-sys --locked --test critical_fanotify
running 4 tests
test the_frozen_flags_and_mask_are_the_measured_ones ... ok
test modify_is_carried_when_a_group_asks_for_it ... ok
test an_overlay_parent_handle_is_the_at_handle_fid_handle ... ok
test create_close_rename_delete_and_a_non_utf8_name_on_both_filesystems ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s

$ cargo test -p blastit-sys --locked --doc
running 8 tests
test crates/blastit-sys/src/fanotify.rs - fanotify::parse_records (line 880) ... ok
test crates/blastit-sys/src/lib.rs - (line 111) ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ cargo clippy -p blastit-sys --all-targets --locked -- -D warnings
    Checking blastit-sys v0.2.0 (/…/.worktrees/release-0.2.0/crates/blastit-sys)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s

$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 449 source files (fanotify_init only in its one watcher)

$ bash -n scripts/check-no-inotify.sh
bash -n exit=0

$ make check
uv run --no-project tools/guard.py
guard: OK (3 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 6 changed file(s)
  no change to a protected critical test
… (rs-fmt, rs-lint, rs-doc, rs-test, test-critical, py-lint, docs-unit, docs-check all green) …
FAIL  docs/reference/cli.md  fence 1  dev-flags  2.2s  blastit dev exited before ready; stdout:
stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 27.1s
make: *** [Makefile:363: docs-test] Error 1
make check exit=2
```

`make check`'s **only** failure is the one the brief names: `docs/reference/cli.md` fence 1 needs
`--no-reload`, which `MB.23.5` delivers (grep of the whole log: `FAIL` × 1). Because `make check`
stops there, every target after `docs-test` was also run on its own, and each is green:

```console
$ for t in rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test \
           doc-tone manifest-purity zed-check plan-validate; do make $t; done
rs-no-python exit=0            # includes scripts/check-no-inotify.sh + --self-test
seam-check exit=0
platform-matrix-check exit=0
workflow-check exit=0          # workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test exit=0    # packaging-gates-test: ok — 49 cases passed
doc-tone exit=0                # doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity exit=0         # manifest-purity: ok — 17 source file(s), no std::env
zed-check exit=0               # OK
plan-validate exit=0           # plan OK

$ make no-inotify
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 449 source files (fanotify_init only in its one watcher)
ok: self-test: a clean source tree passes
ok: self-test: a planted inotify_add_watch is refused and named
ok: self-test: a second fanotify_init is refused and named
ok: self-test: fanotify_init in the one watcher module is allowed
ok: self-test: a planted inotify dynamic symbol is refused
ok: self-test: a fanotify symbol is not an inotify symbol
ok: self-test: a planted notify crate is refused
ok: self-test: a clean dependency tree passes
ok: self-test: the guard fails on a planted call, a planted symbol and a planted crate

$ uv run --no-project tools/critical_guard.py --base b2196e8 --node MB.23.3
critical-guard: b2196e8..working tree, 6 changed file(s)
node MB.23.3: paths ['crates/blastit-sys/src/fanotify.rs', 'crates/blastit-sys/src/lib.rs',
  'crates/blastit-sys/tests/critical_fanotify.rs', 'scripts/check-no-inotify.sh', 'Makefile',
  'docs/knowledge/LESSONS.md']
  no change to a protected critical test
exit=0

$ git diff --stat -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(no output: all three byte-identical)
```

### What the critical suite proves, per acceptance item

1. On **both** a tmpfs (`/dev/shm`, discovered by `filesystem_type`) and this checkout's filesystem
   (`CARGO_TARGET_TMPDIR`, xfs here): create-write-close → `FAN_CLOSE_WRITE` with `event.dir` equal
   to the marked directory; `write tmp` + `rename` → `FAN_MOVED_TO` for the saved name and
   `FAN_MOVED_FROM` for the old one, and **no** `FAN_CREATE`-only event for the saved name; unlink →
   `FAN_DELETE`; `mkdir` → `FAN_CREATE|FAN_ONDIR`; a non-UTF-8 name (`caf\xe9.py`) arrives as those
   exact bytes and differs from its lossy form; a write without close produces no
   `FAN_CLOSE_WRITE`; marking a file is `ENOTDIR` (typed `Unavailable { op: Mark, errno: ENOTDIR }`);
   a drained descriptor is a raw `EAGAIN` and the wrapper reports an empty readout. Every mask is
   tested with `mask & bit == bit`, never `==`.
2. `FAN_MODIFY` reaches `parse_records` unchanged from a test group that asks for it, while
   `MARK_MASK & FAN_MODIFY == 0` and `Watcher::mark_mask() & FAN_MODIFY == 0` (also in the unit
   tests, with `FAN_ACCESS`, `FAN_CLOSE_NOWRITE`, `FAN_Q_OVERFLOW`, `FAN_RENAME`).
3. A forked child does `unshare(CLONE_NEWUSER|CLONE_NEWNS)`, writes its own `setgroups`/`uid_map`/
   `gid_map`, mounts overlayfs over a tmpfs lower/upper/work, asserts `name_to_handle_at` with
   `AT_HANDLE_FID` succeeds while `flags = 0` is `EOPNOTSUPP`, then `Watcher::mark_dir` on the
   overlay succeeds and a write through it resolves back to the overlay directory — so a regression
   to `flags = 0` fails (it would be `EOPNOTSUPP` at `mark_dir`). A refused `unshare`/mount is a
   failure whose message names the step and the errno, never a skip.
4. Unit test: `ENOSYS`, `EPERM`, `EINVAL`, `EOPNOTSUPP`, `ENOSPC`, `EMFILE`, `ENOTDIR` each map to
   `Unavailable { op, errno }` with the injecting op, an errno-less error is `EIO`, a foreign error
   carries no classification, an unknown errno renders `E<number>`, and `NoFileHandle` names the
   filesystem `filesystem_type` reports (plus the fix) while `ENOSYS`/`EACCES` from the probe stay
   `Unavailable`.
5. `scripts/check-no-inotify.sh target/debug/blastit` prints one `ok` line and exits 0; its
   `--self-test` plants `inotify_add_watch` in a scratch copy of a source file and requires exit 1
   with the file named (shown above).
6. `make check` is green except the documented `--no-reload` fence; `Cargo.toml`, `Cargo.lock` and
   `seccomp.rs` are untouched.

## Polish pass (reviewer PASS with six MINOR findings)

Five were real holes in the guard or the tests, one was a stated-constraint deviation. All five code
findings are fixed; the sixth is a design-doc note for `MB.23.5`. No assertion was weakened, no
`#[ignore]` added, no dependency added, `Cargo.toml`/`Cargo.lock` byte-identical.

| # | Finding | Fix |
|---|---|---|
| 1 | `check-no-inotify.sh` listed sources with `git ls-files` only, so an untracked file was invisible — today that is this node's own `fanotify.rs` and `critical_fanotify.rs`, so the allowlist branches were never exercised by the real run | `list_sources` now unions `git ls-files -z` with `git ls-files -z --others --exclude-standard`, and the whole path stays NUL-safe (`grep -zE`, `read -r -d ''`, count via `tr -cd '\0' \| wc -c`). The `--self-test` now makes its scratch tree a real git repository, tracks the clean file, and plants the call in an **untracked** file, which must be refused and named |
| 2 | check 3 banned the literal `fanotify_init` only, while `Watcher::new()` is `pub` and re-exported (`lib.rs`), so any crate could open a second group without tripping the guard | new literal ban on `Watcher::new(` outside the same three files (`src/fanotify.rs`, `src/dev_reload.rs`, `tests/critical_fanotify.rs`), with its own message and fix; the self-test plants it in `crates/blastit-sys/src/second.rs` (refused, named) and in `crates/blastit/src/dev_reload.rs` (allowed) |
| 3 | `child_refused(..., err.raw_os_error())` on classified watcher errors, which carry no errno, so the child's stderr omitted the number the parent's message promises | new `watcher_errno(&io::Error)` = `WatcherError::of(err).map(WatcherError::errno).or_else(\|\| err.raw_os_error())`, used at all three sites (`Watcher::new`, `mark_dir`, `read`) |
| 4 | only `FAN_Q_OVERFLOW` counted as "events dropped", so research §1.6's second signal (an event with no info record) was counted `skipped` | `walk` treats `metadata_len == event_len` as `overflow = true` too (a `FAN_REPORT_FID` group attaches a fid record to every event it reports); new unit test feeds both signals to `parse_records`, the existing overflow assertions are untouched |
| 5 | `HandleStorage` is a hand-written mirror of `libc::file_handle` while the node says "define NO structs" | recorded as an argued design delta (`## Design deltas` item 5): what it is, why libc's type can be named but not allocated, the test that pins its layout, and what a rewrite would look like. The unsafe path is unchanged in this pass |
| 6 | design §4.2's "no separate error enum" / "`err.raw_os_error()`" lines are wrong for the delivered code | noted only (not this node's `paths`): `MB.23.5` must amend both lines to `WatcherError::of(&err)`; folded into follow-up 1 |

Two side effects worth naming. **Coverage changed on purpose**: the real run now scans 452 source
files instead of 449 — the three extra are exactly this node's new, still-untracked
`crates/blastit-sys/src/fanotify.rs`, `crates/blastit-sys/tests/critical_fanotify.rs` and
`scripts/check-no-inotify.sh`, so the allowlist branches are exercised by the real run rather than
only by the self-test. And the guard caught its **own** first draft of the `Makefile` comment (which
spelled both banned literals), so the comment now says "the group syscall and the watcher
constructor" — evidence, not a workaround: the literal ban really is literal.

```console
$ cargo test -p blastit-sys --locked fanotify
test fanotify::tests::parse_records_counts_a_record_without_an_info_area_as_overflow ... ok
test fanotify::tests::parse_records_reports_overflow_and_a_short_buffer ... ok
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 134 filtered out; finished in 0.00s

$ cargo test -p blastit-sys --locked --test critical_fanotify
running 4 tests
test the_frozen_flags_and_mask_are_the_measured_ones ... ok
test modify_is_carried_when_a_group_asks_for_it ... ok
test an_overlay_parent_handle_is_the_at_handle_fid_handle ... ok
test create_close_rename_delete_and_a_non_utf8_name_on_both_filesystems ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s

$ cargo test -p blastit-sys --locked --doc
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ uv run --no-project tools/critical_guard.py --base b2196e8 --node MB.23.3
critical-guard: b2196e8..working tree, 8 changed file(s)
node MB.23.3: paths ['crates/blastit-sys/src/fanotify.rs', 'crates/blastit-sys/src/lib.rs',
  'crates/blastit-sys/tests/critical_fanotify.rs', 'scripts/check-no-inotify.sh', 'Makefile',
  'docs/knowledge/LESSONS.md']
  no change to a protected critical test
exit=0

$ cargo clippy -p blastit-sys --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.77s

$ cargo fmt --all -- --check
fmt exit=0

$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.52s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 452 source files (one fanotify group, one Watcher::new)
guard exit=0

$ bash -n scripts/check-no-inotify.sh && make no-inotify
bash -n exit=0
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 452 source files (one fanotify group, one Watcher::new)
ok: self-test: a clean source tree passes
ok: self-test: a planted call in an untracked file is refused and named
ok: self-test: a planted inotify_add_watch is refused and named
ok: self-test: a second fanotify_init is refused and named
ok: self-test: fanotify_init in the one watcher module is allowed
ok: self-test: a second Watcher::new is refused and named
ok: self-test: Watcher::new in the one policy module is allowed
ok: self-test: a planted inotify dynamic symbol is refused
ok: self-test: a fanotify symbol is not an inotify symbol
ok: self-test: a planted notify crate is refused
ok: self-test: a clean dependency tree passes
ok: self-test: the guard fails on a planted call (tracked or untracked), a second watcher, a planted symbol and a planted crate
make no-inotify exit=0

$ scripts/check-no-inotify.sh --self-test        # same 12 lines as above, exit=0
```

The two negative cases were also run against the real script, outside `--self-test`, so the refusal is
not only a self-test claim:

```console
# find branch (a non-repo scratch root), planted Watcher::new(
$ scripts/check-no-inotify.sh --source-root /tmp/neg-find.XXXX target/debug/blastit
error: crates/x/src/a.rs opens a second watcher outside the one policy:
  crates/x/src/a.rs:1:pub fn a() { let _ = Watcher::new(); }
  fix: one watcher, opened in crates/blastit/src/dev_reload.rs (ADR-0014).
exit=1

# git branch, the plant in an untracked file of a scratch repository
$ git -C /tmp/neg-git.XXXX status --short
A  crates/blastit-sys/src/lib.rs
?? crates/blastit-sys/src/new_untracked.rs
$ scripts/check-no-inotify.sh --source-root /tmp/neg-git.XXXX target/debug/blastit
error: crates/blastit-sys/src/new_untracked.rs creates a fanotify group outside the one watcher:
  crates/blastit-sys/src/new_untracked.rs:1:pub fn b() { let _ = fanotify_init(0, 0); }
  fix: there is exactly one group, in crates/blastit-sys/src/fanotify.rs (ADR-0014).
exit=1
```

`make check`/`make e2e` were **not** re-run: the reviewer's green run on this tree stands, and the
edits touch only `blastit-sys`, the guard script and a `Makefile` comment. `cargo test -p blastit-sys
--locked --doc` was re-run because a public doc comment (`Readout::overflow`) changed. The working
tree also carries `MB.23.5`'s `NODE.md`/`LOG.md` edits from the concurrent session (which is why
`critical_guard` counts 8 changed files); this pass touched none of them.

## Follow-ups (out of scope for this node)

- **`MB.23.5`**: classify a failure with `WatcherError::of(&err)` (`.errno()`, `.errno_name()`), not
  `io::Error::raw_os_error()`, which the typed wrapper does not carry — and **amend design §4.2's two
  wrong lines while you are there**: "no separate error enum" (there *is* one: `WatcherError`, carried
  inside the `io::Error`) and "the policy names the cause from `err.raw_os_error()`" (it must be
  `WatcherError::of(&err).map(WatcherError::errno).or_else(|| err.raw_os_error())`, which is exactly
  what `critical_fanotify.rs` now does for the child's stderr). The ladder's sentences are already
  rendered by `WatcherError`'s `Display` (`<op> failed: <ERRNO>; <fix>`), so the boot warning can be
  built from the typed error plus `filesystem_type`.
- **`MB.23.5`**: `Watcher::mark_dir` marks before it probes, so a probe failure leaves that one mark
  in place. That is harmless for the documented "latch the watcher dead and serve" disposition, but
  the policy must not retry marking after it has stopped.
- **`MB.23.6`**: re-run `scripts/check-no-inotify.sh` on the wheel's `_blastit*.so` (planned); the
  script already takes any number of ELF paths and fails on a missing file.
- The tmpfs leg needs a real tmpfs mount (`/dev/shm` here). A host without one fails with a message
  naming the candidates rather than silently testing one filesystem twice; recorded in `LESSONS.md`.
- `docs/design/dev-fanotify-reload.md` §4.2's `err.raw_os_error()` sentence is now
  `WatcherError::of(&err)`; that page is not in this node's `paths`, so `MB.23.5`/`MB.23.6` should
  fold the one-line correction in when they next touch the design.
