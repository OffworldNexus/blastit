# RESULT — MB.3.4.1: the pure `blastit-fastcgi` crate (design `MB.3.4a`)

**Status:** done (attempt 3; the three attempt-2 review MINORs are resolved — see "Review
resolution (attempt 3)"). Every `# Acceptance` command was re-run in this working tree and is
pasted below with its output tail.

## What landed

| File | Contents |
|---|---|
| `Cargo.toml` | workspace member; `version/edition/rust-version/license/repository/publish/lints` from the workspace; **no `[dependencies]` table at all** (comment explains why) |
| `src/lib.rs` | `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]` (the `blastit-frame` convention), crate docs + one runnable doctest, `pub mod {cgi,param,record}`, `FCGI_VERSION_1`, `FCGI_MAX_CONTENT`, `MAX_PARAMS_BYTES` (65 535), `MAX_PARAM_PAIR_BYTES` (65 531), hand-written `Error` + `Display` + `std::error::Error` + `From` for both codec errors (no `thiserror`) |
| `src/record.rs` | `HEADER_LEN`, the seven type constants, `FCGI_RESPONDER`, `FCGI_KEEP_CONN`, `FCGI_REQUEST_COMPLETE`, `RecordType` (7 named + `Unnamed(u8)`), `Header::{new,decode,encode,total_len}` (8-byte padding, `CL+PL ≤ 65 535`, version must be 1; a 65 529..=65 535-byte content carries **no padding** so the cap is reachable), `Record::{decode}` returning `(record, used)`, `encode(kind, id, content, out)`, `BeginRequest::{responder,encode,decode}` (role 1, flags 0, `reserved[5]=0`), `EndRequest::decode` (`appStatus`/`reserved[3]` ignored, `protocolStatus != Complete` → `ProtocolStatus`), `RecordError { Truncated, ProtocolStatus }` + hand-written `Display`; log-safe `Debug` for `Record` (kind/id/length, never bytes) |
| `src/param.rs` | `Pair`, `encode_pair` (1-byte length below 128, 4-byte with the high bit otherwise, empty name refused), `encode_len` (31-bit ceiling), `Decoder::next_pair` (empty-name terminator `00 00`, bytes after it refused, a terminator with a value refused), `decode_all`, `read_len`; log-safe `Debug` for `Pair` (name + `value_len`, never the value) |
| `src/cgi.rs` | `MAX_HEAD_BYTES` (64 KiB), `MAX_HEADERS` (100), `MAX_VALUE_BYTES` (8 KiB), `DEFAULT_STATUS` (200), `DEFAULT_CONTENT_TYPE` (`text/html`), `Field` (hand-written log-safe `Debug`: name + `value_len`, never the value), `Head { status, headers, content_type }` + `Head::header` (case-insensitive), `head_end` (first `\r\n\r\n`, or a stream that begins with the blank line), `parse_head`, `parse_status` (three digits, `200..=599`, optional SP + reason phrase), `is_token`/`is_tchar`, `is_forbidden` (`0x00..=0x08`, `0x0a..=0x1f`, `0x7f`), `trim_ows`, `essence` (the `Content-Type` media type before the first `;`, OWS-trimmed, `R18`), `CgiError { BadHead, NonTokenName, ForbiddenProxyHeader }` + hand-written `Display`; log-safe `Debug` for `Head` (status, the `Content-Type` essence, header **names**) |
| `Cargo.lock` | the one new workspace-member entry, written by the first build without `--locked` |

No `src/lockdown.rs`, no `tests/**` (those are `MB.3.4.2`'s), no other crate touched, no
protected `critical_*.rs` touched, no `python/` touched.

## Acceptance

`Cargo.lock` already exists, so every run below is `--locked` (attempt 2).

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked
running 38 tests
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests blastit_fastcgi
running 1 test
test crates/blastit-fastcgi/src/lib.rs - (line 27) ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.13s; merged doctests compilation took 0.13s
exit=0
```

```console
$ cargo test -p blastit-fastcgi --locked
running 38 tests
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests blastit_fastcgi
running 1 test
test crates/blastit-fastcgi/src/lib.rs - (line 27) ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit=0
```

```console
$ test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none | tail -n +2)" && echo "zero dependencies"
zero dependencies

$ cargo tree -p blastit-fastcgi -e normal --prefix none
blastit-fastcgi v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-fastcgi)
```

```console
$ grep -q 'forbid(unsafe_code)' crates/blastit-fastcgi/src/lib.rs && echo "crate is safe-only"
crate is safe-only

