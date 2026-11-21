---
id: MB.21.3.7
title: "Model parity (b): StaticPolicy.fallback replaces spa, mount-prefix shape rules, and sandbox-keyed uniqueness"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.3.0]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/routing.rs"
  - "crates/blastit-manifest/src/uid.rs"
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - "crates/blastit-manifest/tests/parse.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-16T02:16"
updated: "2026-11-16T03:35"
---

# Goal

Finish the model half of the parity pair: give `StaticPolicy` the fallback the Blastfile `static`
block needs, and land the whole-file rules that only make sense once mounts exist.

- `StaticPolicy` gains `fallback: Option<String>` and loses `spa: bool` (§9.3), and the retired reader
  lowers `spa = true` to `fallback = Some(index)` (the two are the same fact: a deep link answered by a
  named file). The readers of the removed flag derive it from the new field: `validate.rs`'s two
  "answers every miss itself" refusals keep their exact sentences, `crates/blastit-scheduler/src/route.rs`
  derives `static_forward`, and `static_asset.rs` keeps its own internal `spa` (derived from the new
  field) until `MB.21.4.2` reshapes that path.
- Every `StaticPolicy { … }` literal in the workspace moves with the field: the two manifest suites,
  `docs_fences.rs`, `critical_manifest_edit_executor.rs`, `tests/parse.rs`, the scheduler's
  `tests/common/static_fixture.rs` and `tests/critical_rewrite.rs`.
