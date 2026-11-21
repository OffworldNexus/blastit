---
id: MB.3.43
title: "Close MB.3.40 review MINORs: the gate's call site needs a protected witness, and two new unit tests assert nothing"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.3.40]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/host.rs
  - crates/blastit/tests/critical_php_embed.rs
  - docs/knowledge/LESSONS.md
  - plan/nodes/MB/MB.3/MB.3.40/RESULT.md
  - plan/nodes/MB/MB.3/MB.3.42/RESULT.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T02:52"
updated: "2026-11-15T03:29"
---

# Goal

Close the five MINOR findings from MB.3.40's reviewer. None is a behaviour bug in
the gate itself; four are the "a test that cannot fail" class the `testing` skill
forbids, and one is a boot message that now lies about a missing file.

# Context

Reviewer findings on MB.3.40 (commit `9b77ad2`), in the reviewer's words:

1. `crates/blastit/tests/critical_php_embed.rs:2016-2510` drives `host::Provenance`
   directly, so deleting `deployment_refusal`'s invocation (`stage0.rs:913-919`)
   leaves the **protected** tier green — only the unprotected
   `stage0::php_boot_gate` unit tests redden. The gate's *call site* has no
   protected witness.
2. `crates/blastit/src/stage0.rs:4306` `protected_scratch` returns `None`, so the
   three call-site tests pass with **zero assertions** on a host whose `$HOME` is
   not protected (this host, and reportedly CI as root).
3. `RESULT.md:196-219` calls only the refusal legs unit-level — but
   `the_same_tree_owned_by_init_is_accepted` never boots either (no listener) —
   and §5's "the single `unsafe` in `host.rs` is the pre-existing `getxattr`" is
   wrong: `host.rs:2072` adds a SAFETY-commented **test** `setxattr` (non-test
   `unsafe` is still zero, as required).
4. `crates/blastit/src/host.rs:2055` `a_capable_object_is_refused` early-returns
   unprivileged, so the new `!has_file_caps` refusal has no witness in
   `make check`.
5. `crates/blastit/src/stage0.rs:985` `php_trees` runs before `check_static_roots`
   (`:1784`), so a missing `entry`/`static.root` in a PHP manifest is now
   reported as "it cannot be inspected (No such file …)" instead of the sentence
   that names the missing file.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ cargo test -p blastit --locked
$ make check
$ uv run --no-project tools/critical_guard.py --base 9b77ad2 --node MB.3.43
```
- (1) A protected leg goes red when `deployment_refusal`'s call site is removed
  from `stage0.rs` (paste both runs). Prefer the smallest seam that makes the
  *invocation* observable from the protected suite over exporting a new API; if a
  seam is unavoidable, `#[doc(hidden)]` and say why in `RESULT.md`.
- (2) No test in this area can pass without asserting: `protected_scratch`
  panics (or the fixture is built from a candidate list that must succeed) with
  the reason, rather than returning `None` into a body that then checks nothing.
  Same rule for (4): inject the capability predicate the way `host::placement`
  does with `caps_of`, so the refusal is exercised unprivileged.
- (3) `RESULT.md`'s two false sentences corrected. Do not restate them as
  "unit-level" unless it is true of that specific test.
- (5) A PHP manifest with a missing `entry` (and one with a missing
  `[route.static] root`) is refused with a sentence naming the missing path, not
  "it cannot be inspected (No such file or directory)"; a test pins it.
- `make check` green, protected edits authorized by `critical_guard --node`.

# Notes

- Serializes with MB.3.37/MB.3.41 on `crates/blastit/tests/critical_php_embed.rs`.
- Do not weaken or delete any existing assertion from MB.3.40; this node adds
  witnesses and fixes a message.
