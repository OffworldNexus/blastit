---
id: MB.3.61
title: "Discovery must prefer a library that can serve, and the php_version sentence must not lie"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.59]
paths:
  - crates/blastit/src/stage0.rs
  - "crates/blastit/tests/**"
  - "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md"
  - "e2e/php-sapi/**"
  - "scripts/check-exec-env.sh"
  - scripts/check_workflows.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T19:36"
updated: "2026-11-15T20:55"
---

# Goal

Close the three discovery MINORs from MB.3.59's review. One is a real defect: with two PHP embed
packages installed, discovery prefers a candidate that cannot serve.

# Context

MB.3.59 (commit 755281c) made the minor come from the library instead of its file name. Its reviewer
found, among MINORs:

1. **Ordering defect (reproduced).** `crates/blastit/src/stage0.rs:456-466` tries `libphp82.so`
   before `libphp.so`, so on Alpine with both `php82-embed` and `php84-embed` installed, discovery
   picks `/usr/lib/libphp82.so` and refuses — although `/usr/lib/php84/libphp.so` would serve. Fix:
   try `libphp.so` in the per-minor embed directories before the 8.2 legacy names, and add a
   two-package test (the shape is a manifest with `[route.php]`, both libraries present, no
   `BLASTIT_LIBPHP`, and the boot must serve from the 8.3/8.4 library).
2. **False sentence on an odd but valid object.** `stage0.rs:1611` sets
   `exports_php_version: symbols.php_version.is_some()` from a scan requiring `STT_FUNC` *and*
   readable `st_value`/`st_size`, so an object exporting `php_version` as `STT_NOTYPE`, or with
   `st_size == 0`, is refused with "exports no `php_version`" — a false statement. Presence must come
   from the name alone; the body value stays a separate fact.
3. **`docs/decisions/ADR-0010-…:16`** still lists only the seven dotted names and five directories —
   no packed names, no `/usr/lib/php{82,83,84}` / `/usr/local/lib/php8x`, and neither new refusal.
   The node's Notes required that wording in the same change.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ make rs-test-php-image
$ make check
$ make docs-check
$ uv run --no-project tools/critical_guard.py --base 755281c --node MB.3.61
```
- Two-package case: with both libraries installed and no `BLASTIT_LIBPHP`, the boot serves from the
  8.3/8.4 library (paste the run); a single-package host behaves exactly as it does today.
- An object exporting `php_version` as `STT_NOTYPE` (or zero-size) is not refused as "exports no
  `php_version`" — the sentence names only the real cause. A test pins both shapes.
- ADR-0010's candidate/directory list matches the code, including the two new refusals.
- `make check` exit 0; `make docs-check`; `critical_guard --node` authorizes the protected edits;
  no assertion weakened.

# Notes

- Do not re-open the resolution design; it was verified with a bypass attempt (name/body
  disagreement refuses, an 8.2 body under an 8.3 name is refused).