$ ! grep -rn 'tokio\|UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-fastcgi/src && echo "pure codec, no I/O"
pure codec, no I/O

$ git diff --stat Cargo.lock && echo "lock updated in this leaf"
 Cargo.lock | 4 ++++
 1 file changed, 4 insertions(+)
lock updated in this leaf
```

```console
$ make check                                                     # CARGO_NET_OFFLINE=true
uv run --no-project tools/guard.py
guard: OK (6 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 6 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check                                        # silent
cargo clippy --workspace --all-targets --locked -- -D warnings    # Finished, no warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked  # Finished, no warnings
cargo test --workspace --all-targets --locked
test result: ok. 38 passed; 0 failed; …          (blastit-fastcgi)
…
cargo test --workspace --doc --locked
…
57 fences: 30 executed, 27 accounted, 0 failed · 19.3s
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

Purity of the production half (everything above `#[cfg(test)]`), checked by hand as well as
by clippy:

```console
$ for f in lib record param cgi; do awk '/^#\[cfg\(test\)\]/{exit} {print}' \
    crates/blastit-fastcgi/src/$f.rs | grep -nE '\.unwrap\(\)|\.expect\(|panic!|todo!|unimplemented!|unsafe'; done
lib.rs:1:#![forbid(unsafe_code)]        (the attribute itself)
lib.rs:9://! no process, has **no dependencies** and contains no `unsafe`.  (prose)
```

No `unwrap()`, `expect()`, `panic!`, `unsafe` or slice indexing in the wire path: every read
goes through `get`/`first`/`split_first`/`try_into`, and every length is `try_from`-checked.

## Requirement → test

| Pinned rule (design / brief) | Test |
|---|---|
| header round-trip, 8-byte padding, every length class | `record::tests::header_round_trips_and_pads_to_eight`, `padding_is_written_as_zero_and_skipped` |
| `contentLength` boundary `65 535` ok / `65 536` err, both directions and all seven types | `over_long_content_is_refused` (`Header::new` and `Header::decode`; wire `CL=65 535`+`PL=1` err) |
| a `65 529..=65 535`-byte record carries no padding and still frames; the cap-sized `PARAMS` record decodes in full | `the_maximum_record_carries_no_padding` (`65 527` pads 1, `65 528` pads 0, `65 529..=65 535` pad 0, and each of those full records `Record::decode`s with `content.len() == len` and content intact) |
| total decoder: truncated/over-long never panics | `the_decoder_is_total_on_every_prefix` (every prefix of a 16-byte record), `a_non_v1_header_is_refused` |
| the seven type codes + an unnamed byte | `record_type_codes_round_trip` |
| `Record::decode` keeps an unknown type byte as `Unnamed` (adapter faults) | `an_unnamed_record_type_is_framed_for_the_adapter` |
| `BEGIN_REQUEST`: one, role 1, flags 0, 8 bytes, `reserved[5]=0` | `begin_request_is_the_pinned_responder_body` |
| `ABORT_REQUEST` empty body | `an_abort_request_carries_no_body` |
| `END_REQUEST`: `appStatus`/`reserved[3]` ignored, non-`Complete` refused | `end_request_ignores_app_status_and_reserved_bytes`, `end_request_refuses_an_incomplete_protocol_status` |
| name-value 1-byte below 128 / 4-byte otherwise | `param::tests::short_lengths_are_one_byte_and_long_ones_four` (127/128 boundary, name and value) |
| pair round-trip incl. empty value and both length forms | `pairs_round_trip_through_the_decoder` |
| empty-name terminator ends the stream | `the_empty_name_terminator_ends_the_stream` (terminator, empty stream, terminator with a value, bytes after it) |
| truncated/straddling pair is `Truncated` | `a_truncated_pair_is_an_error` (every prefix + short 4-byte forms) |
| empty name is never a param; 31-bit ceiling | `an_empty_name_is_not_a_param`, `a_length_beyond_the_four_byte_form_is_refused` |
| `Status: 200..=599`, default 200, never forwarded | `cgi::tests::status_accepts_200_to_599_and_is_never_forwarded`, `an_empty_head_is_200_text_html` |
| `1xx`/out-of-range/garbage/duplicate `Status` → fault | `status_refuses_1xx_and_out_of_range` (11 vectors incl. `200OK`, `200\tOK`, `2 0 0`) |
| value bytes `0x00..=0x08`, `0x0a..=0x1f`, `0x7f` refused; HTAB/obs-text allowed | `forbidden_value_bytes_are_refused` |
| non-token name → fault (NUL, space, empty, HTAB, trailing space, `(`, obs-fold) | `non_token_header_name_is_fault` (the name design §7.2 pins) |
| exactly the four proxy-control names → fault, case-insensitively | `the_four_proxy_control_names_are_refused` |
| every other `X-Accel-*` dropped, response stays 200 | `other_x_accel_names_are_dropped` |
| duplicate `Content-Length`/`Transfer-Encoding` → fault | `duplicate_framing_headers_are_refused` |
| missing colon / empty line / unterminated head → fault | `missing_colon_and_empty_line_are_refused` |
| 100 headers / 8 KiB value / 64 KiB head caps, each boundary both ways (an exactly-`MAX_HEAD_BYTES` head parses `Ok` beside the one-byte-over fault) | `the_caps_are_enforced` |
| head/body split at the first `\r\n\r\n`, empty head | `head_end_finds_the_terminator` |
| `Content-Type` lossy + default | `content_type_is_lossy_and_defaulted` |
| `Content-Type` essence before the first `;`, OWS-trimmed, full value kept in `headers` (`R18`) | `content_type_is_the_essence_only` (`text/html; charset=utf-8`, `text/html;charset=utf-8`, bare `text/html`, `text/html ; charset=utf-8`, a quoted `;` in a parameter; empty essence `; charset=x` / bare → `""`, never a fault) |
| OWS trimming, case-insensitive lookup | `lookup_is_case_insensitive_and_ows_is_trimmed` |
| no value ever reaches a log line | `record::tests::the_debug_view_never_prints_content`, `param::tests::the_debug_view_never_prints_a_value`, `cgi::tests::the_debug_view_never_prints_a_value`, `cgi::tests::a_field_debug_never_prints_its_value` (a `Set-Cookie` value is absent from `Field`'s `{:?}`) |

## Decisions a reviewer should see

1. **`RecordError` stays exactly `{ Truncated, ProtocolStatus }`** (design §4.5.1, "do not
   invent a refusal"). `Truncated` is documented as the single framing fault: a short
   header, a short body, a non-v1 `version`, or a length above `FCGI_MAX_CONTENT`. A
   `version` other than 1 cannot be framed, so it is that fault rather than a new variant.
2. **`RecordType::Unnamed(u8)`** rather than an error for a type byte outside the seven.
   The design names seven types and no other refusal; the adapter (`MB.3.6.2.1`) decides
   that an unnamed type is a fault. This keeps the decoder total and the enum exhaustive;
   `an_unnamed_record_type_is_framed_for_the_adapter` pins the decode side and states the
   adapter obligation in the test.
3. **`cgi::head_end` also accepts a stream that begins with the blank line** (a head with
   no fields at all). Without it, `\r\n` + body would never split and a header-less pool
   answer would 502 on `END_REQUEST`. A head with fields is still split at the first
   `\r\n\r\n`, exactly as the design pins.
4. **`DEFAULT_CONTENT_TYPE = "text/html"`.** The design says the `Content-Type` default
   rule lives in `cgi.rs` but does not name the value; `text/html` is the CGI default every
   server assumes when a script sends none. It is a `pub const`, so `MB.3.6.2.1` can pin a
   different value without touching the parser.
5. **`Content-Type` stays in `Head::headers`** (the full field value, so forwarding is a
   straight copy) and is also exposed as `Head::content_type` — the **essence** only
   (everything before the first `;`, OWS-trimmed, lossy-converted) for the `WireRequest`
   observation (`R18`). An empty essence (`Content-Type: ; charset=x`) is `""`, not a fault,
   because the design names no such refusal and the node forbids inventing one; treating it
   as absent is the caller's decision. `Status` is consumed and never forwarded.
6. **`cgi::Header` is called `Field`** to avoid colliding with `record::Header` in a
   caller that imports both.
7. **A maximum-size record carries no padding.** `CL + PL ≤ 65 535` is php-fpm's hard rule
   (research §F), while 8-byte alignment is only a writer convention. For content lengths
   `65 529..=65 535` the two conflict, so `Header::new` emits `paddingLength = 0` and the
   record ends unaligned; `Header::decode` accepts both padded and unpadded forms. Without
   this, the design-legal 65 535-byte `PARAMS` record could not be encoded at all.

## Review resolution (attempt 2)

| Finding | Fix |
|---|---|
| **[MAJOR]** `MAX_PARAMS_BYTES = 65 535` unreachable: `Header::new` rejected `65 529..=65 535` because aligned padding pushed `CL+PL` over the cap, while `Header::decode` accepted `CL=65 535, PL=0` → a design-legal `PARAMS` set encoded as `Truncated`/`502` instead of a clean `400` | `record.rs::Header::new` now emits `paddingLength = 0` when 8-byte alignment would exceed `FCGI_MAX_CONTENT` (research §F: a maximum-size record must have zero padding); `Header::decode` still accepts both forms. All seven record types share `Header::new`, so the fix covers each. `over_long_content_is_refused` now asserts `65 535` ok / `65 536` `Truncated` in **both** directions for every type, and `the_maximum_record_carries_no_padding` pins `65 529..=65 535` (padding 0) plus `65 527` (padding 1) and `65 528` (padding 0) |
| **[MINOR]** `cgi.rs::Field` derived `Debug`, so `{:?}` printed header values (`Set-Cookie`) | hand-written `Debug` for `Field` (name + `value_len` only, matching `Pair`/`Head`); `a_field_debug_never_prints_its_value` asserts a `Set-Cookie` value is absent |
| **[MINOR]** `lib.rs` doc said the pair budget was `65 535 − 8 = 65 527`, constant is `65 531` | reworded to "the [`MAX_PARAMS_BYTES`] budget minus four bytes (design §4.6.2: 'one pair ≤ 65 531 including its length prefixes')" |
| **[MINOR]** test name `a_non_token_name_is_refused` ≠ design §7.2 | renamed to `cgi::tests::non_token_header_name_is_fault`; the requirement table follows |
| **[MINOR]** unknown type byte decodes as `Unnamed` with no test | added `record::tests::an_unnamed_record_type_is_framed_for_the_adapter` (type byte `9` → `RecordType::Unnamed(9)`, record still framed, adapter obligation in a comment) |
| **[MINOR]** `RESULT.md` mischaracterized the budget split | replaced with the measured 585 production / 607 test / 225 rustdoc split (see Follow-ups) |

## Review resolution (attempt 3)

| Finding | Fix |
|---|---|
| **[MINOR]** `cgi.rs` `Head::content_type` was documented as the `Content-Type` **essence** but stored the whole field value (`text/html; charset=utf-8` reached `WireRequest.content_type` and `Head`'s `Debug`), contradicting `R18:236-238` | `parse_head` now records `essence(value)` — everything before the first `;`, OWS-trimmed — in `Head::content_type`; `Head::headers` still carries the full value for forwarding. New `cgi::tests::content_type_is_the_essence_only` pins `text/html; charset=utf-8` → `text/html`, the no-space `text/html;charset=utf-8`, a bare `text/html`, `text/html ; charset=utf-8`, and a quoted `;` inside a parameter (essence stops at the first `;`, so `application/json`); it also asserts the forwarded field keeps the full value. Empty essence (`; charset=x`, bare `Content-Type:`) is `""` — documented on the field and tested — because the design names no such refusal and the node forbids inventing one |
| **[MINOR]** `record.rs::the_maximum_record_carries_no_padding` never asserted `Record::decode` of the max-size `PARAMS` record (the in-repo cap-sized full-decode path was unasserted) | the `65 529..=65 535` loop now builds each full `PARAMS` record, `Record::decode`s it, and asserts `used == wire.len()`, `content_length == len`, `content.len() == len` and byte-for-byte content equality — so the 65 535-byte cap record is proven to decode in one pass |
| **[MINOR]** `cgi.rs::the_caps_are_enforced` proved the 64 KiB head cap only on the refused side | added an exactly-`MAX_HEAD_BYTES` head (seven 8 KiB values + one 8 134-byte value + the blank line = 65 536 bytes) that parses `Ok` with eight fields, beside a one-byte-over (65 537) twin that is `BadHead` — the boundary is now proven both ways |

## Follow-ups

- **Line budget overrun (declared, not hidden).** The leaf changes 1 575 lines
  (`lib.rs` 109, `record.rs` 577, `param.rs` 251, `cgi.rs` 619, `Cargo.toml` 15,
  `Cargo.lock` 4) against the node's ≤ 550 budget. The measured split is **585 production
  code lines** (non-blank, non-doc, above each `#[cfg(test)]`), **607 in-module test lines**
  (564 non-blank), **225 rustdoc lines** (`//!`/`///`), 79 blank separators and 19
  manifest/lock lines — not "entirely rustdoc plus tests": the production half alone is over
  budget. The reviewer's ~572/492/~369 split describes the pre-fix tree; attempt 2 adds
  three tests and attempt 3 adds one test plus a `essence` helper (cgi.rs 563 → 619,
  record.rs 573 → 577). Every §4.5.2/§4.6.2 rule has both an accepted and a refused case,
  which the
  brief calls "the floor, not the ceiling". **No cap, record type or refusal was dropped**;
  if the lead wants the budget enforced, the split to make is `record+param` vs `cgi` (two
  leaves), not a thinner test matrix.
- **`docs/knowledge/LESSONS.md` candidate** (outside this node's `paths`, so not appended):
  `[T]::split`/`split_once` take `FnMut(&T) -> bool`, not a `Pattern` (`slice_split_once` is
  still unstable), so splitting a byte slice on the two-byte `\r\n` needs
  `windows(2).position(..)`; and stripping the head terminator `\r\n\r\n` first removes the
  *last field line's* CRLF too, which is why every multi-line head parsed as `BadHead` until
  the loop stripped only the blank line and required each remaining line to end CRLF.
- **`MB.3.4.2` handoff:** `lib.rs` already declares `pub mod {cgi,param,record}` and
  re-exports `CgiError`/`RecordError`; `lockdown` only needs its `pub mod lockdown;` line,
  `MAX_PARAMS_BYTES`/`MAX_PARAM_PAIR_BYTES` are ready for the count/byte budgets, and
  `param::encode_pair` is the only encoder (an empty name is refused, so the terminator is a
  zero-length `PARAMS` record via `record::encode`).
