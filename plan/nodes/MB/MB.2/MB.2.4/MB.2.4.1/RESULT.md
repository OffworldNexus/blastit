# RESULT MB.2.4.1 — resolve request paths beneath a static root (design MB.2.4a)

**STATUS: done**

## Summary

`crates/blastit-scheduler/src/static_asset.rs` (new) implements the resolver half of
design §4.2/§5: one-decode `segments` (exactly one leading `/`, interior `//`
collapsed, `.` dropped, `..`/backslash/NUL-C0/over-long refused), `is_dotfile`,
the pure `classify` table, `ImmutableGlob`, the `(st_dev, st_ino)` `same_inode` check,
`StaticRoot::{open, open_with, resolve, resolve_with}`, `STATIC_RESOLVE_FLAGS` and the
`Backend::Walk` fallback. The boundary is one `O_PATH|O_DIRECTORY|O_CLOEXEC` dirfd per
root, `openat2` with `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS` for
every probe, an `O_PATH` classification by `fstat`, and an
`O_RDONLY|O_CLOEXEC|O_NONBLOCK|O_NOCTTY` read open only for a regular file, identity
checked against the probe. Never `canonicalize()` + string prefix. The errno table is
total: `ENOENT`/`ENOTDIR` → `Missing`, every other errno → `Hidden`; nothing falls
through. No `unsafe`, no `unwrap`/`expect`/`panic!`/`todo!` and no unchecked indexing
on any request-reachable path (`lib.rs` still `#![forbid(unsafe_code)]`).

`SchedulerConfig` gained `manifest_dir: PathBuf` and `static_max_bytes: u64`
(`STATIC_MAX_FILE_BYTES`, no env var, no manifest key), both with defaults, so the two
existing struct literals keep compiling through `..Default::default()`.

New protected suite `tests/critical_static_resolve.rs` (14 tests) drives the design's
§5.2 matrix over the real S/F/N/D fixture tree on **both** backends, plus the
caller-supplied-segment escape row; the kernel exercised `Openat2` for the requested
`Openat2` root and `Walk` for the requested `Walk` root (recorded below). The
`resolve_with` hook proves the TOCTOU arm: rename the probed leaf away (the probe fd
stays open until the identity check, so a reused inode number cannot satisfy it), write
a new file in its place, expect `Hidden` — with a no-hook control proving the same
lookup serves the original bytes.

`resolve_with` refuses `..`, empty segments and embedded `/` locally (it is `pub`), so
the walk's "cannot leave the root" is a property of this module, not of `segments()`'s
caller; the two backends answer that row identically.

