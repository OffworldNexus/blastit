---
id: MB.21.3.7
title: "Model parity (b): delete the per-view static/PHP accessors, rename StaticOutcome::fallback_missing, move the scheduler and blastit call sites and rewrite the three suites"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.0]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - "crates/blastit-scheduler/src/route.rs"
  - "crates/blastit-scheduler/src/rewrite.rs"
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
  - "crates/blastit-scheduler/tests/critical_static_entry.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - crates/blastit/src/config.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/php_view.rs
  - crates/blastit/tests/critical_learn_executor_blocks.rs
  - "crates/blastit-php-sapi/src/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T02:16"
updated: "2026-11-16T03:04"
---

# Goal

Finish model parity: move every call site off the per-view static/PHP accessors and onto the
mount/PHP model `MB.21.3.0` landed.

- Delete `Route::static_policy()`, `Route::with_static`, `Route::php()` and `Route::with_php`, and every
  temporary bridge `MB.21.3.0` left behind (`RESULT.md` of `MB.21.3.0` lists them).
- Rename `StaticOutcome::SpaLost` to `StaticOutcome::fallback_missing`, with the doc comment "the
  declared `fallback` file is missing or refused" (§9.2 step 3).
- `StaticPolicy` gains `fallback: Option<String>` and loses `spa: bool` (§9.3), and the retired reader
  lowers `spa = true` to `fallback = Some(index)`. The scheduler's two readers of the removed flag —
  `route.rs`'s `static_forward` and `static_asset.rs`'s own `spa` — derive it from the new field
  instead, and every `StaticPolicy { … }` literal in the workspace moves with them.
- Rewrite the three suites that assert the old accessors to the new model, keeping every assertion:
  `crates/blastit-manifest/tests/{critical_static_manifest,critical_php_manifest,docs_fences}.rs`.
- `stage0`, `check`, `audit_v1`, `dev_learn` and `php_view` read `manifest.mounts()`/`manifest.php()`
  instead of a per-route answer.
