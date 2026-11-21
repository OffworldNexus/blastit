# MB.23.5.3 — the dev policy channel, its sender, and the PHP live-reload row

**STATUS: done** (attempt 3). The attempt-1 split and the attempt-2 report below are kept as
the record; `## Attempt 3` lands the two MAJORs (the late-ack attribution and acceptance 7's
listener-set warning) and the three MINORs. The only remaining follow-up is acceptance 6's
`200` half, which needs the `BLASTIT_TEST_PG_URL` job (no Postgres is available here).

<!-- attempt 1's report, kept as the record of what it did and why it stopped -->

**Attempt-1 status: needs_split.** The in-path half is landed, tested and green: dbproxy's
policy channel (configuration members, reader task, atomic `Arc<Allowlist>` swap, one-byte
ack) and stage 0's socketpair (`--dev-policy-fd` to stage 1, `policy_fd`/`dev_policy` in
dbproxy's configuration document). Acceptance items 1–4 and 9–10 were met; **5, 6, 7 and 8
(the supervisor's sender and the PHP row) live in `crates/blastit/src/supervise.rs`, which was
in no `paths` entry of this node** — see *The split* below. (Attempt 2 landed them.)

Base commit `fde43da`, branch `release/0.2.0`, worktree
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` (shared plan tree; nothing
staged, nothing committed).

## What landed

### dbproxy — the receiving end (all in `paths`)

- **`config.rs`**: `policy_fd: Option<i32>` and `dev_policy: bool` (`#[serde(default)]`), and
  `validate_policy()` — a `policy_fd` without `dev_policy` is a configuration error whose
  sentence names `--dbproxy-policy-fd` and `dev_policy`; a negative descriptor and a
  descriptor shared with `obs_fd`/`check_ctl_fd` are refused too. `validate()` and
  `Shared::build` both call it, so `run`, `run_with_ready` and `run_with_listeners` refuse a
  production boot that was somehow handed the channel. `RoutePolicySpec` gained
  `#[serde(deny_unknown_fields)]`, so a document carrying `fd`, `uid` or any invented member
  is refused rather than silently dropped.
- **`lib.rs`**: the `policy` module — the frozen wire shape (`u32 LE length | JSON`,
  ≤ 1 MiB, `{"version":1,"route_policies":[…]}`), a reader that refuses an empty, oversize,
  truncated, wrong-version, wildcard-route, no-proxy or duplicate-pair document **whole**
  before anything is touched, the two ack bytes, and the rule that an **idle** channel waits
  (a dev boot can serve for hours before its first save) while a document that has *started*
  and stalled is bounded.
- **`learn.rs`**: `route_policies` is a `Mutex<HashMap<(String, String), Arc<Allowlist>>>`;
  `Learning::replace_route_policies` builds and validates the whole new map first and then
  replaces it in **one assignment**, and `policy()` now returns the `Arc` in force — so a
  statement already being checked against one policy keeps it, and no reader can ever see a
  half-swapped table. It is authoritative: an omitted pair evaluates against the empty
  allowlist.
- **`state.rs`**: `ListenerRuntime.allowlist` is a `ListenerPolicy` cell (an
  `Arc<Allowlist>` behind one `Mutex`) whose `check()` clones the current `Arc` and evaluates
  against exactly it — this is what makes an **enforcing** per-route listener pick up a swap
  **without touching `hooks.rs`** (`listener.allowlist.check(sql)` compiles unchanged).
  `Shared::replace_route_policies` swaps the learning map and every listener's cell from the
  same `Arc`s; a wildcard listener keeps resolving through `Learning`.
- **`server.rs`**: `spawn_policy`/`serve_policy` — the detached task that adopts the
  descriptor (only when `config.policy_fd` is `Some`, which validation has tied to
  `dev_policy`), reads one document, applies it, and writes **exactly one ack byte**
  (`0x00` swapped / `0x01` refused). A well-framed refusal keeps the channel usable for the
  next save; a framing failure refuses once and closes (no retry loop). Nothing is logged:
  the one warning belongs to the supervisor's side, and dbproxy's stdout is init's.

### stage 0 — the channel (in `paths`)

- `dev_policy_channel()`: one `AF_UNIX`/`SOCK_STREAM` socketpair from
  `blastit_sys::socketpair_stream()`, both ends `O_NONBLOCK`, created **only** when this is a
  `dev` boot with reload on (`--no-reload`, production, `check` and `test` create neither
  end).
- The stage-1 end crosses the `execve` as `--dev-policy-fd=N` (the token and its
  `--stage=1 --dev-root` gate already existed) and is in `keep`; stage 1 already carries it
  into `Supervisor.policy` and into the reload adoption document's `"policy"` member.
- dbproxy's end rides its configuration document (`policy_fd` + `dev_policy: true`) and its
  `keep` list; the parent **drops its copy** after the fork, so the only process a view fork
  could inherit it from never holds it.

### The critical suite (in `paths`)

- `the_policy_channel_reaches_stage_one_and_no_view_holds_it` (acceptance 4): stage 1's
  cmdline carries `--dev-policy-fd=N` and `/proc/<pid>/fd/N` is a socket; the answering view
  — which is `PR_SET_DUMPABLE(0)`, so its own report is the only readable fd table — holds no
  socket inode equal to that end; a view born after a reload holds none either; and the
  channel never moves (same descriptor number and inode after the reload that replaced every
  view).
- The `--no-reload` half was folded into the existing
  `no_reload_watches_nothing_and_serves_the_old_body` row instead of a new boot: the
  reload-on control now also asserts it *has* a channel, and the `--no-reload` boot asserts it
  has none. The row's `cart.fds + 1 == watched_cart.fds` assertion still holds, which is the
  view-side proof that the channel costs a view **no** descriptor.
- `Body` gained one field: the worker's own socket inodes (a trailing field, so every other
  row's prefix parsing is unchanged).
- **Row 8 (EPERM) was already landed** by `MB.23.5.2.2`
  (`an_eperm_watcher_is_one_unavailable_and_the_run_keeps_serving`: exactly one
  `dev.reload.unavailable`, `errno: EPERM`, zero `dev.reload`, zero fanotify descriptors,
  three `200`s and still one warning). It is green here and was **not** duplicated or touched.

## The split (why 5, 6, 7 and 8 are not in this attempt)

Nothing here is a design question; it is a `paths` gap, and the node's own `# Notes`
pre-identifies this class ("stop and report it as a one-line follow-up child rather than
editing outside `paths` silently"). The lead extended `paths` with `php_view.rs`, but the two
relay points the dispatch's items 1 and 2 describe are in `supervise.rs`:

1. **The sender** (§4.6 step 2, acceptance 5/6/7): the document write through the poll set,
   the 100 ms `policy_ack` state, and the one `manifest.warning` ("the new grants were not
   applied; restart blastit dev") live in `supervise.rs::begin_cycle`/the poll loop. Without
   it the channel is complete on both ends but nobody writes to it, so a `Blastfile` grant
   edit does **not** yet apply on save and acceptance 6 cannot be witnessed. The broker is
   already in place: `Supervisor.policy` holds the descriptor and the cycle already has the
   re-parsed `Manifest` (and `stage0::route_declares_proxy` is the predicate the document's
   pair set needs — `stage0.rs` is in `paths` and it can be made `pub(crate)` there).
2. **PHP row 10** needs `supervise.rs::open_report` (it returns `(None, None)` for a PHP slot
   today: "php_view.rs carries no report yet — MB.23.5.3") and
   `supervise.rs::fork_view`'s `PhpViewSpawn { … }` literal. Adding
   `loaded: Option<PhpLoadedReport>` to `PhpViewSpawn` **does not compile** without that one
   literal line, so item 2 of the dispatch cannot be done inside `php_view.rs` alone.
3. `--dbproxy-policy-fd` as an **argv** token would need `cli.rs`,
   `crates/blastit-dbproxy/src/main.rs` and `roles.rs` (three files in no `paths`). The
   descriptor reaches dbproxy through its **configuration document** instead, which is how
   every other dbproxy descriptor except stdio/events/ready already arrives (listener `fd`s
   ride the same document). The security invariant is enforced where it matters:
   `config.validate_policy()` refuses a `policy_fd` without `dev_policy`, and the sentence
   names the switch.

**Proposed child `MB.23.5.3.1`** (`paths`: `crates/blastit/src/supervise.rs`,
`crates/blastit/src/php_view.rs`, `crates/blastit/tests/critical_dev_reload.rs`, and
`cli.rs`/`main.rs`/`roles.rs` only if the argv spelling is wanted): the policy-document write
through the poll set with the 100 ms ack deadline and its one warning; `open_report`'s PHP
arm plus the `PhpViewSpawn` field and keep-list wiring; then rows 6/7 (grant edit applies,
removal denies, listener-set warning) and row 10 (PHP reloads all views, `critical_php_embed`
probe, one-line no-pair message). The `BLASTIT_TEST_PG_URL` job is the only place row 6 can
be witnessed.

## Evidence

Every command ran in the worktree above, on the final tree (dbproxy channel + stage 0 +
the two critical rows + the LESSONS line).

```console
$ cargo test -p blastit-dbproxy --locked policy
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 65 filtered out; finished in 0.22s

$ cargo test -p blastit-dbproxy --locked
test result: ok. 81 passed; 0 failed …   (lib)
test result: ok. 5 passed; 0 failed …    (main)
test result: ok. 9 passed; 0 failed …    (critical_learn_and_redaction, 5.01s)
test result: ok. 24 passed; 0 failed …   (postgres)
test result: ok. 1 passed; 0 failed …
```

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # twice idle
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.50s
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.70s

$ # the same suite 4x with all 8 CPUs in busy loops (the dispatch's "repeat under load")
load1: test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.37s
load2: test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.61s
load3: test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.59s
load4: test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.47s

$ # plus a 40-run idle campaign while the channel's idle-wait rule was being finished: 40/40 green
```

```console
$ cargo test -p blastit --locked --features python,php --test critical_dev_reload php
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.00s
```

Row 10 does not exist yet, so the `php` filter selects nothing — **0 tests is the honest
state, not a pass**: see *The split*.

```console
$ cargo fmt --all -- --check
FMT=0
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
CLIPPY=0
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify
group, one Watcher::new)
```

```console
$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 20 passed; 0 failed …   (critical_dev_reload, build script and Makefile)
test result: ok. 42 passed …             (critical_lsp)
test result: ok. 7 passed …              (critical_parallel_views)
test result: ok. 9 passed …              (critical_php_backend)
test result: ok. 27 passed …             (critical_php_embed, 10.43s)
test result: ok. 283 passed; 0 failed; 2 ignored …   (unit tier; `2 ignored` is pre-existing)
test result: ok. 81 passed …             (dbproxy lib)
Ran 53 tests in 14.323s                  (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 28.6s
ok: …/target/debug/blastit: 3 test seam(s) present, as a debug build must
Ran 19 tests in 1.688s                   (zed-check)
plan OK
```

`make check` failed **twice** before this green run, both times in
`critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot` — a suite this node
neither owns nor touched (`git diff --stat fde43da -- crates/blastit/tests/critical_php_embed.rs
crates/blastit/src/stage1.rs crates/blastit/tests/common/ crates/blastit-php-sapi/` is empty):
once with `no probe shape was written` (the exact failure `MB.23.5.2.1`'s RESULT recorded as
this host's 250 ms probe budget) and once with `the 250 ms probe timeout did not bound the
boot: it took 10.879543992s`. Both panics were read in full; the stub child was killed at
274 ms per the log. Run alone on this tree, the test passed **8/8** in ~0.7 s. This is
recorded as a host-sensitive pre-existing suite, not as "load" and not as green-washing: the
next full `make check` was green end to end.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  4.3s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 24.1s
DOCS_TEST_EXIT=0
```

```console
$ uv run --no-project tools/critical_guard.py --base fde43da --node MB.23.5.3
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.3
GUARD_EXIT=0

$ git diff --stat fde43da -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; FROZEN_EMPTY=0)
```

## Files

```text
 crates/blastit-dbproxy/src/config.rs        | 109 +++++++-
 crates/blastit-dbproxy/src/learn.rs         | 164 +++++++++++-
 crates/blastit-dbproxy/src/lib.rs           | 402 ++++++++++++++++++++++++++++
 crates/blastit-dbproxy/src/server.rs        | 186 +++++++++++++
 crates/blastit-dbproxy/src/state.rs         | 206 ++++++++++++--
 crates/blastit/src/stage0.rs                |  94 +++++++
 crates/blastit/tests/critical_dev_reload.rs | 127 ++++++++-
 docs/knowledge/LESSONS.md                   |   1 +
```

`docs/knowledge/LESSONS.md` is outside this node's `paths`; the dispatch asked for the line
(the row that saves a view file must `wait_marked` its parent first, or under load the save
lands before the file is watched and the row reads as a broken feature — the one real test
defect this attempt hit, before the fix, 1 in 5 loaded runs).

`php_view.rs` is untouched: its one new field cannot compile without the `supervise.rs`
literal.

## Follow-ups

1. **The sender** (acceptance 5/6/7): document write through the poll set, `policy_ack`,
   100 ms deadline, one `manifest.warning`, then `Ctl::Reload`; the adoption document already
   carries `"policy"` exactly when reload is on, and
   `an_invalid_blastfile_keeps_serving_and_the_next_valid_one_reloads_once` passing shows the
   descriptor survives the `execve` and is honoured by the replacement image's
   `Adoption::read` (it refuses a named descriptor it does not hold).
2. **PHP row 10** (`open_report`'s PHP arm + the `PhpViewSpawn` field/keep list + the row,
   with the `critical_php_embed` probe and its one-line no-pair message).
3. `--dbproxy-policy-fd` as an argv token (three out-of-path files) if the design's spelling
   is wanted on top of the configuration-document channel that already works.
4. `acceptance 6`'s `BLASTIT_TEST_PG_URL` job (denied → `200` with one `dev.reload`) needs 1.
5. One unread failure in `critical_dev_reload` (2.68 s, after the `wait_marked` fix, in a run
   whose output was piped through `grep '^test result'`) — the panic was discarded before it
   could be read, which was my process mistake. It was not reproduced in 40+ subsequent runs
   of the final tree (idle or under load) nor in the final `make check`. Named here rather
   than called a flake.

## Attempt 2 — the sender, the PHP wiring, and row 10

Base commit `fde43da`, branch `release/0.2.0`, same worktree. The lead extended `paths` with
`crates/blastit/src/supervise.rs`, `cli.rs`, `roles.rs` and `docs/knowledge/LESSONS.md`, so the
two items attempt 1 had to defer are now implemented. **Nothing from attempt 1 was rewritten**:
the dbproxy channel and the stage-0 socketpair are byte-identical to the tree attempt 1 left
(`git diff` over the five dbproxy files and `stage0.rs`'s channel function shows only the two
changes listed under *attempt 2 additions* below, both additive).

### 1. The sender (`crates/blastit/src/supervise.rs`)

- `POLICY_ACK_TIMEOUT` = **100 ms**, the design's number (`§4.6` step 2), and
  `Supervisor::refresh_policy(&manifest)`:
  - returns immediately on a boot that has no channel and **on a boot that forked no dbproxy**
    (`Adoption::dbproxy.is_none()`) — there is no reader, so there is nothing to warn about;
  - builds the document from the **new manifest's own pairs** (`policy_document`): one entry
    per `(route, proxy)` the file declares or grants, via `stage0::route_declares_proxy` (now
    `pub(crate)`), with the crate's member names and no `fd`/`uid`; a route that names the
    proxy with an empty grant is present with no queries (allows nothing), a route that names
    nothing is absent (denies) — authoritative, exactly as `Learning` evaluates it;
  - writes `u32 LE length | JSON` and reads the one-byte ack through
    `exchange_policy`, which is **non-blocking**: a short write waits for writability and the
    ack for readability inside the same 100 ms deadline (the `write_ctl` shape), so a wedged
    dbproxy can never hold init;
  - emits **exactly one** `manifest.warning`,
    `the new grants were not applied; restart blastit dev (<cause>)`, on a build error, an
    oversize document, a write/read failure, a missing ack or `0x01` — and nothing on success.
- The call site is `begin_cycle`, **after** the one `dev.reload` and **before** `Ctl::Reload`,
  so the first query a replacement view runs is decided by the new grants and the retirement
  starts either way.
- The fd is adopted once (`Adoption::policy`) and set `O_NONBLOCK` in `Supervisor::new`; it is
  carried across a cycle by the adoption document, so it is **never re-created** — the
  attempt-1 critical row's "same descriptor number and inode after a reload" assertion still
  holds.

### 2. The PHP wiring (`supervise.rs` + `php_view.rs`)

- `open_report` is no longer python-gated and no longer skips PHP slots: both backends report
  over the same wire. It returns a crate-local `ViewReportEnd { socket, source }`, and each
  fork converts it at its boundary (`LoadedReport` for the zygote, `PhpLoadedReport` for
  `PhpViewSpawn`) — a **fresh reporter per boot**, because the socketpair is created per
  `spawn_now` and a source's first batch is its `boot` set.
- `PhpViewSpawn.loaded: Option<PhpLoadedReport>`, the write end added to the child's
  `keep` list (an inherited descriptor, like `view.chan`; no environment variable), and
  `serve` passes it to `PhpEmbedRuntime::load_reporting` — the `None` placeholder is gone.
  A shim without the optional sink symbol still degrades to the production behaviour inside
  `load_reporting`.
- `--no-reload` and production are unchanged: no watcher, no report socket, `loaded: None`.

### 3. Row 10 (`crates/blastit/tests/critical_dev_reload.rs`)

`editing_an_included_php_file_reloads_every_view_once_and_serves_the_new_body`, `#[cfg(feature
= "php")]`, over a two-route PHP fixture (`entrypoint php "public/index.php"`, `view app`,
`view api`, an included `public/version.php` the front controller echoes):

- `host_php()` is the probe `critical_php_embed` established, copied small and independent of
  the code under test (raw `std` reads: `executor_globals`, `php_version\0`, no
  `tsrm_startup`; then the shim for the same minor in `BLASTIT_PHP_SHIM_DIR`, beside the test
  binary or in `target/php-sapi`);
- **on this host the served shape ran** — `/usr/lib/libphp8.4.so` +
  `target/php-sapi/libblastit_php84-x86_64-gnu.so` — and the test prints its own evidence
  line: `reloaded (php, …): 2 view(s), 1 file(s), both routes serve v2`;
- the assertions: both routes answer `v1:` first, the workers' reports mark `public/`
  (`wait_marked`, the same state wait the WSGI rows use), saving the *included* file is
  exactly **one** `dev.reload` whose `routes` are `["app","api"]` and whose `files` name
  `public/version.php`, and the next request on **both** routes returns `v2:`;
- the **refusal branch is real**: a host without the pair has its boot refused (bounded wait,
  exit `1`, empty stdout) and the test asserts the documented sentence —
  `install the PHP embed package` plus the entry path — then prints the one-line
  `refused (no usable libphp + shim on this host): …`. Nothing is skipped and nothing is
  `#[ignore]`d.

### 4. Rows 6/7 (the policy channel)

- Attempt 1's suite already covers "a query already in flight keeps the `Arc<Allowlist>` it
  started with" and "an omitted pair denies" (`state.rs` + `learn.rs` unit legs) and
  "`:8080` never drops" (the `Blastfile` row's keep-alive connection, opened before the save).
- Added here: `supervise::policy_tests::the_policy_exchange_writes_the_document_and_reads_one_ack`
  (real socketpair: `0x00` is a swap, `0x01` a refusal, no ack is the 100 ms deadline, a
  closed peer is its own cause) and
  `supervise::policy_tests::a_refused_policy_document_is_one_warning_and_a_swap_is_silent`
  (the real `refresh_policy`: a swap emits nothing, `0x01` is exactly one `manifest.warning`
  with the restart sentence, and a boot with no dbproxy emits nothing).
- What is still not witnessed here is the **200** half of acceptance 6 — a swap observed by an
  actual SQL query needs the `BLASTIT_TEST_PG_URL` job; the dbproxy `0x00`-then-next-statement
  half is `server::tests::the_policy_channel_answers_every_document_with_one_byte`, which flips
  an **enforcing listener's** verdict through the real reader and ack loop.

### Acceptance (all re-run on the final tree)

```console
$ cargo test -p blastit-dbproxy --locked
test result: ok. 81 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
test result: ok. 5 passed; 0 failed … (main)
test result: ok. 9 passed; 0 failed … (critical_learn_and_redaction, 5.04s)
test result: ok. 24 passed; 0 failed … (postgres)
test result: ok. 1 passed; 0 failed …
```

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # twice, idle
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.32s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.74s

$ # and once (three times) under load: all 8 CPUs in busy loops
load1: test result: ok. 21 passed; … finished in 4.48s
load2: test result: ok. 21 passed; … finished in 4.94s
load3: test result: ok. 21 passed; … finished in 5.12s

$ cargo test -p blastit --locked --features python,php --test critical_dev_reload
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.71s

$ cargo test -p blastit --locked --features python,php --test critical_dev_reload -- php
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.50s
```

(The `php` filter now selects **1** test; attempt 1's honest "0 tests" is gone. `blastit`'s
default features include `php`, which is why the plain `--features python` run is also 21.)

```console
$ cargo test -p blastit --locked --features python --lib
test result: ok. 286 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.60s
$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.39s
```

```console
$ cargo fmt --all -- --check
FMT=0
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python,php -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify
group, one Watcher::new)
```

```console
$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 21 passed; 0 failed …   (critical_dev_reload, twice: build script and Makefile)
test result: ok. 42 / 7 / 9 / 27 passed, 0 failed …   (lsp / parallel_views / php_backend / php_embed)
test result: ok. 286 passed; 0 failed; 2 ignored …   (unit tier; `2 ignored` is pre-existing)
test result: ok. 91 / 81 / 73 …                      (the rest of the workspace)
Ran 53 tests in 16.905s                             (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 38.0s
ok: …/target/debug/blastit: 3 test seam(s) present, as a debug build must
Ran 19 tests in 3.395s                              (zed-check)
plan OK
```

No `critical_php_embed` probe flake tripped in this attempt's `make check` (it passed in this
run and in the isolated 27/27 run above); nothing was reordered, raised or disabled.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  2.4s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 36.7s
DOCS_TEST_EXIT=0

$ uv run --no-project tools/critical_guard.py --base fde43da --node MB.23.5.3
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.3

$ git diff --stat fde43da -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; FROZEN_EMPTY=0)
```

### Files (attempt 2 additions only)

- `crates/blastit/src/supervise.rs` — `POLICY_ACK_TIMEOUT`, `refresh_policy`,
  `policy_document`, `exchange_policy`, the call site in `begin_cycle`, the non-blocking
  adoption, `ViewReportEnd`, the both-backends `open_report`, the two `fork_view` arms, and the
  three `policy_tests` legs.
- `crates/blastit/src/php_view.rs` — `PhpViewSpawn.loaded`, the keep-list entry, and
  `load_reporting(spawn.loaded)` (the `None` placeholder is gone).
- `crates/blastit/src/stage0.rs` — `route_declares_proxy` is `pub(crate)` (one word).
- `crates/blastit/src/cli.rs`, `crates/blastit/src/roles.rs` — **not changed**: the descriptor
  reaches dbproxy through its configuration document (`policy_fd` + `dev_policy`), which is
  how every other dbproxy descriptor except stdio/events/ready arrives. The design's
  `--dbproxy-policy-fd` *argv* spelling would also need
  `crates/blastit-dbproxy/src/main.rs` (the wheel's own binary, in no `paths` entry), so it is
  left as the follow-up below; the security invariant is enforced where it matters
  (`DbproxyConfig::validate_policy` refuses a `policy_fd` without `dev_policy`, and its
  sentence names `--dbproxy-policy-fd`).
- `crates/blastit/tests/critical_dev_reload.rs` — the PHP fixture, probe and row 10.
- `docs/knowledge/LESSONS.md` — the attempt-1 line (now inside `paths`).

### Follow-ups (attempt 2)

1. Acceptance **7**'s listener-set warning ("a route gains a store → one `manifest.warning`
   naming the restart") is not implemented: detecting it needs the **boot's** listener pair
   set, which only `stage1.rs` has (it is not in `paths`). The document is still authoritative
   and applied for the entries that exist, so nothing fails open; the warning is the missing
   half.
2. Acceptance **6**'s `200`: a swap observed by a real query needs the `BLASTIT_TEST_PG_URL`
   job (no Postgres here). The swap, the ack and the in-flight `Arc` are witnessed.
3. `--dbproxy-policy-fd` as an argv token (needs `crates/blastit-dbproxy/src/main.rs` as well
   as `cli.rs`/`roles.rs`).
4. Two `-D warnings` findings exist only in the `--no-default-features --features python`
   configuration (`stage1.rs:346` unused `mut`, `supervise.rs`'s `index` binding), which is not
   a gate configuration; both are on lines this node never touched.

## Attempt 3 — the two MAJORs and the three MINORs

Base commit `fde43da`, branch `release/0.2.0`, same worktree. `paths` gained
`crates/blastit/src/stage1.rs`, so acceptance 7 could land here. Nothing the reviewer
verified as good was rewritten: the dbproxy channel, the stage-0 socketpair, the PHP
report wiring and the PHP row keep their shape (the row gained one leg and the fixture one
file). Each finding below names the test that goes **red** if it regresses.

### FIX 1 [MAJOR] — a late ack can never answer the next document

`exchange_policy` now returns [`PolicyOutcome`] — `Swapped`, `Refused(cause)` (in step,
usable), `Broken(cause)` (alignment unknown) — and does three things it did not:

1. **`exchange_policy` drains first.** One non-blocking read before any write: a readable
   byte is an ack nobody waited for, so it is *consumed* (the stream is back in step) and
   reported `Broken("an earlier save's policy answer (0x01) was still unread…")`. Nothing is
   written in that case, so dbproxy never sees a document the supervisor did not get an
   answer for.
2. **A timeout taints the image.** `Supervisor::policy_out_of_step` (an `AtomicBool`) is set
   on every `Broken` outcome (timeout, broken write/read, EOF, unknown byte). While it is
   set, `refresh_policy` sends nothing and emits **one** warning — "…restart blastit dev (an
   earlier save left the policy channel out of step, so its message can no longer be
   attributed)" — because dbproxy may still be applying the document whose ack timed out and
   the next byte would be attributed to the wrong document.
3. A clean `0x01` stays in step: it warns once and a later save may try again (a refusal is a
   decision, not a lost answer).

The `0x00`-is-success path is unchanged. Red if it regresses:
`a_stray_policy_answer_is_reported_and_never_answered_as_this_document_s` (asserts the stray
byte is consumed, nothing is written, and the cause names it) and
`a_late_ack_leaves_the_channel_out_of_step_and_the_next_save_warns` (save 1 times out, the
`0x01` lands late, save 2 must warn — exactly two warnings, the second naming "out of step";
the old code emitted one and reported the second save silent).

### FIX 2 [MAJOR] — acceptance 7: the listener-set change

- `supervise::policy_pairs(manifest)` is the one definition of the document's pair set, and
  `store_set_warning(boot_pairs, manifest)` compares a save's set with it.
- `Parts.boot_pairs` + `Supervisor.boot_pairs`, filled by stage 1 from the manifest it parsed
  (`Serving.boot_pairs`, both `Parts` sites), so the base is the set dbproxy's listeners were
  bound for. A replacement image boots with the new manifest, which is exactly why the warning
  fires on the save that *makes* the change (the old image) and is not repeated on every later
  save.
- `refresh_policy` emits that warning **before** the channel and **before** the dbproxy guard
  (a boot with no dbproxy at all still owes it when a save adds its first store) and still
  sends the document afterwards: the entries that exist are updated, the ones that do not are
  never widened, so nothing fails open. Sentence:
  `the database listener set changed (added|removed route→proxy, …); every socket was bound at
  boot, so the existing listeners keep serving and the new grants apply only to them — restart
  blastit dev to bind the new set`.
- Red if it regresses: `a_store_added_or_removed_is_one_warning_naming_the_restart` (unit),
  `removing_a_store_is_one_warning_and_the_run_keeps_serving` and
  `adding_a_store_is_one_warning_naming_the_restart` (both real `blastit dev` boots on this
  host, `DATABASE_URL` set to a URL that is never dialled, `:8080` keeps answering through the
  cycle). The removal row also asserts the `dev.reload` whose routes are every view and that
  exactly **one** warning names the listener set.

### FIX 3 [MINOR] — the bounded synchronous exchange, measured

The deviation stands (a `policy_ack` poll state would defer `Ctl::Reload` across wakeups and
rewrite the cycle, which is more risk than a 100 ms worst case is worth), and it is now
**recorded and measured**: `POLICY_ACK_TIMEOUT` is the design's 100 ms;
`a_refused_policy_document_is_one_warning_and_a_swap_is_silent` times `refresh_policy` against
a peer that reads the document and never answers and asserts `90 ms <= elapsed < 500 ms`,
printing the measurement. **Measured worst case on this host: 100.7 ms** (the design cap), and
a boot with nothing to send returns in under 50 ms. The stall only happens on the pathological
save; an ordinary ack is immediate. Red if it regresses: that test's elapsed bounds (a removed
deadline would exceed 500 ms; a zero-wait shortcut would fall under 90 ms while claiming a
timeout).

### FIX 4 [MINOR] — the refusal names a field, not a token no role accepts

`DbproxyConfig::validate_policy` now says `policy_fd is 14 but `dev_policy` is false in the
configuration document: the dev policy channel exists only on a `blastit dev` boot with live
reload on, so a production or `check` dbproxy can never acquire it`. Red if it regresses:
`config::tests::the_policy_channel_is_refused_outside_a_dev_boot` and
`server::tests::a_policy_descriptor_without_dev_policy_is_refused_at_boot` (both assert the
sentence names `policy_fd` and `dev_policy`, never `--dbproxy-policy-fd`).

### FIX 5 [MINOR] — the warm compiled template, and what this worktree cannot witness

- The PHP fixture's front controller now reads a **compiled template artefact**
  (`storage/framework/views/compiled.php`, `file_get_contents`, never `include`, so it is not
  in the workers' loaded-file report) and echoes it. The row saves that artefact after the
  include reload and asserts, after `SETTLE`, that `dev.reloads()` is still **1** and that the
  next body carries the new artefact — "a warm compiled template edit needs no restart (the
  framework revalidates it)", and a reporting regression that put cache artefacts in the union
  turns it into a reload and the row red. Red if it regresses:
  `editing_an_included_php_file_reloads_every_view_once_and_serves_the_new_body`.
- **Stated plainly: acceptance 4's dbproxy end-to-end and acceptance 6's `200` half are
  unit-witnessed only in this worktree.** There is no Postgres here, so no `dev` boot can reach
  a real upstream and no SQL query can be observed flipping from denied to `200`. What *is*
  witnessed: the channel reaches stage 1 and no view holds either end (critical row, real
  boot); the reader swaps an **enforcing** listener's verdict through the real ack loop
  (`server::tests::the_policy_channel_answers_every_document_with_one_byte`); the supervisor
  writes the document and reads the ack over a real socketpair
  (`the_policy_exchange_writes_the_document_and_reads_one_ack`); and the cycle's adoption
  document names `policy` and the replacement image refuses a descriptor it does not hold
  (exercised by every `Blastfile` cycle row). The `200` needs the `BLASTIT_TEST_PG_URL` job.

### Acceptance (all re-run on the final tree)

```console
$ cargo test -p blastit-dbproxy --locked
test result: ok. 81 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
test result: ok. 5 passed; 0 failed … (main)
test result: ok. 9 passed; 0 failed … (critical_learn_and_redaction, 5.03s)
test result: ok. 24 passed; 0 failed … (postgres)
test result: ok. 1 passed; 0 failed …
```

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # twice, idle
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.99s
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.18s

$ # and twice with all 8 CPUs in busy loops
load1: test result: ok. 23 passed; … finished in 5.47s
load2: test result: ok. 23 passed; … finished in 6.29s

$ cargo test -p blastit --locked --features python,php --test critical_dev_reload -- php
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 22 filtered out; finished in 1.27s

$ cargo test -p blastit --locked --features python,php --test critical_dev_reload   # three times
test result: ok. 23 passed; 0 failed … finished in 3.70s / 2.94s / 3.21s
```

```console
$ cargo test -p blastit --locked --features python --lib
test result: ok. 289 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.89s
$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.27s
```

```console
$ cargo fmt --all -- --check
FMT=0
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile … in 4.53s
$ cargo clippy -p blastit-dbproxy -p blastit --all-targets --locked --features python,php -- -D warnings
    Finished `dev` profile … in 0.19s
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify
group, one Watcher::new)
```

```console
$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 23 passed; 0 failed …   (critical_dev_reload, build script and Makefile)
test result: ok. 42 / 7 / 9 / 27 passed, 0 failed …   (lsp / parallel_views / php_backend / php_embed)
test result: ok. 289 passed; 0 failed; 2 ignored …   (unit tier; `2 ignored` is pre-existing)
test result: ok. 81 / 91 / 73 …                      (the rest of the workspace)
Ran 53 tests in 23.625s                             (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 32.6s
ok: …/target/debug/blastit: 3 test seam(s) present, as a debug build must
Ran 19 tests in 2.599s                              (zed-check)
plan OK
```

No `critical_php_embed` probe flake tripped in this attempt's `make check` runs (27/27 in the
isolated run above and in both full runs); nothing was raised, disabled or reordered.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  0.4s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 36.7s
DOCS_TEST_EXIT=0

$ uv run --no-project tools/critical_guard.py --base fde43da --node MB.23.5.3
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.3

$ git diff --stat fde43da -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; FROZEN_EMPTY=0)
```

### Files (attempt 3 additions)

- `crates/blastit/src/supervise.rs` — `PolicyOutcome`, the drain-first exchange, the
  `policy_out_of_step` taint, `policy_pairs`, `store_set_warning`, `Parts.boot_pairs` and the
  `changed_pairs` wiring, plus four new unit legs.
- `crates/blastit/src/stage1.rs` — `Serving.boot_pairs` and both `Parts` sites (14 lines).
- `crates/blastit-dbproxy/src/config.rs` — the refusal sentence (FIX 4) and its assertion.
- `crates/blastit-dbproxy/src/server.rs` — that test's assertion.
- `crates/blastit/tests/critical_dev_reload.rs` — the compiled-artefact leg + fixture file, and
  the two store-set rows.

### Follow-ups (attempt 3)

1. Acceptance 6's `200` still needs the `BLASTIT_TEST_PG_URL` job (no Postgres here); the swap,
   the ack, the in-flight `Arc` and the supervisor's write are all witnessed without one.
2. Acceptance 7's warning is per-save: after a cycle the replacement image's base is the new
   manifest, so the same store set is not warned about again (by design — one warning for the
   save that changes it, not one per save).
3. `--dbproxy-policy-fd` as an argv token would also need `crates/blastit-dbproxy/src/main.rs`;
   the configuration-document member is the channel that exists and is tested.
