# Result: MB.21.4.5.5 — translate the scheduler's fixtures and delete its second reader

* base: `2ef786ce381dce05208d8d4d09a50f8eb4d7589b` (MB.21.4.9), working tree uncommitted
* host: `release/0.2.0` worktree; sibling leaves MB.21.4.5.1/.2 have uncommitted
  `crates/blastit-manifest/**` work in the same tree (see Follow-ups)
* status: **done**, including the review's MINOR (retired v1 spellings in doc comments). One
  acceptance command can only fail on files outside this node's `paths`; explained under
  "critical-guard".

## What changed

The scheduler's tests now have exactly one reader, the product's, and the crate's prose names the
`Blastfile` spellings rather than the retired ones.

| File | Change |
|---|---|
| `tests/common/mod.rs` | `Source::{Toml, Blastfile}` **deleted**; `Options::manifest(&str)` **deleted** (`Options::blastfile` is the only setter and only stages a `Blastfile`); `Options::new()`'s default manifest is a `Blastfile`; `World::start` parses with `Manifest::from_blastfile_str` and nothing else; `route(name, path)` now emits `view <name> regex '<path>' {}` |
| `tests/*.rs` (10 fixture files) | every fixture rewritten as `Blastfile` text; `.manifest(…)` call sites all became `.blastfile(…)` |
| `src/route.rs` | 3 unit fixtures on `from_blastfile_str`; doc comments swept: `[route.ingress]` → `body`, `prefork = true` → `prefork`, `body = "none"` → `body none`, and the "retired reader lowers the block to a mount at `/`" note now says the mount declares its root |
| `src/lib.rs` | the crate-level doc example is `Manifest::from_blastfile_str("view about \"/about/\" {}\n")` — same assertions, no behaviour |
| `src/config.rs` | "The directory holding `blastit.toml`" → "The manifest's directory"; the same sentence's `[route.static]` → "a `static` mount" |
| `tests/critical_static_entry.rs` (comment only) | the header's `[route.php] entry = "…"` beside `[route.static] root = "…"` → `entrypoint php "public/index.php"` beside a `static "/" { root "public" }` mount |
| `tests/critical_static_assets.rs`, `tests/common/static_fixture.rs` (comment only) | the last `spa = true` / `immutable = [...]` / `app` prose → a declared `fallback`, `immutable "…"`, `entrypoint` |

Nothing outside `crates/blastit-scheduler/src/{lib,route,config}.rs` and
`crates/blastit-scheduler/tests/**` was touched, and no assertion, fixture or test name changed in
this round: the follow-up edits are comment text and `rustfmt` output only.

## Acceptance

### 1. Both greps print nothing over the whole crate

```console
$ grep -rnE 'from_toml_str|Source::Toml|toml::' crates/blastit-scheduler; echo "exit=$?"
exit=1
$ grep -rnE 'blastit[.]toml' crates/blastit-scheduler; echo "exit=$?"
exit=1
```

`grep -rniE 'toml' crates/blastit-scheduler --include='*.rs'` also prints nothing: the crate no longer
names the retired format at all.

### 2. `cargo test -p blastit-scheduler` exits 0

```console
$ cargo test -p blastit-scheduler 2>&1 | tail -12
test requests_are_routed_by_path_and_method_and_rule ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s

   Doc-tests blastit_scheduler

running 1 test
test crates/blastit-scheduler/src/lib.rs - (line 32) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

all doctests ran in 0.69s; merged doctests compilation took 0.68s
$ cargo test -p blastit-scheduler; echo "exit=$?"   # full run, all binaries
exit=0
```

Per binary, all `ok … 0 failed`: lib 49, admission 11, critical_blastfile_parity 10, critical_charge 7,
critical_ingress_enforcement 16, critical_rewrite 32, critical_static_assets 14, critical_static_entry 6,
critical_static_resolve 18, ctl_channel 9, fairness 17, flow 6, head_allocs 2, isolation 13, lifecycle 18,
memory 14, mount_precedence 4, routing 14, doctests 1.

`cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings` → `Finished` (0 warnings).

### 3. `make rs-fmt` and `make rs-doc`

```console
$ make rs-fmt; echo "exit=$?"
cargo fmt --all -- --check
exit=0
$ make rs-doc; echo "exit=$?"
 Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.26s
   Generated …/target/doc/blastit/index.html and 13 other files
exit=0
```

(RUSTDOCFLAGS="-D warnings"; the crate example is the `Blastfile` one.)

### 4. `make critical-guard BASE=2ef786ce… NODE=MB.21.4.5.5`

