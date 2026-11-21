---
id: MB.36.10
title: "Close the blastit-view-static survivors: declares_body, pre-1970 dates, trim_ows and the CLOEXEC flags"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-view-static/tests/**", "crates/blastit-scheduler/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:05"
updated: "2026-11-18T18:23"
---

# Goal

Close the meaningful `blastit-view-static` survivors the audit found, in the
suites that own them:

- `rewrite::declares_body` / `content_length` — `critical_rewrite.rs` has one
  case; add the boundary rows the mutants show are missing (`Transfer-Encoding`
  present, `Content-Length: 0` vs ` 0 ` vs a non-number, an empty value);
- `static_response::civil_from_days` — a pre-1970 mtime (`touch -d @-86400`) and
  the era boundary, so `-`/`+` and `/` mutations die;
- `static_response::trim_ows` (`&&` vs `||`);
- `static_asset`'s flag composition: `OFlags::CLOEXEC` removed from
  `open_with`/`probe`/`walk` is currently unobservable — prove the descriptor
  does not survive an `execve` (or explain why it cannot be proved).

# Context

`docs/testing/AUDIT-MB.36.md` §4 lists all 37 misses for
`make mutants-rust PKG=blastit-view-static` (218/256 = 85.2 %). The bitflags
`| -> ^` rows are provably equivalent for disjoint flags and need no test; the
rows above are not.

# Acceptance

```console
$ make mutants-rust PKG=blastit-view-static
$ cargo test -p blastit-scheduler --locked --test critical_rewrite
```
- the named survivors are killed, and `missed.txt` names only rows the audit
  marks equivalent;
- the new rows live in an existing suite or a new `critical_*.rs` (both are in
  the path list) and assert the denied direction too.

# Notes

`declares_body`'s oracle belongs to `critical_rewrite.rs` (the scheduler's copy
is the one that stayed); `static_response`'s belongs to
`crates/blastit-view-static/tests/critical_static_assets.rs`.
