---
id: MB.3.4.4
title: "Harden the CGI head parser: reject a ':' in Status, pin the field-less head, protected codec suite for the proxy-control names"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.3.4.2]
paths: ["crates/blastit-fastcgi/src/cgi.rs", "crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T12:36"
updated: "2026-11-14T13:00"
---

# Goal

Close the `parse_status` survivor, the untested `head_end` empty-head branch, and give the CGI
head parser a **protected** oracle: add unit tests to `crates/blastit-fastcgi/src/cgi.rs` and a
new protected `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs`.

Source: `docs/testing/AUDIT-MB.3.4.md` §"Surviving mutants" #3 and the two branch-inspection
findings.

# Context

- **Survivor #3** — `cgi.rs:221` `filter(|digit| *digit < 10)` mutated to `<= 10` is *missed*.
  `':'` is byte 58 and `58 - 48 == 10`, so the mutant accepts it as a digit: `Status: 2:0`
  parses as **300** and `Status: 20:` as **210**, where §4.5.1 says "`Status:` must be three
  digits in `200..=599`" and anything else is `CgiError::BadHead`. A malformed pool head becomes
  a different status instead of a `502`. `status_refuses_1xx_and_out_of_range` has no case with
  a byte whose `byte - b'0' == 10`.
- **`head_end` early return** — `if buf.starts_with(b"\r\n") { return Some(2) }` is never
  asserted. `head_end(b"\r\n\r\n")` is untested; if the early return regressed to the generic
  `windows(4)` search it would return `Some(4)`, and `parse_head(b"\r\n\r\n")` then hits
  `if line.is_empty() { return Err(BadHead) }` — a **legal field-less CGI head** becomes a
  `502`. cargo-mutants generates no mutant for that constant, so the ratio did not catch it.
- **No protected oracle** — §7.2 assigns row 24 (malformed / proxy-control CGI head -> `502`)
  to `critical_fastcgi_dispatch::malformed_or_proxy_control_cgi_head_is_502` (`MB.3.6.2.2`),
  which does not exist yet. Today the only oracle for the four proxy-control names
  (`X-Accel-Redirect`, `X-Sendfile`, `X-Lighttpd-Send-File`, `X-Rewrite-URL` — the sendfile
  escape out of `open_basedir`) is the unprotected in-module
  `cgi::tests::the_four_proxy_control_names_are_refused`. Adding a new
  `crates/*/tests/critical_*.rs` file is always allowed by `tools/critical_guard.py`.

`MB.3.6.2.2` keeps the authoritative end-to-end `502` oracle; this node only pins the codec.

# Acceptance

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_codec --locked
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit-fastcgi --locked
test result: ok. 43 passed; ...        # unittests: 41 + the two new cgi tests
test result: ok. 25 passed; ...        # critical_fastcgi_lockdown, unmodified
test result: ok. 12 passed; ...        # the new protected codec suite
test result: ok. 1 passed; ...         # doc-test

$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-4-mut make mutants-rust PKG=blastit-fastcgi
348 mutants tested in ~60s: 8 missed, 322 caught, 18 unviable
# cgi.rs:221:66 must move from missed.txt to caught.txt; no other file's count may drop
```

Use a **private** `MUTANTS_TMP` (LESSONS #26/#29).

# Notes

Unit tests in `src/cgi.rs`:

- `status_with_a_non_digit_byte_is_bad_head` — `Status: 2:0`, `Status: 20:`, `Status: :00` all
  `Err(CgiError::BadHead)`; `Status: 200`, `Status: 200 OK`, `Status: 599` still parse.
- `a_blank_line_only_head_is_200_text_html` — `head_end(b"\r\n") == Some(2)`,
  `head_end(b"\r\n\r\n") == Some(2)`, and the composed
  `parse_head(&raw[..head_end(raw).unwrap()])` for `b"\r\n\r\nbody"` yields 200,
  `text/html`, zero headers.

Protected suite `tests/critical_fastcgi_codec.rs` (new file, same header comment style as
`critical_fastcgi_lockdown.rs`, `#![allow(clippy::unwrap_used, clippy::expect_used)]`):

- the four proxy-control names, ASCII case-insensitively -> `CgiError::ForbiddenProxyHeader`;
- `X-Accel-Buffering: no` / `-Expires` / `-Charset` / `-Limit-Rate` -> `Ok`, and absent from
  `Head::headers` (a benign SSE directive must not become a `502`);
- `X-Accel-Redirect\0: /x` and ` X-Accel-Redirect: /x` -> `CgiError::NonTokenName`;
- `Status: 100`, `Status: 600`, `Status: 2:0` -> `BadHead`; duplicate `Status` -> `BadHead`;
- caps: exactly 100 headers parse, 101 refuse; an 8 192-byte value parses, 8 193 refuses;
  exactly 64 KiB of head parses, +1 byte refuses;
- the record/param walls that today live only in-module: `EndRequest` with a non-`Complete`
  `protocolStatus` -> `RecordError::ProtocolStatus`; `Header::decode` with `CL + PL > 65 535` ->
  `Truncated`; a truncated name-value pair -> `Truncated`.
