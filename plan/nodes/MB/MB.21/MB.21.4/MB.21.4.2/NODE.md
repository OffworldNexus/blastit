---
id: MB.21.4.2
title: "Prove runtime parity on Blastfile: the capability-map parity suite, the mount/entrypoint sentence re-spellings, and 4.1's five MINORs"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.4.6]
paths:
  - crates/blastit/src/config.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - "crates/blastit/tests/**"
  - "crates/blastit-scheduler/tests/**"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/**"
attempts: 2
max_attempts: 5
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T11:43"
---

# Goal

Prove the running product is the product the capability map describes: one named **runtime** assertion
per row, and any real gap closed here rather than documented away. Finish the boot and `check` sentences
whose subject is now the file's — a mount or the one `entrypoint`, never a route — and close the five
MINOR findings `MB.21.4.1`'s review handed to this leaf.

`MB.21.4.1` already landed the runtime half and this leaf re-does none of it: the scheduler serves `static`
mounts (`MountTable` longest prefix, `MountHead` refusals before views, `Missing` → view → the declared
`static` mount's last-resort file on `GET`/`HEAD` → `404`, `405` after every view); `RouteInfo` no longer
carries `static_policy` or `static_forward`; `Manifest::static_roots`/`check_static_roots` walk mounts
with `Location::Mount`; `stage0::mount_root_refusal` is gone; §9.5's
`is_all_static`/`can_boot_without_app` have the mount arms; `php_view` binds every view from the
file-level `Manifest::php()`; `StaticOutcome::SpaLost` is `FallbackMissing` with §9.4's `url_path`
prefix join; the per-mount `immutable` caps and the `index`/`fallback` dotfile refusal are in
`validate`; and every fixture `make check` reaches is a `Blastfile`. **This leaf translates no
fixture and deletes no accessor.**

`Route::{static_policy,with_static,php,with_php}` stay: `crates/blastit-manifest/src/parse.rs` (deleted in
`MB.21.4.5`) and `validate.rs`'s route arms are their only remaining readers, exactly as
`MB.21.4.1`'s `RESULT.md` recorded. §4.3's removal row is therefore `MB.21.4.5`'s; the most this leaf
can prove is that no `Blastfile` path reads them.

# Context

- Design §9.2–§9.6, §12, §13.1 and §18. The parser produces the same `Manifest`, so this leaf is mostly
  **proof** plus the places where a new spelling lands differently, and it must not weaken an assertion
  anywhere.
- **(a) the runtime parity suite — `crates/blastit-scheduler/tests/critical_blastfile_parity.rs`.** Boot it
  through the scheduler's own harness (`common::{Options::blastfile, World, Script}`, which parses with
  `Manifest::from_blastfile_str`, builds the real `RoutingTable` and hands the mounts over beside the route
  table), so a row is asserted on the manifest a user writes and the scheduler the product runs — not on a
  lowering. One `#[tokio::test]` per row, named after the row, and a header table mapping row → test:
  * static four-step precedence: a nested `static "/assets/v2"` beats `static "/assets"` on the longest
    prefix, `Missing` at step 1 reaches a view, `Hidden`/`Refused` never do, the declared `fallback`
    answers `GET`/`HEAD` only and only from the longest matching mount, `405` after every view;
  * ingress `body json|form|none|raw`, each **allowed and denied** (`json` + a form body → `400`, `none` +
    any body → `413`, `raw` unacknowledged refuses the production boot and warns under `dev`,
    `raw unvalidated` boots with the same warning);
  * `prefork` reaches the route the scheduler spawns/switches;
  * `egress any` and its `EgressRoute` boot warning arrive together (§12.4);
  * a trait's grants are what the view actually receives (`use` order, then inline; `transaction_control`
    OR-ed; queries deduplicated), `postgres … unrestricted` on the default and a named store, table and
    column grants (`select products(id, name)` vs `select products`), and `query '…'` after
    `blastit_sqlpolicy::normalize`;
  * `entrypoint php` answers **every** view from the one file-level entry;
  * `HEAD` is implied by `GET` through the real matcher, not only at parse time.
  Rows the landed suites already prove are named in `RESULT.md` beside the new assertion
  (`crates/blastit-manifest/tests/critical_blastfile_parity.rs` is the model-equality oracle from
  `MB.21.3`; `crates/blastit-scheduler/tests/mount_precedence.rs` is `MB.21.4.1`'s narrative precedence),
  so nothing claims coverage it does not add and no assertion is copied verbatim. The static row asserts
  the **outcome table** (a request → a status), which is the half `mount_precedence.rs` does not pin.
- **(b) the sentences whose subject changed, which `MB.21.4.3` does not own.** §13.2's six boot warnings
  (`crates/blastit-manifest/src/error.rs`) and §10.2's `raw_ingress_refusals` sentence (`validate.rs`), and
  §11.3's `trait` nouns, are **`MB.21.4.3`'s** — this leaf touches none of those strings and edits neither
  file for them. What is left is the boot/`check` prose in this leaf's own files:
  * `stage0::static_app_refusal` still says `BLASTIT_APP: every route is static, so no route can reach an
    app: unset BLASTIT_APP, or declare a route without [route.static]`. A `Blastfile` has no `[route.static]`
    block to point at and no route to name: it becomes the mount's sentence, same trigger, same exit code,
    same order. Its two unit assertions (`stage0.rs`, `starts_with("BLASTIT_APP: every route is static")`)
    follow;
  * `stage0`'s libphp-absent sentence («the route declares no [route.static] block … or add a [route.static]
    block») names the entrypoint and the mount instead;
  * `check`'s `nothing to learn: every route is static` and `audit_v1::Report::all_static`'s doc comment
    stop naming routes;
  * `audit_v1::unrequested_static_roots` and `check.rs`'s `route "{route}": static root "{root}" was never
    requested`. Today the function filters `route.static_policy()`, so on a `Blastfile` it can never report
    anything — the R14 hint is dead exactly where the developer now writes mounts. It iterates mounts, the
    sentence names the mount's prefix, and the rule for "requested" is the mount table plus the observed
    paths (no `stat` at `check` time — a root's existence is the boot audit's job, §9.6). It stays a
    stderr warning: never a finding, never `ok`, never the exit code.
  Every re-spelled string keeps its trigger, its order and its exit code; `RESULT.md` lists each with its
  old and new subject, and the suites that assert the string assert the new one (never merely "a warning
  happened").
- **(c) the five MINOR findings from `MB.21.4.1`'s review, measured on the tree at `1cfdace`.** (The sixth,
  `docs/local.md:80,92`'s prose, is `MB.21.5.1`'s and is not touched here.)
  1. **An escaped manifest path.** `config::missing_manifest` (`config.rs`, `path.display()`), `check`'s
     `read_manifest` and its `path: {error}` arm (`check.rs`), and the two `stage0` boot contexts plus
     `stage1`'s re-read put the operator's `BLASTIT_MANIFEST` string into a one-line sentence unescaped, so
     a path holding a newline or a control byte prints two lines. Reuse `error::clipped` (it escapes control
     bytes and cuts at 120 `char`s) through **one** helper — a third folding copy is the defect, not the fix.
  2. **`fs::read_to_string` at the manifest sites** (`config.rs`'s `manifest_wiring`, `check.rs`'s
     `read_manifest`, `stage0.rs`'s two boot paths, `stage1.rs`'s re-read): a FIFO at `./Blastfile` blocks
     `check` and `dev` forever. `blastfile::read(path)` refuses a non-regular file and caps the read before
     opening; each site keeps its own rendering (a missing file still produces §15.1's one sentence, every
     other error keeps its `path: reason` shape) and its exit code.
  3. **A real `fmt`-regression test.** `print.rs::a_printer_regression_is_refused_rather_than_written`
     never calls `fmt` — it parses two strings. It must call `blastfile::fmt` and be able to fail for a
     printer's reason: every committed fixture under `crates/blastit-manifest/tests/fixtures` formats to
     text that re-parses, and a document `fmt` cannot return a re-parseable, budget-fitting rendering of is
     `Err` rather than a written string. The test's name must not claim more than it asserts.
  4. **One `pattern_is_writable`.** `dev_learn.rs` and `audit_v1.rs` hold byte-identical copies. One
     crate-private definition (in `audit_v1`, which owns the `--json` vocabulary), one doc comment,
     `dev_learn` calls it.
  5. **The static-only predicates disagree.** `Manifest::is_all_static`/`can_boot_without_app` and
     `stage0::is_static_only`/`runs_own_executors` each claim in their doc comments to answer "the same
     question", and do not: the model accepts a `route.static_policy()` (the retired reader's shape) and
     `entrypoint php` with no view, the boot requires `routes().is_empty()` and at least one view for the
     PHP half. §9.5/§12.2 answer it: an `entrypoint php` no view reaches is the `EntrypointNotReached`
     **warning**, never a missing `BLASTIT_APP`; a static-only file is mounts, no views, no executor. One
     answer, and the boot calls it. The retired reader's route arms stay as that reader's compatibility
     path until `MB.21.4.5` and their doc comments must say so instead of claiming equivalence.
- **Paths and Cargo.** `crates/blastit-scheduler/src/**` is in `paths` only so a gap the parity suite proves
  is closed in the leaf that found it (the expectation is a small diff, `MB.21.4.1` landed the runtime). No
  `Cargo.toml`/`Cargo.lock` change is available here: `MB.21.4.5` owns both, and a suite that seems to need
  a new dependency belongs in the crate that already depends on it.

# Acceptance

```console
$ make test-critical
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.2
$ grep -rnE 'Route::with_static|fn with_static\(|fn static_policy\(|fn with_php\(|SpaLost|static_forward' crates/ --include='*.rs' | grep -vE '^crates/blastit-manifest/src/(model|parse)\.rs:'
$ grep -rn 'every route is static' crates/blastit/src crates/blastit/tests
$ grep -rn 'route "site"' crates/blastit/tests/
$ git diff --stat HEAD~1 -- '*.toml'
$ make check
```

1. `crates/blastit-scheduler/tests/critical_blastfile_parity.rs` boots through `Options::blastfile` and the
   real mount/route tables, holds one named case per capability-map row (the eleven rows in Context (a)),
   and its header maps every row to its test. `RESULT.md` says, for each row, whether the assertion is new
   or what landed suite already proved the other half.
2. Every precedence and refusal claim has a **deny side**: `json`+form → `400`, `none`+body → `413`, `raw`
   unacknowledged → production boot refused and `dev` warned, `raw unvalidated` → boots with the same
   warning, a dotfile/`..`/`.php` path under a mount never reaches the view, and a `fallback` missing from
   the longest matching mount is `404` even when an outer mount declares one.
3. A static-only `Blastfile` (mounts, no views) boots and passes `check` with no `BLASTIT_APP`; a mount root
   that is missing or unreadable is refused naming the mount's prefix, not a route; a mount no observed
   request reached is still reported on stderr naming that prefix, and is still only a warning.
4. Each of the five MINOR findings has a test that fails without its fix: the newline-in-path fixture prints
   exactly one sentence from `check` and from `dev`; a FIFO at the discovered manifest path makes `check`
   exit `2` and `dev` exit `1` promptly instead of blocking; the `fmt` test calls `fmt`; `pattern_is_writable`
   exists once; and one fixture set pins both predicates' arms (PHP entrypoint with no view = warning, no
   missing app).
5. The fourth command prints only `crates/blastit-manifest/src/{model,parse}.rs` lines (the accessor
   definitions and the retired reader's own builder — both removed by `MB.21.4.5` with the reader); the
   fifth prints nothing; the sixth prints no assertion.
6. `make test-critical` exit 0 with zero skips — the new suite is picked up by `rs-test-critical`'s
   `crates/*/tests/critical_*.rs` glob — `make rs-test` exit 0, `make check` exit 0, and
   `critical_guard.py --node MB.21.4.2` is clean: every protected suite is extended, never weakened, and
   `RESULT.md` lists each changed assertion with its old and new subject.
7. This leaf translated no fixture: the seventh command prints nothing (no `*.toml` manifest appears,
   changes or disappears in this commit), no fixture file is renamed, and `RESULT.md` names the suites it
   *extended*.
8. Every behavioural change beyond parsing is named in `RESULT.md` with the test that pins it — the
   `HEAD`-from-`GET` matcher case and what "a requested mount" means included. There are none unnamed.

# Notes

- No detector, no sniffer, no second file name, no migration text: a body that is not a `Blastfile` is an
  ordinary `Blastfile:<line>:<col>` syntax error, and this leaf adds no sentence that explains the change to
  a user.
- `MB.21.4.3` owns §13.2's six warnings, §10.2's `raw_ingress_refusals` sentence and §11.3's nouns; do not
  re-spell one here even while touching a neighbouring line.
- `MB.21.4.5` owns `[route.php]`/`[route.static]` prose that only describes the retired reader (`host.rs`,
  `php_view.rs`, `validate.rs`'s stale "one route may declare" cap comments) and the dead
  `stage0::php_worker_budget` sentence. If the sweep meets such a hit outside this leaf's sentences, record
  it in `RESULT.md` with that sibling and do not widen.
- Protected suites under `crates/*/tests/` and `crates/blastit-manifest/tests/` are in `paths` to be
  **extended**: a removed assertion is named in `RESULT.md` with its replacement contract. A mount has no
  route name, so a sentence that loses its subject records the new subject — never a dead noun, and never a
  weaker "a warning exists" check where a string is asserted.
- A case that can only be proven in a container names the `e2e/` scenario that covers it; `MB.21.4.4` owns
  that fixture.
