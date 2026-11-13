---
id: MA.2.1.4
title: "Pin canonical JSON string escaping and strict-string rejection in critical_ingress"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-ingress/tests/critical_ingress.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T13:53"
updated: "2026-11-12T13:58"
---

# Goal

Add the missing protected twins for canonical JSON *string* handling in
`crates/blastit-ingress/tests/critical_ingress.rs`, so neither the parser's strict-string
rejections nor the emitter's re-escaping can regress.

# Context

Finding G1 of `docs/testing/AUDIT-MA.2.1.md`. R14 / §4.2 A promise a canonical re-encode of
strict RFC 8259 JSON. Two hand mutations survive the current 46-test suite: an emitter that emits
the decoded backslash (or a raw newline) instead of escaping it, and (by inspection) a parser that
drops the `byte < 0x20` stop. The first produces invalid JSON on the wire; the second accepts a
document RFC 8259 forbids.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
```

- positive: `transcode_buffered(BodyCodec::Json, "POST", Some(JSON), br#"{"a":"x\\y\"z"}"#)`
  returns exactly `br#"{"a":"x\\y\"z"}"#` (the decoded backslash and quote are re-escaped).
- positive: the `\n`, `\r`, `\t`, `\b` and `\f` escapes re-emit as the short escape, and
  `\u0001` re-emits as `\u0001` (lowercase, four hex digits).
- negative: a raw `0x01` byte inside a string is `Err(IngressReject::JsonSyntax)`.
- negative: an unknown escape such as `\q` is `Err(IngressReject::JsonSyntax)`.
- the existing 46 tests stay unchanged and green; no `#[ignore]`.

# Notes

- Protected file: add tests only. This node's `paths` list covers it, so `tools/critical_guard.py`
  allows the addition; never weaken an existing assertion.
- No production change is expected: all five cases pass on the current tip. If one fails, stop and
  report it instead of tightening or deleting the assertion.
