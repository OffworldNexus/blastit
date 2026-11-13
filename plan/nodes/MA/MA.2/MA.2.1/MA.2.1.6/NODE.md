---
id: MA.2.1.6
title: "Pin check-sample cap boundary and buffered bodyless-body 413 in critical_ingress"
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

Close the two remaining §4.2 A boundary/defensive twins in
`crates/blastit-ingress/tests/critical_ingress.rs`: the exact `MAX_CHECK_SAMPLE_BYTES` boundary
(R36) and `transcode_buffered` rejecting bytes for a bodyless-safe method.

# Context

Finding G3 of `docs/testing/AUDIT-MA.2.1.md`.

- `probe_sample` is specified as truncated only when `body_len > MAX_CHECK_SAMPLE_BYTES`. The
  suite tests `cap + 1` but never `cap`, so the mutation `>` -> `>=` survives (verified).
- §4.2 A row 1 and `transcode_buffered`'s own docs say a `GET`/`HEAD`/`OPTIONS` body with bytes is
  the `413` of `IngressReject::BodyNotAllowed`. The suite proves that through `plan_body` only; the
  guard in `transcode_buffered` is untested and deleting it survives (verified).

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
```

- `probe_sample(Some(JSON), b"{\"a\":1}", MAX_CHECK_SAMPLE_BYTES as u64)` has
  `truncated == false` and `valid_json == true`; the existing `cap + 1` case stays truncated.
- for `method` in `["GET", "HEAD", "OPTIONS"]`:
  `transcode_buffered(BodyCodec::Json, method, Some(JSON), b"{\"a\":1}")` is
  `Err(IngressReject::BodyNotAllowed)`; the zero-byte form still returns `Ok(b"", None)`.
- the existing 45+ tests stay unchanged and green.

# Notes

- Protected file: add tests only, covered by this node's `paths`.
- No production change expected; all cases pass on the current tip.
