---
id: MA.2.1.2
title: "Leaf 2: ingress codecs, canonical re-encode and header scrub"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MA.2.1.1]
paths: ["crates/blastit-ingress/src/**", "crates/blastit-ingress/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:46"
---

# Goal

Implement `crates/blastit-ingress` completely: the head-level `plan_body` routing plan, the `Json`/`Form` canonical transcode matrix with its parser-differential rejections, header scrubbing, the `check` body sampler, and `suggest_fix`, all inside a `#![forbid(unsafe_code)]` crate with a protected critical suite.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 A (signatures + full `plan_body`/`transcode_buffered` matrix), §4.3 budgets, §7.2 row `R14..R16`, §8 Leaf 2; `ADR-0006`; requirements `R14`, `R15`, `R16`.
- Consumes the skeleton from `MA.2.1.1`; adds `parse_mime`, `probe_sample` (64 B MIME cap, `MAX_CHECK_SAMPLE_BYTES` truncation) and `suggest_fix`.
- Security invariants to keep: non-identity `Content-Encoding` -> `400`; strict UTF-8 RFC 8259 with `MAX_JSON_DEPTH = 64`; BOM, trailing bytes, duplicate keys and numeric lexemes outside `[i64::MIN, u64::MAX]` rejected; `Form` validates `%XX` + UTF-8 + NUL; `Raw` stays `BodyPlan::StreamPassthrough` (zero scheduler buffering).
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-ingress --test critical_ingress --locked
$ cargo clippy -p blastit-ingress --all-targets --locked -- -D warnings
$ make check
```

- `crates/blastit-ingress/tests/critical_ingress.rs` is **new** and asserts both twins for every §4.2 A row: the positive re-encodes (`application/vnd.api+json; charset=UTF-8` -> sorted compact JSON, `u64::MAX`, depth `64`, `Form` `;` -> `%3B`, `Raw` -> `StreamPassthrough`, one canonical `content-length`) **and** the negatives (`u64::MAX + 1`, depth `65`, duplicate keys, BOM, `gzip`, invalid `DELETE` body, `%ZZ` / `%00` / `%FF` / `charset=ibm037`, `Transfer-Encoding` stripped by `apply_transcoded_headers`).
- No test is skipped; no `#[ignore]`.

# Notes

- ~360 changed lines; stay within `crates/blastit-ingress/**`. Do not edit `Cargo.toml` (Leaf `MA.2.1.1` already wired `percent-encoding`, `serde`, `serde_json`, `thiserror`, `url`).
- Rejection reasons must be typed (`IngressReject`), never stringly compared in callers.
- Docs sync of `docs/reference/manifest.md` is `MA.3`, not this leaf.
