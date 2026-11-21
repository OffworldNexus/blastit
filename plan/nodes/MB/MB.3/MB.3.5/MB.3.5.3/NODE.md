---
id: MB.3.5.3
title: Name php in ROUTE_KEY_EXPECTED and move the pinned expectation in tests/parse.rs
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.5.1]
paths: ["crates/blastit-manifest/src/parse.rs", "crates/blastit-manifest/tests/parse.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:22"
updated: "2026-11-18T20:36"
---

# Goal

`php` is a reserved route key (`ADR-0010` §2), and the unknown-key diagnostic's
expected-key list (`ROUTE_KEY_EXPECTED` in `crates/blastit-manifest/src/parse.rs`) must
name it while `crates/blastit-manifest/tests/parse.rs` pins that exact string. Keep the two
in step in the same change. The retired external-PHP key is an ordinary unknown key since
`MB.3.17`: not listed, no explanatory sentence.

# Context

- `MB.3.5.4` introduced `[route.php]` (`PhpPolicy`, `Executor::Php`) and reserved the `php`
  key; `MB.3.17` then made the retired external-PHP key an ordinary unknown key.
- The defect this guards against: a developer who mistypes a route key gets an "expected one
  of …" list that omits `php`, so the diagnostic hints at the wrong shape. That is a DX
  defect in a message the manifest page quotes.
- `tests/parse.rs` is a **standard** test (not protected), so moving its expectation is
  allowed; only `critical_*.rs` files are protected, and none of them pins this string
  (`grep -rn 'ROUTE_KEY_EXPECTED' crates/blastit-manifest/` first, and stop if a
  `critical_*` file does).

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ make check
```
- `ROUTE_KEY_EXPECTED` names `php` (and `listen` must **not** appear as a route key — it is
  not one; ADR-0010 §2 rejects it as an unknown key inside `[route.php]`).
- The unknown-key test asserts the new literal, with the denied case (a mistyped key) and
  the accepted twin (a valid route with `[route.php]` still parses) both green.
- No protected `critical_*.rs` file is edited.

# Notes

- One-line-ish change; do not expand it into a parser refactor.
