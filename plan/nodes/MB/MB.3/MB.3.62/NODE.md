---
id: MB.3.62
title: "The two-package image must prove what it claims, and its probe table must not be able to drift"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.3.61]
paths: ["crates/blastit/tests/**", "e2e/php-sapi/**"]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-15T20:57"
updated: "2026-11-15T21:51"
---

# Goal

Close the two substantive MINORs from MB.3.61's review. Both are the "a claim that can drift from
what is proved" class: one e2e image's caption overstates what it demonstrates, and one test table is
a hand copy of production data while claiming it cannot disagree.

# Context

MB.3.61 (commit 71dcaf8) made discovery version-major and fixed the `php_version` presence sentence.
Its reviewer PASSed and left:

1. **`e2e/php-sapi/run-two-package.sh:11,72-73` overstates.** It says "with `php82-embed` and
   `php84-embed` installed and no `BLASTIT_LIBPHP`, the boot served from …", but the suites hand the
   boot the probe's own path (`critical_php_backend.rs:729`, `critical_php_embed.rs:2974`), so the
   image proves **probe order + boot agreement on the real layout**, not stage 0's unset-override
   walk — which is pinned only by a unit test with injected `lib_dirs`. Either add one `blastit dev`
   boot with no `BLASTIT_LIBPHP` asserting `200` in that image (both libraries present), or reword to
   say exactly what the helper does.
2. **`crates/blastit/tests/critical_php_{embed,backend}.rs:192-247`** carry a 13-entry `PROBES` table
   hand-copied from `stage0.rs`'s `LIBPHP_PROBES`, while the doc comment claims the two "cannot
   disagree" — an edit to either side is silent. Tie them together (e.g. assert against
   `include_str!("../src/stage0.rs")`, or a shared constant if one is reachable from the test), or
   drop the claim and say the table is a copy that a reviewer must keep in step.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ cargo test -p blastit --locked --test critical_php_backend
$ make check
```
- The two-package claim is either **proved** (a no-`BLASTIT_LIBPHP` boot in that image answers 200,
  pasted) or **reworded** to the agreement it actually shows. A reader must not be able to infer that
  the unset-override path is image-tested when it is unit-tested.
- Editing one entry of `LIBPHP_PROBES` (in a /tmp copy) makes the test fail, or the comment no longer
  claims they cannot disagree. Paste which.
- `make check` exit 0; no assertion weakened; nothing outside the node's paths.

# Notes

- Do not change discovery behaviour; the ordering was verified (every 8.4 spelling before every 8.3,
  every 8.3 before every 8.2) and is not this node's subject.
