# ADR-0006: Fail-Closed `[route.ingress]` Body Validation and Canonical Re-Encoding

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

Forwarding unvalidated request bodies by default exposes backend frameworks to parser differentials (duplicate JSON keys, out-of-range integer rounding, trailing garbage, malformed percent-encoding, charset/NUL tricks, stale `Transfer-Encoding` headers) and makes it invisible in `blastit.toml` which routes accept request payloads (`S4`, `B5`, `M9`). `0.1.1` needs a fail-closed `[route.ingress]` contract that re-encodes `json` and `form` payloads in memory-safe Rust without buffering `raw` streams, ready for Milestone C WASM CDR.

## Decision

1. **Separated head planning (`plan_body`) and buffered transcoding (`transcode_buffered`):**
   - `blastit_ingress::plan_body(method, policy, content_type, content_encoding, content_length)` runs on `ReqHead` **before** buffering:
     - `BodyCodec::Raw` returns `Ok(BodyPlan::StreamPassthrough)` — streaming directly through existing `1 MiB` flow-control windows with **zero scheduler body buffer** (`R16`).
     - `None` or `BodyCodec::None` rejects `content_length > 0` immediately with `413` (and for chunked requests rejects on the first non-empty `ReqBody` frame with `413`).
     - Non-identity `Content-Encoding` on `Json` or `Form` returns `400` in `0.1.1` (preventing compressed bytes from bypassing validation prior to Milestone C decompression).
     - `blastit-http` tracks a `pending: Option<Frame>` slot on `Stream` so an early `RespHead` (`413`/`400`/`404`/`503`/`504`) observed during upload immediately aborts client socket reads, appends `Connection: close`, and flushes the response without losing frames or waiting on flow-control credit.
2. **Memory-safe decode, canonical re-encode, and framing-header scrub (`crates/blastit-ingress`, `#![forbid(unsafe_code)]`):**
   - `body = "json"`: requires `Content-Type` essence `application/json` or `application/*+json` (case-insensitive `charset=utf-8` or omitted); rejects UTF-8 BOM, lone surrogates, nesting `> 64`, trailing bytes, **duplicate object keys**, and **numeric lexemes outside exact `i64`/`u64`/finite-`f64` representation** (`400 Bad Request`, preventing silent `u64+1 -> f64` mutation without enabling workspace-wide `serde_json/arbitrary_precision`); re-serializes compact UTF-8 JSON with sorted `BTreeMap` keys (`serde_json::to_vec`).
   - `body = "form"`: requires `Content-Type` essence `application/x-www-form-urlencoded` (case-insensitive `charset=utf-8` or omitted); splits on `&` (`;` is literal), validates `%XX` hex pairs and strict UTF-8 via `percent-encoding` (`2.3.2`) with **zero NUL (`\0`) bytes** (`400 Bad Request`), and re-serializes via `url::form_urlencoded::Serializer`.
   - `apply_transcoded_headers` strips all existing `Content-Length`, `Content-Type`, `Transfer-Encoding`, `Content-Encoding`, and `Expect` headers from `ReqHead.headers` and appends a single canonical `Content-Type` and `Content-Length` so WSGI `environ` never sees stale `HTTP_TRANSFER_ENCODING=chunked` or duplicate lengths.
   - `body = "raw"`: emits `ManifestWarning::RawIngress` at boot and accepts optional `allow_raw = true` (`raw_ingress` in `blastit check` when omitted).

## Consequences

- **Positive:** Zero scheduler buffering on `raw` uploads; zero stale framing headers in WSGI `environ`; zero silent JSON bigint corruption; JSON duplicate-key and form NUL/charset/bad-escape parser differentials eliminated.
- **Trade-offs:** Structured `json` and `form` payloads buffer up to `BLASTIT_MAX_BODY_BYTES` in `scheduler` prior to canonical re-encoding.
