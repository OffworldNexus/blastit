# RESULT MB.2.4.4 — `critical_static_assets`: descriptor ownership + the `meta.size` ceiling

- **Base:** `0796921` (`release/0.2.0`), uncommitted working tree (no commit, no `git add`).
- **Files changed:** `crates/blastit-scheduler/tests/critical_static_assets.rs` **only** —
  3 new tests (`14` in the file now), one new helper (`forged_meta`), one new descriptor
  oracle (`open_in_tree`), one new helper (`resolved`). `src/static_response.rs` **untouched**.
- **Closes:** `docs/testing/AUDIT-MB.2.4.md` **G5** (`respond` never `dup`s; `HEAD`/`304` drop
  the descriptor) and **G6** (`||` and the `meta.size` disjunct).
- This is a test-only leaf; no survivor proved a product bug, so no product change was made.

## What the suite now asserts

| Oracle | Test | Mutant it kills |
|---|---|---|
| `body.file.as_raw_fd() == resolved file's as_raw_fd()` — the descriptor **moves** | `a_response_moves_the_one_descriptor_and_never_duplicates_it` | `dup()`/`try_clone()` on the `GET` body |
| in-tree descriptor count `1 → 2 → 2 → 1` across `open` → `resolve` → `respond` → `drop(body)` | same | a `dup` whose original or copy outlives the response |
| `HEAD` and a `304` leave exactly the root's dirfd behind, and 16 bodiless answers in a row leak nothing | `head_and_a_304_close_the_descriptor_instead_of_leaking_it` | a parked/leaked descriptor, a missing `drop` |
| `len == meta.size == static_max_bytes` is a `200` promising exactly that `Content-Length` | `the_ceiling_admits_the_exact_size_and_refuses_either_half_over` | `>` → `>=`, `>` → `==` on **either** disjunct |
| one byte over is `404` + `Content-Length: 0` + `Cache-Control: no-store`, on both halves over **and on each half alone** | same | `\|\|` → `&&`, either disjunct deleted, `>` → `<` |

**Why the count is not flaky:** `open_in_tree` counts only descriptors whose
`/proc/self/fd` link starts with **this test's** fixture base. `static_fixture.rs` gives every
test its own `blastit-static-<pid>-<nanos>-<n>-<S>` tree, so the 13 other tests in the same
binary cannot move my count — a raw `wc -l /proc/self/fd` before/after would flake under the
default parallel harness. `static_fixture.rs` is **not** edited by this leaf.
The at-ceiling file is `set_len(64 MiB)` (sparse, 0 B on disk) and is never read;
`/tmp` holds no leftover `blastit-static-*` directory after the runs (`Drop` runs).

## # Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked
running 14 tests
test a_response_moves_the_one_descriptor_and_never_duplicates_it ... ok
test head_and_a_304_close_the_descriptor_instead_of_leaking_it ... ok
test the_ceiling_admits_the_exact_size_and_refuses_either_half_over ... ok
... (11 pre-existing tests unchanged and green)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
# at base 0796921 + the two sibling leaves' in-flight files
46 / 11 / 9 / 25 / 14 / 18 / 9 / 17 / 6 / 2 / 13 / 18 / 14 passed, 0 failed in every target
all doctests ran in 0.53s
# re-run on the current tip (69504ed, after MB.2.4.3 bb163d5 + MB.2.9 69504ed landed):
46 / 11 / 9 / 32 / 14 / 18 / 9 / 17 / 6 / 2 / 13 / 18 / 14 / 1(doctest) passed, 0 failed
```

```console
$ cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (/home/remy/offworld-nexus/blastit/crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s
```

```console
$ cargo fmt --all -- --check
FMT OK
```

```console
$ uv run --no-project tools/critical_guard.py --base 0796921 --node MB.2.4.4
node MB.2.4.4: paths ['crates/blastit-scheduler/tests/critical_static_assets.rs']
  M  crates/blastit-scheduler/tests/critical_static_assets.rs  [authorized by crates/blastit-scheduler/tests/critical_static_assets.rs]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_static_resolve.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 2 protected critical test(s) changed or deleted without a covering path in node(s) MB.2.4.4.
```

The single-node form never flags my file; it flags `critical_static_resolve.rs` (`MB.2.4.3`)
and `critical_rewrite.rs` (`MB.2.4.5`), which two sibling leaves are editing **concurrently**
under their own paths. With the parallel leaves' nodes named together — the documented gate
form recorded in `MB.2.4/LOG.md` ("the guard's contract is *these nodes authorize these
paths*") — every protected change is authorized:

```console
$ uv run --no-project tools/critical_guard.py --base 0796921 --node MB.2.4.4 --node MB.2.4.3 --node MB.2.4.5
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by crates/blastit-scheduler/tests/critical_rewrite.rs]
  M  crates/blastit-scheduler/tests/critical_static_assets.rs  [authorized by crates/blastit-scheduler/tests/critical_static_assets.rs]
  M  crates/blastit-scheduler/tests/critical_static_resolve.rs  [authorized by crates/blastit-scheduler/tests/critical_static_resolve.rs]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.2.4.4, MB.2.4.3, MB.2.4.5