The strict form of the command **fails, and every violation is a file outside this node's paths** —
the two sibling leaves' in-progress `crates/blastit-manifest/tests/critical_*.rs` edits:

```console
$ uv run --no-project tools/critical_guard.py --base 2ef786ce381dce05208d8d4d09a50f8eb4d7589b --node MB.21.4.5.5
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_php_manifest.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_static_manifest.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_charge.rs  [authorized by crates/blastit-scheduler/tests/**]
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [authorized by crates/blastit-scheduler/tests/**]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by crates/blastit-scheduler/tests/**]
critical-guard: FAILED: 3 protected critical test(s) changed or deleted without a covering path
exit=1
```

Adding the parent epic (which owns those three files, `crates/blastit-manifest/tests/**`) makes it clean,
and no scheduler path is ever a violation:

```console
$ uv run --no-project tools/critical_guard.py --base 2ef786ce… --node MB.21.4.5.5 --node MB.21.4.5
  … all six scheduler/manifest changes authorized …
critical-guard: OK: all 6 protected change(s) are authorized by node(s) MB.21.4.5.5, MB.21.4.5
exit=0
```

(That was run before the two extra comment-only sweeps landed, which are authorized by the same
`crates/blastit-scheduler/tests/**` glob — `tests/critical_static_entry.rs` and
`tests/critical_static_assets.rs` appear with `[authorized by crates/blastit-scheduler/tests/**]`.)

So: my protected suites are authorized by this node; the three remaining hits belong to
`MB.21.4.5.1`/`.2`'s uncommitted work in this shared tree.

### 5. The protected suites keep every case

| Suite | Cases before | Cases after |
|---|---|---|
| `tests/critical_rewrite.rs` | 32 | 32 |
| `tests/critical_ingress_enforcement.rs` | 16 | 16 |
| `tests/critical_charge.rs` | 7 | 7 |
| `tests/critical_static_entry.rs` | 6 | 6 |
| `tests/critical_static_assets.rs` | 14 | 14 |

Counted with `cargo test -p blastit-scheduler --test <suite> -- --list | grep -c ': test$'` before the
first edit and after the last one. Every assertion is byte-identical: only fixture text, the `.blastfile`
setter name, the parse call and (in this round) comment text changed.
`critical_ingress_enforcement.rs` is the file MB.21.4.9 had just rewritten; its head-refusal cases
(`a_declared_body_on_an_ingress_less_route_is_413_from_the_head`,
`an_ingress_less_head_that_waits_holds_one_queue_slot_and_no_more`,
`a_mount_miss_with_a_body_before_req_end_is_413_without_a_view`) are untouched apart from their fixtures.

## `git diff --stat`

```console
$ git diff --stat -- crates/blastit-scheduler
 crates/blastit-scheduler/src/config.rs                       |   6 +-
 crates/blastit-scheduler/src/lib.rs                          |   4 +-
 crates/blastit-scheduler/src/route.rs                        |  53 +++---
 crates/blastit-scheduler/tests/common/mod.rs                 |  49 ++----
 crates/blastit-scheduler/tests/common/static_fixture.rs      |  19 ++-
 crates/blastit-scheduler/tests/critical_charge.rs            |   9 +-
 crates/blastit-scheduler/tests/critical_ingress_enforcement.rs | 80 ++++-----
 crates/blastit-scheduler/tests/critical_rewrite.rs           | 190 ++++++++++++---------
 crates/blastit-scheduler/tests/critical_static_assets.rs     |   3 +-
 crates/blastit-scheduler/tests/critical_static_entry.rs      |   6 +-
 crates/blastit-scheduler/tests/fairness.rs                   |  11 +-
 crates/blastit-scheduler/tests/flow.rs                       |   9 +-
 crates/blastit-scheduler/tests/head_allocs.rs                |  38 ++---
 crates/blastit-scheduler/tests/isolation.rs                  |   6 +-
 crates/blastit-scheduler/tests/lifecycle.rs                  |  32 ++--
 crates/blastit-scheduler/tests/memory.rs                     |  22 ++-
 crates/blastit-scheduler/tests/routing.rs                    | 125 ++++++---------
 17 files changed, 303 insertions(+), 359 deletions(-)
```

## What each suite's fixtures still declare

One line per file, naming every construct the retired documents carried. Nothing was dropped: no fixture
lost a `prefork`, a body codec, an `unvalidated` acknowledgement, a method list or a multi-pattern view.