- The rest of the model half's whole-file rules, which `MB.21.3.0` deliberately left as bridges:
  - §9.1: `validate` refuses a malformed mount prefix (no leading `/`, a trailing `/` other than `"/"`,
    `..`, an empty segment, `\`, NUL, a control byte, `?`, `#`, `%`) and a duplicated prefix
    (`DuplicateMount`); `index` and `fallback` refuse a dotfile segment while `root` keeps today's rules.
  - §9.5/§9.6: `can_boot_without_app` and `is_all_static` take the new shape (`php.is_some()` or no
    views with at least one mount), `Manifest::static_roots` returns `(mount_index, PathBuf)` in mount
    order, and `check_static_roots` iterates mounts, keeping every sentence and the boot order.
  - §10.3: `check_routes` compares `sandbox` ids, not route names, so several endpoints of one `view`
    are legal; a TOML file still gets one sandbox per route, so today's `DuplicateRouteName` sentences
    are unchanged.
- `crates/blastit-manifest/src/lib.rs`: delete the crate-doc intra-doc links to the two removed
  accessors (they would fail `make rs-doc` with `-D warnings`), and `src/error.rs`'s `TooManyRoutes`
  doc + message re-worded to "the route carrying the highest sandbox".
- `src/parse.rs`'s temporary bridge is lossless enough to pin: a two-`[route.static]` file yields one
  `"/"` mount per block and a two-`[route.php]` file keeps the first policy. Add the parse test that
  states both, and say it in the builder's doc comment.
- `crates/blastit-manifest/tests/uid.rs` proves the numbering on the public surface; the "one `view`
  with two endpoints shares one uid" and "a mount owns no uid" cases stay the `src/uid.rs` unit tests
  `MB.21.3.0` landed (doctests are external crates and `with_sandbox` is crate-internal).

**Behaviour does not change in this leaf.** The runtime mount precedence (mounts before views, fallback
after views) is `MB.21.4.2`'s; here the product must behave exactly as it does today, with the same
sentences, the same exit codes and the same `--json` vocabulary.

This leaf is **design `MB.21.3.0b`** (§21). The plan tool has no letter-suffixed ids, so the model half
is `MB.21.3.0` and this half is `MB.21.3.7`.

# Context

- Design: §4.3 (the removal list), §9.2/§9.3/§9.5 (what replaces a per-route static policy), §12.2
  (`manifest.php()` hands the front controller to every view), §21 (this leaf's work and acceptance).
- The `StaticPolicy` field change is here, not in `MB.21.3.0`: every `StaticPolicy { … }` literal in the
  workspace is in a file of this leaf's list (`crates/blastit-scheduler/src/route.rs`,
  `crates/blastit-scheduler/tests/common/static_fixture.rs`,
  `crates/blastit-scheduler/tests/critical_rewrite.rs`, `tests/critical_static_manifest.rs`,
  `tests/critical_php_manifest.rs`, `tests/critical_manifest_edit_executor.rs`), so `MB.21.3.0` could
  not have added the new field without leaving the workspace red outside its paths.
- Measured call sites of the removed accessors, each of which must compile afterwards:
  `crates/blastit-scheduler/src/route.rs`, `crates/blastit-scheduler/tests/critical_rewrite.rs`,
  `crates/blastit/src/{stage0,stage1,check,audit_v1,dev_learn,php_view}.rs`,
  `crates/blastit/tests/critical_learn_executor_blocks.rs`, and the three suites above.
- `StaticOutcome` lives in `crates/blastit-scheduler/src/static_asset.rs`; its readers are
  `rewrite.rs` and the scheduler suites, including `tests/critical_static_resolve.rs`.
- The retired reader and patcher still exist (`MB.21.4.5` deletes them). One named helper in `route.rs`
  may translate a mount back into the per-route static answer the scheduler expects today; that helper
  is the single bridge, not a scattering of special cases, and `RESULT.md` names it and its deletion
  owner (`MB.21.4.2`).
- `crates/blastit-php` does not exist: the PHP crate is `crates/blastit-php-sapi`. It reads no manifest
  accessor today, so the expectation is **zero changes** there; the path is listed because §21 names it
  and because a compiler error, if one appears, is yours to fix here.

# Acceptance

```console
$ make rs-fmt
$ make rs-lint
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --locked
$ grep -rnE 'with_static\(|with_php\(|SpaLost' crates/ --include='*.rs'
$ grep -rnE '\.static_policy\(\)|\.php\(\)' crates/ --include='*.rs'
$ grep -n 'spa' crates/blastit-manifest/src/model.rs
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.3.7
$ make check
```

1. Both greps print nothing: the four accessors, their builders and the `SpaLost` spelling are gone from
   every crate.
2. The workspace compiles with `-D warnings` and `make check` exits 0.
3. Every existing suite keeps its assertions: no test is deleted without a replacement in the same
   leaf, each fixture rewrite is listed in `RESULT.md` with the old and new model shape, and
   `critical_guard.py --node MB.21.3.7` is clean for the three rewritten suites.
4. `critical_php_manifest.rs` asserts `entrypoint php` through `manifest.php()` (one policy for every
   view, `workers` still `None`) and `docs_fences.rs` still validates every documented document it
   validated before.
5. `uid.rs`'s sandbox test from `MB.21.3.0` still passes and the scheduler's route list shows one uid
   per sandbox.
6. No behavioural change: the same sentences and exit codes, and no `--json` key moved (that is
   `MB.21.4.3`'s re-spelling).
7. `StaticPolicy` has `fallback: Option<String>` and no `spa: bool`: the third grep prints nothing, the
   workspace compiles with every `StaticPolicy { … }` literal updated, and the retired reader still
   accepts `spa = true`, lowering it to `fallback = Some(index)` so `critical_static_manifest.rs` keeps
   every one of today's assertions.

# Notes

- Path additions beyond §21's row, each a compile break without them: `crates/blastit/src/php_view.rs`
  (`route.php()`), `crates/blastit/tests/critical_learn_executor_blocks.rs` (`route.static_policy()`),
  `crates/blastit-scheduler/tests/critical_static_resolve.rs` (`SpaLost`), and
  `crates/blastit-php-sapi/src/**` standing in for §21's non-existent `crates/blastit-php/src/**`.
- `MB.21.4.2` owns the runtime precedence and the `static_forward` deletion; do not start it here, and
  do not "fix" a precedence question by editing a test.
