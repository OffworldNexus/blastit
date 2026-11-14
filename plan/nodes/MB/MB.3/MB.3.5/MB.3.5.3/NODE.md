---
id: MB.3.5.3
title: Name fastcgi in ROUTE_KEY_EXPECTED and move the pinned expectation in tests/parse.rs
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.5.1]
paths: ["crates/blastit-manifest/src/parse.rs", "crates/blastit-manifest/tests/parse.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T13:22"
updated: "2026-11-14T13:22"
---

# Goal

`fastcgi` is a reserved route key, but the unknown-key diagnostic's expected-key list
(`ROUTE_KEY_EXPECTED` in `crates/blastit-manifest/src/parse.rs`) never names it, because
`crates/blastit-manifest/tests/parse.rs` pins that exact string. Add the key and move the
pinned expectation in the same change.

# Context

- `MB.3.5.1` introduced `[route.fastcgi]` and the reserved `fastcgi` key. Its
  `ROUTE_KEY_EXPECTED` string was left out of scope because `tests/parse.rs` asserts the
  literal, and neither file was in that leaf's `paths`.
- The result today: a developer who mistypes a route key gets a "expected one of …" list
  that omits `fastcgi`, so the diagnostic hints at the wrong shape. That is a DX defect in
  a message the manifest page quotes.
- `tests/parse.rs` is a **standard** test (not protected), so moving its expectation is
  allowed; only `critical_*.rs` files are protected, and none of them pins this string
  (`grep -rn 'ROUTE_KEY_EXPECTED' crates/blastit-manifest/` first, and stop if a
  `critical_*` file does).

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ make check
```
- `ROUTE_KEY_EXPECTED` names `fastcgi` (and `listen` must **not** appear as a route key —
  it is not one; it is only refused inside `[route.fastcgi]`).
- The unknown-key test asserts the new literal, with the denied case (a mistyped key) and
  the accepted twin (a valid route with `[route.fastcgi]` still parses) both green.
- No protected `critical_*.rs` file is edited.

# Notes

- One-line-ish change; do not expand it into a parser refactor.
