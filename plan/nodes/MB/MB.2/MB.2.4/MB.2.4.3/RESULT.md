# RESULT — MB.2.4.3 (gaps G1–G4): pin the resolver's open contract, both ceilings and the space byte

STATUS: done

Base `cb7a1e4` on `release/0.2.0`. **Test-only.** Landed
`crates/blastit-scheduler/tests/critical_static_resolve.rs` (+285/−9) and
`crates/blastit-scheduler/tests/common/static_fixture.rs` (+25). No product file, no
other suite, no manifest, doc or plan frontmatter was touched; `src/static_asset.rs` is
byte-identical to `b8e8806` (so the audit's mutant line numbers still hold).

## Summary

The audit left 8 meaningful `static_asset.rs` mutants alive because the protected suite
only *recomputed* the one behaviour it claimed to assert. Each gap is now closed with an
accepted case **and** its denied twin, in the design's own language:

- **G1 — the backend is observed, not copied.** `recorded_backend()` reads back the
  selector `open_with` actually stored, from `StaticRoot`'s derived `Debug`
  (`backend: Openat2`), which is the only unprivileged oracle §4.2 allows (the field
  stays private; no `backend()` accessor was added). `every_matrix_row_answers_the_same_on_both_backends`
  now asserts it per fixture, and `open_with_records_the_backend_it_was_asked_for_and_open_asks_for_openat2`
  pins `Walk` ⇒ `Walk` on every kernel, `Openat2` ⇒ the kernel's answer, and
  `StaticRoot::open` (the production entry `run` calls) ⇒ the `openat2` probe. The suite
  log now reads `S: requested Openat2, recorded Openat2` / `requested Walk, recorded Walk`
  on this kernel, and branches on `kernel_supports_openat2()` — never `#[ignore]`.
- **G2 — a symlink-declared root opens.** §5.1's "the only place a symlink is followed,
  and only on the operator's own declaration" is now asserted: `root = "current"` (a
  symlink to `root`) opens on both backends and serves `index.html`, while `link`,
  `link2/x` and `proclink/self/fd/0` *inside* it stay `Hidden`. This states the recorded
  behaviour the audit said ADR-0007's consequences paragraph had left open.
- **G3 — both sides of the file ceiling.** The fixture gains a sparse `cap.bin`
  (`set_len(STATIC_MAX_FILE_BYTES)`, **0 B on disk** — verified `stat` shows `67108864 0`),
  `cap.bin` is asserted `File` with `len == meta.size == STATIC_MAX_FILE_BYTES`, and
  `big.bin` (`+1`) stays `Hidden`, over 4 fixtures × 2 backends without reading a byte.
  The `==` variant of the same mutant is only reachable through a file that passes the
  probe (where `classify` refuses over-cap files) and then **grows on the same inode**
  before the read open, so a new case drives `resolve_with`'s documented hook to do
  exactly that and asserts `Hidden` with the inode unchanged.
- **G4 — a space is not a control byte.** §5.1 refuses `< 0x20`; `0x20` is a legal
  filename byte. `segments("/my file.js") == Ok(["my file.js"])` next to
  `segments("/my\u{1f}file.js") == Err(Control)`, `head_segments("/my%20file.css")` next
  to `/%1f`, and a matrix row `/my%20file.css` ⇒ `Exact("/my file.css")` over the new
  fixture file — asserted at the resolver boundary, policy-independent and on both
  backends.

**Result: 7 of the 8 filed mutants are now `CAUGHT`; the eighth (`541:5` → `true`) is an
equivalence on any kernel that has `openat2`, which this one does.** Raw killed ratio
`59/86` → **`66/86` (68.6 % → 76.7 %)**, and after the audit's equivalence classes
`59/67` (88.1 %) → **`66/66`** — the numerator grew by exactly the seven this node was
filed for, and nothing was reclassified.

## Survivor table — mutant → what now decides it

Line numbers are `crates/blastit-scheduler/src/static_asset.rs` (unchanged since
`b8e8806`).