| Suite | What its fixtures declare now |
|---|---|
| `tests/common/mod.rs` | default `view app "/*" { body raw unvalidated }`; the `route()` helper emits `view <name> regex '<path>' {}` so a v1 regex is carried verbatim (§8.1 `regex` is the one form that takes a pattern verbatim) |
| `tests/routing.rs` (11 documents) | one view with `GET` and **two patterns** (`regex '/products/[0-9]+/', regex '/products/[0-9]+/x'` — rule 0 and rule 1 of one view, which `requests_are_routed_by_path_and_method_and_rule` proves); one view with `body raw unvalidated`; the dot-segment view's `regex '/a/\.\./b'`; the rest single-pattern views |
| `tests/lifecycle.rs` (5 documents) | `prefork` in two fixtures (`view hot "/hot/" { prefork }`, `view hot "/hot/" {\n prefork\n}`), a raw-body view beside a catch-all, three plain views |
| `tests/critical_rewrite.rs` (5 consts + 4 inline parses = 9 documents) | `entrypoint python "app:app"` (the v1 `app` callable, §7.1), `static "/"` with `root`, `fallback "index.html"` (v1 `spa = true`), `immutable "/assets/*"`, `body raw unvalidated`, `body json` |
| `tests/critical_ingress_enforcement.rs` (5 documents) | `body raw unvalidated`, `body json`, `body none`, a view with no `body` at all (the `R12` lane), and the `static "/"` front-controller mount with `view app GET, POST "/*"` |
| `tests/memory.rs` (3 documents) | `prefork` on all three views of the burst fixture, a raw-body view beside `/b/`, three plain views |
| `tests/head_allocs.rs` (2 documents) | no `body` vs `body none` — the two `AllowEmptyOnly` arms the allocation comparison needs |
| `tests/isolation.rs` (1 document) | two plain views (`/a/`, `/b/`) |
| `tests/flow.rs` (1 document) | one plain view (`/slow/`) and one `body raw unvalidated` catch-all (the upload) |
| `tests/fairness.rs` (1 document) | two prefix patterns, `"/a/*"` and `"/b/*"` (the v1 `/a/.*` exactly) |
| `tests/critical_charge.rs` (1 document) | two views, both `body raw unvalidated` |
| `tests/critical_static_entry.rs`, `tests/critical_static_assets.rs` | no document (both already `Blastfile`): comment-only sweeps of the retired spellings |
| `src/route.rs` (3 unit fixtures) | `prefork` (the uid/order test), a view with no `body` (the fail-closed `ingress == None` assertion), and `static "/" { root "public" }` beside `view app "/*" {}` (mount prefix `/`, one mount, no route flag) |
| `src/lib.rs` (1 doc example) | `view about "/about/" {}` — `RoutingTable::new`, `routes_from_manifest`, `run` unchanged |
| `src/config.rs` (1 sentence) | "The manifest's directory, absolute" |

`egress any` appears in **no** fixture, as before: no suite asserts a boot warning, and the one suite that
does (`critical_blastfile_parity.rs::row_egress_any_and_its_warning_arrive_together`) states it in its own
`Blastfile` already. Adding `egress any` to a fixture that asserts "no warning" would have made the
manifest warn, which is exactly the trap the node names.

## Follow-ups (out of this node's `paths`)

1. `[route.ingress]` / `[route.static]` spellings still appear in doc comments of
   `crates/blastit-scheduler/src/{scheduler.rs,rewrite.rs,static_asset.rs}`. `src/route.rs` is inside this
   node's `paths` and **has been swept** as part of the review follow-up; the three remaining files are
   not, and their prose (and `rewrite.rs`'s `[route.static]` narrative) should be swept by whoever owns
   them.
2. The `blastit-manifest` protected suites (`critical_manifest_011.rs`, `critical_php_manifest.rs`,
   `critical_static_manifest.rs`) are modified in this working tree by `MB.21.4.5.1`/`.2`. My
   `critical-guard --node MB.21.4.5.5` run reports them because the tree is shared; nothing in that report
   points at a scheduler path.
3. Host toolchain note: mid-session the shared host's `rustfmt` began asking for import reordering
   (`{Rewrite, decide}` → `{decide, Rewrite}`, `{Mode, chmod}`, constant-last in braced lists) in three
   files of this crate, and `cargo fmt --all -- --check` was briefly red on them without any edit of mine.
   `make rs-fmt` is green now with rustfmt 1.11.0-stable and the tree carries no reordering churn; if CI's
   rustfmt differs, `rs-fmt` may report those three files (`tests/common/static_fixture.rs`,
   `tests/critical_static_assets.rs`, `tests/critical_static_entry.rs`) and nothing else.
