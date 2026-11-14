---
id: MB.2.4.4
title: "critical_static_assets: pin descriptor ownership and the meta.size ceiling"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-scheduler/tests/critical_static_assets.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T23:10"
updated: "2026-11-14T00:28"
---

# Goal

Give the protected response suite the two `static_response.rs` oracles it lacks:
the **single-descriptor** rule (the `OwnedFd` moves into `StaticBody` and is never
duplicated, and `HEAD`/`304` close it) and the **`meta.size` ceiling disjunct** of
`StaticRoot::respond`.

# Context

Design §4.2 says `respond` "consumes the file: the descriptor moves into
`StaticBody`, never `dup`ed". No test observes it: `every_response_table_row...`
only reads `body.file`, so a `dup()` (one leaked fd per static response, then fd
exhaustion under load) or a leaked fd on `HEAD`/`304` passes every suite. The
audit's mutation campaign cannot generate that mutant (it is an API misuse, not an
expression change), which is exactly why it needs an explicit assertion.

`respond` refuses a file above the ceiling on **two** conditions,
`file.len > self.max_bytes || file.meta.size > self.max_bytes`. The suite's
`forged()` helper sets `len == meta.size`, so removing the second disjunct is
unobservable; only the `len` half is exercised (`an_oversize_file_is_a_404...`).

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked
```

and, locally reverted afterwards:

- `dup(&file.file)` in `respond` (or a missing `drop`) must fail the suite;
- `if file.len > self.max_bytes` (second disjunct deleted) must fail the suite.

# Notes

- Capture `std::os::fd::AsRawFd::as_raw_fd()` of the resolved `StaticFile` before
  the call and compare it with `response.body.unwrap().file.as_raw_fd()`: equal
  means moved, not duplicated.
- For `HEAD` and for a `304`, assert the descriptor is closed after `respond`
  (`rustix::fs::fstat(raw)` is `EBADF`; the test is single-threaded, so no other
  fd can take that number between the calls).
- Extend `forged()` with a second constructor (or an argument) that keeps `len`
  under the ceiling while `meta.size` is over it, and assert the same `404` with
  `Content-Length: 0` and `Cache-Control: no-store`.
- No product change is required. Do not touch `critical_static_resolve.rs` from
  this task (the resolver oracles have their own node).