Two real bugs were caught by the suite while building it, and fixed in the resolver
rather than in the test: `/link2/x` answered the SPA fallback on `Walk` (an
`O_PATH|O_NOFOLLOW|O_DIRECTORY` open of a symlink is `ENOTDIR` there, not `ELOOP`), and
`/` on `Walk` answered `SpaFallback` (the empty relative path reached an empty-name
lookup instead of the root dirfd). A blanket-refusing resolver was also checked to fail
the suite (row 14's accepted twin), so the refusals are not vacuous.

## Evidence

Base: `40ba576`. Rust: `cargo 1.95.0` (workspace rust-version 1.88), branch
`release/0.2.0`.

### A — `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked`

```console
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### B — `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_resolve --locked`

```console
running 14 tests
test classify_is_total_over_every_file_type ... ok
test same_inode_compares_device_and_inode ... ok
test immutable_globs_match_the_design_vectors ... ok
test segments_refuses_every_malformed_shape ... ok
test the_documented_limits_and_flags_are_the_ones_asserted ... ok
test the_dotfile_rule_exempts_exactly_dot_well_known ... ok
test segments_accepts_every_legal_shape ... ok
test an_unopenable_root_names_the_root_it_could_not_open ... ok
test an_spa_route_whose_index_is_missing_is_spa_lost ... ok
test caller_supplied_segments_cannot_leave_the_root ... ok
test the_cafe_file_is_the_nfc_bytes_and_nothing_else ... ok
test the_production_head_refuses_what_segments_never_sees ... ok
test a_leaf_swapped_between_the_probe_and_the_read_is_hidden ... ok
test every_matrix_row_answers_the_same_on_both_backends ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

Backend record (`--nocapture`, one line per fixture/backend):

```console
S: requested Openat2, kernel exercised Openat2
S: requested Walk, kernel exercised Walk
F: requested Openat2, kernel exercised Openat2
F: requested Walk, kernel exercised Walk
N: requested Openat2, kernel exercised Openat2
N: requested Walk, kernel exercised Walk
D: requested Openat2, kernel exercised Openat2
D: requested Walk, kernel exercised Walk
```

Mutation sanity (manual, reverted before the final runs): making the composed verdict
answer `Hidden` for every well-formed path fails the suite on row 14's accepted twin
(`expected file:SpaFallback:/index.html:43, got hidden on Openat2`), and the two bugs
noted above failed rows 23 and 35 respectively.

### C — `cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings`

```console
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.54s
```

`make rs-lint` (workspace clippy, the node's acceptance line) also passes:

```console
PYTHONHOME="…/cpython-3.14.10-linux-x86_64-gnu" PYO3_PYTHON="…/python3.14" LD_LIBRARY_PATH="…/lib" \
  cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Checking blastit v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit)
    Checking blastit-py v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.25s
```

### D — `RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler --no-deps --locked`

```console
 Documenting blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.28s
   Generated /home/remy/offworld-nexus/blastit/target/doc/blastit_scheduler/index.html
```

### E — `cargo fmt --all -- --check`

```console
fmt: clean
```

### F — `uv run --no-project tools/critical_guard.py --base 40ba576 --node MB.2.4.1`

```console
critical-guard: 40ba576..working tree, 12 changed file(s)
node MB.2.4.1: paths ['crates/blastit-scheduler/src/static_asset.rs', 'crates/blastit-scheduler/src/config.rs', 'crates/blastit-scheduler/src/lib.rs', 'crates/blastit-scheduler/tests/critical_static_resolve.rs', 'crates/blastit-scheduler/tests/common/static_fixture.rs']
  no change to a protected critical test
```

### G — `CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked`

```console
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_static_resolve.rs (target/debug/…/critical_static_resolve-…)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
… (admission 11, critical_ingress_enforcement 9, ctl_channel 9, fairness 17, flow 6,
   head_allocs 2, isolation 13, lifecycle 18, memory 14, routing 14, doc-tests 1)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

(`make check` / `make e2e` were deliberately not run: the lead gates those.)

## Matrix, row by row

Every row is executed against fixture **S**, **F**, **N** and **D** on `Backend::Openat2`
**and** `Backend::Walk`; `Spa` becomes `Missing` for F/N (their forward/404 split is
`decide`'s, `MB.2.5.1`) and every `File` answer's bytes and `len` are compared with the
fixture file (`content_of`), so a wrong or empty `200` cannot pass.

| # | Request (raw, decoded once) | Expected (S / F / N) | Asserted |
|---|---|---|---|
| 1 | `/../etc/passwd` | Refused(Traversal) ×3 | `StaticOutcome::Refused(Traversal)` |
| 2 | `/%2e%2e/%2e%2e/etc/passwd` | Refused(Traversal) ×3 | Traversal |
| 3 | `/..%2f..%2fetc%2fpasswd` | Refused(Traversal) ×3 | Traversal |
| 4 | `/assets/app.…js%2f..%2f..%2fetc%2fpasswd` | Refused(Traversal) ×3 | Traversal |
| 5 | `/%2Fetc/passwd` | Refused(Absolute) ×3 | Absolute |
| 6 | `//etc/passwd` | Refused(Absolute) ×3 | Absolute |
| 7 | `/foo%5c..%5cbar` | Refused(Backslash) ×3 | Backslash |
| 8 | `/%c0%ae%c0%ae/etc/passwd` | Refused(Encoding) ×3 | decode → `None`; Encoding |
| 9 | `/%00.js` | Refused(Control) ×3 | Control |
| 10 | `/%1f.js` | Refused(Control) ×3 | Control |
| 11 | `/%ff.js` | Refused(Encoding) ×3 | decode → `None`; Encoding |
| 12 | `/%zz` | Refused(Encoding) ×3 | decode → `None`; Encoding |
| 13 | `/` + 4096 bytes + `.js` | Refused(TooLong) ×3 | TooLong |
| 14 | `/%252e%252e/%252e%252e/etc/passwd` | Spa / Missing / Missing | SpaFallback(/index.html), Missing |
| 15 | `/.env` | Hidden ×3 | Hidden (dotfile, step 4) |
| 16 | `/.git/config` | Hidden ×3 | Hidden |
| 17 | `/.git/` | Hidden ×3 | Hidden |
| 18 | `/.well-known/acme-challenge/token` | Exact(token) ×3 | Exact, bytes + len |
| 19 | `/.well-known/missing` | Spa / Missing / Missing | SpaFallback, Missing |
| 20 | `/.well-known/../.env` | Refused(Traversal) ×3 | Traversal |
| 21 | `/.well-known/.env` | Hidden ×3 | Hidden |
| 22 | `/link` (→ /etc/passwd) | Hidden ×3 | Hidden (`ELOOP`/`Symlink`) |
| 23 | `/link2/x` | Hidden ×3 | Hidden (`ELOOP` on both backends) |
| 24 | `/fifo` · `/sock` | Hidden ×3 | Hidden (Fifo/Socket, never read) |
| 25 | `/mode000` | Hidden ×3 | Hidden (probe ok, read open `EACCES`) |
| 26 | `/big.bin` (> cap) | Hidden ×3 | Hidden (`classify` before any read) |
| 27 | `/assets` (dir, no index) | Spa / Missing / Missing | SpaFallback, Missing |
| 28 | `/docs` and `/docs/` | DirectoryIndex(/docs/index.html) ×3 | same `url_path`, bytes, len |
| 29 | `/missing` | Spa / Missing / Missing | SpaFallback, Missing |
| 30 | `/docs%2Findex.html` | Exact(/docs/index.html) ×3 | Exact |
| 31 | `/DOCS` | Spa / Missing / Missing | SpaFallback, Missing |
| 32 | `/assets/app.…js?v=1` | Exact(asset) ×3 | Exact (query never resolved) |
| 33 | `/assets/app.…js/` | Exact(asset) ×3 | Exact, one `url_path` |
| 34 | `/index.html` (D) | Exact(/index.html) | Exact (globbable by D's pattern) |
| 35 | `/` and `/dashboard` (D) | DirectoryIndex / Spa | DirectoryIndex, SpaFallback (kind drives no-cache) |
| 36 | `/device` | e2e only (needs root) | `classify(CharacterDevice)` = `Err(Hidden)` |
| 37 | `/caf%C3%A9.js` (NFC) | Exact(/café.js) ×3 | Exact, NFC bytes |
| 38 | `/cafe%CC%81.js` (NFD) | Spa / Missing / Missing | SpaFallback, Missing |
| 39 | `/proclink/self/fd/0` | Hidden ×3 | Hidden (`ELOOP` at `proclink`) |
| 40 | `/assets//app.…js` | Exact(asset) ×3 | Exact, single `/` `url_path` |
| escape | `["..", …(to `/`), "etc", "passwd"]`, `[".."]`, `["assets","..","..","etc","passwd"]`, `[""]`, `["assets","","app.js"]`, `["assets/app.js"]` — caller-supplied segments, not a request target | Hidden on both backends | `Hidden` from `resolve` **and** `resolve_with`; report-time check: with the guard removed `Walk` answers `file:Exact:/../../../etc/passwd:1440` while `Openat2` answers `Hidden` |

`STATIC_RESOLVE_FLAGS` is asserted equal to
`BENEATH|NO_SYMLINKS|NO_MAGICLINKS` and to *not* contain `NO_XDEV`, `IN_ROOT` or
`CACHED`; `classify` is tabled over `RegularFile` (in cap, at cap, over cap,
zero-length), `Directory`, `Fifo`, `Socket`, `CharacterDevice`, `BlockDevice`,
`Symlink`, `Unknown`; `same_inode` over equal, different-inode, different-device;
`ImmutableGlob` over every design §4.5 vector plus its `None` cases; `segments` over
`/`, `/a//b`, `/x.js/`, `.`-dropping, `%2e`-as-data, 255-byte segment, exactly-4096
path, `//x`, `///x`, no-leading-`/`, empty, backslash, NUL, `0x1f`, DEL (accepted),
4097-byte path, 256-byte segment.

## Review fixups (attempt 2)

Four findings, four exact fixes; nothing else was restructured.

### 1 [MAJOR] the walk could leave the root through a caller-supplied `..`

`resolve_with` now rejects any segment that is `..`, empty, or contains `/` before any
syscall (`static_asset.rs`, top of `resolve_with`), so "the walk cannot leave the root"
is a property of this module and not of `segments()`'s caller. The empty **vector** is
still the root directory. New protected test
`caller_supplied_segments_cannot_leave_the_root` runs `Walk` **and** `Openat2` over
`[".."; depth→/] + ["etc","passwd"]`, `[".."]`, `["assets","..","..","etc","passwd"]`,
`[""]`, `["assets","","app.js"]`, `["assets/app.js"]`, calling both `resolve` and
`resolve_with`, and asserts `Hidden` for each plus a still-serving `resolve(&[])`.

Evidence that the row is not vacuous — the same test with the guard removed (`if false
&& segments…`), then restored:

```console
Walk: resolve(["..", "..", "..", "etc", "passwd"]) must be Hidden, got file:Exact:/../../../etc/passwd:1440
```

(`Openat2` answers `Hidden` for the same vector — that divergence was the bypass.)

### 2 [MINOR] the probe fd was dropped before the read open

`finish` keeps `probe` open across `open_beneath` and `fstat`, evaluates
`same_inode(...)`, and only then `drop(probe)`; the probed inode is pinned, so a reused
inode number at the same path cannot satisfy the identity check. The substitution test
(`a_leaf_swapped_between_the_probe_and_the_read_is_hidden`, both backends) still passes
and still keeps the old inode alive under `swap.old.js` so the numbers cannot collide.

### 3 [MINOR] the suite tested a copy of the decoder and the dotfile rule

`static_asset::head_segments` (`#[doc(hidden)] pub`, decoded target → production query
cut, production `percent_decode`, `segments`, production `is_dotfile` → segments or the
final `StaticOutcome`) now backs the suite's `verdict`, and
`the_production_head_refuses_what_segments_never_sees` table-drives rows 8/11/12 (via
`head_segments` → `Refused(Encoding)`), rows 15–17/21 (`Hidden` by the production
dotfile rule), row 14 (`Ok(["%2e%2e","%2e%2e","etc","passwd"])` — one decode) and row 32
(the query is cut by the production decoder). The test-local `percent_decode`/`hex`/
`verdict` mirror is deleted, so deleting `is_dotfile` from a later node now fails this
suite.

### 4 [MINOR] the extra `backend()` accessor is gone

`StaticRoot` is back to design §4.2's surface: `backend` is a private field, the
`#[doc(hidden)] pub fn backend()` is removed, and the suite records the exercised
backend from its own `openat2` kernel probe (`kernel_supports_openat2`) when printing
`<fixture>: requested <backend>, kernel exercised <backend>`.

### Re-run tails after the fixes

`cargo test -p blastit-scheduler --locked` (13 targets, all green):

```console
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_static_resolve.rs (target/debug/…/critical_static_resolve-…)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
… ctl_channel 9, fairness 17, flow 6, head_allocs 2, isolation 13, lifecycle 18,
  memory 14, routing 14, doc-tests 1 — all `ok`
```

`cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings`:

```console
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
```

`RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler --no-deps --locked`:

```console
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.46s
   Generated /home/remy/offworld-nexus/blastit/target/doc/blastit_scheduler/index.html
```

`cargo fmt --all -- --check`:

```console
clean
```

`uv run --no-project tools/critical_guard.py --base 40ba576 --node MB.2.4.1`:

```console
node MB.2.4.1: paths ['crates/blastit-scheduler/src/static_asset.rs', 'crates/blastit-scheduler/src/config.rs', 'crates/blastit-scheduler/src/lib.rs', 'crates/blastit-scheduler/tests/critical_static_resolve.rs', 'crates/blastit-scheduler/tests/common/static_fixture.rs']
  A  crates/blastit-scheduler/tests/critical_static_resolve.rs  [added (always allowed)]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.2.4.1
```

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/static_asset.rs` | new, 685 lines, **400 production (non-comment, non-blank) lines** — at the 400 ceiling |
| `crates/blastit-scheduler/src/config.rs` | +14: `manifest_dir`, `static_max_bytes`, defaults |
| `crates/blastit-scheduler/src/lib.rs` | +1: `pub mod static_asset;` |
| `crates/blastit-scheduler/tests/critical_static_resolve.rs` | new protected suite, 14 tests, 868 lines |
| `crates/blastit-scheduler/tests/common/static_fixture.rs` | new S/F/N/D fixture builder (S/F/N/D, `café.js` NFC, `proclink -> /proc`, `fifo`, `sock`, `mode000`, sparse `big.bin`, `swap.js`) |

No other crate, no `Cargo.toml`/`Cargo.lock` edit (`rustix`'s `fs` feature already
covered `openat2`, `mkfifoat`, `ResolveFlags` and `chmod`).

## Deviations (named explicitly)

1. **Line budget overrun.** The node budget says ≤ 600 changed lines; the delivery is
   ~1,700 added lines (400 resolver + ~780 suite + 147 fixture + 15 config/lib). The
   design mandates the 40-row protected matrix over four fixtures and two backends plus
   the walk fallback and the TOCTOU seam; trimming to 600 would cut required rows. The
   resolver itself is inside its 400-line ceiling; the suite is not counted by the
   design's module rule.
2. **`classify` and `Directory`.** Signature is the design's
   `Result<(), StaticOutcome>`; `Directory` is `Err(StaticOutcome::Missing)` ("not a
   file at this path"), which the resolver turns into the index lookup. Documented on
   the function; a directory is never reported to the caller as `Missing` by
   `resolve`.
3. **`StaticRefusal::Encoding` is never returned by `segments`.** `segments` takes an
   already-decoded `&str`; a malformed escape or invalid UTF-8 is refused by the single
   decode in `path.rs` (`400`) before it. The variant exists so the responder's `400`
   mapping is one table; it is asserted through the composed verdict.
4. **Head-step entry point instead of a decode mirror.** `path::percent_decode` is
   `pub(crate)`, so `static_asset::head_segments` (`#[doc(hidden)] pub`, added for the
   attempt-2 review) composes the production query cut, the production one-decode,
   `segments` and the production `is_dotfile`; the suite table-drives rows 8/11/12,
   15–17/21 and 32 through it. There is no test-local decoder or dotfile rule left.
5. **`segments` on a path with no leading `/`** returns `Absolute` (design names the
   refusal only for a second leading `/`). A request target is absolute-form, and the
   shape is the one a naive join can misread; documented on the function.
6. **`SpaLost` for a hidden or directory `root/index`.** Under `spa = true` any
   non-regular index is `SpaLost` (`404`, identical observable) rather than `Hidden`, so
   the fallback has exactly two answers.
7. **No `backend()` accessor.** The attempt-2 review removed the extra
   `#[doc(hidden)] pub fn StaticRoot::backend()`; the backend field is private
   (`pub(crate)`-free) and the suite records the exercised backend from the same
   `openat2` kernel probe in the test (`kernel_supports_openat2`), so design §4.2's
   "exactly §4.2" surface is intact.
8. **`url_path` of an index answer** is the answering file's resolved path
   (`/docs/index.html`, `/index.html`), so `/docs` and `/docs/` share one key and one
   `ETag`; the cache rule keys on `AnswerKind` in `MB.2.4.2`.
9. **Body assertions use byte equality** (`content_of`) rather than a sha256 recomputed
   in the test: every fixture file is a few dozen bytes, and byte equality is strictly
   stronger than a hash comparison. The bytes and `len` are both asserted.
10. **`#[allow(dead_code)]` on `StaticRoot::{root, immutable}`**: they are the fields
    the design assigns to `MB.2.4.2` (`respond`'s glob read) and to the boot message
    (`MB.2.5.2`/`MB.2.6`); this leaf only stores them, and workspace clippy denies the
    warning. Each carries the reason inline.

## Follow-ups

1. `docs/knowledge/LESSONS.md` (out of this node's paths): `O_PATH|O_NOFOLLOW` on a
   symlink **succeeds** and opens the link itself, so `O_PATH|O_NOFOLLOW|O_DIRECTORY`
   on a symlink is `ENOTDIR` (not `ELOOP`) — a per-segment walk must `fstat` each
   intermediate and map `Symlink` to `ELOOP` by hand to answer identically to
   `openat2(RESOLVE_NO_SYMLINKS)`. Also: `openat2(dirfd, "", …)` is not the root
   directory; the empty relative path has to become `"."`.
2. `MB.2.5.2` must set `StaticRoot.max_bytes` from `SchedulerConfig.static_max_bytes`
   immediately after `open` (the field is `pub(crate)` and defaults to
   `STATIC_MAX_FILE_BYTES`), and open roots with `manifest_dir` from the stage-1
   document. Its head path should call `static_asset::head_segments` so the decode,
   `segments` and dotfile rule are wired exactly once (that is the entry point the
   protected suite drives).
3. `MB.2.4.2` consumes `StaticRoot.{immutable, max_bytes}` (`pub(crate)`) and must key
   the year-long cache on `AnswerKind::Exact` only, using `ImmutableGlob::matches` on
   `StaticFile::url_path`.
4. The node's `# Acceptance` names `make rs-lint` / `make critical-guard`; this run used
   the per-crate clippy the dispatch lists **and** `make rs-lint` (both green), plus
   `tools/critical_guard.py --base 40ba576 --node MB.2.4.1`. That base is the node's
   declared base (the dispatch's); `develop` is at `7dfa863`, so
   `make critical-guard BASE=develop` would additionally diff every release-branch
   commit ahead of `develop` and is the lead's call at the gate.
