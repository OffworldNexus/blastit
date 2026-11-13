---
id: MA.2.1.5
title: "Pin raw ';' as form data (R15 parser differential) in critical_ingress"
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

Pin that a raw `;` in an `application/x-www-form-urlencoded` body is *data*, not a pair
separator, in `crates/blastit-ingress/tests/critical_ingress.rs` (R15, ADR-0006).

# Context

Finding G2 of `docs/testing/AUDIT-MA.2.1.md`. ADR-0006 and §4.2 A say the form codec "splits on
`&` (`;` is literal)". The existing `form_reencodes_semicolon` only feeds `%3B`, so it never
exercises the differential it names: a mutant that also splits on `;` survives the 46-test suite
(verified by hand mutation). This is the classic `parse_qs` semicolon divergence between
frameworks, exactly the class the re-encode exists to remove.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
```

- `transcode_buffered(BodyCodec::Form, "POST", Some(FORM), b"a=1;b=2")` returns exactly
  `b"a=1%3Bb%3D2"` (one pair; `;` and `=` in the value are percent-encoded).
- `transcode_buffered(BodyCodec::Form, "POST", Some(FORM), b"a=1&b=2")` returns
  `b"a=1&b=2"` (two pairs).
- `a=%3b` (lowercase hex) decodes to `;` and re-encodes as `%3B`, pinning the accepted hex cases.
- the existing 46 tests stay unchanged and green.

# Notes

- Protected file: add tests only, covered by this node's `paths`.
- No production change expected; all three cases pass on the current tip. If one fails, stop and
  report.
