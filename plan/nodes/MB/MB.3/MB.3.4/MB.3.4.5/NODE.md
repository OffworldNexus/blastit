---
id: MB.3.4.5
title: "Apply the 8192-byte value rule inside lockdown::resolve so no public function returns an over-long CONTENT_TYPE"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.4.2]
paths: ["crates/blastit-fastcgi/src/lockdown.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T12:36"
updated: "2026-11-14T13:09"
---

HISTORICAL — written for the retired FastCGI/php-fpm design. ADR-0010 replaced it (embedded host NTS libphp in per-route view workers). Do not implement from this node.

# Goal

Close the `MB.3.4.2` accepted MINOR (b): `lockdown::resolve` is `pub` and applies **no**
8 192-byte value rule to `CONTENT_TYPE`, so `resolve(b"CONTENT_TYPE", ...)` returns an
over-long value while `build` drops it. Make the crate-wide invariant true — **no public
function returns a param value above `MAX_PARAM_VALUE_BYTES`** — and pin it.

Source: `docs/testing/AUDIT-MB.3.4.md` §"The two accepted MINORs" (2).

# Context

`lockdown.rs:286`:

```rust
b"CONTENT_TYPE" => return facts.content_type.map(fact_value).transpose(),
```

has no cap. `build` special-cases the name before calling `resolve`
(`lockdown.rs:322-329`) and records the drop, so the protected test
`over_long_content_type_is_dropped_not_refused` is green — but a second caller of the public
`resolve` gets `Ok(Some(9 000 bytes))`. §4.6.2 says a value above 8 192 bytes is **dropped**,
and §4.6.2's re-serialisation rule has `exchange` re-run "the same deterministic pure function
from `plan.head`" (`MB.3.6.2.1`), so the hole is reachable by the next leaf.

# Acceptance

```console
$ cargo test -p blastit-fastcgi --locked
test result: ok. 44 passed; ...        # unittests: 43 + the new resolve test
test result: ok. 12 passed; ...        # critical_fastcgi_codec (MB.3.4.4)
test result: ok. 30 passed; ...        # critical_fastcgi_lockdown, UNMODIFIED
test result: ok. 1 passed; ...         # doc-test

$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make rs-lint
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean
```

# Notes

- Suggested shape (the implementer may pick another, but the invariant is the acceptance):
  `resolve` returns `Ok(None)` for a `CONTENT_TYPE` longer than `MAX_PARAM_VALUE_BYTES`, and
  `build` records the drop when `facts.content_type.is_some()` and the resolved value is `None`
  — so the existing `over_long_content_type_is_dropped_not_refused` assertions
  (8 192 emitted, 8 193 dropped + recorded, `u64::MAX` `CONTENT_LENGTH` emitted) stay green
  **without editing the protected file**.
- New unit test in `src/lockdown.rs` (`lockdown::tests`): 8 192 bytes -> `Ok(Some(len 8 192))`;
  8 193 bytes -> `Ok(None)`; and `build` still reports `dropped() == [b"CONTENT_TYPE"]` for the
  same facts.
- Do **not** edit `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs`: this node's
  `paths` do not cover it, and `tools/critical_guard.py` would fail the review.
- Update the `resolve` rustdoc to state the cap, since `#![deny(missing_docs)]` is on and the
  function is public.