- §9.1: `validate` refuses a malformed mount prefix (no leading `/`, a trailing `/` other than `"/"`,
  `..`, an empty segment, `\`, NUL, a control byte, `?`, `#`, `%`) and a duplicated prefix
  (`DuplicateMount`); `index` and `fallback` refuse a dotfile segment, `root` keeps today's rules.
- §10.3: `check_routes` compares `sandbox` ids, not route names, so several endpoints of one `view` are
  legal. A TOML file still gets one sandbox per route, so every `DuplicateRouteName` sentence and every
  suite that pins it is unchanged.
- `src/error.rs`'s `TooManyRoutes` doc and message re-worded to "the route carrying the highest
  sandbox", which is what the check now names.
- `src/parse.rs`'s bridge is pinned by a test rather than left implicit: the reader's mount list holds
  **one mount per prefix and the first `[route.static]` block wins**, exactly as `php_of` already keeps
  the first front controller. One mount per block is impossible while §9.1's `DuplicateMount` is an
  invariant: every `[route.static]` block lowers to the `"/"` prefix, so two blocks would be two `"/"`
  mounts and a protected suite pins that a three-static-route `blastit.toml` validates. Say it in the
  builder's doc comment and assert it in `tests/parse.rs`.

**What this leaf deliberately does NOT do** (design §4.3's "Consumers" column already assigns these to
`MB.21.4.2`, and none of them can be done without changing what the product does today):

- deleting `Route::static_policy()`/`with_static`/`php`/`with_php`. The four accessors are the only
  route→mount and route→php association in the model until the runtime reads mounts, and four protected
  contracts read them (the scheduler's "an app route matches before a later static route", the
  `route "site"` static-root sentence, the `route "app"` PHP refusal, and the all-static `check` path).
  Deleting them here would either break those suites or need a `mount_routes` index the design does not
  state; `MB.21.4.2` removes them together with the precedence work that makes the association moot.
- renaming `StaticOutcome::SpaLost` to `fallback_missing` (§9.2 step 3) and the §9.5/§9.6
  `is_all_static`/`static_roots` shape. Both are runtime-shaped: §9.5's "no views, at least one mount"
  flips a TOML static-only site to "needs `BLASTIT_APP`" while the retired reader is still in service,
  and the rename reaches `rewrite.rs` plus two more protected suites. `MB.21.4.2` owns both, with the
  runtime precedence.

**Behaviour does not change in this leaf.** Same sentences, same exit codes, same `--json` vocabulary,
and `blastit.toml` keeps parsing and serving exactly as today.

This leaf is the second half of design `MB.21.3.0b` (§21), narrowed to what is green on its own commit;
the hand-off to `MB.21.4.2` is recorded here and in the epic's `RESULT.md`.

# Context

- Design: §4.3 (the removal list), §9.2/§9.3/§9.5 (what replaces a per-route static policy), §12.2
  (`manifest.php()` hands the front controller to every view), §21 (this leaf's work and acceptance).
- The `StaticPolicy` field change is here, not in `MB.21.3.0`: every `StaticPolicy { … }` literal in the
  workspace is in a file of this leaf's list (`crates/blastit-scheduler/src/route.rs`,
  `crates/blastit-scheduler/tests/common/static_fixture.rs`,
  `crates/blastit-scheduler/tests/critical_rewrite.rs`, `tests/critical_static_manifest.rs`,
  `tests/critical_php_manifest.rs`, `tests/critical_manifest_edit_executor.rs`, `tests/parse.rs`, and
  the two in-crate unit-test literals `MB.21.3.0` wrote in `src/routing.rs` and `src/uid.rs`), so
  `MB.21.3.0` could not have added the new field without leaving the workspace red outside its paths.
- Measured readers of the removed flag, all in this leaf's list: `src/validate.rs` (two refusals),
  `crates/blastit-scheduler/src/route.rs` (`static_forward`), `static_asset.rs` (its own internal
  `spa`). The four per-view accessors keep their sites outside this leaf, which is why they are
  `MB.21.4.2`'s (see `# Goal`).
- `MB.21.3.0`'s `tests/parse.rs` asserts the bridge's byte-for-byte `spa` carrying; those four
  assertions are yours to re-point at `fallback`.
- The retired reader and patcher still exist (`MB.21.4.5` deletes them), so nothing here may remove a
  TOML code path: `crates/blastit-manifest/Cargo.toml` keeps its `toml` dependency.

# Acceptance

```console
$ make rs-fmt
$ make rs-lint
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --locked
$ grep -n 'spa' crates/blastit-manifest/src/model.rs
$ grep -rn 'spa = true' crates/blastit-manifest/tests/critical_static_manifest.rs crates/blastit-manifest/tests/critical_php_manifest.rs
$ uv run --no-project tools/critical_guard.py --base ef783db0c5c094a9f6aff5e2042e7955c40cadb5 --node MB.21.3.7
$ make check
```

1. `StaticPolicy` has `fallback: Option<String>` and no `spa: bool`: the first grep prints nothing, the
   workspace compiles with every literal updated, and the retired reader still accepts `spa = true`,
   lowering it to `fallback = Some(index)` — so `critical_static_manifest.rs` keeps every one of
   today's assertions (its `spa = true` fixtures stay, re-pointed at the new field) and the second grep
   still finds them.
2. The workspace compiles with `-D warnings` (`make rs-lint`, `make rs-doc`) and `make check` exits 0.
3. No protected suite loses a claim: `critical_static_manifest.rs`, `critical_php_manifest.rs`,
   `critical_manifest_edit_executor.rs` and `critical_rewrite.rs` change only where the field name
   changed; `critical_guard.py --node MB.21.3.7` is clean, and `RESULT.md` lists each edit with the old
   and new shape.
4. §9.1 is tested both ways: a `static` block with a malformed prefix (and two with the same prefix,
   `DuplicateMount`) is refused with message and position, a legal `"/"` and `"/assets"` mount is
   accepted, and `index "…"`/`fallback "…"` refuse a dotfile segment while `root ".output"` is legal.
5. §10.3 is tested: two routes of one sandbox are legal, two sandboxes with the same name are
   `DuplicateRouteName`, and the TOML path (one sandbox per route) keeps its current sentences.
6. `tests/parse.rs` pins the bridge: two `[route.static]` blocks still validate and yield one `"/"`
   mount (the first block wins, since §9.1 makes a second `"/"` mount a `DuplicateMount`), two
   `[route.php]` blocks keep the first policy, and the builder's doc comment says both.
7. No behavioural change: the same sentences and exit codes, no `--json` key moved (that is
   `MB.21.4.3`'s re-spelling), and `blastit.toml` parses and serves exactly as before.

# Notes

- Nothing outside `paths` may change. The four per-view accessors, `StaticOutcome::SpaLost`,
  `is_all_static`/`static_roots` and the scheduler's own precedence are `MB.21.4.2`'s (see `# Goal`);
  do not start them here, and do not "fix" a precedence question by editing a test.
- If you find a call site that this leaf's field change breaks and that is **not** in `paths`, stop and
  report it: the list above came from a measured pass, and a new one means the plan is wrong, not you.
- `MB.21.4.2`'s `paths` will need widening to take the deferred items; note every one you touch in
  `# Follow-ups` so the epic can carry the list.