```

Anti-flake spot check: 10 consecutive runs of the whole file under the default parallel
harness, `14 passed` each time.

## Local reversion proof (throwaway worktree copy, `src` restored byte-identically)

Base `0796921` + the new suite, in `target/mb244/probe`, `src/static_response.rs` mutated and
reverted; final `cmp` against the shipped source: **identical**.

| Mutation to `respond` | Result |
|---|---|
| `file: file.file.try_clone().expect("MUTANT: dup")` on the `GET` body | `FAILED a_response_moves_the_one_descriptor_and_never_duplicates_it` — `assertion left == right failed: Openat2: the resolved descriptor itself moves into StaticBody / left: 4 right: 5` (13 passed, 1 failed) |
| `HEAD` arm `std::mem::forget(file.file)` (missing `drop`) | `FAILED head_and_a_304_close_the_descriptor_instead_of_leaking_it` — `left: 2 right: 1` "HEAD must drop the descriptor, not park it" (13 passed, 1 failed) |
| `if file.len > self.max_bytes {` (second disjunct deleted) | `FAILED the_ceiling_admits_the_exact_size_and_refuses_either_half_over` — `left: 200 right: 404`, "only meta.size over the ceiling" (13 passed, 1 failed) |
| `if file.len >= self.max_bytes \|\| …` (`>` → `>=`) | same test — `left: 404 right: 200`, "a file exactly at static_max_bytes is served" |
| `if file.len > self.max_bytes && …` (`\|\|` → `&&`) | same test — `left: 200 right: 404`, "only len over the ceiling" |

## Mutation campaign — before / after

Oracle = the whole `blastit-scheduler` test package (all 15 targets), file-scoped to
`**/src/static_response.rs`. `before` = pristine `0796921` worktree (old suite, 11 tests);
`after` = same tree + this leaf's suite (byte-identical to the shipped file, verified with `cmp`).
Same command both times: `scripts/mutants-run.sh -p blastit-scheduler -f '**/src/static_response.rs' -o … -j 2 --timeout 90`, `TMPDIR` on disk, cgroup `MemoryMax=12G`.

| Run | Tested | Caught | Timeout | Missed | Unviable | Killed / all | Killed / viable |
|---|---|---|---|---|---|---|---|
| **before** (old suite) | 161 | 147 | 0 | 12 | 2 | 147/161 = **91.3 %** | 147/159 = **92.5 %** |
| **after** (this leaf) | 161 | 152 | 0 | 7 | 2 | 152/161 = **94.4 %** | 152/159 = **95.6 %** |

`161 mutants tested in 25m: 12 missed, 147 caught, 2 unviable` / `161 mutants tested in 25m: 7 missed, 152 caught, 2 unviable`.
The 2 unviable mutants are the same `Default::default()` replacements in both runs
(`diff` of the two `unviable.txt` files is empty). No mutant went from caught to missed.

### Survivors that died (mutant → the assertion that now kills it)

| Mutant (`static_response.rs`) | Was | Now killed by |
|---|---|---|
| `94:38` `\|\|` → `&&` | missed | `forged_meta(len=ceiling+1, size=0)` → 404 and `forged_meta(len=0, size=ceiling+1)` → 404 (each half alone) |
| `94:21` `>` → `>=` (first disjunct) | missed | `forged(len = size = ceiling)` → 200 with `Content-Length: 64 MiB` |
| `94:21` `>` → `==` (first disjunct) | missed | `len = ceiling+1` → must be 404 |
| `94:56` `>` → `>=` (second disjunct) | missed | same at-ceiling 200 |
| `94:56` `>` → `==` (second disjunct) | missed | `size = ceiling+1, len = 0` → must be 404 |

### Remaining 7 survivors (out of this leaf's scope, unchanged by it)

| Mutant | Why it survives | Owner |
|---|---|---|
| `218:41` `trim_ows`: `&&` → `\|\|` | no case feeds a value with only one of space/tab at an end; `critical_static_assets.rs` owns `http_date`/`Content-Type`, not `trim_ows`' tab rule — the audit recorded it as "accepted, untested, not a security boundary" | accepted (audit §GAPS tail) |
| `301:17`, `305:42`, `305:64`, `313:21` (6 `civil_from_days` arithmetic) | no leap-day/century vector (`2000-02-29`, `1900-03-01`); the audit recorded this as accepted-with-reason; a tolerant `http_date` date vector belongs to the leaf that owns `http_date`'s vectors | accepted (audit §GAPS tail) |

## Deviation from the dispatch

`/var/tmp` is **not writable in this session** — the shell refuses any `/var/tmp` path
(`Permission denied: external_directory`, confirmed with a `mkdir` probe, before any campaign
started), so `df -h /var/tmp` could not be read either. Scratch therefore went to
`target/mb244/` **inside the workspace, on disk** (`/dev/mapper/chulak-incus`, **646 G free**,
20 % used) and never to `/tmp` (tmpfs, 32 G).

## Gotcha measured (not a LESSONS edit — `docs/` is out of this node's paths)

`cargo-mutants` copies the workspace **and** re-exports the caller's `TMPDIR` to the test
process, so `TMPDIR` must be **≤ ~51 characters** here: the fixture's socket path is
`$TMPDIR/blastit-static-<pid>-<19-digit nanos>-<n>-<S>/root/sock` (worst case 56 B + `TMPDIR`),
and `UnixListener::bind` panics `path must be shorter than SUN_LEN` past 107 B. A deep scratch
path fails the **unmutated baseline** with 16 `critical_rewrite` failures (`ERROR cargo test
failed in an unmutated tree, so no mutants were tested`) — it looks like a broken suite, not a
path-length problem. This is the audit's latent risk (b), now measured.

## Follow-ups (not filed here)

1. `trim_ows`' `&&`→`||` survivor and the six `civil_from_days` century/leap victims still have
   no oracle; both were already recorded as accepted-with-reason in `AUDIT-MB.2.4.md`, but the
   `http_date` vectors are cheap and belong to a `critical_static_assets` follow-up.
2. The fixture's `sun_path` head-room (audit risk b) deserves either a shorter fixture name or a
   hard precondition message that names `TMPDIR`, instead of a `SUN_LEN` panic.