| Mutant | If it shipped | Assertion that now fails |
|---|---|---|
| `293:26`, `293:46` (`\|`→`&`/`^` in `open_with`'s root `OFlags`) | root dirfd loses `O_DIRECTORY`/`O_CLOEXEC` | **still `MISSED`, and still an equivalence** (the audit's class): a `O_PATH`-only descriptor is a valid directory descriptor, and `\|`→`^` over disjoint bits is `OR`. The symlink-root case opens its dirfd through this same composition and cannot distinguish it either. |
| `302:34` (`==`→`!=`) | an explicit `Walk` root silently becomes `Openat2` | `open_with_records_…`: `recorded_backend(open(fixture, Walk)) == Walk`; matrix: requested `Walk`, recorded `Walk`. |
| `302:51` (`\|\|`→`&&`) | `Walk` requested, `Openat2` taken (the kernel probe is consulted even for an explicit `Walk`) | same `Walk` assertion. |
| `302:54` (delete `!` on `supports_openat2`) | every root on a kernel **with** `openat2` silently takes `Walk` — the `RESOLVE_BENEATH\|NO_SYMLINKS\|NO_MAGICLINKS` boundary is lost | `open_with_records_…`: `recorded_backend(open(fixture, Openat2)) == Openat2`; matrix: the `Openat2` column's `wanted`; `StaticRoot::open`. |
| `541:5` (`supports_openat2` → `false`) | every root takes `Walk` forever | same `Openat2` assertions (the probe now cannot lie). |
| `541:5` (`supports_openat2` → `true`) | **equivalent on any kernel that has `openat2`** (this one does): the function already returns `true` there, so no test can distinguish it. Reported, not faked. | — (equivalence, same class as the audit's 12) |
| `429:16` (`>`→`>=`) | a file **exactly at** `static_max_bytes` answers `Hidden`/404 | `the_file_ceiling_is_inclusive_at_static_max_bytes`: `cap.bin` ⇒ `File` with `len == STATIC_MAX_FILE_BYTES`. |
| `429:16` (`>`→`==`) | a file that grows past the ceiling between the probe and the read is streamed anyway | `a_file_that_grows_past_the_ceiling_between_the_probe_and_the_read_is_hidden`: same inode grown to `+1` ⇒ `Hidden`, with the un-hooked one-byte control still served. |
| `575:40` (`<`→`<=`) | a decoded space is `StaticRefusal::Control`, so `/my%20file.css` answers `400` | matrix row 10 twin `/my%20file.css` ⇒ `Exact("/my file.css")`, `segments("/my file.js")`, `head_segments("/my%20file.css")`, `ImmutableGlob::new("/my file.css")`. |
| `653:9` (delete `RegularFile => Err(Hidden)`) | over-cap regular files then fall to `_` ⇒ same `Hidden` | equivalent (unchanged; `cap.bin`/`big.bin` now also pin the `<=` boundary of `classify`). |

## Evidence

### # Acceptance — the suite

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_resolve --locked

running 18 tests
test classify_is_total_over_every_file_type ... ok
test immutable_globs_match_the_design_vectors ... ok
test an_unopenable_root_names_the_root_it_could_not_open ... ok
test same_inode_compares_device_and_inode ... ok
test a_root_declared_through_a_symlink_is_followed_at_open_and_nowhere_else ... ok
test segments_refuses_every_malformed_shape ... ok
test the_documented_limits_and_flags_are_the_ones_asserted ... ok
test caller_supplied_segments_cannot_leave_the_root ... ok
test segments_accepts_every_legal_shape ... ok
test the_dotfile_rule_exempts_exactly_dot_well_known ... ok
test an_spa_route_whose_index_is_missing_is_spa_lost ... ok
test a_file_that_grows_past_the_ceiling_between_the_probe_and_the_read_is_hidden ... ok
test the_production_head_refuses_what_segments_never_sees ... ok
test a_leaf_swapped_between_the_probe_and_the_read_is_hidden ... ok
test the_cafe_file_is_the_nfc_bytes_and_nothing_else ... ok
test the_file_ceiling_is_inclusive_at_static_max_bytes ... ok
test open_with_records_the_backend_it_was_asked_for_and_open_asks_for_openat2 ... ok
test every_matrix_row_answers_the_same_on_both_backends ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

(14 → 18 tests; none removed, none weakened, no `#[ignore]`.)

### The backend is really exercised (matrix log, `--nocapture`)

```console
$ … -- --nocapture | grep requested
S: requested Openat2, recorded Openat2
S: requested Walk, recorded Walk
F: requested Openat2, recorded Openat2
F: requested Walk, recorded Walk
N: requested Openat2, recorded Openat2
N: requested Walk, recorded Walk
D: requested Openat2, recorded Openat2
D: requested Walk, recorded Walk
```

### Whole crate

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
… test result: ok. 46 passed … (lib)
… admission 11, critical_ingress_enforcement 9, critical_rewrite 25,
… critical_static_assets 14 (incl. MB.2.4.4's in-flight additions), ctl_channel 9,
… critical_static_resolve 18, fairness 17, flow 6, head_allocs 2, isolation 13,
… lifecycle 18, memory 14, routing 14, doc 1
test result: ok. 1 passed; 0 failed; … (doctests)
```

### Lints and guards

```console
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.25s

$ cargo fmt --all -- --check && echo "fmt: clean"
fmt: clean
```

`critical_guard` runs against the **shared** checkout, where the sibling `MB.2.4.4`
agent has uncommitted edits to `critical_static_assets.rs` (the G6 `forged_meta`
helper, `+205/−2`, mtime 23:46). That file is outside this node's paths and is not
this node's change, so the shared run names it and fails:

```console
$ uv run --no-project tools/critical_guard.py --base cb7a1e4 --node MB.2.4.3
critical-guard: cb7a1e4..working tree, 44 changed file(s)
node MB.2.4.3: paths ['crates/blastit-scheduler/tests/critical_static_resolve.rs', 'crates/blastit-scheduler/tests/common/static_fixture.rs']
  M  crates/blastit-scheduler/tests/critical_static_assets.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_static_resolve.rs  [authorized by crates/blastit-scheduler/tests/critical_static_resolve.rs]
critical-guard: FAILED: 1 protected critical test(s) changed or deleted without a covering path …
```

The same guard over **only this node's two files** — a detached worktree at `cb7a1e4`
with `critical_static_resolve.rs` and `common/static_fixture.rs` copied in, plus the
node's own `NODE.md` (created after `cb7a1e4`) — passes:

```console
$ cd .worktrees/mb243-guard && uv run --no-project …/tools/critical_guard.py --base cb7a1e4 --node MB.2.4.3
critical-guard: cb7a1e4..working tree, 2 changed file(s)
node MB.2.4.3: paths ['crates/blastit-scheduler/tests/critical_static_resolve.rs', 'crates/blastit-scheduler/tests/common/static_fixture.rs']
  M  crates/blastit-scheduler/tests/critical_static_resolve.rs  [authorized by crates/blastit-scheduler/tests/critical_static_resolve.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.2.4.3
```

(Worktree removed afterwards with `git worktree remove --force`; `static_fixture.rs` is
not a protected pattern, so it does not appear.)

### Campaign — file-scoped, `cargo mutants` 27.1.0

`df -h /var/tmp` showed **646 G free** (floor 20 G), scratch on disk, cgroup cap on.

```console
$ TMPDIR=/var/tmp/mb243-mut CARGO_NET_OFFLINE=true MUTANTS_JOBS=2 MUTANTS_TIMEOUT=90 \
    scripts/mutants-run.sh -p blastit-scheduler -f '**/src/static_asset.rs' -o /var/tmp/mb243-out
mutants: scratch TMPDIR=/var/tmp/mb243-mut (must be on disk; …)
mutants: command=cargo mutants -p blastit-scheduler -f **/src/static_asset.rs -o /var/tmp/mb243-out -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 96 mutants to test
ok       Unmutated baseline in 45s build + 2s test
…
96 mutants tested in 10m: 20 missed, 63 caught, 10 unviable, 3 timeouts
```

```console
$ wc -l /var/tmp/mb243-out/mutants.out/{caught,missed,timeout,unviable}.txt
  63 caught.txt
  20 missed.txt
   3 timeout.txt
  10 unviable.txt
```

| Campaign | tested | caught | timeout | missed | unviable | killed / viable | after equivalence |
|---|---|---|---|---|---|---|---|
| audit (`b8e8806`) | 96 | 57 | 2 | 27 | 10 | 59 / 86 = **68.6 %** | 59 / 67 = 88.1 % |
| `MB.2.4.3` (`cb7a1e4` + this suite) | 96 | 63 | 3 | 20 | 10 | 66 / 86 = **76.7 %** | 66 / 66 = 100 % |

The denominator is unchanged (86 viable; `static_asset.rs` is byte-identical to
`b8e8806`, so the mutant set is identical). Seven mutants moved from `MISSED` to
`CAUGHT` — the exact seven this node was filed for (the eighth, `541:5 -> true`, is the
equivalence below). One moved the other way, `40:42 * with /` (`STATIC_CHUNK_BYTES` = 0),
which the audit had `CAUGHT` and is now `TIMEOUT` — see Follow-up 2; it is a product
property of `MB.2.5.2`'s pump and outside this node's paths.

**Now caught — per survivor:**

```console
$ grep -hE "static_asset.rs:(302|429|575|541:5)" /var/tmp/mb243-out/mutants.out/caught.txt
…:302:51: replace || with && in StaticRoot::open_with
…:302:34: replace == with != in StaticRoot::open_with
…:302:54: delete ! in StaticRoot::open_with
…:429:16: replace > with == in StaticRoot::finish
…:429:16: replace > with >= in StaticRoot::finish
…:541:5: replace supports_openat2 -> bool with false
…:575:40: replace < with <= in segments

$ grep -h "541:5" /var/tmp/mb243-out/mutants.out/missed.txt
…:541:5: replace supports_openat2 -> bool with true
```

**The 20 survivors are exactly the audit's 19 equivalence/unobservable classes plus that
one probe mutant** (12 `|`→`^`/disjoint-bit equivalences + 7 `|`→`&` flag degenerations
with no unprivileged oracle + `541:5 -> true`), so every behaviour a user can hit in this
module is now pinned. That is also why "after equivalence" reads 100 %: the numerator did
not shrink, the remaining survivors are the classes the audit already justified by hand.

The campaign copied the tree with every assertion above in place; the only edits after it
started were module-doc prose and a `cargo fmt` import re-order (no assertion changed).
The copy predates the sibling `MB.2.4.4` agent's uncommitted additions to
`critical_static_assets.rs`, which are not part of this oracle.

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/tests/critical_static_resolve.rs` | `recorded_backend`; backend asserted per fixture and for `StaticRoot::open`; matrix row 10 twin for `%20`; space/C0 boundary in `segments` + `head_segments` + `ImmutableGlob`; 4 new tests (open contract, at-cap ceiling, grow-past-ceiling, symlink-declared root). |
| `crates/blastit-scheduler/tests/common/static_fixture.rs` | `my file.css` (+ bytes, `content_of` entry), sparse `cap.bin` at exactly `STATIC_MAX_FILE_BYTES`, `grow.bin` (1 B) with `GROW_BIN_BYTES`. |

## Follow-ups

1. **`541:5` `supports_openat2 → true` is an equivalence on any kernel with `openat2`.**
   It can only differ on a kernel without the syscall, which no unprivileged CI job here
   has. If the test-curator wants it killed, the honest route is an `MB.2.7` e2e job on a
   seccomp-filtered runtime that makes `openat2` return `ENOSYS`; nothing in this leaf can.
2. **NEW, out of this node's paths (`scheduler.rs` / `MB.2.5.2`): `STATIC_CHUNK_BYTES` = 0
   is a `TIMEOUT`, not a failure.** `static_asset.rs:40:42 * with /` was `CAUGHT` in the
   audit's `b8e8806` campaign (no pump tests existed there) and now runs the 90 s clock
   out in the `critical_rewrite.rs` pump cases. Chunk 0 should abort the body as a short
   read within a bounded time (`R10`'s "a short read aborts the response"), not park the
   request: worth a look from whoever owns `pump_static`.
3. **`LESSONS.md` candidate (not written: the file is outside this node's `paths` and is
   dirty with another agent's work):** `finish`'s second ceiling check is unreachable
   from `resolve` alone, because `classify` refuses an over-cap file at the probe first —
   which is why the `>`→`==` mutant survived even with `/big.bin` in the matrix. Only a
   file that grows **on the same inode** between probe and read reaches it.
4. **`recorded_backend` depends on `StaticRoot`'s derived `Debug` printing its private
   `backend` field.** If a future change hand-writes `Debug` or drops the field, the
   helper panics with a message naming that fact rather than silently passing. Adding a
   `#[doc(hidden)] pub fn backend(&self)` would be the alternative, but §4.2's surface is
   frozen — this is the documented trade-off from the node.
5. **`critical_guard` in the shared checkout** fails on `MB.2.4.4`'s uncommitted
   `critical_static_assets.rs` (see Evidence). Nothing to fix here: the lead stages each
   node's own paths at commit time; the node-scoped worktree run is the true result.
6. **`LESSONS.md` candidate (2/2), host/tooling:** in this shared checkout
   `cargo fmt --all` followed immediately by `cargo fmt --all -- --check` can come back
   **red with the block it just wrote**, because a concurrent agent's `make check`/`fmt`
   rewrote the same file in between (the 2024 style-edition import order flips). Symptom:
   `--check` wants `{Backend, STATIC_MAX_FILE_BYTES, StaticRoot}` while the file has
   `{Backend, StaticRoot, STATIC_MAX_FILE_BYTES}`. Re-run `cargo fmt --all` and confirm
   `--check` is clean twice before reporting; do not "fix" imports by hand.
