# RESULT MA.2.1.2 — Leaf 2: ingress codecs, canonical re-encode and header scrub

## Summary

`crates/blastit-ingress` is complete: `plan_body` (head-level routing), `transcode_buffered`
(`Json`/`Form` canonical re-encode with parser-differential rejections), `parse_mime`,
`probe_sample`, `suggest_fix`, `apply_transcoded_headers`, and the typed `IngressReject` with an
HTTP status mapping. The crate stays `#![forbid(unsafe_code)]` + `#![deny(missing_docs)]`, has no
`unwrap()`/`expect()`/`panic!()` in library paths, and never puts body bytes in an error.

Design decisions worth a reviewer's attention:

- **Hand-written strict JSON parser** (`src/json.rs`) instead of a `serde_json` visitor.
  `serde_json` reports an integer lexeme that overflows `u64` as `visit_f64`, so a visitor cannot
  distinguish `u64::MAX + 1` from a legitimate float — the exact silent-rounding bug `I2-M1`
  forbids. The parser validates UTF-8 once, rejects BOM/trailing bytes/duplicate keys (after escape
  decoding)/lone surrogates/non-finite numbers, bounds nesting at `MAX_JSON_DEPTH = 64`, keeps
  integers exact in `i64`/`u64`, and re-emits canonically (sorted keys, no whitespace, `f64` through
  `serde_json::Number::from_f64` so `1e0` becomes `1.0`).
- **`plan_body` also validates the codec's `Content-Type`/charset**, so a mismatched head is a `400`
  before the scheduler buffers up to `BLASTIT_MAX_BODY_BYTES`. `transcode_buffered` re-checks it, so
  a direct caller gets the same typed rejection.
- **Zero-length `DELETE` is `AllowEmptyOnly`** whatever its `Content-Type` (§4.2 A row 1: a `DELETE`
  with `0` B is `Ok(b"", None)`); a `DELETE` that declares bytes follows the `POST` codec rules.
  `GET`/`HEAD`/`OPTIONS` with any byte are `413`.
- **`probe_sample` gates validity on the declared essence**: `valid_json`/`valid_form` are only true
  when declaring that codec would accept the observed request. A JSON body sent as `text/plain` is
  therefore not "valid JSON", so `suggest_fix` never proposes a codec that would `400` the traffic
  it just observed. Truncated (> 64 KiB) samples force both flags false and suggest
  `([Raw], true)` (`R36`).
- **`Form`** validates every `%XX` (strict hex), decodes with `percent_encoding::percent_decode` +
  UTF-8, rejects NUL, drops empty `&` segments (matching `urllib.parse.parse_qsl`), and re-encodes
  with `url::form_urlencoded::Serializer` (so `;` becomes `%3B`).

## Evidence

### 1. `cargo test -p blastit-ingress --test critical_ingress --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Both twins per §4.2 A row: vendor `+json` + charset -> sorted compact JSON, `u64::MAX`/`i64::MIN`
exact, depth 64, `Form` `;` -> `%3B`, `Raw` -> `StreamPassthrough`, one canonical
`content-length`; and `u64::MAX + 1`, `i64::MIN - 1`, depth 65, duplicate keys (including via
`\u0061`), BOM, `gzip`/`br`, invalid `DELETE` body, `%ZZ`/`%4`/`%00`/`%FF`/`charset=ibm037`,
`Transfer-Encoding`/`Content-Encoding`/`Expect` stripped, body on `GET`/`HEAD`/`OPTIONS`, body on an
undeclared/`none` route (`413`), trailing bytes, lone surrogate, invalid UTF-8, non-finite number,
empty JSON body, and CT mismatch. No `#[ignore]`, no skipped test.

### 2. `cargo clippy -p blastit-ingress --all-targets --locked -- -D warnings`

```console
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-ingress --all-targets --locked -- -D warnings
    Checking blastit-ingress v0.1.0 (/home/remy/offworld-nexus/blastit/crates/blastit-ingress)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s
```

### 3. `cargo fmt --check`

```console
$ cargo fmt -p blastit-ingress -- --check && echo 'ingress fmt clean'
ingress fmt clean

$ cargo fmt --all -- --check && echo 'WORKSPACE_FMT_OK'
WORKSPACE_FMT_OK
```

### 4. `make check`

```console
$ CARGO_NET_OFFLINE=true make check; echo "EXIT=$?"
uv run --no-project tools/guard.py
guard: OK (6 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 13 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
... (workspace clippy, rustdoc, tests: 60 `test result: ok` lines)
cargo test -p blastit-ingress --test critical_ingress
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
cargo test -p blastit-sqlpolicy --test critical_literals
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The first `make check` attempts stopped at `rs-fmt` on the **parallel** node `MA.2.1.3`'s
in-progress `crates/blastit-sqlpolicy/**` edits (out of this node's `paths`); once that node
finished, the full chain passed with exit `0`.

### 5. Crate-local full test run

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --locked
running 4 tests   (mime + form unit tests)      ... ok
running 46 tests  (critical_ingress)            ... ok
running 1 test    (doc test in src/lib.rs)      ... ok
```

## Files

- `crates/blastit-ingress/src/lib.rs` — public API, constants, `BodyPlan`, `BodySample`,
  `TranscodedBody`, `IngressReject` (+ `status()`), `parse_mime`, `plan_body`,
  `transcode_buffered`, `apply_transcoded_headers`, `probe_sample`, `suggest_fix`.
- `crates/blastit-ingress/src/json.rs` — strict RFC 8259 parser and canonical emitter.
- `crates/blastit-ingress/src/form.rs` — `%XX`/UTF-8/NUL validation and canonical form encoder.
- `crates/blastit-ingress/src/mime.rs` — essence/charset parsing, 64-byte cap, essence predicates.
- `crates/blastit-ingress/tests/critical_ingress.rs` — **new** protected critical suite (46 tests).

No `Cargo.toml`, `docs/**`, or other crate was touched; the only `plan/` write is this
`RESULT.md`.

## Follow-ups

- The implementation is ~1.15k src lines against §8's "~360" estimate. The overrun is the
  hand-written JSON parser (~400 lines with rustdoc), which `I2-M1`'s exact `u64::MAX + 1`
  rejection makes necessary; a `serde_json` visitor cannot tell an overflowing integer lexeme from
  an `f64`. If the lead prefers, a future leaf could swap it for a `serde_json` visitor plus a
  numeric-lexeme pre-scan, but that is strictly more code and more fragile.
- `suggest_fix` maps a sample that is neither valid JSON nor a valid form (including an empty body
  with no `Content-Type`) to `([Raw], true)`. `BodySample` carries no body length, so the audit
  layer should skip routes whose `HttpObsLine.body_len == 0` before calling it, to avoid proposing
  `raw` for a route that only ever receives empty bodies.
- `plan_body` validates the codec's `Content-Type`/charset at the head (before buffering), which is
  earlier than §4.2 A's matrix strictly requires. It only tightens the `400`; the scheduler leaf
  should keep calling `transcode_buffered` so a direct caller still gets the same typed rejection.
