---
id: MB.26.27
title: "Close the blastit-view-static survivors: declares_body, pre-1970 dates and trim_ows"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-view-static/tests/**", "crates/blastit-scheduler/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:23"
updated: "2026-11-18T18:51"
---

# Goal

Close the meaningful `blastit-view-static` survivors the audit found, in the
suites that own them:

- `rewrite::declares_body` / `content_length` — `critical_rewrite.rs` has one
  case; add the boundary rows the mutants show are missing (`Transfer-Encoding`
  present, `Content-Length: 0` vs ` 0 ` vs a non-number, an empty value);
- `static_response::civil_from_days` — four rows the audit re-derived: `316:17` is the
  **pre-1970** branch (needs `http_date(-86_400 * 400)` or older); `320:42`'s `/ 36_524` term is 0
  for every tested date (needs a date ~100 years into a 400-year era); `320:64` differs on the
  era's last day; `328:21`'s `- 9` is only taken for January/February (needs a 2024-01-15-style
  case);
- `static_response::trim_ows` (`&&` vs `||` at `233:41`) — a value with trailing whitespace;

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

# Provenance

Re-filed from `MB.36.10` (the `MB.36` audit) so the deferred gap stays actionable:
priority-3 coverage, not an `MB.36` acceptance claim. The body above is `MB.36.10`'s own, unchanged.
